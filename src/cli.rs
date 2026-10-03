//! Window-free project and scene operations used by the `rusting` CLI.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::asset_import::{
    self, asset_references, AssetImportError, AssetProvenance, ImportReport,
    ImportSettings,
};
use crate::project::{
    create_project_from, open_project, OpenProject, ProjectError,
    ProjectTemplate,
};
use crate::runtime::{
    cook_scene, read_scene_document, SceneDocument, SceneEntity, SceneIoError,
};

/// Stable envelope for this and future commands, including scene patches and capture.
#[derive(Debug, Serialize)]
pub struct CliResult {
    pub schema_version: u32,
    pub ok: bool,
    pub data: Value,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Default, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// 1-based line and column in `file`, for text the tools could not parse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// JSON pointer into `file`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_location: Option<String>,
    /// The scene object the diagnostic is about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityRef>,
    /// A `scene patch` operation that fixes the problem for certain;
    /// `rusting fix` applies every one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct EntityRef {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl EntityRef {
    fn of(entity: &SceneEntity) -> Self {
        Self {
            id: entity.id,
            name: entity.name.clone(),
        }
    }
}

/// `--limit`, `--fields` and `--summary`: keep a listing command's output
/// within an agent's budget.
#[derive(Debug, Default)]
pub struct Shape {
    pub limit: Option<usize>,
    pub fields: Option<Vec<String>>,
    pub summary: bool,
}

impl Shape {
    /// Applies the options to every array directly under `data`: `fields`
    /// keeps only those keys of each object, `limit` keeps the first items,
    /// and `summary` replaces the array with `{"count": N}`. Each cut array's
    /// omitted count goes under `data.omitted`.
    pub fn apply(&self, data: &mut Value) {
        let Some(map) = data.as_object_mut() else {
            return;
        };
        let mut omitted = serde_json::Map::new();
        for (key, value) in map.iter_mut() {
            let Some(items) = value.as_array_mut() else {
                continue;
            };
            if let Some(fields) = &self.fields {
                for item in items.iter_mut() {
                    if let Some(object) = item.as_object_mut() {
                        object.retain(|name, _| fields.contains(name));
                    }
                }
            }
            let total = items.len();
            if self.summary {
                *value = serde_json::json!({ "count": total });
                continue;
            }
            if let Some(limit) = self.limit.filter(|limit| *limit < total) {
                items.truncate(limit);
                omitted.insert(key.clone(), (total - limit).into());
            }
        }
        if !omitted.is_empty() {
            map.insert("omitted".into(), Value::Object(omitted));
        }
    }
}

impl CliResult {
    pub fn success(data: Value) -> Self {
        Self {
            schema_version: 1,
            ok: true,
            data,
            diagnostics: Vec::new(),
        }
    }

    pub fn failure(
        code: &'static str,
        message: impl Into<String>,
        file: Option<PathBuf>,
    ) -> Self {
        Self {
            schema_version: 1,
            ok: false,
            data: Value::Null,
            diagnostics: vec![Diagnostic {
                code,
                severity: "error",
                message: message.into(),
                file,
                ..Diagnostic::default()
            }],
        }
    }

    pub fn exit_code(&self) -> i32 {
        if self.ok {
            0
        } else if self.diagnostics.iter().any(|d| d.code == "CLI_USAGE") {
            2
        } else {
            1
        }
    }
}

fn project_error(error: ProjectError, path: &Path) -> CliResult {
    let code = match error {
        ProjectError::InvalidName | ProjectError::InvalidBinaryName => {
            "PROJECT_INVALID"
        }
        ProjectError::MissingParent(_) | ProjectError::MissingFile(_) => {
            "PROJECT_MISSING_FILE"
        }
        ProjectError::AlreadyExists(_) => "PROJECT_EXISTS",
        ProjectError::UnsupportedVersion(_) => "PROJECT_VERSION",
        ProjectError::Json(_) => "PROJECT_MANIFEST_JSON",
        ProjectError::Io(_) => "PROJECT_IO",
    };
    CliResult::failure(code, error.to_string(), Some(path.to_owned()))
}

fn scene_error(error: SceneIoError, path: &Path) -> CliResult {
    let code = match error {
        SceneIoError::Io(_) => "SCENE_IO",
        SceneIoError::Source(_) => "SCENE_JSON",
        SceneIoError::UnsupportedVersion(_) => "SCENE_VERSION",
        SceneIoError::DuplicateEntity(_)
        | SceneIoError::DuplicateName(_)
        | SceneIoError::MissingParent(_)
        | SceneIoError::HierarchyCycle(_) => "SCENE_STRUCTURE",
        SceneIoError::Reflection(_) => "SCENE_COMPONENT_FIELD",
        _ => "SCENE_INVALID",
    };
    let mut result =
        CliResult::failure(code, error.to_string(), Some(path.to_owned()));
    locate_scene_error(&error, path, &mut result.diagnostics[0]);
    result
}

/// Points a scene load error at the line, or at the object, that caused it.
fn locate_scene_error(
    error: &SceneIoError,
    path: &Path,
    diagnostic: &mut Diagnostic,
) {
    if let SceneIoError::Source(error) = error {
        diagnostic.line = u32::try_from(error.line()).ok();
        diagnostic.column = u32::try_from(error.column()).ok();
        let missing = error
            .to_string()
            .strip_prefix("missing field `")
            .and_then(|rest| rest.split('`').next().map(str::to_owned));
        if let Some((key, fix)) =
            missing.and_then(|missing| misspelled_key(path, &missing))
        {
            diagnostic.message +=
                &format!("; `{key}` looks like a misspelling of it");
            diagnostic.fix = Some(fix);
        }
        return;
    }
    // Structure errors come after decoding, so the file still decodes.
    let Some(document) = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SceneDocument>(&bytes).ok())
    else {
        return;
    };
    let Some(index) = crate::runtime::error_object(&document, error) else {
        return;
    };
    let mut pointer = format!("/entities/{index}");
    if let SceneIoError::Reflection(error) = error {
        let key = error.component.replace('~', "~0").replace('/', "~1");
        pointer = format!("{pointer}/components/{key}");
    }
    diagnostic.scene_location = Some(pointer);
    diagnostic.entity = Some(EntityRef::of(&document.entities[index]));
}

/// A `rename_key` fix for a required field the scene file misspells: the
/// only key within two edits of `missing` in an object that lacks it, when
/// that key appears once in the file.
fn misspelled_key(path: &Path, missing: &str) -> Option<(String, Value)> {
    fn walk(
        value: &Value,
        pointer: &str,
        missing: &str,
        out: &mut Vec<(String, String)>,
    ) {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    let child_pointer = format!(
                        "{pointer}/{}",
                        crate::scene_patch::escape(key)
                    );
                    if !object.contains_key(missing)
                        && crate::scene_patch::edit_distance(key, missing) <= 2
                    {
                        out.push((key.clone(), child_pointer.clone()));
                    }
                    walk(child, &child_pointer, missing, out);
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    walk(item, &format!("{pointer}/{index}"), missing, out);
                }
            }
            _ => {}
        }
    }
    let text = std::fs::read_to_string(path).ok()?;
    let raw: Value = serde_json::from_str(&text).ok()?;
    let mut candidates = Vec::new();
    walk(&raw, "", missing, &mut candidates);
    let [(key, pointer)] = candidates.as_slice() else {
        return None;
    };
    (key_positions(&text, key).len() == 1).then(|| {
        (
            key.clone(),
            json!({"op": "rename_key", "path": pointer, "to": missing}),
        )
    })
}

/// Byte offsets of `"key"` used as an object key (followed by `:`).
fn key_positions(text: &str, key: &str) -> Vec<usize> {
    let quoted = format!("\"{key}\"");
    text.match_indices(&quoted)
        .filter(|(start, _)| {
            text[start + quoted.len()..].trim_start().starts_with(':')
        })
        .map(|(start, _)| start)
        .collect()
}

fn project_data(project: &OpenProject) -> Value {
    json!({
        "root": project.root,
        "manifest_path": project.root.join("project.json"),
        "manifest": project.manifest,
        "main_scene": project.scene_path,
        "cooked_scene": project.root.join(&project.manifest.cooked_scene),
        "cargo_manifest": project.root.join("Cargo.toml"),
        "code_path": project.code_path,
        "binary_name": project.manifest.binary_name,
        "engine_version": env!("CARGO_PKG_VERSION"),
    })
}

pub fn new_project(
    parent: &Path,
    name: &str,
    template: ProjectTemplate,
) -> CliResult {
    match create_project_from(parent, name, template) {
        Ok(project) => {
            let mut data = project_data(&project);
            data["next"] = json!(
                "Run `rusting check`. The first build compiles the engine \
                 and takes a few minutes; later builds take seconds."
            );
            CliResult::success(data)
        }
        Err(error) => project_error(error, &parent.join(name)),
    }
}

pub fn inspect_project(root: &Path) -> CliResult {
    match open_project(root) {
        Ok(project) => CliResult::success(project_data(&project)),
        Err(error) => project_error(error, root),
    }
}

fn scene_warnings(path: &Path, document: &SceneDocument) -> Vec<Diagnostic> {
    let base = path.parent().unwrap_or(Path::new("."));
    asset_references(document)
        .into_iter()
        .filter_map(|(asset, index)| {
            let resolved = if asset.is_absolute() {
                asset.clone()
            } else {
                base.join(&asset)
            };
            (!resolved.is_file()).then(|| Diagnostic {
                code: "SCENE_MISSING_ASSET",
                severity: "warning",
                message: format!(
                    "referenced asset `{}` does not exist",
                    asset.display()
                ),
                file: Some(resolved),
                scene_location: Some(format!(
                    "/entities/{index}/mesh_renderer"
                )),
                entity: Some(EntityRef::of(&document.entities[index])),
                ..Diagnostic::default()
            })
        })
        .collect()
}

/// A `SCENE_MISSING_ASSET` per sound cue whose clip is not a file under
/// `assets/`. Clips are relative to `assets/`, unlike the scene-relative
/// asset `reference`, so a pasted `../assets/...` path is caught here.
fn sound_clip_diagnostics(
    root: &Path,
    document: &SceneDocument,
) -> Vec<Diagnostic> {
    document
        .entities
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| {
            let cue: crate::runtime::SoundCue = serde_json::from_str(
                entity.components.get("rusting.sound_cue")?,
            )
            .ok()?;
            let resolved = root.join("assets").join(&cue.clip);
            (!resolved.is_file()).then(|| Diagnostic {
                code: "SCENE_MISSING_ASSET",
                severity: "warning",
                message: format!(
                    "sound clip `{}` is not a file under assets/; clips are relative to assets/ (`sounds/hit.wav`), not to the scene",
                    cue.clip
                ),
                file: Some(resolved),
                scene_location: Some(format!(
                    "/entities/{index}/components/rusting.sound_cue"
                )),
                entity: Some(EntityRef::of(entity)),
                ..Diagnostic::default()
            })
        })
        .collect()
}

/// A `CODE_MISSING_ASSET` per literal path in game code, such as
/// `load_text("charts/easy.json")` or `play_sound("sfx/hit.wav", ..)`, that
/// is not a file under `assets/`. Paths built at run time are not checked.
fn code_asset_diagnostics(root: &Path) -> Vec<Diagnostic> {
    const CALLS: [&str; 4] = [
        "load_text(\"",
        "play_sound(\"",
        "play_sound_looped(\"",
        "play_sound_with(\"",
    ];
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    files.sort();
    let mut diagnostics = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (line, content) in text.lines().enumerate() {
            for call in CALLS {
                for (start, _) in content.match_indices(call) {
                    let rest = &content[start + call.len()..];
                    let Some(path) = rest.split('"').next() else {
                        continue;
                    };
                    if root.join("assets").join(path).is_file() {
                        continue;
                    }
                    diagnostics.push(Diagnostic {
                        code: "CODE_MISSING_ASSET",
                        severity: "warning",
                        message: format!(
                            "{}:{}: `{path}` is not a file under assets/",
                            file.strip_prefix(root).unwrap_or(&file).display(),
                            line + 1
                        ),
                        file: Some(file.clone()),
                        ..Diagnostic::default()
                    });
                }
            }
        }
    }
    diagnostics
}

fn rust_files(folder: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

/// One error per scene key that loading ignores. A key within two edits of
/// exactly one unset known key, written once in the file, gets a rename fix.
fn dropped_field_diagnostics(
    path: &Path,
    document: &SceneDocument,
) -> Vec<Diagnostic> {
    let Some((text, raw)) =
        std::fs::read_to_string(path).ok().and_then(|text| {
            let raw = serde_json::from_str::<Value>(&text).ok()?;
            Some((text, raw))
        })
    else {
        return Vec::new();
    };
    crate::scene_patch::dropped_fields(&raw, document)
        .into_iter()
        .map(|field| {
            let entity = field.entity.map(|index| &document.entities[index]);
            let pointer = match field.entity {
                Some(index) => format!("/entities/{index}{}", field.path),
                None => field.path.clone(),
            };
            let key = field.path.rsplit('/').next().unwrap_or_default();
            let key = key.replace("~1", "/").replace("~0", "~");
            let fix = field
                .suggestion
                .as_ref()
                .filter(|_| key_positions(&text, &key).len() == 1)
                .map(|suggestion| {
                    json!({"op": "rename_key", "path": pointer, "to": suggestion})
                });
            let hint = match &field.suggestion {
                Some(suggestion) => format!("did you mean `{suggestion}`?"),
                None => format!("known fields: {}", field.known.join(", ")),
            };
            Diagnostic {
                code: "SCENE_UNKNOWN_FIELD",
                severity: "error",
                message: format!(
                    "`{key}` is not a field here and loading ignores it; {hint}"
                ),
                file: Some(path.to_owned()),
                scene_location: Some(pointer),
                entity: entity.map(EntityRef::of),
                fix,
                ..Diagnostic::default()
            }
        })
        .collect()
}

/// One error per simulation part in the main scene that does not support
/// the project's `determinism` mode, located at the first body using it.
fn determinism_diagnostics(
    project: &OpenProject,
    document: &SceneDocument,
) -> Vec<Diagnostic> {
    let mode = project.manifest.determinism;
    let mut parts = std::collections::BTreeMap::new();
    let mut first_body = std::collections::BTreeMap::new();
    for (index, entity) in document.entities.iter().enumerate() {
        let Some(body) = &entity.physics_body else {
            continue;
        };
        if let Some((part, supports)) =
            crate::runtime::body_part(body, Some(&project.root))
        {
            first_body.entry(part.clone()).or_insert(index);
            parts.insert(part, supports);
        }
    }
    let Err(error) = crate::runtime::check_parts(mode, &parts) else {
        return Vec::new();
    };
    error
        .offenders
        .into_iter()
        .map(|offender| Diagnostic {
            code: "DETERMINISM_UNSUPPORTED",
            severity: "error",
            message: format!(
                "`{}` supports determinism {:?}, but project.json asks for {mode:?}",
                offender.part, offender.supports
            ),
            file: Some(project.scene_path.clone()),
            scene_location: Some(format!(
                "/entities/{}/physics_body",
                first_body[&offender.part]
            )),
            entity: Some(EntityRef::of(
                &document.entities[first_body[&offender.part]],
            )),
            ..Diagnostic::default()
        })
        .collect()
}

fn read_scene(path: &Path) -> Result<SceneDocument, CliResult> {
    read_scene_document(path).map_err(|error| scene_error(error, path))
}

pub fn inspect_scene(path: &Path) -> CliResult {
    let document = match read_scene(path) {
        Ok(document) => document,
        Err(result) => return result,
    };
    // The runtime loader migrates old scenes in memory. Report both versions.
    let source_version = std::fs::read(path).ok().and_then(|bytes| {
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| {
                value.get("format_version").and_then(Value::as_u64)
            })
    });
    let cameras: Vec<_> = document.entities.iter().filter_map(|entity| entity.camera.map(|camera| json!({"id": entity.id, "name": entity.name, "active": camera.active, "priority": camera.priority, "projection": camera.projection}))).collect();
    let classes: BTreeSet<_> = document
        .entities
        .iter()
        .flat_map(|entity| entity.classes.iter().cloned())
        .collect();
    let assets: BTreeSet<_> = asset_references(&document)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    let mut result = CliResult::success(json!({
        "path": path, "name": document.name, "revision": file_revision(path),
        "scene_version": source_version.unwrap_or(u64::from(document.format_version)),
        "loaded_scene_version": document.format_version,
        "entity_count": document.entities.len(), "cameras": cameras,
        "classes": classes, "referenced_assets": assets,
    }));
    result.diagnostics = scene_warnings(path, &document);
    result
}

#[derive(Debug, Clone)]
pub enum SceneFilter {
    Id(Uuid),
    Name(String),
    Class(String),
    Component(String),
    All,
}

fn matches_filter(entity: &SceneEntity, filter: &SceneFilter) -> bool {
    match filter {
        SceneFilter::Id(id) => entity.id == *id,
        SceneFilter::Name(name) => entity.name.as_deref() == Some(name),
        SceneFilter::Class(class) => {
            entity.classes.iter().any(|candidate| candidate == class)
        }
        SceneFilter::Component(component) => match component.as_str() {
            "transform" => entity.transform.is_some(),
            "mesh_renderer" => entity.mesh_renderer.is_some(),
            "camera" => entity.camera.is_some(),
            "visible" => entity.visible.is_some(),
            "physics_body" => entity.physics_body.is_some(),
            "rigid_body" => entity.rigid_body.is_some(),
            "collider" => entity.collider.is_some(),
            "collision_layers" => entity.collision_layers.is_some(),
            "gpu_physics_watch" => entity.gpu_physics_watch.is_some(),
            "directional_light" => entity.directional_light.is_some(),
            "point_light" => entity.point_light.is_some(),
            "spot_light" => entity.spot_light.is_some(),
            name => entity.components.contains_key(name),
        },
        SceneFilter::All => true,
    }
}

pub fn query_scene(path: &Path, filter: &SceneFilter) -> CliResult {
    let document = match read_scene(path) {
        Ok(document) => document,
        Err(result) => return result,
    };
    let mut entities: Vec<_> = document
        .entities
        .iter()
        .filter(|entity| matches_filter(entity, filter))
        .collect();
    entities.sort_by_key(|entity| entity.id);
    // Registered components are stored as JSON text; print them as objects.
    let entities: Vec<Value> = entities
        .iter()
        .map(|entity| {
            let mut value = json!(entity);
            if let Some(map) = value["components"].as_object_mut() {
                for component in map.values_mut() {
                    if let Some(parsed) = component
                        .as_str()
                        .and_then(|text| serde_json::from_str(text).ok())
                    {
                        *component = parsed;
                    }
                }
            }
            value
        })
        .collect();
    let mut result = CliResult::success(
        json!({"path": path, "revision": file_revision(path), "count": entities.len(), "entities": entities}),
    );
    result.diagnostics = scene_warnings(path, &document);
    result
}

fn file_revision(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| crate::runtime::scene_revision(&bytes))
}

pub fn patch_scene(path: &Path, patch_path: &Path, dry_run: bool) -> CliResult {
    let patch = match std::fs::read(patch_path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            serde_json::from_slice::<crate::scene_patch::ScenePatch>(&bytes)
                .map_err(|error| error.to_string())
        }) {
        Ok(patch) => patch,
        Err(message) => {
            return CliResult::failure(
                "PATCH_JSON",
                message,
                Some(patch_path.to_owned()),
            )
        }
    };
    apply_scene_patch(path, &patch, Some(patch_path), dry_run)
}

/// `patch_file` is where the patch came from, if it is a file, so an
/// operation error can point into it.
fn apply_scene_patch(
    path: &Path,
    patch: &crate::scene_patch::ScenePatch,
    patch_file: Option<&Path>,
    dry_run: bool,
) -> CliResult {
    match crate::scene_patch::patch_scene_file(path, patch, dry_run) {
        Ok(outcome) => {
            let mut result = CliResult::success(
                json!({"path": path, "dry_run": dry_run, "patch": outcome}),
            );
            result.diagnostics = outcome
                .unvalidated_components
                .iter()
                .map(|name| Diagnostic {
                    code: "PATCH_UNVALIDATED_COMPONENT",
                    severity: "warning",
                    message: format!(
                        "component `{name}` is not built in; the game validates it on load"
                    ),
                    file: Some(path.to_owned()),
                    ..Diagnostic::default()
                })
                .collect();
            result
        }
        Err(error) => {
            use crate::scene_patch::PatchError;
            let mut result = CliResult::failure(
                error.code(),
                error.to_string(),
                Some(path.to_owned()),
            );
            let diagnostic = &mut result.diagnostics[0];
            match &error {
                PatchError::Operation { operation, .. }
                | PatchError::Field { operation, .. } => {
                    if let Some(patch_file) = patch_file {
                        diagnostic.file = Some(patch_file.to_owned());
                        diagnostic.scene_location =
                            Some(format!("/operations/{operation}"));
                    }
                }
                PatchError::Scene(error) => {
                    locate_scene_error(error, path, diagnostic);
                }
                PatchError::InvalidObject { id, name, .. } => {
                    diagnostic.entity = Some(EntityRef {
                        id: *id,
                        name: name.clone(),
                    });
                }
                _ => {}
            }
            if let PatchError::Field { id, .. } = &error {
                diagnostic.entity = read_scene_document(path)
                    .ok()
                    .and_then(|scene| {
                        scene
                            .entities
                            .iter()
                            .find(|e| e.id == *id)
                            .map(EntityRef::of)
                    })
                    .or(Some(EntityRef {
                        id: *id,
                        name: None,
                    }));
            }
            result.data =
                json!({"path": path, "revision": file_revision(path)});
            result
        }
    }
}

pub fn validate_project(root: &Path) -> CliResult {
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let document = match read_scene(&project.scene_path) {
        Ok(document) => document,
        Err(result) => return result,
    };
    let mut diagnostics = scene_warnings(&project.scene_path, &document);
    // Imported-asset errors (missing files or dependencies, bad metadata)
    // block shipping; license and not-imported warnings do not.
    diagnostics.extend(
        list_assets(&project.root)
            .diagnostics
            .into_iter()
            .filter(|d| d.severity == "error"),
    );
    diagnostics.extend(determinism_diagnostics(&project, &document));
    diagnostics
        .extend(dropped_field_diagnostics(&project.scene_path, &document));
    diagnostics.extend(sound_clip_diagnostics(&project.root, &document));
    diagnostics.extend(code_asset_diagnostics(&project.root));
    let valid = diagnostics.is_empty();
    CliResult {
        schema_version: 1,
        ok: valid,
        data: json!({"root": project.root, "main_scene": project.scene_path, "entity_count": document.entities.len()}),
        diagnostics: diagnostics
            .into_iter()
            .map(|mut d| {
                d.severity = "error";
                d
            })
            .collect(),
    }
}

/// Applies every certain fix `validate` finds to the main scene. Each fix
/// renames one misspelled key in place in the file text, so nothing else in
/// the file changes; validation then runs again, because a file that did not
/// load can show its next problem only once the first is fixed. With
/// `dry_run`, only the first round is reported and nothing is written.
/// Problems without a certain fix stay in the diagnostics.
pub fn fix_project(root: &Path, dry_run: bool) -> CliResult {
    let mut applied = Vec::new();
    if let Some(outdated) = outdated_agents(root) {
        let path = root.join("AGENTS.md");
        if !dry_run {
            let refreshed = std::fs::copy(&path, root.join("AGENTS.md.old"))
                .and_then(|_| std::fs::write(&path, PROJECT_AGENTS));
            if let Err(error) = refreshed {
                return CliResult::failure(
                    "IO_ERROR",
                    error.to_string(),
                    Some(path),
                );
            }
        }
        applied.push(json!({
            "message": "replaced AGENTS.md with this engine's copy; the old one is AGENTS.md.old",
            "file": outdated.file,
            "operation": "refresh_agents",
        }));
    }
    // Each round renames at least one key, so this bounds a file with many.
    for _ in 0..64 {
        let mut validation = validate_project(root);
        let (fixes, remaining): (Vec<_>, Vec<_>) = validation
            .diagnostics
            .drain(..)
            .partition(|diagnostic| diagnostic.fix.is_some());
        if fixes.is_empty() || dry_run {
            applied.extend(fixes.iter().map(fix_summary));
            return fix_result(validation, remaining, applied, dry_run);
        }
        for fix in &fixes {
            let scene = fix.file.clone().unwrap_or_default();
            if let Err(error) = apply_rename(&scene, fix.fix.as_ref()) {
                return scene_error(error.into(), &scene);
            }
            applied.push(fix_summary(fix));
        }
    }
    CliResult::failure(
        "SCENE_INVALID",
        "the scene still needs key renames after 64 rounds of `rusting fix`",
        None,
    )
}

fn fix_summary(fix: &Diagnostic) -> Value {
    json!({"message": fix.message, "file": fix.file, "operation": fix.fix})
}

fn fix_result(
    mut result: CliResult,
    remaining: Vec<Diagnostic>,
    fixes: Vec<Value>,
    dry_run: bool,
) -> CliResult {
    if !result.data.is_object() {
        result.data = json!({});
    }
    result.data["dry_run"] = json!(dry_run);
    result.data["fixed"] = json!(fixes.len());
    result.data["fixes"] = Value::Array(fixes);
    result.ok = result.ok && remaining.iter().all(|d| d.severity != "error");
    result.diagnostics.extend(remaining);
    result
}

/// Renames the one key a `rename_key` fix names, in place in the file text.
fn apply_rename(scene: &Path, fix: Option<&Value>) -> std::io::Result<()> {
    let text = std::fs::read_to_string(scene)?;
    let fix = fix.cloned().unwrap_or_default();
    let (Some(pointer), Some(to)) = (fix["path"].as_str(), fix["to"].as_str())
    else {
        return Err(std::io::Error::other("the fix is not a key rename"));
    };
    let key = pointer.rsplit('/').next().unwrap_or_default();
    let key = key.replace("~1", "/").replace("~0", "~");
    let [start] = key_positions(&text, &key)[..] else {
        return Err(std::io::Error::other(format!(
            "`{key}` is no longer a key exactly once in the file"
        )));
    };
    let quoted_len = key.len() + 2;
    let text = format!(
        "{}{}{}",
        &text[..start],
        serde_json::to_string(to).unwrap_or_default(),
        &text[start + quoted_len..]
    );
    crate::runtime::write_atomic(scene, text.as_bytes())
}

pub fn cook_project(root: &Path) -> CliResult {
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let validation = validate_project(root);
    if !validation.ok {
        return validation;
    }
    let destination = project.root.join(&project.manifest.cooked_scene);
    match cook_scene(&project.scene_path, &destination) {
        Ok(()) => CliResult::success(
            json!({"root": project.root, "source": project.scene_path, "output": destination}),
        ),
        Err(error) => scene_error(error, &project.scene_path),
    }
}

/// Provenance and settings flags shared by `asset import` and `reimport`.
#[derive(Clone, Debug, Default)]
pub struct AssetImportFlags {
    pub folder: PathBuf,
    pub from: Option<PathBuf>,
    pub provenance: AssetProvenance,
    pub max_size: Option<u32>,
    pub dry_run: bool,
}

fn asset_result(
    result: Result<ImportReport, AssetImportError>,
    root: &Path,
) -> CliResult {
    match result {
        Ok(report) => {
            let diagnostics = report
                .warnings
                .iter()
                .map(|warning| Diagnostic {
                    code: "ASSET_NO_LICENSE",
                    severity: "warning",
                    message: warning.clone(),
                    file: Some(root.join(&report.path)),
                    ..Diagnostic::default()
                })
                .collect();
            CliResult {
                diagnostics,
                ..CliResult::success(json!({ "asset": report }))
            }
        }
        Err(error) => CliResult::failure(
            error.code(),
            error.to_string(),
            error.path().map(Path::to_owned),
        ),
    }
}

pub fn import_asset(
    root: &Path,
    source: &Path,
    flags: &AssetImportFlags,
) -> CliResult {
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let settings = ImportSettings {
        max_size: flags.max_size,
    };
    asset_result(
        asset_import::import_asset(
            &project.root,
            source,
            &flags.folder,
            &flags.provenance,
            &settings,
            flags.dry_run,
        ),
        &project.root,
    )
}

pub fn reimport_asset(
    root: &Path,
    asset: &str,
    flags: &AssetImportFlags,
) -> CliResult {
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let settings = flags.max_size.map(|max_size| ImportSettings {
        max_size: Some(max_size),
    });
    asset_result(
        asset_import::reimport_asset(
            &project.root,
            asset,
            flags.from.as_deref(),
            &flags.provenance,
            settings.as_ref(),
            flags.dry_run,
        ),
        &project.root,
    )
}

/// Runs a `project.json` generator hook and imports its file into
/// `flags.folder`, or replaces `replace` when given.
pub fn generate_asset(
    root: &Path,
    hook: &str,
    prompt: &str,
    replace: Option<&str>,
    flags: &AssetImportFlags,
) -> CliResult {
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let target = match replace {
        Some(asset) => asset_import::GenerateTarget::Replace { asset },
        None => asset_import::GenerateTarget::Import {
            folder: &flags.folder,
        },
    };
    asset_result(
        asset_import::generate_asset(
            &project.root,
            &project.manifest.generators,
            hook,
            prompt,
            target,
            flags.dry_run,
        ),
        &project.root,
    )
}

/// Imported assets with references; asset errors make the result fail.
pub fn list_assets(root: &Path) -> CliResult {
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let catalog = asset_import::list_assets(&project.root);
    CliResult {
        schema_version: 1,
        ok: catalog.issues.iter().all(|issue| issue.severity != "error"),
        data: json!({ "assets": catalog.assets }),
        diagnostics: catalog
            .issues
            .into_iter()
            .map(|issue| Diagnostic {
                code: issue.code,
                severity: issue.severity,
                message: issue.message,
                file: Some(issue.file),
                ..Diagnostic::default()
            })
            .collect(),
    }
}

/// `rusting docs`: with no query the item list, else a search, one item, or
/// the brief. `budget` is in tokens.
pub fn docs(action: DocsAction<'_>) -> CliResult {
    use crate::docs::{brief, find, items, search, tokens, within_budget};
    match action {
        DocsAction::List => {
            let list: Vec<_> = items()
                .iter()
                .map(|item| json!({"id": item.id, "kind": item.kind, "title": item.title}))
                .collect();
            CliResult::success(json!({ "items": list }))
        }
        DocsAction::Search(query, limit) => {
            let (matches, total) = search(query, limit);
            let mut data = json!({
                "query": query,
                "matches": matches,
                "shown": matches.len(),
                "total": total,
            });
            if total == 0 {
                data["verdict"] = json!(format!(
                    "No matches for `{query}`. Every word must appear in an item; try fewer words."
                ));
            }
            CliResult::success(data)
        }
        DocsAction::Show(id, budget) => match find(id) {
            Some(item) => {
                let (mut text, truncated) = within_budget(&item.text, budget);
                // `tokens` counts the page text only, not the notice below.
                let shown = tokens(&text);
                if truncated {
                    let more = item.text.lines().count() - text.lines().count();
                    text += &format!(
                        "\n... {more} more lines; pass `--budget {}` to see all\n",
                        tokens(&item.text) + 50
                    );
                }
                CliResult::success(json!({
                    "id": item.id,
                    "title": item.title,
                    "text": text,
                    "tokens": shown,
                    "truncated": truncated,
                    "total_tokens": tokens(&item.text),
                }))
            }
            None => {
                let (hits, _) = search(id, 5);
                let near: Vec<_> =
                    hits.iter().filter_map(|hit| hit["id"].as_str()).collect();
                CliResult::failure(
                    "CLI_USAGE",
                    format!(
                        "no docs item `{id}`; {}",
                        if near.is_empty() {
                            "run `rusting docs` to list them".to_owned()
                        } else {
                            format!("closest: {}", near.join(", "))
                        }
                    ),
                    None,
                )
            }
        },
        DocsAction::Brief(budget) => {
            let (text, listed, total) = brief(budget);
            CliResult::success(json!({
                "text": text,
                "tokens": tokens(&text),
                "listed": listed,
                "total": total,
            }))
        }
    }
}

/// What `rusting docs` was asked to do.
pub enum DocsAction<'a> {
    List,
    /// Query and the most matches to return.
    Search(&'a str, usize),
    /// Item id and token budget.
    Show(&'a str, usize),
    /// Token budget.
    Brief(usize),
}

/// Explains one diagnostic code, or lists every code when `code` is `None`.
pub fn explain(code: Option<&str>) -> CliResult {
    use crate::diagnostics::{lookup, to_json, CODES};
    match code {
        None => CliResult::success(
            json!({ "codes": CODES.iter().map(to_json).collect::<Vec<_>>() }),
        ),
        Some(code) => match lookup(&code.to_ascii_uppercase()) {
            Some(info) => CliResult::success(to_json(info)),
            None => {
                let prefix = code.split('_').next().unwrap_or(code);
                let similar: Vec<_> = CODES
                    .iter()
                    .filter(|info| {
                        info.code.starts_with(&prefix.to_ascii_uppercase())
                    })
                    .map(|info| info.code)
                    .collect();
                CliResult::failure(
                    "CLI_USAGE",
                    if similar.is_empty() {
                        format!("no diagnostic code `{code}`; run `rusting explain` to list them")
                    } else {
                        format!(
                            "no diagnostic code `{code}`; similar: {}",
                            similar.join(", ")
                        )
                    },
                    None,
                )
            }
        },
    }
}

pub fn list_presets() -> CliResult {
    CliResult::success(json!({ "presets": crate::art_direction::PRESETS }))
}

/// Applies an art-direction preset to a scene as one scene patch.
/// `only` limits it to some of `art_direction::PRESET_SCOPES`.
pub fn apply_preset(
    path: &Path,
    name: &str,
    only: &[&str],
    dry_run: bool,
) -> CliResult {
    let scopes = crate::art_direction::PRESET_SCOPES;
    if let Some(scope) = only.iter().find(|scope| !scopes.contains(scope)) {
        return CliResult::failure(
            "PRESET_SCOPE_UNKNOWN",
            format!(
                "no preset scope `{scope}`; choose from {}",
                scopes.join(", ")
            ),
            None,
        );
    }
    let Some(preset) = crate::art_direction::preset(name) else {
        let names: Vec<_> = crate::art_direction::PRESETS
            .iter()
            .map(|preset| preset.name)
            .collect();
        return CliResult::failure(
            "PRESET_UNKNOWN",
            format!("no preset `{name}`; choose one of {}", names.join(", ")),
            None,
        );
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return scene_error(error.into(), path),
    };
    let document = match read_scene(path) {
        Ok(document) => document,
        Err(result) => return result,
    };
    let mut patch = crate::art_direction::preset_patch(&document, preset, only);
    patch.expected_revision = Some(crate::runtime::scene_revision(&bytes));
    apply_scene_patch(path, &patch, None, dry_run)
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// `probe` also opens the device a run would pick and runs a tiny GPU job on
/// it, on a separate thread with a 15 s limit so a wedged driver cannot hang
/// the CLI.
pub fn doctor(probe: bool) -> CliResult {
    let mut devices = Vec::new();
    if let Ok(library) = vulkano::VulkanLibrary::new() {
        if let Ok(instance) =
            vulkano::instance::Instance::new(library, Default::default())
        {
            if let Ok(found) = instance.enumerate_physical_devices() {
                devices = found
                    .map(|device| device.properties().device_name.clone())
                    .collect();
            }
        }
    }
    CliResult::success(json!({
        "engine_version": env!("CARGO_PKG_VERSION"),
        "platform": {"os": std::env::consts::OS, "arch": std::env::consts::ARCH},
        "build_tools": {"cargo": tool_available("cargo"), "rustc": tool_available("rustc"), "glslc": tool_available("glslc")},
        "vulkan": {"available": !devices.is_empty(), "devices": devices},
        "probe": probe.then(probe_gpu),
    }))
}

fn probe_gpu() -> Value {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(crate::rendering::probe_vulkan());
    });
    match receiver.recv_timeout(Duration::from_secs(15)) {
        Ok(Ok(device)) => json!({"ok": true, "selected_device": device}),
        Ok(Err(error)) => json!({"ok": false, "error": error}),
        Err(_) => {
            json!({"ok": false, "error": "the GPU probe did not finish in 15 s"})
        }
    }
}

/// Output of one child process run by the CLI.
struct ProcessRun {
    /// Exit code; `None` when killed at the timeout or by a signal.
    code: Option<i32>,
    success: bool,
    timed_out: bool,
    stdout: String,
    stderr: String,
    duration: Duration,
}

impl ProcessRun {
    fn data(&self) -> Value {
        json!({
            "exit_code": self.code,
            "timed_out": self.timed_out,
            "duration_ms": self.duration.as_millis() as u64,
            "stdout": self.stdout,
            "stderr": self.stderr,
        })
    }
}

/// Runs `command` to completion, or kills it after `timeout`.
fn run_process(
    command: &mut Command,
    timeout: Option<Duration>,
) -> std::io::Result<ProcessRun> {
    use std::io::Read;

    let start = Instant::now();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            text
        })
    };
    let stdout = read(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let stderr = read(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if timeout.is_some_and(|timeout| start.elapsed() >= timeout) {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(ProcessRun {
        code: status.code(),
        success: status.success(),
        timed_out,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
        duration: start.elapsed(),
    })
}

/// Last `lines` lines of `text`, for failure messages.
fn tail(text: &str, lines: usize) -> String {
    let all: Vec<_> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Runs `cargo <command>` on the project; `Err` is the failed result.
/// Per-file build and asset reload errors, with relative paths resolved
/// against the project root.
fn reload_diagnostics(root: &Path, output: &str) -> Vec<Diagnostic> {
    crate::project::reload_diagnostics(output)
        .into_iter()
        .map(|diagnostic| Diagnostic {
            code: match diagnostic.kind {
                "shader" => "SHADER_BUILD_ERROR",
                "asset" => "ASSET_RELOAD_FAILED",
                _ => "RUST_BUILD_ERROR",
            },
            severity: if diagnostic.kind == "asset" {
                "warning"
            } else {
                "error"
            },
            message: diagnostic.message,
            file: diagnostic.file.map(|file| root.join(file)),
            line: diagnostic.line,
            ..Diagnostic::default()
        })
        .collect()
}

/// Engine patterns that fix common rustc errors in game code: a match is an
/// error line containing every needle of a row.
const ENGINE_HINTS: &[(&[&str], &str)] = &[
    (
        &["error[E0499]", "scene"],
        "A `GameObject` or counter borrow keeps `scene` busy. Copy the values you need out (`let x = scene.object(\"A\").position();`), or finish each object chain in one statement before the next `scene` call.",
    ),
    (
        &["error[E0502]", "scene"],
        "A borrow from `scene` is still alive. Read into a local first, then write: `let p = scene.object(\"A\").position(); scene.object(\"B\").set_position(p);`.",
    ),
    (
        &["error[E0599]", "GameScene"],
        "`GameScene` has no such method. Run `rusting docs search <word>` to find the real name in the API index.",
    ),
    (
        &["error[E0599]", "GameObject"],
        "`GameObject` has no such method. Run `rusting docs search GameObject` to list its methods.",
    ),
    (
        &["error[E0432]", "rusting_engine"],
        "Import from the prelude: `use rusting_engine::prelude::*;`. Run `rusting docs show api/GameScene::object` for signatures.",
    ),
    (
        &["error[E0308]", "f64"],
        "Scene values are `f32`. Write `1.0_f32`, `x as f32`, or let inference pick `f32` by removing a `f64` annotation.",
    ),
    (
        &["error[E0308]", "[f32; 3]"],
        "Positions, rotations (radians) and scales are `[f32; 3]` arrays: `[x, y, z]`, not a tuple or a slice.",
    ),
];

/// One `RUST_ENGINE_HINT` per distinct engine pattern seen in rustc output.
fn engine_hints(root: &Path, output: &str) -> Vec<Diagnostic> {
    let mut hints: Vec<Diagnostic> = Vec::new();
    for line in output.lines() {
        for (needles, hint) in ENGINE_HINTS {
            if !needles.iter().all(|needle| line.contains(needle))
                || hints.iter().any(|seen| seen.message == *hint)
            {
                continue;
            }
            let mut place =
                line.split(": ").next().unwrap_or_default().split(':');
            let file = place.next().filter(|f| f.ends_with(".rs"));
            let number = place.next().and_then(|n| n.parse().ok());
            hints.push(Diagnostic {
                code: "RUST_ENGINE_HINT",
                severity: "hint",
                message: (*hint).to_owned(),
                file: file.map(|file| root.join(file)),
                line: number,
                ..Diagnostic::default()
            });
        }
    }
    hints
}

fn cargo(
    project: &OpenProject,
    command: &str,
    release: bool,
    target: Option<&str>,
) -> Result<ProcessRun, CliResult> {
    let mut cargo = Command::new("cargo");
    cargo.arg(command);
    if release {
        cargo.arg("--release");
    }
    if let Some(target) = target {
        cargo.args(["--target", target]);
    }
    cargo
        .args(["--message-format", "short", "--manifest-path"])
        .arg(project.root.join("Cargo.toml"))
        .current_dir(&project.root);
    let run = run_process(&mut cargo, None).map_err(|error| {
        CliResult::failure(
            "BUILD_TOOL_MISSING",
            format!("could not run cargo: {error}"),
            None,
        )
    })?;
    if run.success {
        return Ok(run);
    }
    let mut result = CliResult::failure(
        "BUILD_FAILED",
        format!("cargo {command} failed:\n{}", tail(&run.stderr, 40)),
        Some(project.root.join("Cargo.toml")),
    );
    result.data = json!({"root": project.root, "cargo": run.data()});
    result
        .diagnostics
        .extend(reload_diagnostics(&project.root, &run.stderr));
    result
        .diagnostics
        .extend(engine_hints(&project.root, &run.stderr));
    Err(result)
}

/// Writes a deliberately failing scenario for `name`. The check stays red
/// until the agent names a real entity and value.
fn scaffold_scenario(root: &Path, name: &str) -> Result<PathBuf, CliResult> {
    let path = root.join("tests").join(format!("{name}.json"));
    if path.exists() {
        return Err(CliResult::failure(
            "PROJECT_EXISTS",
            format!("{} already exists", path.display()),
            Some(path),
        ));
    }
    let scenario = json!({
        "name": format!("{name} works (edit this scenario)"),
        "ticks": 10,
        "steps": [{"tick": 1, "expect": {
            "entity": "TODO entity name",
            "path": "/transform/position",
            "exists": true
        }}]
    });
    let written = std::fs::create_dir_all(root.join("tests")).and_then(|()| {
        std::fs::write(&path, serde_json::to_string_pretty(&scenario).unwrap())
    });
    match written {
        Ok(()) => Ok(path),
        Err(error) => Err(CliResult::failure(
            "IO_ERROR",
            error.to_string(),
            Some(path),
        )),
    }
}

fn scaffold_name(name: &str) -> Result<(), CliResult> {
    let valid = name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        return Ok(());
    }
    Err(CliResult::failure(
        "CLI_USAGE",
        format!("`{name}` is not a snake_case name like `spin_coins`"),
        None,
    ))
}

/// `rusting add scenario`: a failing scenario in `tests/`.
pub fn add_scenario(root: &Path, name: &str) -> CliResult {
    if let Err(result) = scaffold_name(name) {
        return result;
    }
    match scaffold_scenario(root, name) {
        Ok(path) => CliResult::success(json!({
            "scenario": path,
            "next": "Name a real entity and expected value in the scenario, then run `rusting test`.",
        })),
        Err(result) => result,
    }
}

/// `rusting add system`: a documented stub in `src/main.rs` plus a failing
/// scenario. The stub is not called until `update` calls it.
pub fn add_system(root: &Path, name: &str) -> CliResult {
    if let Err(result) = scaffold_name(name) {
        return result;
    }
    let code_path = root.join("src/main.rs");
    let source = match std::fs::read_to_string(&code_path) {
        Ok(source) => source,
        Err(error) => {
            return CliResult::failure(
                "PROJECT_MISSING_FILE",
                error.to_string(),
                Some(code_path),
            )
        }
    };
    if source.contains(&format!("fn {name}(")) {
        return CliResult::failure(
            "PROJECT_EXISTS",
            format!("`fn {name}` already exists in src/main.rs"),
            Some(code_path),
        );
    }
    let scenario = match scaffold_scenario(root, name) {
        Ok(path) => path,
        Err(result) => return result,
    };
    let stub = format!(
        "\n/// TODO: describe what `{name}` does each tick.\nfn {name}(_scene: &mut GameScene<'_>, _time: &FrameTime) {{\n    todo!(\"implement {name}\");\n}}\n"
    );
    if let Err(error) = std::fs::write(&code_path, source + &stub) {
        return CliResult::failure(
            "IO_ERROR",
            error.to_string(),
            Some(code_path),
        );
    }
    CliResult::success(json!({
        "function": code_path,
        "scenario": scenario,
        "next": format!("Call `{name}(scene, time)` from `update`, replace the `todo!`, and make the scenario check real values."),
    }))
}

/// Every leaf of `value` keyed by its JSON pointer path.
fn flatten_leaves(
    value: &Value,
    path: String,
    out: &mut BTreeMap<String, Value>,
) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, inner) in map {
                flatten_leaves(inner, format!("{path}/{key}"), out);
            }
        }
        Value::Array(items)
            if !items.is_empty() && items.iter().any(|i| i.is_object()) =>
        {
            for (index, inner) in items.iter().enumerate() {
                flatten_leaves(inner, format!("{path}/{index}"), out);
            }
        }
        leaf => {
            out.insert(path, leaf.clone());
        }
    }
}

fn leaf_changes(before: &Value, after: &Value) -> Vec<Value> {
    let (mut a, mut b) = (BTreeMap::new(), BTreeMap::new());
    flatten_leaves(before, String::new(), &mut a);
    flatten_leaves(after, String::new(), &mut b);
    let paths: BTreeSet<_> = a.keys().chain(b.keys()).cloned().collect();
    paths
        .into_iter()
        .filter(|path| a.get(path) != b.get(path))
        .map(|path| json!({"path": path, "before": a.get(&path), "after": b.get(&path)}))
        .collect()
}

/// `rusting diff`: entities added, removed and changed (by ID) between two
/// scenes, with the path, old value and new value of every changed field.
pub fn diff_scenes(before: &Path, after: &Path) -> CliResult {
    let mut documents = Vec::new();
    for path in [before, after] {
        match read_scene(path) {
            Ok(document) => documents
                .push(serde_json::to_value(document).unwrap_or(Value::Null)),
            Err(result) => return result,
        }
    }
    let by_id = |document: &Value| -> BTreeMap<String, Value> {
        document["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|entity| {
                (
                    entity["id"].as_str().unwrap_or("").to_owned(),
                    entity.clone(),
                )
            })
            .collect()
    };
    let (old, new) = (by_id(&documents[0]), by_id(&documents[1]));
    let brief =
        |entity: &Value| json!({"id": entity["id"], "name": entity["name"]});
    let added: Vec<_> = new
        .iter()
        .filter(|(id, _)| !old.contains_key(*id))
        .map(|(_, e)| brief(e))
        .collect();
    let removed: Vec<_> = old
        .iter()
        .filter(|(id, _)| !new.contains_key(*id))
        .map(|(_, e)| brief(e))
        .collect();
    let changed: Vec<_> = old
        .iter()
        .filter_map(|(id, entity)| {
            let changes = leaf_changes(entity, new.get(id)?);
            (!changes.is_empty()).then(|| json!({"id": id, "name": new[id]["name"], "changes": changes}))
        })
        .collect();
    let mut scene_a = documents[0].clone();
    let mut scene_b = documents[1].clone();
    scene_a["entities"] = Value::Null;
    scene_b["entities"] = Value::Null;
    let scene = leaf_changes(&scene_a, &scene_b);
    CliResult::success(json!({
        "identical": added.is_empty() && removed.is_empty() && changed.is_empty() && scene.is_empty(),
        "scene": scene,
        "added": added,
        "removed": removed,
        "changed": changed,
    }))
}

/// `rusting scene map`: each tile map as a character grid with a legend.
/// Other entities that sit over a map are drawn as letters, so a model
/// without vision can check a 2D layout.
pub fn map_scene(path: &Path) -> CliResult {
    let document = match read_scene(path) {
        Ok(document) => document,
        Err(result) => return result,
    };
    let origin = |entity: &SceneEntity| {
        entity
            .transform
            .map_or([0.0, 0.0], |t| [t.position[0], t.position[1]])
    };
    let mut maps = Vec::new();
    for entity in &document.entities {
        let Some(text) =
            entity.components.get(crate::runtime::TILE_MAP_COMPONENT)
        else {
            continue;
        };
        let Ok(tile_map) =
            serde_json::from_str::<crate::runtime::TileMap>(text)
        else {
            continue;
        };
        let mut grid: Vec<Vec<char>> = tile_map
            .rows
            .iter()
            .map(|row| row.chars().collect())
            .collect();
        let mut legend: Vec<Value> = tile_map
            .tiles
            .iter()
            .map(|(key, kind)| json!({"symbol": key, "meaning": if kind.solid { "solid tile" } else { "tile" }}))
            .collect();
        let mut markers = ('A'..='Z')
            .filter(|c| !tile_map.tiles.contains_key(&c.to_string()));
        let base = origin(entity);
        for other in document
            .entities
            .iter()
            .filter(|o| o.id != entity.id && o.transform.is_some())
        {
            let at = origin(other);
            let column = ((at[0] - base[0]) / tile_map.tile_size).floor();
            let row = (-(at[1] - base[1]) / tile_map.tile_size).floor();
            let in_grid = column >= 0.0
                && row >= 0.0
                && grid
                    .get(row as usize)
                    .is_some_and(|r| (column as usize) < r.len());
            let Some(marker) = in_grid.then(|| markers.next()).flatten() else {
                continue;
            };
            grid[row as usize][column as usize] = marker;
            legend.push(json!({"symbol": marker.to_string(), "entity": other.name, "id": other.id, "column": column as usize, "row": row as usize}));
        }
        maps.push(json!({
            "entity": entity.name,
            "id": entity.id,
            "tile_size": tile_map.tile_size,
            "rows": grid.iter().map(|row| row.iter().collect::<String>()).collect::<Vec<_>>(),
            "legend": legend,
        }));
    }
    CliResult::success(json!({"maps": maps}))
}

/// `rusting inspect --tick N`: runs the game without a window to tick `N`
/// and reports the scene form of each named entity (all named entities of
/// the main scene when `entities` is empty) after that tick's update.
pub fn inspect_tick(root: &Path, tick: u32, entities: &[String]) -> CliResult {
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let names: Vec<String> = if entities.is_empty() {
        match read_scene(&project.scene_path) {
            Ok(document) => document
                .entities
                .iter()
                .filter_map(|entity| entity.name.clone())
                .collect(),
            Err(result) => return result,
        }
    } else {
        entities.to_vec()
    };
    let steps: Vec<_> = names
        .iter()
        .map(|name| json!({"tick": tick, "log": {"entity": name, "path": ""}}))
        .collect();
    let scenario = json!({"name": format!("inspect tick {tick}"), "ticks": tick, "steps": steps});
    let file = project.root.join("build/inspect-tick.json");
    let written = std::fs::create_dir_all(project.root.join("build"))
        .and_then(|()| std::fs::write(&file, scenario.to_string()));
    if let Err(error) = written {
        return CliResult::failure("IO_ERROR", error.to_string(), Some(file));
    }
    let mut result = run_game_project(
        root,
        RunOptions {
            scenario: Some(file),
            ..RunOptions::default()
        },
    );
    if result.ok {
        let state: serde_json::Map<String, Value> = result.data["scenario"]
            ["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .zip(&names)
            .map(|(step, name)| (name.clone(), step["actual"].clone()))
            .collect();
        result.data = json!({"tick": tick, "entities": state});
    }
    result
}

/// Validates the project and compiles it with the debug build that `run`
/// and `test` reuse.
pub fn check_project(root: &Path) -> CliResult {
    let validation = validate_project(root);
    if !validation.ok {
        return validation;
    }
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    // `cargo build`, not `cargo check`: the debug build is the one `run` and
    // `test` reuse, so checking first costs no second engine compile.
    match cargo(&project, "build", false, None) {
        Ok(run) => {
            let mut result = CliResult::success(
                json!({"root": project.root, "cargo": run.data()}),
            );
            result.diagnostics.extend(outdated_cli(&project.root));
            result.diagnostics.extend(outdated_agents(&project.root));
            result
        }
        Err(result) => result,
    }
}

/// The AGENTS.md `rusting new` writes.
const PROJECT_AGENTS: &str = include_str!("project_agents.md");

/// An `AGENTS_OUTDATED` warning when the project's AGENTS.md is an engine
/// copy (it opens like one) that differs from this engine's.
fn outdated_agents(root: &Path) -> Option<Diagnostic> {
    let path = root.join("AGENTS.md");
    let text = std::fs::read_to_string(&path).ok()?;
    let opening = "# Agent guide\n\nThis is a Rusting game project";
    (text.starts_with(opening) && text != PROJECT_AGENTS).then(|| Diagnostic {
        code: "AGENTS_OUTDATED",
        severity: "warning",
        message: "AGENTS.md differs from this engine's copy, so it may miss \
                  newer features; `rusting fix` replaces it and keeps yours \
                  as AGENTS.md.old"
            .into(),
        file: Some(path),
        ..Diagnostic::default()
    })
}

/// A `CLI_OUTDATED` warning when the game's `rusting_engine` has another
/// version than this CLI, or is a path dependency with source newer than
/// this executable (an engine edited after `cargo install`).
fn outdated_cli(root: &Path) -> Option<Diagnostic> {
    // Cargo.lock lists `name`, then `version`, then `source` only for a
    // registry or git package.
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).ok()?;
    let package = lock
        .split("[[package]]")
        .find(|package| package.contains("name = \"rusting_engine\""))?;
    let version = package
        .lines()
        .find_map(|line| line.strip_prefix("version = "))?
        .trim_matches('"');
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let engine_path = manifest
        .lines()
        .find(|line| line.trim_start().starts_with("rusting_engine"))
        .and_then(|line| line.split("path = \"").nth(1))
        .and_then(|rest| rest.split('"').next())
        .filter(|_| !package.contains("source = "));
    let install = engine_path.map_or_else(
        || "cargo install rusting_engine --locked".to_owned(),
        |path| {
            format!(
                "cargo install --path {} --locked",
                root.join(path).display()
            )
        },
    );
    let message = if version != env!("CARGO_PKG_VERSION") {
        format!(
            "this CLI is RustingEngine {}, the game builds against {version}",
            env!("CARGO_PKG_VERSION")
        )
    } else {
        let source = root.join(engine_path?).join("src");
        let installed = std::env::current_exe()
            .ok()?
            .metadata()
            .ok()?
            .modified()
            .ok()?;
        let (newest, file) = newest_modified(&source)?;
        let later = newest
            .duration_since(installed)
            .ok()
            .filter(|later| !later.is_zero())?
            .as_secs();
        let later = match later {
            0..3600 => format!("{} min", later / 60),
            3600..86_400 => format!("{} h", later / 3600),
            _ => format!("{} days", later / 86_400),
        };
        format!(
            "the engine source changed after this CLI was built: {} is {later} newer",
            file.display()
        )
    };
    Some(Diagnostic {
        code: "CLI_OUTDATED",
        severity: "warning",
        message: format!(
            "{message}; docs and schema may be stale. Reinstall: {install}"
        ),
        ..Diagnostic::default()
    })
}

/// The newest file under `folder` and when it changed.
fn newest_modified(folder: &Path) -> Option<(std::time::SystemTime, PathBuf)> {
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                newest_modified(&path)
            } else {
                Some((entry.metadata().ok()?.modified().ok()?, path))
            }
        })
        .max()
}

/// How `rusting run` and `rusting test` start the game.
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    pub release: bool,
    /// Run this many ticks without a window, then exit.
    pub headless_ticks: Option<u32>,
    /// Stop a game still running after this long. Reaching it is not a
    /// failure: the game ran without crashing until then.
    pub timeout: Option<Duration>,
    /// Run this scenario file instead, and fail when it fails.
    pub scenario: Option<PathBuf>,
    /// Record the windowed session's input to this replay file on exit.
    pub record: Option<PathBuf>,
    /// Play this replay file back headless, failing where a tick's state
    /// hash differs from the recording.
    pub replay: Option<PathBuf>,
    /// Rewrite scenario `golden` images instead of comparing them.
    pub update_golden: bool,
    /// Run the scenario to its last tick after a failed check.
    pub keep_going: bool,
}

/// Cooks the main scene, builds the game, and runs it as a debug session:
/// the game's standard input and output carry the line protocol of
/// [`crate::debug_session`]. Returns only a failure; on success the game
/// ran until `quit` or end of input.
pub fn debug_game_project(root: &Path) -> CliResult {
    let cooked = cook_project(root);
    if !cooked.ok {
        return cooked;
    }
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    if let Err(result) = cargo(&project, "build", false, None) {
        return result;
    }
    let executable = match crate::project::built_executable(
        &project.root,
        &project.root.join("Cargo.toml"),
        &project.manifest.binary_name,
        None,
        false,
    ) {
        Ok(executable) => executable,
        Err(error) => return CliResult::failure("BUILD_FAILED", error, None),
    };
    match Command::new(&executable)
        .current_dir(&project.root)
        .env(crate::debug_session::DEBUG_SESSION_ENV, "1")
        .status()
    {
        Ok(status) if status.success() => CliResult::success(json!({})),
        Ok(status) => CliResult::failure(
            "GAME_FAILED",
            format!("the game exited with {status}"),
            Some(executable),
        ),
        Err(error) => CliResult::failure(
            "GAME_FAILED",
            format!("could not start {}: {error}", executable.display()),
            Some(executable),
        ),
    }
}

/// Cooks the main scene, builds the game, and runs it from the project
/// folder, the same way the editor's Build and Run does.
pub fn run_game_project(root: &Path, options: RunOptions) -> CliResult {
    let start = Instant::now();
    let headless =
        options.headless_ticks.is_some() || options.scenario.is_some();
    if options.record.is_some() && (headless || options.replay.is_some()) {
        return CliResult::failure(
            "CLI_USAGE",
            "--record records a windowed session; it cannot be combined with \
             --ticks, --scenario or --replay (a scenario file is already a \
             recording)",
            None,
        );
    }
    if let Some(replay) = &options.replay {
        if headless {
            return CliResult::failure(
                "CLI_USAGE",
                "--replay runs headless on its own; drop --ticks and --scenario",
                None,
            );
        }
        if !replay.is_file() {
            return CliResult::failure(
                "FILE_NOT_FOUND",
                format!("no replay file at {}", replay.display()),
                Some(replay.clone()),
            );
        }
    }
    let cooked = cook_project(root);
    if !cooked.ok {
        return cooked;
    }
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let build = match cargo(&project, "build", options.release, None) {
        Ok(build) => build,
        Err(result) => return result,
    };
    let executable = match crate::project::built_executable(
        &project.root,
        &project.root.join("Cargo.toml"),
        &project.manifest.binary_name,
        None,
        options.release,
    ) {
        Ok(executable) => executable,
        Err(error) => return CliResult::failure("BUILD_FAILED", error, None),
    };
    let mut game = Command::new(&executable);
    game.current_dir(&project.root);
    let final_scene = project.root.join("build/final.rscene");
    if let Some(ticks) = options.headless_ticks {
        let _ = std::fs::remove_file(&final_scene);
        game.env(crate::project::HEADLESS_TICKS_ENV, ticks.to_string())
            .env(crate::project::FINAL_SCENE_OUT_ENV, &final_scene);
    }
    for (path, variable) in [
        (&options.record, crate::project::REPLAY_OUT_ENV),
        (&options.replay, crate::project::REPLAY_PLAY_ENV),
    ] {
        if let Some(path) = path {
            // The game runs in the project root, not the caller's folder.
            game.env(variable, std::path::absolute(path).unwrap_or_default());
        }
    }
    let report_path = project.root.join("build/scenario-report.json");
    if let Some(scenario) = &options.scenario {
        let scenario = match std::path::absolute(scenario) {
            Ok(scenario) if scenario.is_file() => scenario,
            _ => {
                return CliResult::failure(
                    "FILE_NOT_FOUND",
                    format!("no scenario file at {}", scenario.display()),
                    Some(scenario.clone()),
                )
            }
        };
        let _ = std::fs::remove_file(&report_path);
        // Each scenario starts with an empty data folder of its own, so
        // saves from one run never leak into the next.
        let user_data = project
            .root
            .join("build/test-userdata")
            .join(scenario.file_stem().unwrap_or_default());
        let _ = std::fs::remove_dir_all(&user_data);
        game.env(crate::project::USER_DATA_ENV, user_data);
        game.env(crate::scenario::TEST_SCENARIO_ENV, scenario)
            .env(crate::scenario::TEST_REPORT_ENV, &report_path);
        if options.update_golden {
            game.env(crate::scenario::UPDATE_GOLDEN_ENV, "1");
        }
        if options.keep_going {
            game.env(crate::scenario::KEEP_GOING_ENV, "1");
        }
    }
    // A recording game closes itself at the timeout so it can save; the
    // kill comes later, for a game that hangs.
    let mut timeout = options.timeout;
    if let (Some(limit), Some(_)) = (timeout, &options.record) {
        game.env(
            crate::project::QUIT_AFTER_MS_ENV,
            limit.as_millis().to_string(),
        );
        timeout = Some(limit + Duration::from_secs(10));
    }
    let launched_after = start.elapsed();
    let run = match run_process(&mut game, timeout) {
        Ok(run) => run,
        Err(error) => {
            return CliResult::failure(
                "GAME_FAILED",
                format!("could not start {}: {error}", executable.display()),
                Some(executable),
            )
        }
    };
    let first_frame = crate::project::first_frame_ms(&run.stderr);
    let mut data = json!({
        "root": project.root,
        "executable": executable,
        "headless_ticks": options.headless_ticks,
        "build": {"duration_ms": build.duration.as_millis() as u64},
        "game": run.data(),
        "timings": {
            "build_ms": build.duration.as_millis() as u64,
            "game_first_frame_ms": first_frame,
            // Includes loading the scene; a debug build shows here first.
            "headless_ms_per_tick": crate::project::tick_time_ms(&run.stderr),
            "command_to_first_frame_ms": first_frame
                .map(|ms| launched_after.as_millis() as u64 + ms),
        },
    });
    // Game output can only hold asset reload failures, not build errors.
    let mut asset_warnings = reload_diagnostics(&project.root, &run.stderr);
    asset_warnings
        .retain(|diagnostic| diagnostic.code == "ASSET_RELOAD_FAILED");
    let mut result = 'result: {
        if let Some(scenario) = options.scenario {
            let report =
                std::fs::read_to_string(&report_path).ok().and_then(|text| {
                    serde_json::from_str::<crate::scenario::ScenarioReport>(
                        &text,
                    )
                    .ok()
                });
            let Some(report) = report else {
                let mut result = CliResult::failure(
                    "GAME_FAILED",
                    format!(
                        "game wrote no scenario report:\n{}",
                        tail(&run.stderr, 40)
                    ),
                    Some(scenario),
                );
                result.data = data;
                break 'result result;
            };
            data["scenario"] = json!(report);
            let Some(failure) = &report.first_failure else {
                break 'result CliResult::success(data);
            };
            let mut message = format!(
                "tick {} step {}: {}",
                failure.tick, failure.step, failure.message
            );
            if !run.stderr.trim().is_empty() {
                message += &format!(
                    "\ngame stderr (last 20 lines):\n{}",
                    tail(&run.stderr, 20)
                );
            }
            let mut result =
                CliResult::failure("SCENARIO_FAILED", message, Some(scenario));
            result.data = data;
            break 'result result;
        }
        if final_scene.is_file() && options.headless_ticks.is_some() {
            data["final_scene"] = json!(final_scene);
        }
        if run.success || run.timed_out {
            break 'result CliResult::success(data);
        }
        let mut result = CliResult::failure(
            "GAME_FAILED",
            format!(
                "game exited with {}:\n{}",
                run.code
                    .map_or("a signal".into(), |code| format!("code {code}")),
                tail(&run.stderr, 40)
            ),
            Some(executable),
        );
        result.data = data;
        result
    };
    result.diagnostics.extend(asset_warnings);
    result
}

/// Runs every `.json` scenario in `folder`, in name order, and fails when
/// any of them fails.
pub fn test_game_folder(
    root: &Path,
    folder: &Path,
    options: RunOptions,
) -> CliResult {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect(),
        Err(error) => {
            return CliResult::failure(
                "FILE_NOT_FOUND",
                format!("cannot read {}: {error}", folder.display()),
                Some(folder.to_path_buf()),
            )
        }
    };
    files.sort();
    if files.is_empty() {
        return CliResult::failure(
            "FILE_NOT_FOUND",
            format!("no scenario files in {}", folder.display()),
            Some(folder.to_path_buf()),
        );
    }
    let mut runs = Vec::new();
    let mut first_failure = None;
    for file in files {
        let result = run_game_project(
            root,
            RunOptions {
                scenario: Some(file.clone()),
                ..options.clone()
            },
        );
        let message = match result.diagnostics.first() {
            Some(diagnostic) if !result.ok => diagnostic.message.clone(),
            _ => format!(
                "{} after {} ticks",
                result.data["scenario"]["name"],
                result.data["scenario"]["ticks_run"]
            ),
        };
        let logs: Vec<_> = result.data["scenario"]["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|step| {
                step["message"].as_str().is_some_and(|message| {
                    message.starts_with("log: ") || message.contains("; warning: ")
                })
            })
            .map(|step| json!({"tick": step["tick"], "message": step["message"]}))
            .collect();
        runs.push(json!({
            "file": file,
            "ok": result.ok,
            "message": message,
            "logs": logs,
        }));
        if !result.ok && first_failure.is_none() {
            first_failure = Some(result);
        }
    }
    let data = json!({"root": root, "scenarios": runs});
    match first_failure {
        None => CliResult::success(data),
        Some(mut result) => {
            result.data = data;
            result
        }
    }
}

/// One way to build and run the game for [`check_game_determinism`].
struct DeterminismConfig {
    name: &'static str,
    release: bool,
    /// Pin the game to one CPU with `taskset`, so bevy and rayon start one
    /// worker thread instead of one per core.
    one_cpu: bool,
}

/// Builds the game in each configuration, runs it headless for `ticks`
/// ticks, and compares every tick's state hash with the first
/// configuration's: debug, release, and release on one CPU when `taskset`
/// is available. At the first divergence both runs repeat up to that tick
/// to name the first divergent entity. GPU-class bodies do not simulate
/// headless, so vendor and driver differences are not covered.
pub fn check_game_determinism(root: &Path, ticks: u32) -> CliResult {
    let cooked = cook_project(root);
    if !cooked.ok {
        return cooked;
    }
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    let mut configs = vec![
        DeterminismConfig {
            name: "debug",
            release: false,
            one_cpu: false,
        },
        DeterminismConfig {
            name: "release",
            release: true,
            one_cpu: false,
        },
    ];
    if tool_available("taskset") {
        configs.push(DeterminismConfig {
            name: "release-one-cpu",
            release: true,
            one_cpu: true,
        });
    }
    let out_dir = project.root.join("build/determinism");
    if let Err(error) = std::fs::create_dir_all(&out_dir) {
        return CliResult::failure(
            "IO_ERROR",
            error.to_string(),
            Some(out_dir),
        );
    }
    let run = |config: &DeterminismConfig, ticks: u32| {
        let executable = crate::project::built_executable(
            &project.root,
            &project.root.join("Cargo.toml"),
            &project.manifest.binary_name,
            None,
            config.release,
        )
        .map_err(|error| CliResult::failure("BUILD_FAILED", error, None))?;
        let report_path = out_dir.join(format!("{}-{ticks}.json", config.name));
        let _ = std::fs::remove_file(&report_path);
        let mut game = if config.one_cpu {
            let mut game = Command::new("taskset");
            game.args(["--cpu-list", "0"]).arg(&executable);
            game
        } else {
            Command::new(&executable)
        };
        game.current_dir(&project.root)
            .env(crate::project::HEADLESS_TICKS_ENV, ticks.to_string())
            .env(crate::project::STATE_HASH_OUT_ENV, &report_path);
        let output = run_process(&mut game, None).map_err(|error| {
            CliResult::failure(
                "GAME_FAILED",
                format!("could not start {}: {error}", executable.display()),
                Some(executable.clone()),
            )
        })?;
        std::fs::read_to_string(&report_path)
            .ok()
            .and_then(|text| {
                serde_json::from_str::<crate::runtime::StateHashReport>(&text)
                    .ok()
            })
            .ok_or_else(|| {
                CliResult::failure(
                    "GAME_FAILED",
                    format!(
                        "{} run wrote no state hashes:\n{}",
                        config.name,
                        tail(&output.stderr, 40)
                    ),
                    Some(executable),
                )
            })
    };
    let mut reports = Vec::new();
    for config in &configs {
        if let Err(result) = cargo(&project, "build", config.release, None) {
            return result;
        }
        match run(config, ticks) {
            Ok(report) => reports.push(report),
            Err(result) => return result,
        }
    }
    let mut data = json!({
        "root": project.root,
        "ticks": ticks,
        "configurations": configs
            .iter()
            .zip(&reports)
            .map(|(config, report)| json!({
                "name": config.name,
                "final_hash": report.ticks.last().map(|tick| tick.1),
            }))
            .collect::<Vec<_>>(),
    });
    for (config, report) in configs.iter().zip(&reports).skip(1) {
        let Some(tick) = crate::runtime::first_divergent_tick(
            &reports[0].ticks,
            &report.ticks,
        ) else {
            continue;
        };
        // Repeat both runs up to the divergent tick for entity hashes there.
        let tick_count = u32::try_from(tick).unwrap_or(ticks);
        let entity =
            match (run(&configs[0], tick_count), run(config, tick_count)) {
                (Ok(first), Ok(second)) => {
                    crate::runtime::first_divergent_entity(
                        &first.entities,
                        &second.entities,
                    )
                }
                (Err(result), _) | (_, Err(result)) => return result,
            };
        data["divergence"] = json!({
            "configurations": [configs[0].name, config.name],
            "tick": tick,
            "entity": entity,
        });
        let mut result = CliResult::failure(
            "DETERMINISM_DIVERGED",
            format!(
                "{} and {} diverge at tick {tick}{}",
                configs[0].name,
                config.name,
                entity
                    .and_then(|entity| entity.name)
                    .map(|name| format!(", first in `{name}`"))
                    .unwrap_or_default()
            ),
            None,
        );
        result.data = data;
        return result;
    }
    CliResult::success(data)
}

/// Runs a scenario in a debug and then a release build and compares the
/// CPU state hash of every tick, plus the GPU body hash of every tick both
/// runs delivered. `gpu` requires GPU hashes. Same machine and driver only:
/// it finds unseeded randomness and order-dependent code, not vendor
/// differences.
pub fn check_scenario_determinism(
    root: &Path,
    scenario: &Path,
    gpu: bool,
) -> CliResult {
    let mut runs = Vec::new();
    let mut scenario_passed = true;
    for (name, release) in [("debug", false), ("release", true)] {
        eprintln!("determinism: building and running the {name} build...");
        let result = run_game_project(
            root,
            RunOptions {
                release,
                scenario: Some(scenario.to_path_buf()),
                keep_going: true,
                ..RunOptions::default()
            },
        );
        // A failed expect step still leaves hashes worth comparing: a stale
        // pinned hash is exactly what this check helps to re-pin.
        let hashes = |key: &str| -> BTreeMap<u64, u64> {
            serde_json::from_value::<Vec<(u64, u64)>>(
                result.data["scenario"][key].clone(),
            )
            .unwrap_or_default()
            .into_iter()
            .collect()
        };
        let (cpu, gpu_hashes) =
            (hashes("state_hashes"), hashes("gpu_state_hashes"));
        if cpu.is_empty() && gpu_hashes.is_empty() && !result.ok {
            return result;
        }
        scenario_passed &= result.ok;
        if gpu && gpu_hashes.is_empty() {
            return CliResult::failure(
                "DETERMINISM_NO_GPU_STATE",
                "the scenario recorded no GPU state hashes; set \"gpu\": true in it and give it GPU physics bodies",
                Some(scenario.to_path_buf()),
            );
        }
        runs.push((cpu, gpu_hashes));
    }
    let compare = |first: &BTreeMap<u64, u64>, second: &BTreeMap<u64, u64>| {
        first
            .iter()
            .filter_map(|(tick, hash)| {
                second.get(tick).map(|other| (*tick, *hash, *other))
            })
            .collect::<Vec<_>>()
    };
    let cpu = compare(&runs[0].0, &runs[1].0);
    let gpu_compared = compare(&runs[0].1, &runs[1].1);
    let mut data = json!({
        "root": root,
        "scenario": scenario,
        "ticks_compared": cpu.len(),
        "final_hash": cpu.last().map(|entry| entry.1),
        "gpu_ticks_compared": gpu_compared.len(),
        "gpu_final_hash": gpu_compared.last().map(|entry| entry.1),
        "scenario_passed": scenario_passed,
    });
    let diverged = |hashes: &[(u64, u64, u64)]| {
        hashes
            .iter()
            .find(|(_, first, second)| first != second)
            .map(|entry| entry.0)
    };
    let divergence = diverged(&cpu)
        .map(|tick| (tick, "CPU world state"))
        .or_else(|| diverged(&gpu_compared).map(|tick| (tick, "GPU bodies")));
    match divergence {
        None => {
            let run = runs[0].0.keys().last().copied().unwrap_or_default();
            data["ticks_run"] = json!(run);
            data["verdict"] = json!(format!(
                "deterministic: debug == release on all {run} ticks (the hash at each tick covers the whole state){}{}",
                cpu.last()
                    .map(|entry| format!(", final hash {:#018x}", entry.1))
                    .unwrap_or_default(),
                if scenario_passed { "" } else { "; the scenario itself failed" }
            ));
            CliResult::success(data)
        }
        Some((tick, state)) => {
            data["divergence"] = json!({"tick": tick, "state": state});
            let mut result = CliResult::failure(
                "DETERMINISM_DIVERGED",
                format!(
                    "the debug and release runs of the scenario diverge at tick {tick} ({state})"
                ),
                Some(scenario.to_path_buf()),
            );
            result.data = data;
            result
        }
    }
}

/// Ticks the exported game runs headless to verify it.
const EXPORT_VERIFY_TICKS: u32 = 60;

/// Cooks, builds a release, and packages the game into a new folder under
/// `parent`, the same way the editor's Export does. A native export is then
/// verified: a copy of the folder in a fresh temporary location must run
/// [`EXPORT_VERIFY_TICKS`] headless ticks and exit cleanly. Cross-target
/// exports are not run.
pub fn export_game_project(
    root: &Path,
    parent: &Path,
    target: Option<&str>,
) -> CliResult {
    let cooked = cook_project(root);
    if !cooked.ok {
        return cooked;
    }
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    if let Err(result) = cargo(&project, "build", true, target) {
        return result;
    }
    let exported = match crate::project::export_built_game(
        &project.root,
        &project.root.join("Cargo.toml"),
        parent,
        &project.manifest.name,
        &project.manifest.binary_name,
        &project.manifest.cooked_scene,
        target,
    ) {
        Ok(exported) => exported,
        Err(error) => {
            return CliResult::failure(
                "EXPORT_FAILED",
                error,
                Some(parent.to_owned()),
            )
        }
    };
    let mut files: Vec<_> = walk_files(&exported)
        .into_iter()
        .filter_map(|file| {
            file.strip_prefix(&exported).ok().map(Path::to_path_buf)
        })
        .collect();
    files.sort();
    let mut data = json!({
        "root": project.root,
        "export_path": exported,
        "target": target,
        "files": files,
        "verified": null,
    });
    if target.is_some() {
        let mut result = CliResult::success(data);
        result.diagnostics.push(Diagnostic {
            code: "EXPORT_NOT_VERIFIED",
            severity: "warning",
            message: "cross-target exports are not run on this machine".into(),
            file: Some(exported),
            ..Diagnostic::default()
        });
        return result;
    }
    let verified = verify_export(&exported, &project.manifest.binary_name);
    let ok = verified.as_ref().is_ok_and(|run| run.success);
    data["verified"] = match &verified {
        Ok(run) => run.data(),
        Err(error) => json!({"error": error}),
    };
    if ok {
        return CliResult::success(data);
    }
    let message = match &verified {
        Ok(run) => format!(
            "exported game failed its headless run:\n{}",
            tail(&run.stderr, 40)
        ),
        Err(error) => format!("could not verify the export: {error}"),
    };
    let mut result =
        CliResult::failure("EXPORT_VERIFY_FAILED", message, Some(exported));
    result.data = data;
    result
}

/// Runs a copy of the export from a fresh temporary folder, so it cannot
/// read files through the export's own path.
fn verify_export(
    exported: &Path,
    binary_name: &str,
) -> Result<ProcessRun, String> {
    let isolated = std::env::temp_dir()
        .join(format!("rusting-export-verify-{}", Uuid::new_v4()));
    let result =
        crate::project::copy_directory(exported, &isolated).and_then(|()| {
            let executable = isolated
                .join(crate::project::executable_name(binary_name, None));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    &executable,
                    std::fs::Permissions::from_mode(0o755),
                )
                .map_err(|error| error.to_string())?;
            }
            let mut game = Command::new(&executable);
            game.current_dir(&isolated)
                .env(
                    crate::project::HEADLESS_TICKS_ENV,
                    EXPORT_VERIFY_TICKS.to_string(),
                )
                .env_remove("RUSTING_SCENE_PATH");
            run_process(&mut game, Some(Duration::from_secs(120)))
                .map_err(|error| error.to_string())
        });
    let _ = std::fs::remove_dir_all(&isolated);
    result
}

fn walk_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                walk_files(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

/// What `rusting capture` renders.
#[derive(Clone, Debug)]
pub struct CaptureOptions {
    /// Camera by persistent ID or name; `None` uses the scene's active
    /// camera, as the game would.
    pub camera: Option<String>,
    /// Fixed ticks to simulate before the captured frame.
    pub tick: u32,
    pub extent: [u32; 2],
    /// PNG destination. Not written when Vulkan is unavailable.
    pub output: PathBuf,
    /// Pixels, from the top-left corner, to map to scene objects.
    pub picks: Vec<[u32; 2]>,
    /// Rectangles `[x, y, width, height]` whose covering objects are listed.
    pub pick_rects: Vec<[u32; 4]>,
    /// False leaves the HUD and other runtime UI out (`--no-hud`).
    pub hud: bool,
}

/// Loads a scene, simulates `tick` fixed ticks, and renders one camera
/// offscreen to a PNG. Every tick is rendered, so GPU physics advances as in
/// the game; game code from the project is not run. Picks and camera data
/// come from the CPU, so they are reported, with a `VULKAN_UNAVAILABLE`
/// error, even when no Vulkan device exists.
pub fn capture_scene(scene: &Path, options: &CaptureOptions) -> CliResult {
    use crate::rendering::capture::HeadlessCapture;
    use crate::runtime::{
        load_scene, HybridPhysicsPlugin, RenderCameraOverride,
        RenderExtractPlugin, SceneLoadMode,
    };
    use crate::{App, AssetPlugin};

    let [width, height] = options.extent;
    if width == 0 || height == 0 {
        return CliResult::failure(
            "CLI_USAGE",
            "capture size must be at least 1x1",
            None,
        );
    }
    if let Some(&[x, y]) = options
        .picks
        .iter()
        .find(|[x, y]| *x >= width || *y >= height)
    {
        return CliResult::failure(
            "CLI_USAGE",
            format!("pick {x},{y} is outside the {width}x{height} capture"),
            None,
        );
    }
    let document = match read_scene(scene) {
        Ok(document) => document,
        Err(result) => return result,
    };
    let warnings = scene_warnings(scene, &document);

    let mut app = App::new();
    if let Err(error) = app
        .add_plugin(AssetPlugin)
        .and_then(|app| app.add_plugin(HybridPhysicsPlugin))
        .and_then(|app| app.add_plugin(RenderExtractPlugin))
    {
        return CliResult::failure("CAPTURE_FAILED", error.to_string(), None);
    }
    // Game code does not run during a capture, so its components are kept
    // as data rather than rejected.
    app.world_mut()
        .resource_mut::<crate::runtime::SceneComponentRegistry>()
        .keep_unregistered();
    if let Err(error) =
        load_scene(app.world_mut(), scene, SceneLoadMode::Replace)
    {
        return scene_error(error, scene);
    }
    if let Some(wanted) = &options.camera {
        let Some(camera) =
            crate::scenario::find_camera(app.world_mut(), wanted)
        else {
            return CliResult::failure(
                "CAMERA_NOT_FOUND",
                format!("no camera has the ID or name `{wanted}`"),
                Some(scene.to_owned()),
            );
        };
        app.world_mut()
            .resource_mut::<RenderCameraOverride>()
            .entity = Some(camera);
    }

    let mut gpu_error = None;
    let mut capture = match HeadlessCapture::new(options.extent) {
        Ok(capture) => Some(capture),
        Err(error) => {
            gpu_error = Some(("VULKAN_UNAVAILABLE", error));
            None
        }
    };
    for tick in 0..=options.tick {
        let delta = crate::scenario::tick_delta(&app, tick);
        let stepped = match capture.as_mut() {
            Some(capture) => capture.frame(&mut app, delta),
            None => app
                .update(delta)
                .map(drop)
                .map_err(|error| error.to_string()),
        };
        if let Err(error) = stepped {
            return CliResult::failure(
                "CAPTURE_FAILED",
                format!("tick {tick}: {error}"),
                None,
            );
        }
    }
    let rgba = match capture.as_mut() {
        Some(capture) if !options.hud => {
            match capture.view_rgba(app.world(), None) {
                Ok(rgba) => Some(rgba),
                Err(error) => {
                    return CliResult::failure("CAPTURE_FAILED", error, None)
                }
            }
        }
        capture => capture.map(|capture| capture.rgba()),
    };
    let render = capture
        .as_mut()
        .map_or(Value::Null, |capture| capture.metadata(&app));
    let Some(view) =
        crate::scenario::CameraView::active(app.world(), options.extent)
    else {
        return CliResult::failure(
            "CAMERA_NOT_FOUND",
            "the scene has no active camera; pass --camera",
            Some(scene.to_owned()),
        );
    };
    let picks: Vec<_> = options
        .picks
        .iter()
        .map(|&pixel| {
            let mut pick = view.pick(app.world_mut(), pixel);
            pick["color"] = json!(rgba.as_ref().map(|rgba| {
                let at = ((pixel[1] * width + pixel[0]) * 4) as usize;
                [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
            }));
            pick
        })
        .collect();
    let pick_rects: Vec<_> = options
        .pick_rects
        .iter()
        .map(|&[x, y, w, h]| {
            view.pick_rect(app.world_mut(), [x, y, x + w, y + h])
        })
        .collect();
    let mut data = json!({
        "scene": scene,
        "tick": options.tick,
        "extent": options.extent,
        "output": Value::Null,
        "camera": view.data(app.world()),
        "render": render,
        "pick_method": "mesh_bounds",
        "picks": picks,
        "pick_rects": pick_rects,
    });

    let mut result = match (gpu_error, capture) {
        (Some((code, error)), _) => {
            let mut result = CliResult::failure(code, error, None);
            result.data = data;
            result
        }
        (None, Some(_)) => {
            let saved = crate::rendering::capture::save_rgba(
                &options.output,
                rgba.as_deref().unwrap_or_default(),
                options.extent,
            );
            if let Err(error) = saved {
                return CliResult::failure(
                    "CAPTURE_FAILED",
                    error,
                    Some(options.output.clone()),
                );
            }
            data["output"] = json!(options.output);
            CliResult::success(data)
        }
        (None, None) => unreachable!("a capture exists without a GPU error"),
    };
    result.diagnostics.extend(warnings);
    result
}

#[cfg(test)]
mod shape_tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_old_engine_agents_md_is_flagged_and_refreshed() {
        let root = std::env::temp_dir()
            .join(format!("rusting-agents-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("AGENTS.md");
        std::fs::write(
            &path,
            "# Agent guide\n\nThis is a Rusting game project. Old.",
        )
        .unwrap();
        assert_eq!(outdated_agents(&root).unwrap().code, "AGENTS_OUTDATED");
        let _ = fix_project(&root, false);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PROJECT_AGENTS);
        assert!(std::fs::read_to_string(root.join("AGENTS.md.old"))
            .unwrap()
            .ends_with("Old."));
        assert!(outdated_agents(&root).is_none());
        std::fs::write(&path, "# My own guide").unwrap();
        assert!(outdated_agents(&root).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn record_and_replay_mistakes_fail_before_the_build() {
        let code = |options: RunOptions| {
            let result = run_game_project(Path::new("/nonexistent"), options);
            result.diagnostics[0].code
        };
        let ticks = Some(200);
        let record = Some(PathBuf::from("r.rec"));
        assert_eq!(
            code(RunOptions {
                headless_ticks: ticks,
                record,
                ..RunOptions::default()
            }),
            "CLI_USAGE"
        );
        let missing = Some(PathBuf::from("/nonexistent/missing.rec"));
        assert_eq!(
            code(RunOptions {
                headless_ticks: ticks,
                replay: missing.clone(),
                ..RunOptions::default()
            }),
            "CLI_USAGE"
        );
        let result = run_game_project(
            Path::new("/nonexistent"),
            RunOptions {
                replay: missing,
                ..RunOptions::default()
            },
        );
        assert_eq!(result.diagnostics[0].code, "FILE_NOT_FOUND");
        assert!(result.diagnostics[0].message.contains("missing.rec"));
    }

    #[test]
    fn shape_limits_projects_and_summarizes_listings() {
        let data = || json!({"n": 1, "xs": [{"a": 1, "b": 2}, {"a": 3, "b": 4}, {"a": 5}]});
        let mut limited = data();
        Shape {
            limit: Some(2),
            fields: Some(vec!["a".into()]),
            summary: false,
        }
        .apply(&mut limited);
        assert_eq!(limited["xs"], json!([{"a": 1}, {"a": 3}]));
        assert_eq!(limited["omitted"], json!({"xs": 1}));
        assert_eq!(limited["n"], 1);
        let mut counted = data();
        Shape {
            summary: true,
            ..Shape::default()
        }
        .apply(&mut counted);
        assert_eq!(counted["xs"], json!({"count": 3}));
    }
}

#[cfg(test)]
mod hint_tests {
    use super::*;

    #[test]
    fn rustc_errors_get_the_engine_pattern_that_fixes_them() {
        let output = "src/main.rs:12:9: error[E0499]: cannot borrow `*scene` as mutable more than once at a time\n\
src/main.rs:13:9: error[E0599]: no method named `teleport` found for struct `GameScene<'_>` in the current scope\n\
src/main.rs:20:1: error[E0499]: cannot borrow `*scene` as mutable more than once at a time\n\
src/main.rs:30:5: error[E0425]: cannot find value `x` in this scope";
        let hints = engine_hints(Path::new("/p"), output);
        assert_eq!(hints.len(), 2, "one per pattern: {hints:?}");
        assert_eq!(hints[0].code, "RUST_ENGINE_HINT");
        assert_eq!(hints[0].line, Some(12));
        assert_eq!(hints[0].file.as_deref(), Some(Path::new("/p/src/main.rs")));
        assert!(hints[1].message.contains("rusting docs search"));
    }

    #[test]
    fn literal_asset_paths_in_game_code_must_be_files() {
        let root = std::env::temp_dir()
            .join(format!("rusting-code-assets-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("assets/charts")).unwrap();
        std::fs::write(root.join("assets/charts/easy.json"), "{}").unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "let a = scene.load_text(\"charts/easy.json\");\n\
             let b = scene.load_text(\"charts/hard.json\");\n\
             scene.play_sound(&clip, 1.0);\n",
        )
        .unwrap();
        let found = code_asset_diagnostics(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            found[0].message,
            "src/main.rs:2: `charts/hard.json` is not a file under assets/"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
