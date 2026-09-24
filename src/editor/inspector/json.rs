//! Typed Inspector widgets for game-defined components, which the scene
//! registry stores as JSON.

use egui::{DragValue, Ui};
use serde_json::Value;

use super::widgets;

/// Draws `value` as typed rows and returns true when the user changed it.
/// Numbers keep their integer or float kind so the component still
/// deserializes after an edit.
pub fn edit_value(ui: &mut Ui, label: &str, value: &mut Value) -> bool {
    match value {
        Value::Null => {
            widgets::value(ui, label, "None");
            false
        }
        Value::Bool(value) => widgets::checkbox(ui, label, value),
        Value::String(value) => widgets::text(ui, label, value),
        Value::Number(number) => {
            if let Some(mut integer) = number.as_i64() {
                let changed =
                    widgets::drag(ui, label, DragValue::new(&mut integer));
                if changed {
                    *number = integer.into();
                }
                changed
            } else {
                let mut float = number.as_f64().unwrap_or_default();
                let changed = widgets::drag(
                    ui,
                    label,
                    DragValue::new(&mut float).speed(0.01),
                );
                if let Some(edited) =
                    changed.then(|| serde_json::Number::from_f64(float))
                {
                    *number = edited.unwrap_or_else(|| number.clone());
                }
                changed
            }
        }
        Value::Array(items) => match vec3_kind(items) {
            Some(true) => {
                let mut values =
                    items.iter().map(|item| item.as_i64().unwrap_or_default());
                let mut values =
                    [(); 3].map(|()| values.next().unwrap_or_default());
                let changed = widgets::vec3(ui, label, &mut values, 1.0);
                if changed {
                    *items = values.iter().map(|&value| value.into()).collect();
                }
                changed
            }
            Some(false) => {
                let mut values =
                    items.iter().map(|item| item.as_f64().unwrap_or_default());
                let mut values =
                    [(); 3].map(|()| values.next().unwrap_or_default());
                let changed = widgets::vec3(ui, label, &mut values, 0.01);
                if changed {
                    *items = values.iter().map(|&value| value.into()).collect();
                }
                changed
            }
            None => nested(ui, label, |ui| {
                let mut changed = false;
                for (index, item) in items.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        changed |= edit_value(ui, &format!("[{index}]"), item);
                    });
                }
                changed
            }),
        },
        Value::Object(fields) => nested(ui, label, |ui| {
            let mut changed = false;
            for (name, field) in fields.iter_mut() {
                ui.push_id(name.as_str(), |ui| {
                    changed |= edit_value(ui, &field_label(name), field);
                });
            }
            changed
        }),
    }
}

/// Draws the fields of a component's top-level JSON object directly, without
/// an extra nested header.
pub fn edit_component(ui: &mut Ui, value: &mut Value) -> bool {
    match value {
        Value::Object(fields) => {
            let mut changed = false;
            for (name, field) in fields.iter_mut() {
                ui.push_id(name.as_str(), |ui| {
                    changed |= edit_value(ui, &field_label(name), field);
                });
            }
            changed
        }
        other => edit_value(ui, "Value", other),
    }
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

/// `Some(true)` for three integers, `Some(false)` for three numbers with at
/// least one float, `None` for anything that is not a 3-vector.
fn vec3_kind(items: &[Value]) -> Option<bool> {
    (items.len() == 3 && items.iter().all(Value::is_number))
        .then(|| items.iter().all(|item| item.is_i64()))
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
    use super::*;
    use serde_json::json;

    #[test]
    fn three_number_arrays_become_vectors_that_keep_their_number_kind() {
        assert_eq!(vec3_kind(&[json!(1), json!(2), json!(3)]), Some(true));
        assert_eq!(vec3_kind(&[json!(1), json!(2.5), json!(3)]), Some(false));
        assert_eq!(vec3_kind(&[json!(1), json!(2)]), None);
        assert_eq!(vec3_kind(&[json!(1), json!("a"), json!(3)]), None);
    }

    #[test]
    fn field_names_become_captions() {
        assert_eq!(field_label("move_speed"), "Move Speed");
        assert_eq!(field_label("hp"), "Hp");
    }

    #[test]
    fn drawing_without_input_leaves_the_value_unchanged() {
        let original = json!({
            "speed": 2.5,
            "lives": 3,
            "alive": true,
            "tag": "player",
            "spawn": [0.0, 1.0, 2.0],
            "cells": [1, 2, 3],
            "stats": { "armor": 4, "extra": [1, 2, 3, 4] },
            "target": null
        });
        let mut value = original.clone();
        let context = egui::Context::default();
        let mut changed = true;
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                changed = edit_component(ui, &mut value);
            });
        });
        assert!(!changed);
        assert_eq!(value, original);
    }
}
