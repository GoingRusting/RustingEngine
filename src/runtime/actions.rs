//! Named input actions decoupled from raw devices.
//!
//! Gameplay code binds an action name ("jump", "fire") to one or more
//! [`InputBinding`]s and queries the action instead of raw keys/buttons, so
//! rebinding never touches gameplay code. `InputBinding` is an enum so a
//! future `Gamepad(GamepadButton)` variant slots in without changing the
//! `ActionMap` API.

use std::collections::HashMap;

use bevy_ecs::prelude::{Resource, World};

use crate::runtime::input::{KeyCode, MouseButton};
use crate::runtime::RuntimeInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputBinding {
    Key(KeyCode),
    Mouse(MouseButton),
}

/// Maps action names to one or more raw input bindings (any bound input
/// satisfies the action).
#[derive(Resource, Default)]
pub struct ActionMap {
    bindings: HashMap<String, Vec<InputBinding>>,
}

impl ActionMap {
    pub fn bind(
        &mut self,
        action: impl Into<String>,
        binding: InputBinding,
    ) -> &mut Self {
        self.bindings
            .entry(action.into())
            .or_default()
            .push(binding);
        self
    }

    #[must_use]
    pub fn held(&self, input: &RuntimeInput, action: &str) -> bool {
        self.bindings_for(action).any(|binding| match binding {
            InputBinding::Key(key) => input.key_held(*key),
            InputBinding::Mouse(button) => input.mouse_held(*button),
        })
    }

    #[must_use]
    pub fn just_pressed(&self, input: &RuntimeInput, action: &str) -> bool {
        self.bindings_for(action).any(|binding| match binding {
            InputBinding::Key(key) => input.key_just_pressed(*key),
            InputBinding::Mouse(button) => input.mouse_just_pressed(*button),
        })
    }

    #[must_use]
    pub fn just_released(&self, input: &RuntimeInput, action: &str) -> bool {
        self.bindings_for(action).any(|binding| match binding {
            InputBinding::Key(key) => input.key_just_released(*key),
            InputBinding::Mouse(button) => input.mouse_just_released(*button),
        })
    }

    fn bindings_for(
        &self,
        action: &str,
    ) -> impl Iterator<Item = &InputBinding> {
        self.bindings.get(action).into_iter().flatten()
    }
}

pub(super) fn install(world: &mut World) {
    world.insert_resource(ActionMap::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_fires_when_any_bound_input_is_active() {
        let mut map = ActionMap::default();
        map.bind("jump", InputBinding::Key(KeyCode::Space));
        map.bind("jump", InputBinding::Mouse(MouseButton::Left));

        let mut input = RuntimeInput::default();
        assert!(!map.just_pressed(&input, "jump"));

        input.record_key(KeyCode::Space, true);
        assert!(map.just_pressed(&input, "jump"));
        assert!(map.held(&input, "jump"));

        input.clear_frame_edges();
        assert!(!map.just_pressed(&input, "jump"));
        assert!(map.held(&input, "jump"));

        input.record_key(KeyCode::Space, false);
        assert!(map.just_released(&input, "jump"));

        assert!(!map.held(&input, "unbound_action"));
    }
}
