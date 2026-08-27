//! Central editor shortcuts and Scene View fly-camera input.
//!
//! Every editor command gets an action name before it gets a key. A future
//! Settings panel can change the map without changing window-event code.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy_ecs::prelude::{Resource, World};
use nalgebra::{Rotation3, Vector3};
use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window};

use crate::Transform;

use super::{EditorState, EditorWorkspace};

/// Named editor commands that can receive user-configurable shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditorShortcutAction {
    /// Captures or releases the pointer for Scene View FPS navigation.
    ToggleFlyCamera,
}

/// One keyboard binding stored independently from the action it triggers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorShortcut {
    /// Physical key, so the default works consistently across keyboard layouts.
    pub key: KeyCode,
}

/// The one source of truth for editor keyboard bindings.
///
/// The future Settings panel will edit and persist this resource instead of
/// scattering key checks over individual panels and window events.
#[derive(Resource, Clone, Debug)]
pub struct EditorShortcuts {
    bindings: HashMap<EditorShortcutAction, EditorShortcut>,
}

impl Default for EditorShortcuts {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        // Numpad 0 is intentionally used instead of Escape. Escape remains a
        // normal editor/UI key and will not unexpectedly lock the pointer.
        bindings.insert(
            EditorShortcutAction::ToggleFlyCamera,
            EditorShortcut {
                key: KeyCode::Numpad0,
            },
        );
        Self { bindings }
    }
}

impl EditorShortcuts {
    /// Returns the current key assigned to an editor action.
    #[must_use]
    pub fn get(&self, action: EditorShortcutAction) -> Option<EditorShortcut> {
        self.bindings.get(&action).copied()
    }

    /// Changes an action binding. Settings UI will call this method later.
    pub fn set(
        &mut self,
        action: EditorShortcutAction,
        shortcut: EditorShortcut,
    ) {
        self.bindings.insert(action, shortcut);
    }

    fn matches(&self, action: EditorShortcutAction, key: KeyCode) -> bool {
        self.get(action).is_some_and(|shortcut| shortcut.key == key)
    }
}

/// Temporary input state for the editor camera. It is never serialized.
#[derive(Resource, Clone, Debug)]
pub struct EditorFlyCamera {
    /// True while Scene View owns pointer look and movement keys.
    pub active: bool,
    pressed_keys: HashSet<KeyCode>,
    pending_mouse_delta: [f32; 2],
    /// Movement speed in world units per second.
    pub speed: f32,
    /// Multiplier while Shift is held.
    pub sprint_multiplier: f32,
    /// Mouse radians per physical pixel.
    pub look_sensitivity: f32,
}

impl Default for EditorFlyCamera {
    fn default() -> Self {
        Self {
            active: false,
            pressed_keys: HashSet::new(),
            pending_mouse_delta: [0.0, 0.0],
            speed: 6.0,
            sprint_multiplier: 3.0,
            look_sensitivity: 0.002,
        }
    }
}

/// Handles keyboard input after egui has received the operating-system event.
///
/// Returns true when the event belongs to the fly camera. The caller can use
/// this later when it adds more input-routing behaviour.
pub fn handle_keyboard_input(
    world: &mut World,
    window: &Window,
    event: &KeyEvent,
    ui_wants_keyboard: bool,
) -> bool {
    let PhysicalKey::Code(key) = event.physical_key else {
        return false;
    };
    let is_pressed = event.state == ElementState::Pressed;
    let is_fly_active = world.resource::<EditorFlyCamera>().active;
    let scene_view_is_active =
        world.resource::<EditorState>().workspace == EditorWorkspace::Scene;
    let toggle = world
        .resource::<EditorShortcuts>()
        .matches(EditorShortcutAction::ToggleFlyCamera, key);
    // Releasing fly mode must work even if an old UI text field still has focus.
    let can_toggle =
        is_fly_active || (scene_view_is_active && !ui_wants_keyboard);
    if toggle && is_pressed && !event.repeat && can_toggle {
        set_fly_camera_active(world, window, !is_fly_active);
        return true;
    }
    if !is_fly_active {
        return false;
    }
    let mut fly = world.resource_mut::<EditorFlyCamera>();
    if is_pressed {
        fly.pressed_keys.insert(key);
    } else {
        fly.pressed_keys.remove(&key);
    }
    true
}

/// Receives raw pointer movement while the editor owns the captured pointer.
pub fn add_mouse_delta(world: &mut World, delta: (f64, f64)) {
    let mut fly = world.resource_mut::<EditorFlyCamera>();
    if fly.active {
        fly.pending_mouse_delta[0] += delta.0 as f32;
        fly.pending_mouse_delta[1] += delta.1 as f32;
    }
}

/// Applies one frame of pointer look and WASD/vertical movement.
pub fn update_fly_camera(world: &mut World, delta: Duration) {
    let (active, keys, mouse_delta, speed, sprint_multiplier, sensitivity) = {
        let mut fly = world.resource_mut::<EditorFlyCamera>();
        let mouse_delta = std::mem::take(&mut fly.pending_mouse_delta);
        (
            fly.active,
            fly.pressed_keys.clone(),
            mouse_delta,
            fly.speed,
            fly.sprint_multiplier,
            fly.look_sensitivity,
        )
    };
    if !active {
        return;
    }
    let Some(camera) = world.resource::<EditorState>().editor_camera else {
        return;
    };
    let Some(mut transform) = world.get_mut::<Transform>(camera) else {
        return;
    };

    transform.rotation[1] -= mouse_delta[0] * sensitivity;
    // Window mouse Y grows downward. In the editor camera convention that
    // must subtract from pitch, otherwise looking up and down feels swapped.
    transform.rotation[0] =
        (transform.rotation[0] - mouse_delta[1] * sensitivity).clamp(-1.5, 1.5);

    let rotation = Rotation3::from_euler_angles(
        transform.rotation[0],
        transform.rotation[1],
        transform.rotation[2],
    );
    let forward = rotation * Vector3::new(0.0, 0.0, -1.0);
    let right = rotation * Vector3::new(1.0, 0.0, 0.0);
    let mut movement = Vector3::zeros();
    if keys.contains(&KeyCode::KeyW) {
        movement += forward;
    }
    if keys.contains(&KeyCode::KeyS) {
        movement -= forward;
    }
    if keys.contains(&KeyCode::KeyD) {
        movement += right;
    }
    if keys.contains(&KeyCode::KeyA) {
        movement -= right;
    }
    if keys.contains(&KeyCode::Space) {
        movement.y += 1.0;
    }
    if keys.contains(&KeyCode::ControlLeft)
        || keys.contains(&KeyCode::ControlRight)
    {
        movement.y -= 1.0;
    }
    if movement.norm_squared() > f32::EPSILON {
        let sprinting = keys.contains(&KeyCode::ShiftLeft)
            || keys.contains(&KeyCode::ShiftRight);
        let speed = speed * if sprinting { sprint_multiplier } else { 1.0 };
        let step = movement.normalize() * speed * delta.as_secs_f32();
        transform.position[0] += step.x;
        transform.position[1] += step.y;
        transform.position[2] += step.z;
    }
}

/// Changes pointer capture and clears keys so movement cannot get stuck.
fn set_fly_camera_active(world: &mut World, window: &Window, active: bool) {
    let mut fly = world.resource_mut::<EditorFlyCamera>();
    fly.active = active;
    fly.pressed_keys.clear();
    fly.pending_mouse_delta = [0.0, 0.0];
    let _ = window.set_cursor_grab(if active {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    });
    window.set_cursor_visible(!active);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numpad_zero_is_the_default_fly_camera_shortcut() {
        let shortcuts = EditorShortcuts::default();
        assert_eq!(
            shortcuts.get(EditorShortcutAction::ToggleFlyCamera),
            Some(EditorShortcut {
                key: KeyCode::Numpad0,
            }),
        );
    }

    #[test]
    fn fly_camera_moves_the_editor_camera_only() {
        let mut world = World::new();
        let camera = world.spawn(Transform::default()).id();
        let mut state = EditorState::default();
        state.editor_camera = Some(camera);
        world.insert_resource(state);
        let mut fly = EditorFlyCamera {
            active: true,
            ..EditorFlyCamera::default()
        };
        fly.pressed_keys.insert(KeyCode::KeyW);
        world.insert_resource(fly);

        update_fly_camera(&mut world, Duration::from_secs(1));

        let transform = world.get::<Transform>(camera).unwrap();
        assert!(transform.position[2] < -5.9);
    }
}
