//! Runtime-side input capture for shipped games.
//!
//! This is distinct from the legacy `crate::input::InputState`, which only
//! drives the editor's fly camera. `RuntimeInput` is populated directly by
//! `ProjectApplication::window_event` in `project_runner.rs` from raw winit
//! events, and is meant to be read by gameplay systems through [`App`] like
//! any other resource.

use std::collections::HashSet;

use bevy_ecs::prelude::{Resource, World};
pub use winit::event::MouseButton;
pub use winit::keyboard::KeyCode;

/// Held keys, mouse buttons, and cursor position for the current frame.
///
/// `just_pressed`/`just_released` sets are edge-triggered: they hold exactly
/// the keys or buttons that changed state since the last call to
/// [`RuntimeInput::clear_frame_edges`], which `ProjectApplication` calls once
/// per rendered frame after gameplay systems have had a chance to read them.
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

    /// Clears this frame's edge sets. Called once per rendered frame, after
    /// gameplay systems have read them, so the next frame starts empty.
    pub fn clear_frame_edges(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.mouse_just_pressed.clear();
        self.mouse_just_released.clear();
    }
}

pub(super) fn install(world: &mut World) {
    world.insert_resource(RuntimeInput::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_press_sets_held_and_just_pressed_once() {
        let mut input = RuntimeInput::default();
        input.record_key(KeyCode::Space, true);
        assert!(input.key_held(KeyCode::Space));
        assert!(input.key_just_pressed(KeyCode::Space));

        // Auto-repeat must not re-fire just_pressed.
        input.record_key(KeyCode::Space, true);
        assert!(input.key_just_pressed(KeyCode::Space));

        input.clear_frame_edges();
        assert!(input.key_held(KeyCode::Space));
        assert!(!input.key_just_pressed(KeyCode::Space));
    }

    #[test]
    fn key_release_sets_just_released_and_clears_held() {
        let mut input = RuntimeInput::default();
        input.record_key(KeyCode::KeyW, true);
        input.clear_frame_edges();
        input.record_key(KeyCode::KeyW, false);
        assert!(!input.key_held(KeyCode::KeyW));
        assert!(input.key_just_released(KeyCode::KeyW));

        input.clear_frame_edges();
        assert!(!input.key_just_released(KeyCode::KeyW));
    }

    #[test]
    fn mouse_button_and_cursor_are_tracked() {
        let mut input = RuntimeInput::default();
        input.record_mouse_button(MouseButton::Left, true);
        assert!(input.mouse_held(MouseButton::Left));
        assert!(input.mouse_just_pressed(MouseButton::Left));

        input.record_cursor_position([12.0, 34.0]);
        assert_eq!(input.cursor_position(), Some([12.0, 34.0]));

        input.clear_frame_edges();
        input.record_mouse_button(MouseButton::Left, false);
        assert!(input.mouse_just_released(MouseButton::Left));
        assert!(!input.mouse_held(MouseButton::Left));
    }
}
