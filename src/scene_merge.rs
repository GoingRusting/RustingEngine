//! Three-way merge of scene files by entity ID and field, for
//! `rusting merge` and the git merge driver it backs.

use serde_json::{Map, Value};

/// A field both sides changed differently. The merged scene keeps ours.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MergeConflict {
    /// Entity ID, or `None` for a scene-level field.
    pub entity: Option<String>,
    pub name: Option<String>,
    /// The field, such as `transform` or `components/game.health`; `entity`
    /// when one side deleted the entity and the other changed it.
    pub field: String,
}

/// Merges `ours` and `theirs`, both edited from `base`. Entities are matched
/// by `id`; a change on one side wins; the same change on both sides is
/// kept once; different changes to one field (each `components` entry is
/// its own field) are a conflict that keeps ours. Entities keep our order,
/// followed by entities only theirs added, in their order. An entity one
/// side deleted and the other changed is kept, with a conflict.
#[must_use]
pub fn merge_scenes(
    base: &Value,
    ours: &Value,
    theirs: &Value,
) -> (Value, Vec<MergeConflict>) {
    let object =
        |value: &'_ Value| value.as_object().cloned().unwrap_or_default();
    let (base, ours, theirs) = (object(base), object(ours), object(theirs));
    let mut conflicts = Vec::new();
    let mut merged =
        merge_fields(&base, &ours, &theirs, &["entities"], |field| {
            conflicts.push(MergeConflict {
                entity: None,
                name: None,
                field,
            });
        });
    let list = |scene: &Map<String, Value>| -> Vec<Map<String, Value>> {
        scene
            .get("entities")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entity| entity.as_object().cloned())
            .collect()
    };
    let (base_list, our_list, their_list) =
        (list(&base), list(&ours), list(&theirs));
    let find = |list: &[Map<String, Value>], id: &Value| {
        list.iter()
            .find(|entity| entity.get("id") == Some(id))
            .cloned()
    };
    let mut order: Vec<Value> = our_list
        .iter()
        .filter_map(|e| e.get("id").cloned())
        .collect();
    for entity in &their_list {
        let id = entity.get("id").cloned().unwrap_or(Value::Null);
        if !order.contains(&id) && find(&base_list, &id).is_none() {
            order.push(id);
        }
    }
    // Deleted by ours but changed by theirs: keep it, at the end.
    for entity in &their_list {
        let id = entity.get("id").cloned().unwrap_or(Value::Null);
        if !order.contains(&id)
            && find(&base_list, &id).as_ref() != Some(entity)
        {
            order.push(id);
        }
    }
    let mut entities = Vec::new();
    for id in order {
        let (b, o, t) = (
            find(&base_list, &id),
            find(&our_list, &id),
            find(&their_list, &id),
        );
        let name = o
            .as_ref()
            .or(t.as_ref())
            .and_then(|e| e.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let conflict = |field: String, conflicts: &mut Vec<MergeConflict>| {
            conflicts.push(MergeConflict {
                entity: id.as_str().map(str::to_owned),
                name: name.clone(),
                field,
            });
        };
        let entity = match (&b, &o, &t) {
            (_, Some(o), Some(t)) => {
                let b = b.clone().unwrap_or_default();
                let mut found = Vec::new();
                let mut entity =
                    merge_fields(&b, o, t, &["components"], |f| found.push(f));
                let part = |side: &Map<String, Value>| {
                    object(side.get("components").unwrap_or(&Value::Null))
                };
                let components =
                    merge_fields(&part(&b), &part(o), &part(t), &[], |f| {
                        found.push(format!("components/{f}"));
                    });
                entity.insert("components".into(), Value::Object(components));
                for field in found {
                    conflict(field, &mut conflicts);
                }
                Some(entity)
            }
            // One side deleted it: gone unless the other side changed it.
            (Some(b), None, Some(kept)) | (Some(b), Some(kept), None) => {
                (kept != b).then(|| {
                    conflict("entity".into(), &mut conflicts);
                    kept.clone()
                })
            }
            (None, Some(kept), None) | (None, None, Some(kept)) => {
                Some(kept.clone())
            }
            _ => None,
        };
        entities.extend(entity.map(Value::Object));
    }
    merged.insert("entities".into(), Value::Array(entities));
    (Value::Object(merged), conflicts)
}

/// Three-way merge of each field of an object, skipping `skip`. A missing
/// field is `null`, so an added or removed field merges like a change.
fn merge_fields(
    base: &Map<String, Value>,
    ours: &Map<String, Value>,
    theirs: &Map<String, Value>,
    skip: &[&str],
    mut conflict: impl FnMut(String),
) -> Map<String, Value> {
    let mut merged = Map::new();
    let keys = ours
        .keys()
        .chain(theirs.keys())
        .filter(|key| !skip.contains(&key.as_str()));
    for key in keys {
        if merged.contains_key(key) {
            continue;
        }
        let (b, o, t) = (base.get(key), ours.get(key), theirs.get(key));
        let value = if o == b {
            t
        } else if t == b || o == t {
            o
        } else {
            conflict(key.clone());
            o
        };
        if let Some(value) = value {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entity(id: &str, x: f64) -> Value {
        json!({"id": id, "name": id, "transform": {"position": [x, 0.0, 0.0]}, "components": {}})
    }

    fn scene(entities: Vec<Value>) -> Value {
        json!({"format_version": 9, "name": "S", "entities": entities})
    }

    fn ids(scene: &Value) -> Vec<&str> {
        scene["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn edits_to_different_entities_and_fields_merge_cleanly() {
        let base =
            scene(vec![entity("a", 0.0), entity("b", 0.0), entity("d", 0.0)]);
        let mut renamed = entity("b", 0.0);
        renamed["name"] = json!("Bee");
        let mut tagged_ours = entity("a", 5.0);
        tagged_ours["components"] = json!({"game.x": "1"});
        let ours = scene(vec![tagged_ours, entity("b", 0.0)]);
        let mut tagged_theirs = entity("a", 0.0);
        tagged_theirs["components"] = json!({"game.y": "2"});
        let theirs = scene(vec![
            tagged_theirs,
            renamed,
            entity("d", 0.0),
            entity("c", 1.0),
        ]);

        let (merged, conflicts) = merge_scenes(&base, &ours, &theirs);
        assert_eq!(conflicts, []);
        assert_eq!(ids(&merged), ["a", "b", "c"]);
        let a = &merged["entities"][0];
        assert_eq!(a["transform"]["position"][0], 5.0);
        assert_eq!(a["components"], json!({"game.x": "1", "game.y": "2"}));
        assert_eq!(merged["entities"][1]["name"], "Bee");
    }

    #[test]
    fn the_same_field_changed_twice_is_a_conflict_that_keeps_ours() {
        let base = scene(vec![entity("a", 0.0), entity("e", 0.0)]);
        let ours = scene(vec![entity("a", 1.0)]);
        let theirs = scene(vec![entity("a", 2.0), entity("e", 9.0)]);

        let (merged, conflicts) = merge_scenes(&base, &ours, &theirs);
        assert_eq!(merged["entities"][0]["transform"]["position"][0], 1.0);
        let fields: Vec<_> = conflicts
            .iter()
            .map(|c| (c.entity.as_deref(), c.field.as_str()))
            .collect();
        assert_eq!(fields, [(Some("a"), "transform"), (Some("e"), "entity")]);
        // Deleted by us, moved by them: kept so their change is not lost.
        assert_eq!(ids(&merged), ["a", "e"]);
    }
}
