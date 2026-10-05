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
    error_object, parse_scene_document, scene_revision,
    set_registered_component, validate_scene_structure, write_atomic, App,
    SceneComponentRegistry, SceneDocument, SceneIoError,
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
    /// Like `create`, but replaces the entity with the same `id`, or else
    /// the same `name`, when one exists. The replacement keeps the old ID,
    /// so children stay attached. A patch of upserts can run again. With
    /// `merge`, the given fields merge into the old entity (objects key by
    /// key, `null` removes a key) and fields left out keep their values.
    Upsert {
        entity: Value,
        #[serde(default)]
        merge: bool,
    },
    /// Replaces or inserts the value at `path`. With `expected`, the current
    /// value must equal it first.
    Set {
        id: EntityRef,
        path: String,
        value: Value,
        #[serde(default)]
        expected: Option<Value>,
    },
    /// Removes an object key or array element, for example an optional
    /// section such as `/collider`.
    Remove {
        id: EntityRef,
        path: String,
        #[serde(default)]
        expected: Option<Value>,
    },
    /// Moves an entity under `parent`, or to the root with `null`.
    Reparent {
        id: EntityRef,
        parent: Option<EntityRef>,
    },
    /// Copies one entity without its children. The copy has no name unless
    /// `name` is given, because scene names are unique.
    Duplicate {
        id: EntityRef,
        #[serde(default)]
        new_id: Option<Uuid>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Deletes an entity and all of its descendants. With `missing_ok`, a
    /// missing entity is skipped, so a patch can run again.
    Delete {
        id: EntityRef,
        #[serde(default)]
        missing_ok: bool,
    },
    /// Like `set`, on the scene itself: `/name`, `/render/...` or
    /// `/simulation/...`. Entities and the format version are off limits.
    SetScene {
        path: String,
        value: Value,
        #[serde(default)]
        expected: Option<Value>,
    },
}

/// An entity addressed by persistent ID or by its unique name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EntityRef {
    Id(Uuid),
    Name(String),
}

impl From<Uuid> for EntityRef {
    fn from(id: Uuid) -> Self {
        Self::Id(id)
    }
}

impl EntityRef {
    fn resolve(&self, entities: &[Value]) -> Result<Uuid, String> {
        let name = match self {
            Self::Id(id) => return Ok(*id),
            Self::Name(name) => name,
        };
        let mut found = entities
            .iter()
            .filter(|entity| {
                entity.get("name").and_then(Value::as_str) == Some(name)
            })
            .filter_map(entity_id);
        match (found.next(), found.next()) {
            (Some(id), None) => Ok(id),
            (None, _) => Err(format!("no entity is named `{name}`")),
            _ => Err(format!("more than one entity is named `{name}`")),
        }
    }
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
    /// The patched scene breaks a scene rule at one object.
    InvalidObject {
        id: Uuid,
        name: Option<String>,
        message: String,
    },
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
            Self::InvalidObject { id, name, message } => write!(
                formatter,
                "patched scene is invalid at object {id}{}: {message}",
                name.as_ref()
                    .map_or(String::new(), |name| format!(" (`{name}`)"))
            ),
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
            Self::Invalid(_) | Self::InvalidObject { .. } => "PATCH_INVALID",
            Self::Scene(_) => "SCENE_INVALID",
        }
    }
}

/// Entity in patch form: registered component strings parsed to JSON.
pub(crate) fn entity_form(entity: &crate::runtime::SceneEntity) -> Value {
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

/// JSON merge patch (RFC 7386): objects merge key by key, `null` removes a
/// key, anything else replaces.
fn merge_into(target: &mut Value, patch: Value) {
    let (Value::Object(target), Value::Object(patch)) = (&mut *target, &patch)
    else {
        *target = patch;
        return;
    };
    for (key, value) in patch {
        if value.is_null() {
            target.remove(key);
        } else {
            merge_into(
                target.entry(key.clone()).or_insert(Value::Null),
                value.clone(),
            );
        }
    }
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
        PatchOperation::Create { entity }
        | PatchOperation::Upsert { entity, .. } => {
            let Value::Object(object) = entity else {
                return Err(fail("`entity` must be an object".into()));
            };
            let mut object = object.clone();
            if let Some(Value::String(parent)) = object.get("parent") {
                if parent.parse::<Uuid>().is_err() {
                    let id = EntityRef::Name(parent.clone())
                        .resolve(entities)
                        .map_err(fail)?;
                    object.insert("parent".into(), id.to_string().into());
                }
            }
            let existing = matches!(operation, PatchOperation::Upsert { .. })
                .then(|| {
                    entities.iter().position(|old| match object.get("id") {
                        Some(id) => old.get("id") == Some(id),
                        None => object.get("name").is_some_and(|name| {
                            !name.is_null() && old.get("name") == Some(name)
                        }),
                    })
                })
                .flatten();
            if let Some(position) = existing {
                object
                    .entry("id")
                    .or_insert_with(|| entities[position]["id"].clone());
                let new = Value::Object(object);
                if matches!(
                    operation,
                    PatchOperation::Upsert { merge: true, .. }
                ) {
                    merge_into(&mut entities[position], new);
                } else {
                    entities[position] = new;
                }
                return Ok(());
            }
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
            let id = id.resolve(entities).map_err(fail)?;
            let position = find(entities, id)?;
            let entity = &mut entities[position];
            check_expected(index, id, entity, path, expected.as_ref())?;
            set_pointer(entity, path, value.clone()).map_err(fail)?;
        }
        PatchOperation::Remove { id, path, expected } => {
            let id = id.resolve(entities).map_err(fail)?;
            let position = find(entities, id)?;
            let entity = &mut entities[position];
            check_expected(index, id, entity, path, expected.as_ref())?;
            remove_pointer(entity, path).map_err(fail)?;
        }
        PatchOperation::Reparent { id, parent } => {
            let id = id.resolve(entities).map_err(fail)?;
            let parent = parent
                .as_ref()
                .map(|parent| parent.resolve(entities))
                .transpose()
                .map_err(fail)?;
            let position = find(entities, id)?;
            let entity = &mut entities[position];
            entity["parent"] = serde_json::to_value(parent).unwrap_or_default();
        }
        PatchOperation::Duplicate { id, new_id, name } => {
            let id = id.resolve(entities).map_err(fail)?;
            let mut copy = entities[find(entities, id)?].clone();
            copy["id"] =
                Value::String(new_id.unwrap_or_else(Uuid::new_v4).to_string());
            copy["name"] = name.clone().map_or(Value::Null, Value::String);
            entities.push(copy);
        }
        PatchOperation::SetScene { .. } => {
            unreachable!("apply_patch handles scene operations")
        }
        PatchOperation::Delete { id, missing_ok } => {
            let found = id
                .resolve(entities)
                .map_err(fail)
                .and_then(|id| find(entities, id).map(|_| id));
            let id = match found {
                Err(_) if *missing_ok => return Ok(()),
                found => found?,
            };
            let removed = descendants(entities, id);
            entities.retain(|entity| {
                entity_id(entity).is_none_or(|id| !removed.contains(&id))
            });
        }
    }
    Ok(())
}

pub(crate) fn escape(key: &str) -> String {
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

/// Fills fields left out of built-in sections from `default`, so a patch can
/// write `{"transform": {"position": [0, 1, 0]}}`. An enum variant other than
/// the default one is kept as given. A key the default does not have is an
/// error, since serde would drop it and the field would silently take its
/// default. `path` names the section for the message.
// Registered components pass `strict: false`: reflection already rejects
// their unknown fields, and a map default holds sample keys.
// ponytail: an object whose default is empty (a map) is not checked.
fn fill_defaults(
    value: &mut Value,
    default: &Value,
    path: &str,
    strict: bool,
) -> Result<(), String> {
    let (Value::Object(value), Value::Object(default)) = (value, default)
    else {
        return Ok(());
    };
    let variant = |key: &String| key.starts_with(char::is_uppercase);
    if default.len() == 1
        && default.keys().all(variant)
        && !default.keys().all(|key| value.contains_key(key))
    {
        return Ok(());
    }
    if strict && !default.is_empty() {
        if let Some(key) = value.keys().find(|key| !default.contains_key(*key))
        {
            let known: Vec<&str> = default.keys().map(String::as_str).collect();
            return Err(format!(
                "unknown field `{key}` in `{path}`; known fields: {}",
                known.join(", ")
            ));
        }
    }
    for (key, default) in default {
        match value.get_mut(key) {
            // An RGB color where the scene stores RGBA gets alpha 1.
            Some(Value::Array(rgb))
                if key.ends_with("color")
                    && rgb.len() == 3
                    && default
                        .as_array()
                        .is_some_and(|rgba| rgba.len() == 4) =>
            {
                rgb.push(1.0.into());
            }
            Some(value) => {
                fill_defaults(
                    value,
                    default,
                    &format!("{path}/{key}"),
                    strict,
                )?;
            }
            None => {
                value.insert(key.clone(), default.clone());
            }
        }
    }
    Ok(())
}

/// The first object key of `wanted` that `kept` does not have, as a path.
fn dropped_key(wanted: &Value, kept: &Value, path: &str) -> Option<String> {
    let (Value::Object(wanted), Value::Object(kept)) = (wanted, kept) else {
        return None;
    };
    wanted.iter().find_map(|(key, value)| {
        let path = format!("{path}/{}", escape(key));
        match kept.get(key) {
            Some(kept) => dropped_key(value, kept, &path),
            None => Some(path),
        }
    })
}

/// A key in a scene file that loading ignores, usually a misspelling.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DroppedField {
    /// Index in `entities`, or `None` for a scene setting.
    pub entity: Option<usize>,
    /// JSON pointer inside the entity or the scene, such as
    /// `/directional_light/intensity`.
    pub path: String,
    /// Keys the object does have.
    pub known: Vec<String>,
    /// The one known key within two edits of the dropped one, if exactly one
    /// is, and the scene file does not set it already.
    pub suggestion: Option<String>,
}

/// Every key in `raw` (the scene file's JSON) that decoding into `document`
/// dropped. Registered component strings are checked on load instead.
#[must_use]
pub fn dropped_fields(
    raw: &Value,
    document: &SceneDocument,
) -> Vec<DroppedField> {
    let mut found = Vec::new();
    let Ok(Value::Object(mut kept)) = serde_json::to_value(document) else {
        return found;
    };
    let kept_entities = kept.remove("entities");
    let mut settings = raw.clone();
    if let Value::Object(settings) = &mut settings {
        settings.remove("entities");
    }
    collect_dropped(&settings, &Value::Object(kept), "", None, &mut found);
    let raw_entities = raw.get("entities").and_then(Value::as_array);
    let kept_entities = kept_entities.as_ref().and_then(Value::as_array);
    for (index, (raw, kept)) in raw_entities
        .into_iter()
        .flatten()
        .zip(kept_entities.into_iter().flatten())
        .enumerate()
    {
        let (mut raw, mut kept) = (raw.clone(), kept.clone());
        for value in [&mut raw, &mut kept] {
            if let Value::Object(object) = value {
                object.remove("components");
            }
        }
        collect_dropped(&raw, &kept, "", Some(index), &mut found);
    }
    found
}

fn collect_dropped(
    raw: &Value,
    kept: &Value,
    path: &str,
    entity: Option<usize>,
    found: &mut Vec<DroppedField>,
) {
    match (raw, kept) {
        (Value::Object(raw), Value::Object(kept)) => {
            for (key, value) in raw {
                let path = format!("{path}/{}", escape(key));
                if let Some(kept) = kept.get(key) {
                    collect_dropped(value, kept, &path, entity, found);
                    continue;
                }
                // `null` is how an absent optional section is written.
                if value.is_null() {
                    continue;
                }
                let unset =
                    kept.keys().filter(|known| !raw.contains_key(*known));
                let mut close =
                    unset.filter(|known| edit_distance(key, known) <= 2);
                let suggestion = match (close.next(), close.next()) {
                    (Some(only), None) => Some(only.clone()),
                    _ => None,
                };
                found.push(DroppedField {
                    entity,
                    path,
                    known: kept.keys().cloned().collect(),
                    suggestion,
                });
            }
        }
        (Value::Array(raw), Value::Array(kept)) => {
            for (index, (raw, kept)) in raw.iter().zip(kept).enumerate() {
                collect_dropped(
                    raw,
                    kept,
                    &format!("{path}/{index}"),
                    entity,
                    found,
                );
            }
        }
        _ => {}
    }
}

/// Levenshtein distance over characters.
pub(crate) fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(above).min(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Applies `patch` to a parsed scene. Nothing changes on error.
pub fn apply_patch(
    document: &SceneDocument,
    patch: &ScenePatch,
) -> Result<(SceneDocument, PatchOutcome), PatchError> {
    let before: Vec<Value> =
        document.entities.iter().map(entity_form).collect();
    let mut entities = before.clone();
    let Ok(Value::Object(mut settings)) = serde_json::to_value(document) else {
        unreachable!("a scene serializes as an object");
    };
    settings.remove("entities");
    let mut settings = Value::Object(settings);
    let settings_before = settings.clone();
    for (index, operation) in patch.operations.iter().enumerate() {
        if let PatchOperation::SetScene {
            path,
            value,
            expected,
        } = operation
        {
            if path.is_empty()
                || path.starts_with("/entities")
                || path == "/format_version"
            {
                return Err(PatchError::Operation {
                    operation: index,
                    message: format!("`{path}` cannot change with set_scene"),
                });
            }
            check_expected(
                index,
                Uuid::nil(),
                &settings,
                path,
                expected.as_ref(),
            )?;
            set_pointer(&mut settings, path, value.clone()).map_err(
                |message| PatchError::Operation {
                    operation: index,
                    message,
                },
            )?;
            continue;
        }
        apply_operation(&mut entities, index, operation)?;
    }
    let (defaults, component_defaults) = crate::schema::defaults();
    // Entities the patch did not touch are not this patch's to reject.
    let untouched: std::collections::HashSet<String> =
        before.iter().map(Value::to_string).collect();
    for entity in &mut entities {
        if untouched.contains(&entity.to_string()) {
            continue;
        }
        let Value::Object(sections) = entity else {
            continue;
        };
        for (key, section) in sections.iter_mut() {
            if let Some(default) = defaults.get(key) {
                fill_defaults(section, default, key, true)
                    .map_err(PatchError::Invalid)?;
            }
        }
        if let Some(Value::Object(components)) = sections.get_mut("components")
        {
            for (key, component) in components {
                if let Some(default) = component_defaults.get(key) {
                    fill_defaults(component, default, key, false)
                        .map_err(PatchError::Invalid)?;
                }
            }
        }
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
    let mut scene = settings.clone();
    scene["entities"] = Value::Array(stored);
    let patched: SceneDocument = serde::Deserialize::deserialize(&scene)
        .map_err(|error| {
            scene["entities"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(locate_invalid)
                .unwrap_or_else(|| PatchError::Invalid(error.to_string()))
        })?;
    // serde drops a key it does not know, so a mistyped scene setting would
    // report success and change nothing.
    if let Ok(Value::Object(mut kept)) = serde_json::to_value(&patched) {
        kept.remove("entities");
        if let Some(path) = dropped_key(&settings, &Value::Object(kept), "") {
            return Err(PatchError::Invalid(format!(
                "unknown scene setting `{path}`"
            )));
        }
    }
    validate_scene_structure(&patched).map_err(|error| {
        match error_object(&patched, &error) {
            Some(index) => invalid_object(&patched.entities[index], &error),
            None => PatchError::Invalid(error.to_string()),
        }
    })?;
    let unvalidated_components = validate_components(&patched)?;

    let by_id = |values: &[Value]| -> BTreeMap<Uuid, Value> {
        values
            .iter()
            .filter_map(|entity| Some((entity_id(entity)?, entity.clone())))
            .collect()
    };
    // Diff the parsed result, not the patch input: both sides then hold the
    // same f32-rounded values and filled-in defaults, so a rerun reports no
    // changes.
    let after: Vec<Value> = patched.entities.iter().map(entity_form).collect();
    let (old, new) = (by_id(&before), by_id(&after));
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
    let mut settings_after = serde_json::to_value(&patched).unwrap_or_default();
    if let Value::Object(fields) = &mut settings_after {
        fields.remove("entities");
    }
    let mut leaves = Vec::new();
    diff_values(
        String::new(),
        &settings_before,
        &settings_after,
        &mut leaves,
    );
    outcome
        .changes
        .extend(leaves.into_iter().map(|(path, before, after)| FieldChange {
            id: Uuid::nil(),
            name: None,
            path,
            before,
            after,
        }));
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

/// For an entity that does not parse, the JSON pointer of the field at
/// fault and serde's message: the deepest key whose removal makes it parse.
fn locate_invalid(entity: &Value) -> Option<PatchError> {
    let parses = |value: &Value| {
        serde_json::from_value::<crate::runtime::SceneEntity>(value.clone())
    };
    let error = parses(entity).err()?;
    let defaults = &crate::schema::defaults().0;
    let mut path = String::new();
    'narrow: while let Some(Value::Object(fields)) = entity.pointer(&path) {
        for key in fields.keys() {
            let child =
                format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
            // Put the default back where there is one; else drop the key.
            let section = child[1..].split('/').next().unwrap_or_default();
            let default = defaults
                .get(section)
                .and_then(|value| value.pointer(&child[1 + section.len()..]));
            let mut trial = entity.clone();
            match (default, trial.pointer_mut(&path)) {
                (Some(default), Some(Value::Object(parent))) => {
                    parent.insert(key.clone(), default.clone());
                }
                (None, Some(Value::Object(parent))) => {
                    parent.remove(key);
                }
                _ => {}
            }
            // A one-key object is an enum tag: removing it never parses.
            if parses(&trial).is_ok() || fields.len() == 1 {
                path = child;
                continue 'narrow;
            }
        }
        break;
    }
    Some(PatchError::InvalidObject {
        id: entity_id(entity).unwrap_or_default(),
        name: entity["name"].as_str().map(str::to_owned),
        message: format!("{path}: {error}"),
    })
}

fn invalid_object(
    entity: &crate::runtime::SceneEntity,
    error: &SceneIoError,
) -> PatchError {
    PatchError::InvalidObject {
        id: entity.id,
        name: entity.name.clone(),
        message: error.to_string(),
    }
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
            set_registered_component(world, scratch, name, text)
                .map_err(|error| invalid_object(entity, &error))?;
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

    #[test]
    fn dropped_fields_suggest_only_one_close_unset_key() {
        assert_eq!(edit_distance("qualty", "quality"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        let raw = json!({
            "format_version": 6, "name": "Main", "entities": [
                {"id": CUBE, "parent": null, "name": "Cube", "visble": false,
                 "nam": "x", "components": {"game.anything": "{}"}}],
            "render": {"quality": "High", "culling": "Auto", "qualty": "Low"}
        });
        let document: SceneDocument =
            serde_json::from_value(raw.clone()).unwrap();
        let found = dropped_fields(&raw, &document);
        let at =
            |path: &str| found.iter().find(|field| field.path == path).unwrap();
        // `quality` is already set, so `qualty` is not certainly it.
        assert_eq!(at("/render/qualty").suggestion, None);
        assert_eq!(at("/render/qualty").entity, None);
        assert_eq!(at("/visble").suggestion.as_deref(), Some("visible"));
        assert_eq!(at("/visble").entity, Some(0));
        // `name` is set by the file; nothing else is within two edits.
        assert_eq!(at("/nam").suggestion, None);
        assert_eq!(found.len(), 3, "components are checked on load: {found:?}");
    }

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
    fn a_misspelled_field_in_a_section_is_an_error() {
        let path = scene_file("misspelled");
        let error = patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "set", "id": CUBE, "path": "/directional_light",
                 "value": {"intensity": 5.0}},
            ]})),
            true,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("unknown field `intensity`"), "{message}");
        assert!(message.contains("illuminance"), "{message}");
    }

    #[test]
    fn a_full_material_and_light_pass_the_field_check() {
        let path = scene_file("full-material");
        let (defaults, _) = crate::schema::defaults();
        let mut material = defaults["mesh_renderer"].clone();
        material["material"]["Inline"]["transmission"] = json!(0.5);
        patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "set", "id": CUBE, "path": "/mesh_renderer", "value": material},
                {"op": "set", "id": LAMP, "path": "/directional_light",
                 "value": {"illuminance": 5.0}},
            ]})),
            true,
        )
        .unwrap();
    }

    #[test]
    fn created_sections_take_defaults_for_missing_fields() {
        let path = scene_file("defaults");
        patch_scene_file(&path, &patch(json!({"operations": [
            {"op": "create", "entity": {"id": "00000000-0000-0000-0000-000000000005", "name": "Gem",
             "transform": {"position": [1.0, 2.0, 3.0]},
             "mesh_renderer": {"mesh": {"BuiltinPrimitive": "Sphere"},
                               "material": {"Inline": {"base_color": [1.0, 0.0, 0.0, 1.0]}}},
             "collider": {"shape": {"Sphere": {"radius": 0.5}}, "sensor": true},
             "components": {"rusting.counter": {"name": "gems"}}}},
        ]})), false)
        .unwrap();
        let scene =
            parse_scene_document(&std::fs::read(&path).unwrap()).unwrap();
        let gem = entity_form(&scene.entities[2]);
        assert_eq!(gem["transform"]["scale"], json!([1.0, 1.0, 1.0]));
        assert_eq!(
            gem["mesh_renderer"]["mesh"],
            json!({"BuiltinPrimitive": "Sphere"})
        );
        assert_eq!(
            gem["mesh_renderer"]["material"]["Inline"]["roughness"],
            json!(0.5)
        );
        assert_eq!(
            gem["collider"]["shape"],
            json!({"Sphere": {"radius": 0.5}})
        );
        assert_eq!(gem["collider"]["friction"], json!(0.5));
        assert_eq!(gem["camera"], Value::Null, "absent sections stay absent");
        assert_eq!(
            gem["components"]["rusting.counter"],
            json!({"name": "gems", "value": 0, "target": null})
        );
    }

    #[test]
    fn set_scene_changes_scene_fields_but_not_entities() {
        let path = scene_file("settings");
        let outcome = patch_scene_file(&path, &patch(json!({"operations": [
            {"op": "set_scene", "path": "/name", "value": "Level 1", "expected": "Main"},
        ]})), false)
        .unwrap();
        assert_eq!(outcome.changes[0].id, Uuid::nil());
        assert_eq!(outcome.changes[0].path, "/name");
        let scene =
            parse_scene_document(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(scene.name, "Level 1");
        assert_eq!(scene.entities.len(), 2);

        let error = patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "set_scene", "path": "/entities/0", "value": null},
            ]})),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot change"), "{error}");

        let error = patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "set_scene", "path": "", "value": {"format_version": 1}},
            ]})),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot change"), "{error}");
    }

    #[test]
    fn a_mistyped_scene_setting_is_an_error() {
        let path = scene_file("typo");
        let error = patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "set_scene", "path": "/render/qualty", "value": "High"},
            ]})),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("/render/qualty"), "{error}");
    }

    #[test]
    fn operations_address_entities_by_unique_name() {
        let path = scene_file("names");
        patch_scene_file(&path, &patch(json!({"operations": [
            {"op": "create", "entity": {"name": "Shelf", "parent": "Cube"}},
            {"op": "set", "id": "Shelf", "path": "/visible", "value": false},
            {"op": "reparent", "id": "Lamp", "parent": "Shelf"},
        ]})), false)
        .unwrap();
        let scene =
            parse_scene_document(&std::fs::read(&path).unwrap()).unwrap();
        let shelf = &scene.entities[2];
        assert_eq!(shelf.parent, Some(CUBE.parse().unwrap()));
        assert_eq!(shelf.visible, Some(false));
        assert_eq!(scene.entities[1].parent, Some(shelf.id));

        let error = patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "delete", "id": "Nobody"},
            ]})),
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("no entity is named `Nobody`"));
    }

    #[test]
    fn a_bad_field_in_a_created_entity_is_named_by_its_path() {
        let path = scene_file("bad_field");
        let error = patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "create", "entity": {"name": "Floor",
                    "mesh_renderer": {"mesh": {"BuiltinPrimitive": "Cube"},
                        "material": {"Inline": {"base_color": [1, 0]}}}}}
            ]})),
            true,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("(`Floor`): /mesh_renderer/material/Inline/base_color: invalid length 2"),
            "{error}"
        );
        // RGB is fine: alpha defaults to 1.
        patch_scene_file(
            &path,
            &patch(json!({"operations": [
                {"op": "create", "entity": {"name": "Floor",
                    "mesh_renderer": {"mesh": {"BuiltinPrimitive": "Cube"},
                        "material": {"Inline": {"base_color": [1, 0, 0]}}}}}
            ]})),
            true,
        )
        .unwrap();
    }

    #[test]
    fn delete_can_skip_a_missing_entity_and_upsert_can_merge() {
        let path = scene_file("merge");
        let run = patch(json!({"operations": [
            {"op": "delete", "id": "Gone", "missing_ok": true},
            {"op": "upsert", "merge": true,
             "entity": {"name": "Cube", "transform": {"position": [0, 5, 0]}, "visible": null}},
        ]}));
        let before =
            parse_scene_document(&std::fs::read(&path).unwrap()).unwrap();
        patch_scene_file(&path, &run, false).unwrap();
        patch_scene_file(&path, &run, false).unwrap();
        let after =
            parse_scene_document(&std::fs::read(&path).unwrap()).unwrap();
        let cube = |document: &SceneDocument| {
            document
                .entities
                .iter()
                .find(|entity| entity.name.as_deref() == Some("Cube"))
                .unwrap()
                .clone()
        };
        let (old, new) = (cube(&before), cube(&after));
        let (old_t, new_t) = (old.transform.unwrap(), new.transform.unwrap());
        assert_eq!(new_t.position, [0.0, 5.0, 0.0]);
        assert_eq!(new_t.scale, old_t.scale);
        assert_eq!(new.mesh_renderer, old.mesh_renderer, "kept");
        let strict =
            patch(json!({"operations": [{"op": "delete", "id": "Gone"}]}));
        assert!(patch_scene_file(&path, &strict, false).is_err());
    }

    #[test]
    fn a_patch_of_upserts_runs_twice_and_keeps_ids() {
        let path = scene_file("upsert");
        let layout = patch(json!({"operations": [
            {"op": "upsert", "entity": {"name": "Shelf", "parent": "Cube", "visible": false,
             "transform": {"position": [13.0, 0.85, -17.0]},
             "point_light": {"color": [0.9, 0.8, 0.7], "intensity": 0.3, "range": 2.0}}},
            {"op": "reparent", "id": "Lamp", "parent": "Shelf"},
        ]}));
        patch_scene_file(&path, &layout, false).unwrap();
        let first =
            parse_scene_document(&std::fs::read(&path).unwrap()).unwrap();
        let rerun = patch_scene_file(&path, &layout, false).unwrap();
        assert!(rerun.changes.is_empty(), "{:?}", rerun.changes);
        assert_eq!(rerun.revision_before, rerun.revision_after);
        let second =
            parse_scene_document(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(second.entities.len(), 3);
        assert_eq!(second.entities[2].id, first.entities[2].id);
        assert_eq!(second.entities[2].visible, Some(false));
        assert_eq!(second.entities[1].parent, Some(first.entities[2].id));
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
