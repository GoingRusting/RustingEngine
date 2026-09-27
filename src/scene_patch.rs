//! Atomic scene patch batches for agents and tools.
//!
//! A patch is a list of operations applied to a scene file as one unit. The
//! operations address entities by their persistent `id` and fields by JSON
//! pointers into the entity's scene form, with registered components parsed
//! (the same form scenario `expect` paths and the schema catalog use). Every
//! operation must succeed and the result must pass the scene loader's checks
//! before anything is written. `expected_revision` and per-field `expected`
//! values turn concurrent edits into conflicts instead of silent overwrites.

use std::collections::{BTreeMap, HashSet};
use std::fmt::{Display, Formatter};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::runtime::{
    parse_scene_document, scene_revision, set_registered_component,
    validate_scene_structure, write_atomic, App, SceneComponentRegistry,
    SceneDocument, SceneIoError,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePatch {
    /// Revision from `rusting scene inspect`; the patch fails with a conflict
    /// when the file changed since.
    #[serde(default)]
    pub expected_revision: Option<String>,
    pub operations: Vec<PatchOperation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatchOperation {
    /// Adds an entity in scene form. A missing `id` gets a new random one.
    Create { entity: Value },
    /// Replaces or inserts the value at `path`. With `expected`, the current
    /// value must equal it first.
    Set {
        id: Uuid,
        path: String,
        value: Value,
        #[serde(default)]
        expected: Option<Value>,
    },
    /// Removes an object key or array element, for example an optional
    /// section such as `/collider`.
    Remove {
        id: Uuid,
        path: String,
        #[serde(default)]
        expected: Option<Value>,
    },
    /// Moves an entity under `parent`, or to the root with `null`.
    Reparent { id: Uuid, parent: Option<Uuid> },
    /// Copies one entity without its children. The copy has no name unless
    /// `name` is given, because scene names are unique.
    Duplicate {
        id: Uuid,
        #[serde(default)]
        new_id: Option<Uuid>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Deletes an entity and all of its descendants.
    Delete { id: Uuid },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldChange {
    pub id: Uuid,
    pub name: Option<String>,
    pub path: String,
    pub before: Value,
    pub after: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct PatchOutcome {
    pub revision_before: String,
    /// Revision of the written file, or the one it would have on a dry run.
    pub revision_after: String,
    pub created: Vec<Uuid>,
    pub deleted: Vec<Uuid>,
    pub changes: Vec<FieldChange>,
    /// Components the engine does not know; game code validates them on load.
    pub unvalidated_components: Vec<String>,
    pub written: bool,
}

#[derive(Debug)]
pub enum PatchError {
    Revision {
        expected: String,
        actual: String,
    },
    Field {
        operation: usize,
        id: Uuid,
        path: String,
        expected: Value,
        actual: Value,
    },
    Operation {
        operation: usize,
        message: String,
    },
    Invalid(String),
    Scene(SceneIoError),
}

impl Display for PatchError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Revision { expected, actual } => write!(
                formatter,
                "scene revision is {actual}, patch expected {expected}; \
                 re-read the scene and rebuild the patch"
            ),
            Self::Field {
                operation,
                id,
                path,
                expected,
                actual,
            } => write!(
                formatter,
                "operation {operation}: {id}{path} is {actual}, patch \
                 expected {expected}"
            ),
            Self::Operation { operation, message } => {
                write!(formatter, "operation {operation}: {message}")
            }
            Self::Invalid(message) => {
                write!(formatter, "patched scene is invalid: {message}")
            }
            Self::Scene(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for PatchError {}

impl PatchError {
    /// Stable CLI diagnostic code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Revision { .. } | Self::Field { .. } => "SCENE_CONFLICT",
            Self::Operation { .. } => "PATCH_OPERATION",
            Self::Invalid(_) => "PATCH_INVALID",
            Self::Scene(_) => "SCENE_INVALID",
        }
    }
}

/// Entity in patch form: registered component strings parsed to JSON.
fn entity_form(entity: &crate::runtime::SceneEntity) -> Value {
    let mut value = serde_json::to_value(entity).expect("entities serialize");
    if let Some(Value::Object(components)) = value.get_mut("components") {
        for component in components.values_mut() {
            if let Value::String(text) = component {
                if let Ok(parsed) = serde_json::from_str(text) {
                    *component = parsed;
                }
            }
        }
    }
    value
}

fn entity_id(entity: &Value) -> Option<Uuid> {
    entity.get("id")?.as_str()?.parse().ok()
}

fn split_pointer(path: &str) -> Result<(&str, String), String> {
    let (parent, last) = path
        .rsplit_once('/')
        .filter(|_| path.starts_with('/'))
        .ok_or_else(|| format!("`{path}` is not a JSON pointer"))?;
    if path == "/id" {
        return Err("`/id` is persistent and cannot change".into());
    }
    Ok((parent, last.replace("~1", "/").replace("~0", "~")))
}

fn check_expected(
    index: usize,
    id: Uuid,
    entity: &Value,
    path: &str,
    expected: Option<&Value>,
) -> Result<(), PatchError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = entity.pointer(path).cloned().unwrap_or(Value::Null);
    if &actual == expected {
        Ok(())
    } else {
        Err(PatchError::Field {
            operation: index,
            id,
            path: path.to_owned(),
            expected: expected.clone(),
            actual,
        })
    }
}

fn set_pointer(
    entity: &mut Value,
    path: &str,
    value: Value,
) -> Result<(), String> {
    let (parent, key) = split_pointer(path)?;
    match entity.pointer_mut(parent) {
        Some(Value::Object(object)) => {
            object.insert(key, value);
            Ok(())
        }
        Some(Value::Array(array)) => {
            let slot = key
                .parse::<usize>()
                .ok()
                .and_then(|index| array.get_mut(index))
                .ok_or_else(|| format!("`{path}` is out of range"))?;
            *slot = value;
            Ok(())
        }
        _ => Err(format!("`{parent}` does not exist or is not a container")),
    }
}

fn remove_pointer(entity: &mut Value, path: &str) -> Result<(), String> {
    let (parent, key) = split_pointer(path)?;
    let removed = match entity.pointer_mut(parent) {
        Some(Value::Object(object)) => object.remove(&key).is_some(),
        Some(Value::Array(array)) => match key.parse::<usize>() {
            Ok(index) if index < array.len() => {
                array.remove(index);
                true
            }
            _ => false,
        },
        _ => false,
    };
    removed
        .then_some(())
        .ok_or_else(|| format!("`{path}` does not exist"))
}

fn descendants(entities: &[Value], root: Uuid) -> HashSet<Uuid> {
    let mut found = HashSet::from([root]);
    // Parents may appear after children, so repeat until nothing is added.
    loop {
        let before = found.len();
        for entity in entities {
            let parent = entity
                .get("parent")
                .and_then(Value::as_str)
                .and_then(|id| id.parse().ok());
            if let (Some(parent), Some(id)) = (parent, entity_id(entity)) {
                if found.contains(&parent) {
                    found.insert(id);
                }
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

fn apply_operation(
    entities: &mut Vec<Value>,
    index: usize,
    operation: &PatchOperation,
) -> Result<(), PatchError> {
    let fail = |message: String| PatchError::Operation {
        operation: index,
        message,
    };
    let find = |entities: &[Value], id: Uuid| {
        entities
            .iter()
            .position(|entity| entity_id(entity) == Some(id))
            .ok_or_else(|| fail(format!("entity {id} does not exist")))
    };
    match operation {
        PatchOperation::Create { entity } => {
            let Value::Object(object) = entity else {
                return Err(fail("`entity` must be an object".into()));
            };
            let mut object = object.clone();
            object
                .entry("id")
                .or_insert_with(|| Value::String(Uuid::new_v4().to_string()));
            entities.push(Value::Object(object));
        }
        PatchOperation::Set {
            id,
            path,
            value,
            expected,
        } => {
            let position = find(entities, *id)?;
            let entity = &mut entities[position];
            check_expected(index, *id, entity, path, expected.as_ref())?;
            set_pointer(entity, path, value.clone()).map_err(fail)?;
        }
        PatchOperation::Remove { id, path, expected } => {
            let position = find(entities, *id)?;
            let entity = &mut entities[position];
            check_expected(index, *id, entity, path, expected.as_ref())?;
            remove_pointer(entity, path).map_err(fail)?;
        }
        PatchOperation::Reparent { id, parent } => {
            let position = find(entities, *id)?;
            let entity = &mut entities[position];
            entity["parent"] = serde_json::to_value(parent).unwrap_or_default();
        }
        PatchOperation::Duplicate { id, new_id, name } => {
            let mut copy = entities[find(entities, *id)?].clone();
            copy["id"] =
                Value::String(new_id.unwrap_or_else(Uuid::new_v4).to_string());
            copy["name"] = name.clone().map_or(Value::Null, Value::String);
            entities.push(copy);
        }
        PatchOperation::Delete { id } => {
            find(entities, *id)?;
            let removed = descendants(entities, *id);
            entities.retain(|entity| {
                entity_id(entity).is_none_or(|id| !removed.contains(&id))
            });
        }
    }
    Ok(())
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn diff_values(
    path: String,
    before: &Value,
    after: &Value,
    out: &mut Vec<(String, Value, Value)>,
) {
    match (before, after) {
        (Value::Object(before), Value::Object(after)) => {
            let keys: std::collections::BTreeSet<_> =
                before.keys().chain(after.keys()).collect();
            for key in keys {
                diff_values(
                    format!("{path}/{}", escape(key)),
                    before.get(key).unwrap_or(&Value::Null),
                    after.get(key).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        _ if before != after => {
            out.push((path, before.clone(), after.clone()));
        }
        _ => {}
    }
}

/// Applies `patch` to a parsed scene. Nothing changes on error.
pub fn apply_patch(
    document: &SceneDocument,
    patch: &ScenePatch,
) -> Result<(SceneDocument, PatchOutcome), PatchError> {
    let before: Vec<Value> =
        document.entities.iter().map(entity_form).collect();
    let mut entities = before.clone();
    for (index, operation) in patch.operations.iter().enumerate() {
        apply_operation(&mut entities, index, operation)?;
    }

    // Back to stored form: registered components are JSON strings.
    let mut stored = entities.clone();
    for entity in &mut stored {
        if let Some(Value::Object(components)) = entity.get_mut("components") {
            for component in components.values_mut() {
                *component = Value::String(component.to_string());
            }
        }
    }
    let mut patched = document.clone();
    patched.entities = serde_json::from_value(Value::Array(stored))
        .map_err(|error| PatchError::Invalid(error.to_string()))?;
    validate_scene_structure(&patched)
        .map_err(|error| PatchError::Invalid(error.to_string()))?;
    let unvalidated_components = validate_components(&patched)?;

    let by_id = |values: &[Value]| -> BTreeMap<Uuid, Value> {
        values
            .iter()
            .filter_map(|entity| Some((entity_id(entity)?, entity.clone())))
            .collect()
    };
    let (old, new) = (by_id(&before), by_id(&entities));
    let mut outcome = PatchOutcome {
        revision_before: String::new(),
        revision_after: String::new(),
        created: new
            .keys()
            .filter(|id| !old.contains_key(id))
            .copied()
            .collect(),
        deleted: old
            .keys()
            .filter(|id| !new.contains_key(id))
            .copied()
            .collect(),
        changes: Vec::new(),
        unvalidated_components,
        written: false,
    };
    for (id, after) in &new {
        let Some(before) = old.get(id) else { continue };
        let mut leaves = Vec::new();
        diff_values(String::new(), before, after, &mut leaves);
        let name = after.get("name").and_then(Value::as_str).map(str::to_owned);
        outcome.changes.extend(leaves.into_iter().map(
            |(path, before, after)| FieldChange {
                id: *id,
                name: name.clone(),
                path,
                before,
                after,
            },
        ));
    }
    Ok((patched, outcome))
}

/// Restores each built-in component through the registry so bad values fail
/// here, not when the game loads the scene. Returns unknown component names.
fn validate_components(
    document: &SceneDocument,
) -> Result<Vec<String>, PatchError> {
    let mut app = App::new();
    app.add_plugin(crate::AssetPlugin)
        .map_err(|error| PatchError::Invalid(error.to_string()))?;
    let world = app.world_mut();
    let known: HashSet<String> = world
        .resource::<SceneComponentRegistry>()
        .names()
        .map(str::to_owned)
        .collect();
    let mut unknown = std::collections::BTreeSet::new();
    for entity in &document.entities {
        let scratch = world.spawn_empty().id();
        for (name, text) in &entity.components {
            if !known.contains(name) {
                unknown.insert(name.clone());
                continue;
            }
            set_registered_component(world, scratch, name, text).map_err(
                |error| {
                    PatchError::Invalid(format!(
                        "entity {}: {error}",
                        entity.id
                    ))
                },
            )?;
        }
    }
    Ok(unknown.into_iter().collect())
}

/// Applies `patch` to the scene file at `path` and writes it atomically,
/// unless `dry_run`. The file is re-read just before writing so an edit made
/// while the patch was applied still reports a conflict.
pub fn patch_scene_file(
    path: &Path,
    patch: &ScenePatch,
    dry_run: bool,
) -> Result<PatchOutcome, PatchError> {
    let bytes =
        std::fs::read(path).map_err(|error| PatchError::Scene(error.into()))?;
    let revision = scene_revision(&bytes);
    if let Some(expected) = &patch.expected_revision {
        if *expected != revision {
            return Err(PatchError::Revision {
                expected: expected.clone(),
                actual: revision,
            });
        }
    }
    let document = parse_scene_document(&bytes).map_err(PatchError::Scene)?;
    let (patched, mut outcome) = apply_patch(&document, patch)?;
    let output = serde_json::to_vec_pretty(&patched)
        .map_err(|error| PatchError::Scene(error.into()))?;
    outcome.revision_before = revision.clone();
    outcome.revision_after = scene_revision(&output);
    if !dry_run {
        // ponytail: a writer between this read and the rename still wins;
        // an OS file lock closes that window if it ever matters.
        let current = std::fs::read(path)
            .map_err(|error| PatchError::Scene(error.into()))?;
        if scene_revision(&current) != revision {
            return Err(PatchError::Revision {
                expected: revision,
                actual: scene_revision(&current),
            });
        }
        write_atomic(path, &output)
            .map_err(|error| PatchError::Scene(error.into()))?;
        outcome.written = true;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const CUBE: &str = "00000000-0000-0000-0000-000000000001";
    const LAMP: &str = "00000000-0000-0000-0000-000000000002";

    fn scene_file(name: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir()
            .join(format!("rusting-patch-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("main.rscene");
        let scene = json!({
            "format_version": 6, "name": "Main", "entities": [
                {"id": CUBE, "parent": null, "name": "Cube",
                 "transform": {"position": [0.0, 1.0, 0.0], "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]}},
                {"id": LAMP, "parent": CUBE, "name": "Lamp"},
            ]
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&scene).unwrap())
            .unwrap();
        path
    }

    fn patch(value: Value) -> ScenePatch {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn batch_applies_every_operation_and_reports_a_diff() {
        let path = scene_file("apply");
        let outcome = patch_scene_file(&path, &patch(json!({"operations": [
            {"op": "set", "id": CUBE, "path": "/transform/position/1", "value": 4.0, "expected": 1.0},
            {"op": "set", "id": CUBE, "path": "/components/rusting.player_controller",
             "value": {"walk_speed": 3.0}},
            {"op": "duplicate", "id": CUBE, "new_id": "00000000-0000-0000-0000-000000000003", "name": "Cube 2"},
            {"op": "reparent", "id": LAMP, "parent": null},
            {"op": "create", "entity": {"id": "00000000-0000-0000-0000-000000000004", "parent": LAMP, "name": "Child"}},
            {"op": "delete", "id": LAMP},
        ]})), false)
        .unwrap();
        assert!(outcome.written);
        assert_eq!(
            outcome.created.len(),
            1,
            "created then deleted child is not listed"
        );
        assert_eq!(outcome.deleted, vec![LAMP.parse::<Uuid>().unwrap()]);
        assert!(outcome
            .changes
            .iter()
            .any(|change| change.path == "/transform/position"
                && change.after == json!([0.0, 4.0, 0.0])));
        let written = crate::runtime::read_scene_document(&path).unwrap();
        assert_eq!(written.entities.len(), 2);
        assert_eq!(written.entities[0].transform.unwrap().position[1], 4.0);
        let controller =
            &written.entities[0].components["rusting.player_controller"];
        assert!(controller.contains("walk_speed"));
        assert_eq!(
            outcome.revision_after,
            scene_revision(&std::fs::read(&path).unwrap())
        );
    }

    #[test]
    fn failures_and_dry_runs_leave_the_file_untouched() {
        let path = scene_file("atomic");
        let original = std::fs::read(&path).unwrap();
        let revision = scene_revision(&original);
        let failing = [
            // Second operation fails after the first one succeeded.
            (
                json!({"operations": [
                {"op": "set", "id": CUBE, "path": "/name", "value": "Moved"},
                {"op": "set", "id": CUBE, "path": "/missing/field", "value": 1}]}),
                "PATCH_OPERATION",
            ),
            (
                json!({"expected_revision": "0000000000000000", "operations": []}),
                "SCENE_CONFLICT",
            ),
            (
                json!({"operations": [{"op": "set", "id": CUBE, "path": "/transform/position/1",
                "value": 2.0, "expected": 9.0}]}),
                "SCENE_CONFLICT",
            ),
            (
                json!({"operations": [{"op": "set", "id": CUBE, "path": "/name", "value": "Lamp"}]}),
                "PATCH_INVALID",
            ),
            (
                json!({"operations": [{"op": "reparent", "id": CUBE, "parent": LAMP}]}),
                "PATCH_INVALID",
            ),
            (
                json!({"operations": [{"op": "set", "id": CUBE, "path": "/transform/scale", "value": "big"}]}),
                "PATCH_INVALID",
            ),
            (
                json!({"operations": [{"op": "set", "id": CUBE, "path": "/components/rusting.player_controller",
                "value": {"walk_speed": "fast"}}]}),
                "PATCH_INVALID",
            ),
            (
                json!({"operations": [{"op": "set", "id": CUBE, "path": "/id", "value": LAMP}]}),
                "PATCH_OPERATION",
            ),
        ];
        for (value, code) in failing {
            let error = patch_scene_file(&path, &patch(value.clone()), false)
                .unwrap_err();
            assert_eq!(error.code(), code, "{value}: {error}");
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        let outcome = patch_scene_file(&path, &patch(json!({"expected_revision": revision,
            "operations": [{"op": "set", "id": CUBE, "path": "/components/game.health", "value": 3}]})), true)
            .unwrap();
        assert!(!outcome.written);
        assert_eq!(outcome.unvalidated_components, vec!["game.health"]);
        assert_eq!(outcome.changes[0].path, "/components/game.health");
        assert_ne!(outcome.revision_after, revision);
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}
