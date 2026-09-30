//! Engine integration for reusable named input actions.

use bevy_ecs::prelude::{Component, World};
use serde::{Deserialize, Deserializer, Serialize};

pub use rusting_core::input::{ActionMap, InputBinding};

use super::{KeyCode, MouseButton};

pub(super) fn install(world: &mut World) {
    world.insert_resource(ActionMap::default());
}

/// Binds a named action to keys and mouse buttons from scene data, so game
/// code and scenarios can use an action no Rust code bound. Inputs are winit
/// key names (`KeyF`, `Space`, `ArrowUp`, `Digit1`) or `MouseLeft`,
/// `MouseRight` and `MouseMiddle`. Removing the component does not unbind.
#[derive(
    Component, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(default)]
pub struct InputAction {
    pub action: String,
    #[serde(deserialize_with = "input_names")]
    pub inputs: Vec<String>,
}

/// The binding an input name stands for.
pub fn parse_input(name: &str) -> Result<InputBinding, String> {
    let mouse = match name {
        "MouseLeft" => Some(MouseButton::Left),
        "MouseRight" => Some(MouseButton::Right),
        "MouseMiddle" => Some(MouseButton::Middle),
        _ => None,
    };
    if let Some(button) = mouse {
        return Ok(InputBinding::Mouse(button));
    }
    serde_json::from_value::<KeyCode>(serde_json::Value::from(name))
        .map(InputBinding::Key)
        .map_err(|_| {
            format!(
                "unknown input `{name}`: use a key name such as KeyF, Space \
                 or ArrowUp, or MouseLeft, MouseRight or MouseMiddle"
            )
        })
}

fn input_names<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    let names = Vec::<String>::deserialize(deserializer)?;
    for name in &names {
        parse_input(name).map_err(serde::de::Error::custom)?;
    }
    Ok(names)
}

/// Adds every [`InputAction`]'s bindings to the [`ActionMap`]. Runs each
/// frame; bindings already there are skipped.
pub fn bind_input_actions(world: &mut World) {
    let mut query = world.query::<&InputAction>();
    let bindings: Vec<(String, InputBinding)> = query
        .iter(world)
        .flat_map(|action| {
            action.inputs.iter().filter_map(|name| {
                Some((action.action.clone(), parse_input(name).ok()?))
            })
        })
        .collect();
    let mut map = world.resource_mut::<ActionMap>();
    for (action, binding) in bindings {
        if !map.bindings(&action).contains(&binding) {
            map.bind(action, binding);
        }
    }
}
