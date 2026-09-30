//! Window-free project and scene operations used by the `rusting` CLI.

use std::collections::BTreeSet;
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

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_location: Option<String>,
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
                scene_location: None,
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
    CliResult::failure(code, error.to_string(), Some(path.to_owned()))
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
        Ok(project) => CliResult::success(project_data(&project)),
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
            })
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
    apply_scene_patch(path, &patch, dry_run)
}

fn apply_scene_patch(
    path: &Path,
    patch: &crate::scene_patch::ScenePatch,
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
                    scene_location: None,
                })
                .collect();
            result
        }
        Err(error) => {
            let mut result = CliResult::failure(
                error.code(),
                error.to_string(),
                Some(path.to_owned()),
            );
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
                    scene_location: None,
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
                scene_location: None,
            })
            .collect(),
    }
}

pub fn list_presets() -> CliResult {
    CliResult::success(json!({ "presets": crate::art_direction::PRESETS }))
}

/// Applies an art-direction preset to a scene as one scene patch.
pub fn apply_preset(path: &Path, name: &str, dry_run: bool) -> CliResult {
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
    let mut patch = crate::art_direction::preset_patch(&document, preset);
    patch.expected_revision = Some(crate::runtime::scene_revision(&bytes));
    apply_scene_patch(path, &patch, dry_run)
}

fn tool_available(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

pub fn doctor() -> CliResult {
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
    }))
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
            scene_location: diagnostic.line.map(|line| format!("line {line}")),
        })
        .collect()
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
    Err(result)
}

/// Validates the project and type-checks its Rust code with `cargo check`.
pub fn check_project(root: &Path) -> CliResult {
    let validation = validate_project(root);
    if !validation.ok {
        return validation;
    }
    let project = match open_project(root) {
        Ok(project) => project,
        Err(error) => return project_error(error, root),
    };
    match cargo(&project, "check", false, None) {
        Ok(run) => CliResult::success(
            json!({"root": project.root, "cargo": run.data()}),
        ),
        Err(result) => result,
    }
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
}

/// Cooks the main scene, builds the game, and runs it from the project
/// folder, the same way the editor's Build and Run does.
pub fn run_game_project(root: &Path, options: RunOptions) -> CliResult {
    let start = Instant::now();
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
        game.env(crate::scenario::TEST_SCENARIO_ENV, scenario)
            .env(crate::scenario::TEST_REPORT_ENV, &report_path);
    }
    let launched_after = start.elapsed();
    let run = match run_process(&mut game, options.timeout) {
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
            let mut result = CliResult::failure(
                "SCENARIO_FAILED",
                format!(
                    "tick {} step {}: {}",
                    failure.tick, failure.step, failure.message
                ),
                Some(scenario),
            );
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
                step["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("log: "))
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
            scene_location: None,
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
    let rgba = capture.as_ref().map(HeadlessCapture::rgba);
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
    let mut data = json!({
        "scene": scene,
        "tick": options.tick,
        "extent": options.extent,
        "output": Value::Null,
        "camera": view.data(app.world()),
        "render": render,
        "pick_method": "mesh_bounds",
        "picks": picks,
    });

    let mut result = match (gpu_error, capture) {
        (Some((code, error)), _) => {
            let mut result = CliResult::failure(code, error, None);
            result.data = data;
            result
        }
        (None, Some(capture)) => {
            if let Err(error) = capture.save(&options.output) {
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
