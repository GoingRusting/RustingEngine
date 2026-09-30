//! Runtime-side input capture and named action mapping.
//!
//! `RuntimeInput` holds raw keyboard and mouse state for gameplay systems.
//! Window integrations record raw events into it and clear edge state once per
//! rendered frame after gameplay systems have had a chance to read it.

use std::collections::{BTreeSet, HashMap};

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// Fired the frame a mouse button is pressed over a renderable entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickEvent {
    pub entity: Entity,
    pub button: MouseButton,
    pub world_position: [f32; 3],
}

/// Held keys, mouse buttons, and cursor position for the current frame.
///
/// `just_pressed`/`just_released` sets are edge-triggered: they hold exactly
/// the keys or buttons that changed state since the last call to
/// [`RuntimeInput::clear_frame_edges`]. Runtime integrations call that method
/// once per rendered frame after gameplay systems have had a chance to read
/// them.
///
/// It serializes whole, so replays can record what each frame's systems saw.
#[derive(
    Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize,
)]
pub struct RuntimeInput {
    keys_held: BTreeSet<KeyCode>,
    keys_just_pressed: BTreeSet<KeyCode>,
    keys_just_released: BTreeSet<KeyCode>,
    mouse_held: BTreeSet<MouseButton>,
    mouse_just_pressed: BTreeSet<MouseButton>,
    mouse_just_released: BTreeSet<MouseButton>,
    cursor_position: Option<[f32; 2]>,
    viewport_size: [f32; 2],
    mouse_motion: [f32; 2],
    cursor_captured: bool,
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

    /// Raw mouse movement since the last [`Self::clear_frame_edges`]. It keeps
    /// counting while the cursor is captured and cannot move.
    #[must_use]
    pub fn mouse_motion(&self) -> [f32; 2] {
        self.mouse_motion
    }

    /// True when gameplay asked the window to hide and lock the cursor.
    #[must_use]
    pub fn cursor_captured(&self) -> bool {
        self.cursor_captured
    }

    /// Asks the window to hide and lock the cursor, for mouse look. The
    /// window runner applies the request after the frame's systems ran.
    pub fn set_cursor_captured(&mut self, captured: bool) {
        self.cursor_captured = captured;
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

    /// Releases every held key and button, reporting each as just released.
    /// Call it when the window loses focus, because the matching release
    /// events then go to another window.
    pub fn release_all(&mut self) {
        self.keys_just_released.append(&mut self.keys_held);
        self.mouse_just_released.append(&mut self.mouse_held);
    }

    /// Records the latest cursor position from a `CursorMoved` event.
    pub fn record_cursor_position(&mut self, position: [f32; 2]) {
        self.cursor_position = Some(position);
    }
    /// Adds one raw `DeviceEvent::MouseMotion` delta.
    pub fn record_mouse_motion(&mut self, delta: [f32; 2]) {
        self.mouse_motion[0] += delta[0];
        self.mouse_motion[1] += delta[1];
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
        self.mouse_motion = [0.0; 2];
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
#[derive(Resource, Clone, Default)]
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
    /// Removes one binding of `action`; other bindings stay.
    pub fn unbind(&mut self, action: &str, binding: InputBinding) {
        if let Some(list) = self.bindings.get_mut(action) {
            list.retain(|bound| *bound != binding);
            if list.is_empty() {
                self.bindings.remove(action);
            }
        }
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
    /// Inputs bound to `action`, in binding order; empty when unbound.
    #[must_use]
    pub fn bindings(&self, action: &str) -> &[InputBinding] {
        self.bindings.get(action).map_or(&[], Vec::as_slice)
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
    fn mouse_motion_adds_up_until_the_frame_ends() {
        let mut input = RuntimeInput::default();
        input.record_mouse_motion([1.0, -2.0]);
        input.record_mouse_motion([0.5, 1.0]);
        assert_eq!(input.mouse_motion(), [1.5, -1.0]);
        input.clear_frame_edges();
        assert_eq!(input.mouse_motion(), [0.0, 0.0]);
    }

    #[test]
    fn release_all_clears_held_input_as_releases() {
        let mut input = RuntimeInput::default();
        input.record_key(KeyCode::KeyW, true);
        input.record_mouse_button(MouseButton::Right, true);
        input.clear_frame_edges();
        input.release_all();
        assert!(!input.key_held(KeyCode::KeyW));
        assert!(input.key_just_released(KeyCode::KeyW));
        assert!(input.mouse_just_released(MouseButton::Right));
    }

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
