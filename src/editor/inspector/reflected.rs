//! Inspector rows for registered components, drawn from their reflected
//! types: each field gets the widget its type and hints call for, with
//! ranges, units, and variant lists from the description.

use std::path::Path;

use bevy_ecs::prelude::World;
use egui::{ecolor, DragValue, Ui};
use serde_json::{json, Value};

use super::widgets;
use crate::assets::{read_data_file, AssetServer};
use crate::reflect::{
    variant_zero, AssetKind, FieldInfo, Hints, TypeInfo, VariantFields,
    ASSET_KEY, DATA_KEY,
};
use crate::runtime::{Name, SceneId};

/// Draws a component's fields and returns true when the user changed one.
/// `value` is the component's scene form; `world` supplies the objects and
/// assets that references can pick.
pub fn edit_component(
    ui: &mut Ui,
    world: &World,
    info: &TypeInfo,
    value: &mut Value,
) -> bool {
    match (info, value) {
        (TypeInfo::Struct(info), Value::Object(map)) => {
            edit_fields(ui, world, &info.fields, map)
        }
        (info, value) => {
            edit_value(ui, world, "Value", info, &Hints::default(), value)
        }
    }
}

fn edit_fields(
    ui: &mut Ui,
    world: &World,
    fields: &[FieldInfo],
    map: &mut serde_json::Map<String, Value>,
) -> bool {
    let mut changed = false;
    for field in fields {
        let Some(value) = map.get_mut(field.name) else {
            continue;
        };
        ui.push_id(field.name, |ui| {
            changed |= edit_value(
                ui,
                world,
                &field_label(field.name),
                &field.ty,
                &field.hints,
                value,
            );
        });
    }
    changed
}

fn edit_value(
    ui: &mut Ui,
    world: &World,
    label: &str,
    info: &TypeInfo,
    hints: &Hints,
    value: &mut Value,
) -> bool {
    match (info, value) {
        (TypeInfo::Bool, Value::Bool(value)) => {
            widgets::checkbox(ui, label, value)
        }
        (TypeInfo::Int { min, max, .. }, Value::Number(number)) => {
            let Some(mut integer) = number.as_i64() else {
                widgets::value(ui, label, &number.to_string());
                return false;
            };
            let low =
                hints.min.map_or(*min, |low| (low.ceil() as i128).max(*min));
            let high = hints
                .max
                .map_or(*max, |high| (high.floor() as i128).min(*max));
            let clamp = |bound: i128| {
                bound.clamp(i64::MIN.into(), i64::MAX.into()) as i64
            };
            // A saved value outside the hint range stays until the user
            // drags it; clamping on draw would edit the scene unasked.
            let drag = DragValue::new(&mut integer)
                .range(clamp(low)..=clamp(high))
                .clamp_existing_to_range(false);
            let changed = widgets::drag(ui, label, with_unit(drag, hints));
            if changed {
                *number = integer.into();
            }
            changed
        }
        (TypeInfo::Float, Value::Number(number)) => {
            let mut float = number.as_f64().unwrap_or_default();
            let drag = DragValue::new(&mut float)
                .speed(0.01)
                .range(
                    hints.min.unwrap_or(f64::NEG_INFINITY)
                        ..=hints.max.unwrap_or(f64::INFINITY),
                )
                .clamp_existing_to_range(false);
            let changed = widgets::drag(ui, label, with_unit(drag, hints));
            if let Some(edited) = changed
                .then(|| serde_json::Number::from_f64(float))
                .flatten()
            {
                *number = edited;
            }
            changed
        }
        (TypeInfo::String | TypeInfo::Path, Value::String(text)) => {
            widgets::text(ui, label, text)
        }
        (TypeInfo::Entity, value) => {
            let objects = scene_objects(world);
            pick(ui, label, value, &objects)
        }
        (TypeInfo::Handle(kind), value) => {
            edit_handle(ui, world, label, kind, value)
        }
        (TypeInfo::Array(item, len), Value::Array(items)) => {
            edit_array(ui, world, label, item, *len, hints, items)
        }
        (TypeInfo::List(item), Value::Array(items)) => {
            nested(ui, label, |ui| {
                let mut changed = false;
                let mut removed = None;
                for (index, value) in items.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .small_button("−")
                                .on_hover_text("Remove")
                                .clicked()
                            {
                                removed = Some(index);
                            }
                            ui.vertical(|ui| {
                                changed |= edit_value(
                                    ui,
                                    world,
                                    &format!("[{index}]"),
                                    item,
                                    &Hints::default(),
                                    value,
                                );
                            });
                        });
                    });
                }
                if let Some(index) = removed {
                    items.remove(index);
                    changed = true;
                }
                let initial = initial_value(world, item);
                let addable = !(initial.is_null()
                    && matches!(
                        **item,
                        TypeInfo::Entity | TypeInfo::Handle(_)
                    ));
                if ui
                    .add_enabled(addable, egui::Button::new("+").small())
                    .on_hover_text("Add item")
                    .on_disabled_hover_text("Nothing to reference yet")
                    .clicked()
                {
                    items.push(initial);
                    changed = true;
                }
                changed
            })
        }
        // ponytail: map keys cannot be added or renamed here yet; see the
        // backlog in docs/editor-overhaul.md.
        (TypeInfo::Map(item), Value::Object(map)) => nested(ui, label, |ui| {
            let mut changed = false;
            for (key, value) in map.iter_mut() {
                ui.push_id(key.as_str(), |ui| {
                    changed |= edit_value(
                        ui,
                        world,
                        key,
                        item,
                        &Hints::default(),
                        value,
                    );
                });
            }
            changed
        }),
        (TypeInfo::Option(inner), value) => {
            let mut present = !value.is_null();
            let mut changed = widgets::checkbox(ui, label, &mut present);
            if changed {
                *value = if present {
                    initial_value(world, inner)
                } else {
                    Value::Null
                };
                if present && value.is_null() {
                    // Nothing to reference yet: the option stays empty.
                    present = false;
                    changed = false;
                }
            }
            if present {
                ui.indent(label, |ui| {
                    changed |=
                        edit_value(ui, world, "Value", inner, hints, value);
                });
            }
            changed
        }
        (TypeInfo::Struct(info), Value::Object(map)) => {
            nested(ui, label, |ui| edit_fields(ui, world, &info.fields, map))
        }
        (TypeInfo::Enum(info), value) => {
            let current = match &*value {
                Value::String(name) => name.clone(),
                Value::Object(map) if map.len() == 1 => {
                    map.keys().next().cloned().unwrap_or_default()
                }
                other => return mismatch(ui, label, other),
            };
            let Some(variant) = info.variant(&current) else {
                return mismatch(ui, label, value);
            };
            let options: Vec<(&'static str, &str)> = info
                .variants
                .iter()
                .map(|variant| (variant.name, variant.name))
                .collect();
            let mut selected = variant.name;
            if widgets::choice(ui, label, &mut selected, &options) {
                let chosen = info.variant(selected).expect("listed variant");
                *value = variant_zero(chosen);
                return true;
            }
            let Value::Object(map) = value else {
                return false;
            };
            let Some(payload) = map.get_mut(variant.name) else {
                return false;
            };
            let mut changed = false;
            ui.indent(label, |ui| {
                changed = match (&variant.fields, payload) {
                    (VariantFields::Struct(fields), Value::Object(map)) => {
                        edit_fields(ui, world, fields, map)
                    }
                    (VariantFields::Tuple(items), payload)
                        if items.len() == 1 =>
                    {
                        edit_value(
                            ui,
                            world,
                            "Value",
                            &items[0],
                            &Hints::default(),
                            payload,
                        )
                    }
                    (VariantFields::Tuple(items), Value::Array(values)) => {
                        let mut changed = false;
                        for (index, (item, value)) in
                            items.iter().zip(values).enumerate()
                        {
                            ui.push_id(index, |ui| {
                                changed |= edit_value(
                                    ui,
                                    world,
                                    &format!("[{index}]"),
                                    item,
                                    &Hints::default(),
                                    value,
                                );
                            });
                        }
                        changed
                    }
                    (_, other) => mismatch(ui, "Value", other),
                };
            });
            changed
        }
        (_, other) => mismatch(ui, label, other),
    }
}

/// Scene objects a reference can point at, as (scene form, caption),
/// sorted by caption.
fn scene_objects(world: &World) -> Vec<(Value, String)> {
    let Some(mut query) = world.try_query::<(&SceneId, Option<&Name>)>() else {
        return Vec::new();
    };
    let mut objects: Vec<_> = query
        .iter(world)
        .map(|(id, name)| {
            let caption = name.map_or_else(
                || id.0.to_string()[..8].to_owned(),
                |name| name.0.clone(),
            );
            (Value::String(id.0.to_string()), caption)
        })
        .collect();
    objects.sort_by(|a, b| (&a.1, a.0.as_str()).cmp(&(&b.1, b.0.as_str())));
    objects
}

/// Loaded assets of `kind`, as (scene form, caption).
// ponytail: only assets something already loaded are listed; drag from the
// Assets panel or a file browser would reach the rest.
fn loaded_assets(world: &World, kind: AssetKind) -> Vec<(Value, String)> {
    world
        .get_resource::<AssetServer>()
        .map(|assets| kind.loaded_paths(assets))
        .unwrap_or_default()
        .into_iter()
        .map(|path| {
            let caption = path.display().to_string();
            (json!({ ASSET_KEY: path }), caption)
        })
        .collect()
}

/// Caption of the choice that keeps a data asset's values in the object.
const UNIQUE: &str = "Unique (saved with this object)";

/// A drop-down of loaded files. Data asset handles also offer `Unique`,
/// which keeps a private copy in the object, like Godot's Make Unique, and
/// edit that copy's fields right here.
fn edit_handle(
    ui: &mut Ui,
    world: &World,
    label: &str,
    kind: &AssetKind,
    value: &mut Value,
) -> bool {
    let mut choices = loaded_assets(world, *kind);
    let Some(embedded) = kind.embedded else {
        return pick(ui, label, value, &choices);
    };
    // Switching a file reference to Unique starts from the file's values.
    let unique = if value.get(DATA_KEY).is_some() {
        value.clone()
    } else {
        let file = value
            .get(ASSET_KEY)
            .and_then(Value::as_str)
            .and_then(|path| read_data_file(Path::new(path)).ok());
        json!({ DATA_KEY: file.map_or_else(embedded.default, |(_, data)| data) })
    };
    choices.push((unique, UNIQUE.to_owned()));
    let mut changed = pick(ui, label, value, &choices);
    if let Some(data) = value.get_mut(DATA_KEY) {
        ui.indent(label, |ui| {
            changed |= edit_component(ui, world, &(embedded.info)(), data);
        });
    }
    changed
}

/// The value a new list item or a switched-on option starts with: the
/// first object or asset for references, or the type's zero value.
fn initial_value(world: &World, info: &TypeInfo) -> Value {
    let first = |choices: Vec<(Value, String)>| {
        choices
            .into_iter()
            .next()
            .map_or(Value::Null, |(value, _)| value)
    };
    match info {
        TypeInfo::Entity => first(scene_objects(world)),
        TypeInfo::Handle(kind) => {
            match (loaded_assets(world, *kind), kind.embedded) {
                (assets, Some(embedded)) if assets.is_empty() => {
                    json!({ DATA_KEY: (embedded.default)() })
                }
                (assets, _) => first(assets),
            }
        }
        other => other.zero_value(),
    }
}

/// A drop-down over `choices`. It commits only on selection, so a
/// reference never holds a half-typed ID or path.
fn pick(
    ui: &mut Ui,
    label: &str,
    value: &mut Value,
    choices: &[(Value, String)],
) -> bool {
    const CURRENT: usize = usize::MAX;
    let mut options: Vec<(usize, &str)> = choices
        .iter()
        .enumerate()
        .map(|(index, (_, caption))| (index, caption.as_str()))
        .collect();
    let mut selected = choices
        .iter()
        .position(|(choice, _)| choice == value)
        .unwrap_or(CURRENT);
    let current = match &*value {
        Value::Null => "None".to_owned(),
        Value::String(text) => format!("{text} (missing)"),
        other if other.get(DATA_KEY).is_some() => UNIQUE.to_owned(),
        other => other.get(ASSET_KEY).and_then(Value::as_str).map_or_else(
            || other.to_string(),
            |path| format!("{path} (missing)"),
        ),
    };
    if selected == CURRENT {
        options.push((CURRENT, &current));
    }
    if widgets::choice(ui, label, &mut selected, &options)
        && selected != CURRENT
    {
        *value = choices[selected].0.clone();
        return true;
    }
    false
}

fn edit_array(
    ui: &mut Ui,
    world: &World,
    label: &str,
    item: &TypeInfo,
    len: usize,
    hints: &Hints,
    items: &mut Vec<Value>,
) -> bool {
    let floats = |items: &[Value]| -> Option<Vec<f64>> {
        items.iter().map(Value::as_f64).collect()
    };
    // egui's color buttons edit linear values; sRGB fields convert around
    // them. Alpha is linear either way.
    let srgb = hints.unit.starts_with("sRGB");
    let to_linear = |channel: f64| {
        let channel = channel as f32;
        if srgb {
            ecolor::linear_from_gamma(channel)
        } else {
            channel
        }
    };
    let from_linear = |channel: f32| {
        f64::from(if srgb {
            ecolor::gamma_from_linear(channel)
        } else {
            channel
        })
    };
    match (item, len, floats(items)) {
        (TypeInfo::Float, 3, Some(values)) if hints.color => {
            let mut rgb = [0, 1, 2].map(|index| to_linear(values[index]));
            let changed = widgets::color(ui, label, &mut rgb);
            if changed {
                *items = rgb
                    .iter()
                    .map(|&channel| from_linear(channel).into())
                    .collect();
            }
            changed
        }
        (TypeInfo::Float, 4, Some(values)) if hints.color => {
            let mut rgba = [
                to_linear(values[0]),
                to_linear(values[1]),
                to_linear(values[2]),
                values[3] as f32,
            ];
            let changed = widgets::color4(ui, label, &mut rgba);
            if changed {
                *items = vec![
                    from_linear(rgba[0]).into(),
                    from_linear(rgba[1]).into(),
                    from_linear(rgba[2]).into(),
                    f64::from(rgba[3]).into(),
                ];
            }
            changed
        }
        (TypeInfo::Float, 3, Some(values)) => {
            let mut values = [values[0], values[1], values[2]];
            let changed = widgets::vec3(ui, label, &mut values, 0.01);
            if changed {
                *items = values.iter().map(|&value| value.into()).collect();
            }
            changed
        }
        (TypeInfo::Int { .. }, 3, _) if items.iter().all(Value::is_i64) => {
            let mut values = [0, 1, 2]
                .map(|index| items[index].as_i64().unwrap_or_default());
            let changed = widgets::vec3(ui, label, &mut values, 1.0);
            if changed {
                *items = values.iter().map(|&value| value.into()).collect();
            }
            changed
        }
        _ => nested(ui, label, |ui| {
            let mut changed = false;
            for (index, value) in items.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    changed |= edit_value(
                        ui,
                        world,
                        &format!("[{index}]"),
                        item,
                        &Hints::default(),
                        value,
                    );
                });
            }
            changed
        }),
    }
}

fn with_unit<'a>(drag: DragValue<'a>, hints: &Hints) -> DragValue<'a> {
    // Long units ("logical pixels") do not fit the field; the schema has them.
    if hints.unit.is_empty() || hints.unit.chars().count() > 5 {
        drag
    } else {
        drag.suffix(format!(" {}", hints.unit))
    }
}

/// A value that does not have its type's shape, shown read-only.
fn mismatch(ui: &mut Ui, label: &str, value: &Value) -> bool {
    widgets::value(ui, label, &value.to_string());
    false
}

fn nested(
    ui: &mut Ui,
    label: &str,
    body: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    egui::CollapsingHeader::new(label)
        .id_salt(ui.id().with(label))
        .default_open(true)
        .show(ui, body)
        .body_returned
        .unwrap_or(false)
}

/// `move_speed` -> `Move Speed`, matching Godot's property captions.
fn field_label(name: &str) -> String {
    name.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use bevy_ecs::entity::Entity;
    use serde_json::json;

    use super::*;
    use crate::assets::{Handle, TextureAsset};
    use crate::reflect::Reflect;

    #[allow(dead_code)]
    enum Shape {
        Point,
        Circle { radius: f32 },
        Named(String),
    }

    #[allow(dead_code)]
    struct Everything {
        speed: f32,
        lives: u8,
        alive: bool,
        tag: String,
        file: PathBuf,
        spawn: [f32; 3],
        cells: [i32; 3],
        tint: [f32; 4],
        waypoints: Vec<[f32; 2]>,
        stats: BTreeMap<String, i32>,
        target: Option<Entity>,
        texture: Option<Handle<TextureAsset>>,
        shape: Shape,
        other: Shape,
    }

    crate::reflect! {
        enum Shape { Point, Circle { radius: f32 { min: 0.0 } }, Named(String) }
    }

    crate::reflect! {
        struct Everything {
            speed: f32 { unit: "m/s", min: 0.0 },
            lives: u8,
            alive: bool,
            tag: String,
            file: PathBuf,
            spawn: [f32; 3],
            cells: [i32; 3],
            tint: [f32; 4] { color: true },
            waypoints: Vec<[f32; 2]>,
            stats: BTreeMap<String, i32>,
            target: Option<Entity>,
            texture: Option<Handle<TextureAsset>>,
            shape: Shape,
            other: Shape,
        }
    }

    #[test]
    fn references_start_at_the_first_object_or_stay_empty() {
        let mut world = World::new();
        assert_eq!(initial_value(&world, &TypeInfo::Entity), Value::Null);
        let id = uuid::Uuid::new_v4();
        // Captions sort, so "Alarm" comes first whatever the spawn order.
        world.spawn((SceneId(uuid::Uuid::new_v4()), Name("Switch".into())));
        world.spawn((SceneId(id), Name("Alarm".into())));
        assert_eq!(
            initial_value(&world, &TypeInfo::Entity),
            Value::String(id.to_string())
        );
        assert_eq!(
            initial_value(&world, &Handle::<TextureAsset>::type_info()),
            Value::Null
        );
        assert_eq!(initial_value(&world, &TypeInfo::Float), json!(0.0));
    }

    #[test]
    fn field_names_become_captions() {
        assert_eq!(field_label("move_speed"), "Move Speed");
        assert_eq!(field_label("hp"), "Hp");
    }

    #[test]
    fn drawing_without_input_leaves_the_value_unchanged() {
        // Speed is below its hint minimum: drawing must not clamp it.
        let original = json!({
            "speed": -2.5,
            "lives": 3,
            "alive": true,
            "tag": "player",
            "file": "levels/one.json",
            "spawn": [0.0, 1.0, 2.0],
            "cells": [1, 2, 3],
            "tint": [1.0, 0.5, 0.25, 1.0],
            "waypoints": [[0.0, 1.0], [2.0, 3.0]],
            "stats": {"armor": 4},
            "target": "5a3f0c1e-7a2b-4c1d-9e8f-0123456789ab",
            "texture": {"$asset": "textures/crate.png"},
            "shape": {"Circle": {"radius": 1.5}},
            "other": "Point",
            "$version": 2
        });
        let mut value = original.clone();
        let context = egui::Context::default();
        let mut changed = true;
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                changed = edit_component(
                    ui,
                    &World::new(),
                    &Everything::type_info(),
                    &mut value,
                );
            });
        });
        assert!(!changed);
        assert_eq!(value, original);
    }
}
