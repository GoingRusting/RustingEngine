//! Runtime-side input capture and named action mapping.
//!
//! `RuntimeInput` holds raw keyboard and mouse state for gameplay systems.
//! Window integrations record raw events into it and clear edge state once per
//! rendered frame after gameplay systems have had a chance to read it.

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::Resource;
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// Held keys, mouse buttons, and cursor position for the current frame.
///
/// `just_pressed`/`just_released` sets are edge-triggered: they hold exactly
/// the keys or buttons that changed state since the last call to
/// [`RuntimeInput::clear_frame_edges`]. Runtime integrations call that method
/// once per rendered frame after gameplay systems have had a chance to read
/// them.
#[derive(Resource, Default)]
pub struct RuntimeInput {
    keys_held: HashSet<KeyCode>,
    keys_just_pressed: HashSet<KeyCode>,
    keys_just_released: HashSet<KeyCode>,
    mouse_held: HashSet<MouseButton>,
    mouse_just_pressed: HashSet<MouseButton>,
    mouse_just_released: HashSet<MouseButton>,
    cursor_position: Option<[f32; 2]>,
    viewport_size: [f32; 2],
}

impl RuntimeInput {
    #[must_use]
    pub fn key_held(&self, key: KeyCode) -> bool {
        self.keys_held.contains(&key)
    }
    #[must_use]
    pub fn key_just_pressed(&self, key: KeyCode) -> bool {
        self.keys_just_pressed.contains(&key)
    }
    #[must_use]
    pub fn key_just_released(&self, key: KeyCode) -> bool {
        self.keys_just_released.contains(&key)
    }
    #[must_use]
    pub fn mouse_held(&self, button: MouseButton) -> bool {
        self.mouse_held.contains(&button)
    }
    #[must_use]
    pub fn mouse_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_just_pressed.contains(&button)
    }
    #[must_use]
    pub fn mouse_just_released(&self, button: MouseButton) -> bool {
        self.mouse_just_released.contains(&button)
    }
    #[must_use]
    pub fn cursor_position(&self) -> Option<[f32; 2]> {
        self.cursor_position
    }

    /// Current window/render-viewport size in pixels, used to convert cursor
    /// positions into normalized device coordinates for gameplay picking.
    #[must_use]
    pub fn viewport_size(&self) -> [f32; 2] {
        self.viewport_size
    }

    /// Records one keyboard `WindowEvent`. Ignores OS auto-repeat: repeated
    /// presses do not re-fire `just_pressed`.
    pub fn record_key(&mut self, key: KeyCode, pressed: bool) {
        if pressed {
            if self.keys_held.insert(key) {
                self.keys_just_pressed.insert(key);
            }
        } else if self.keys_held.remove(&key) {
            self.keys_just_released.insert(key);
        }
    }

    /// Records one mouse button `WindowEvent`.
    pub fn record_mouse_button(&mut self, button: MouseButton, pressed: bool) {
        if pressed {
            if self.mouse_held.insert(button) {
                self.mouse_just_pressed.insert(button);
            }
        } else if self.mouse_held.remove(&button) {
            self.mouse_just_released.insert(button);
        }
    }

    /// Records the latest cursor position from a `CursorMoved` event.
    pub fn record_cursor_position(&mut self, position: [f32; 2]) {
        self.cursor_position = Some(position);
    }
    /// Records the current window/render-viewport size in pixels.
    pub fn record_viewport_size(&mut self, size: [f32; 2]) {
        self.viewport_size = size;
    }

    /// Clears this frame's edge sets. Runtime integrations call this once per
    /// rendered frame after gameplay systems have read them, so the next frame
    /// starts empty.
    pub fn clear_frame_edges(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.mouse_just_pressed.clear();
        self.mouse_just_released.clear();
    }
}

/// A raw input source which can satisfy a named gameplay action.
///
/// The enum deliberately leaves room for a future `Gamepad(GamepadButton)`
/// variant without changing the [`ActionMap`] API.
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::Resource;
    #[test]
    fn runtime_input_and_action_map_are_ecs_resources() {
        fn assert_resource<T: Resource>() {}
        assert_resource::<RuntimeInput>();
        assert_resource::<ActionMap>();
    }
    #[test]
    fn keyboard_edges_and_auto_repeat_are_tracked() {
        let mut input = RuntimeInput::default();
        input.record_key(KeyCode::Space, true);
        assert!(input.key_held(KeyCode::Space));
        assert!(input.key_just_pressed(KeyCode::Space));
        input.record_key(KeyCode::Space, true);
        input.clear_frame_edges();
        assert!(input.key_held(KeyCode::Space));
        assert!(!input.key_just_pressed(KeyCode::Space));
        input.record_key(KeyCode::Space, false);
        assert!(input.key_just_released(KeyCode::Space));
    }
    #[test]
    fn mouse_cursor_and_viewport_are_tracked() {
        let mut input = RuntimeInput::default();
        input.record_mouse_button(MouseButton::Left, true);
        input.record_cursor_position([12.0, 34.0]);
        input.record_viewport_size([800.0, 600.0]);
        assert!(input.mouse_held(MouseButton::Left));
        assert!(input.mouse_just_pressed(MouseButton::Left));
        assert_eq!(input.cursor_position(), Some([12.0, 34.0]));
        assert_eq!(input.viewport_size(), [800.0, 600.0]);
        input.clear_frame_edges();
        input.record_mouse_button(MouseButton::Left, false);
        assert!(input.mouse_just_released(MouseButton::Left));
        assert!(!input.mouse_held(MouseButton::Left));
    }
    #[test]
    fn actions_cover_keyboard_mouse_and_edge_clearing() {
        let mut map = ActionMap::default();
        map.bind("jump", InputBinding::Key(KeyCode::Space))
            .bind("jump", InputBinding::Mouse(MouseButton::Left));
        let mut input = RuntimeInput::default();
        input.record_key(KeyCode::Space, true);
        assert!(map.just_pressed(&input, "jump"));
        assert!(map.held(&input, "jump"));
        input.clear_frame_edges();
        assert!(!map.just_pressed(&input, "jump"));
        assert!(map.held(&input, "jump"));
        input.record_key(KeyCode::Space, false);
        assert!(map.just_released(&input, "jump"));
        input.clear_frame_edges();
        input.record_mouse_button(MouseButton::Left, true);
        assert!(map.just_pressed(&input, "jump"));
        assert!(map.held(&input, "jump"));
        assert!(!map.held(&input, "unbound_action"));
    }
}
