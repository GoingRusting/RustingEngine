//! Central editor shortcuts and Scene View fly-camera input.
//!
//! Every editor command gets an action name before it gets a key. A future
//! Settings panel can change the map without changing window-event code.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy_ecs::prelude::{Resource, World};
use nalgebra::{Rotation3, Vector3};
use winit::event::{ElementState, KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window};

use crate::runtime::{Camera, GlobalTransform, MeshRenderer, Projection};
use crate::{AssetServer, Transform};

use super::{
    EditorGizmoDrag, EditorState, EditorViewport, EditorWorkspace, GizmoAxis,
};

/// Input state in which a shortcut is meaningful.
///
/// A physical key may be bound once in each context. For example, `S` selects
/// the scale gizmo in Scene View and moves backward while flying.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutContext {
    SceneView,
    TransformModal,
    FlyCamera,
}

/// Named editor commands that can receive user-configurable shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    SceneView(SceneViewAction),
}

/// Commands that are meaningful while the Scene View owns keyboard input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SceneViewAction {
    /// Captures or releases the pointer for Scene View FPS navigation.
    ToggleFly,
    FlyForward,
    FlyBackward,
    FlyLeft,
    FlyRight,
    FlyDown,
    FlyUp,
    FlySprint,
    FocusObject,
    TransformModes(TransformModes),
    TransformAxis(GizmoAxis),
}

impl SceneViewAction {
    /// Stable display order used by shortcut settings and tests.
    pub const ALL: [Self; 15] = [
        Self::ToggleFly,
        Self::FlyForward,
        Self::FlyBackward,
        Self::FlyLeft,
        Self::FlyRight,
        Self::FlyDown,
        Self::FlyUp,
        Self::FlySprint,
        Self::FocusObject,
        Self::TransformModes(TransformModes::Move),
        Self::TransformModes(TransformModes::Rotate),
        Self::TransformModes(TransformModes::Scale),
        Self::TransformAxis(GizmoAxis::X),
        Self::TransformAxis(GizmoAxis::Y),
        Self::TransformAxis(GizmoAxis::Z),
    ];

    #[must_use]
    pub const fn context(self) -> ShortcutContext {
        match self {
            Self::FlyForward
            | Self::FlyBackward
            | Self::FlyLeft
            | Self::FlyRight
            | Self::FlyDown
            | Self::FlyUp
            | Self::FlySprint => ShortcutContext::FlyCamera,
            Self::TransformAxis(_) => ShortcutContext::TransformModal,
            Self::ToggleFly | Self::FocusObject | Self::TransformModes(_) => {
                ShortcutContext::SceneView
            }
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ToggleFly => "Toggle fly camera",
            Self::FlyForward => "Fly forward",
            Self::FlyBackward => "Fly backward",
            Self::FlyLeft => "Fly left",
            Self::FlyRight => "Fly right",
            Self::FlyDown => "Fly down",
            Self::FlyUp => "Fly up",
            Self::FlySprint => "Fly faster",
            Self::FocusObject => "Focus selected object",
            Self::TransformModes(TransformModes::Move) => "Move gizmo",
            Self::TransformModes(TransformModes::Rotate) => "Rotate gizmo",
            Self::TransformModes(TransformModes::Scale) => "Scale gizmo",
            Self::TransformModes(TransformModes::Combo) => "Combined gizmo",
            Self::TransformAxis(GizmoAxis::X) => "Constrain to X axis",
            Self::TransformAxis(GizmoAxis::Y) => "Constrain to Y axis",
            Self::TransformAxis(GizmoAxis::Z) => "Constrain to Z axis",
        }
    }
}

/// One keyboard binding stored independently from the action it triggers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    /// Physical key, so the default works consistently across keyboard layouts.
    pub key: KeyCode,
}

/// The one source of truth for editor keyboard bindings.
///
/// The future Settings panel will edit and persist this resource instead of
/// scattering key checks over individual panels and window events.
#[derive(Resource, Clone, Debug)]
pub struct EditorShortcuts {
    bindings: HashMap<ShortcutAction, KeyBinding>,
}

impl Default for EditorShortcuts {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        let defaults = [
            (SceneViewAction::ToggleFly, KeyCode::Numpad0),
            (SceneViewAction::FlyForward, KeyCode::KeyW),
            (SceneViewAction::FlyBackward, KeyCode::KeyS),
            (SceneViewAction::FlyLeft, KeyCode::KeyA),
            (SceneViewAction::FlyRight, KeyCode::KeyD),
            (SceneViewAction::FlyDown, KeyCode::ControlLeft),
            (SceneViewAction::FlyUp, KeyCode::Space),
            (SceneViewAction::FlySprint, KeyCode::ShiftLeft),
            (SceneViewAction::FocusObject, KeyCode::KeyF),
            (
                SceneViewAction::TransformModes(TransformModes::Move),
                KeyCode::KeyG,
            ),
            (
                SceneViewAction::TransformModes(TransformModes::Rotate),
                KeyCode::KeyR,
            ),
            (
                SceneViewAction::TransformModes(TransformModes::Scale),
                KeyCode::KeyS,
            ),
            (SceneViewAction::TransformAxis(GizmoAxis::X), KeyCode::KeyX),
            (SceneViewAction::TransformAxis(GizmoAxis::Y), KeyCode::KeyY),
            (SceneViewAction::TransformAxis(GizmoAxis::Z), KeyCode::KeyZ),
        ];
        for (action, key) in defaults {
            bindings
                .insert(ShortcutAction::SceneView(action), KeyBinding { key });
        }
        Self { bindings }
    }
}

impl EditorShortcuts {
    /// Returns the current key assigned to an editor action.
    #[must_use]
    pub fn get(&self, action: ShortcutAction) -> Option<KeyBinding> {
        self.bindings.get(&action).copied()
    }

    /// Changes an action binding. Settings UI will call this method later.
    pub fn set(
        &mut self,
        action: ShortcutAction,
        shortcut: KeyBinding,
    ) -> Option<ShortcutAction> {
        // Only duplicates within the action's context conflict. The same key
        // remains available in another context.
        let ShortcutAction::SceneView(new_action) = action;
        let replaced = self.bindings.iter().find_map(|(candidate, binding)| {
            let ShortcutAction::SceneView(candidate_action) = *candidate;
            (*candidate != action
                && candidate_action.context() == new_action.context()
                && binding.key == shortcut.key)
                .then_some(*candidate)
        });
        if let Some(replaced) = replaced {
            self.bindings.remove(&replaced);
        }
        self.bindings.insert(action, shortcut);
        replaced
    }

    /// Resolves a physical key in one editor context without panicking when it
    /// is unbound. Context-specific lookup lets a future Code Editor reuse W,
    /// F, or any other key without conflicting with Scene View.
    #[must_use]
    pub fn scene_view_action(
        &self,
        context: ShortcutContext,
        key: KeyCode,
    ) -> Option<SceneViewAction> {
        self.bindings.iter().find_map(|(action, binding)| {
            let ShortcutAction::SceneView(action) = *action;
            (binding.key == key && action.context() == context)
                .then_some(action)
        })
    }
}

/// Transform operation selected for the Scene View gizmo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TransformModes {
    Move,
    Rotate,
    Scale,
    #[default]
    Combo,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EditorTransformMode {
    pub active_mode: TransformModes,
    pub axis_mask: [bool; 3],
    pub start_requested: bool,
}

impl Default for EditorTransformMode {
    fn default() -> Self {
        Self {
            active_mode: TransformModes::Combo,
            axis_mask: [true; 3],
            start_requested: false,
        }
    }
}

impl EditorTransformMode {
    pub(crate) fn select_only_or_toggle(&mut self, axis: GizmoAxis) {
        let index = axis_index(axis);
        if self.axis_mask == [true; 3] {
            self.axis_mask = [false; 3];
            self.axis_mask[index] = true;
        } else {
            self.axis_mask[index] = !self.axis_mask[index];
            if !self.axis_mask.iter().any(|enabled| *enabled) {
                self.axis_mask = [true; 3];
            }
        }
    }
}

const fn axis_index(axis: GizmoAxis) -> usize {
    match axis {
        GizmoAxis::X => 0,
        GizmoAxis::Y => 1,
        GizmoAxis::Z => 2,
    }
}

/// Temporary input state for the editor camera. It is never serialized.
#[derive(Resource, Clone, Debug)]
pub struct EditorFlyCamera {
    /// True while Scene View owns pointer look and movement keys.
    pub active: bool,
    pressed_actions: HashSet<SceneViewAction>,
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
            pressed_actions: HashSet::new(),
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
    let transform_active = world
        .get_resource::<EditorGizmoDrag>()
        .is_some_and(EditorGizmoDrag::is_active)
        || world.resource::<EditorTransformMode>().start_requested;
    let (scene_action, transform_action, fly_action) = {
        let shortcuts = world.resource::<EditorShortcuts>();
        (
            shortcuts.scene_view_action(ShortcutContext::SceneView, key),
            shortcuts.scene_view_action(ShortcutContext::TransformModal, key),
            shortcuts.scene_view_action(ShortcutContext::FlyCamera, key),
        )
    };
    // Fly bindings win while pointer capture is active. ToggleFly is the only
    // normal Scene View action that remains available as a fallback, allowing
    // the user to release pointer capture again.
    let action = if is_fly_active {
        fly_action.or_else(|| {
            (scene_action == Some(SceneViewAction::ToggleFly))
                .then_some(SceneViewAction::ToggleFly)
        })
    } else if transform_active {
        transform_action.or_else(|| {
            scene_action.filter(|action| {
                matches!(action, SceneViewAction::TransformModes(_))
            })
        })
    } else {
        scene_action
    };
    // Releasing fly mode must work even if an old UI text field still has focus.
    let can_toggle =
        is_fly_active || (scene_view_is_active && !ui_wants_keyboard);
    if action == Some(SceneViewAction::ToggleFly)
        && is_pressed
        && !event.repeat
        && can_toggle
    {
        set_fly_camera_active(world, window, !is_fly_active);
        return true;
    }
    if !is_fly_active && (!scene_view_is_active || ui_wants_keyboard) {
        return false;
    }
    let Some(action) = action else {
        return false;
    };
    if action == SceneViewAction::FocusObject {
        if is_pressed && !event.repeat {
            camera_to_object(world);
        }
        return true;
    }
    if let SceneViewAction::TransformModes(mode) = action {
        if is_pressed && !event.repeat {
            let mut transform = world.resource_mut::<EditorTransformMode>();
            transform.active_mode = mode;
            transform.axis_mask = [true; 3];
            transform.start_requested = true;
        }
        return true;
    }
    if let SceneViewAction::TransformAxis(axis) = action {
        if is_pressed && !event.repeat {
            world
                .resource_mut::<EditorTransformMode>()
                .select_only_or_toggle(axis);
        }
        return true;
    }
    if !is_fly_active {
        return false;
    }
    let mut fly = world.resource_mut::<EditorFlyCamera>();
    if is_pressed {
        fly.pressed_actions.insert(action);
    } else {
        fly.pressed_actions.remove(&action);
    }
    true
}

/// Holds the fly camera while the secondary mouse button is down in Scene View.
///
/// An active transform keeps ownership of the secondary button so right-click
/// can cancel it instead of unexpectedly entering fly mode.
pub fn handle_mouse_button_input(
    world: &mut World,
    window: &Window,
    state: ElementState,
    button: MouseButton,
    cursor_position: [f64; 2],
) -> bool {
    if button != MouseButton::Right {
        return false;
    }
    let fly_active = world.resource::<EditorFlyCamera>().active;
    if state == ElementState::Released {
        if fly_active {
            set_fly_camera_active(world, window, false);
            return true;
        }
        return false;
    }

    let transform_active = world
        .get_resource::<EditorGizmoDrag>()
        .is_some_and(EditorGizmoDrag::is_active)
        || world.resource::<EditorTransformMode>().start_requested;
    if transform_active {
        return false;
    }
    let scene_active =
        world.resource::<EditorState>().workspace == EditorWorkspace::Scene;
    let viewport = *world.resource::<EditorViewport>();
    let inside_viewport = viewport.valid
        && cursor_position[0] >= f64::from(viewport.offset[0])
        && cursor_position[1] >= f64::from(viewport.offset[1])
        && cursor_position[0]
            < f64::from(viewport.offset[0] + viewport.extent[0])
        && cursor_position[1]
            < f64::from(viewport.offset[1] + viewport.extent[1]);
    if scene_active && inside_viewport {
        set_fly_camera_active(world, window, true);
        return true;
    }
    false
}

/// Receives raw pointer movement while the editor owns the captured pointer.
pub fn add_mouse_delta(world: &mut World, delta: (f64, f64)) {
    let mut fly = world.resource_mut::<EditorFlyCamera>();
    if fly.active {
        fly.pending_mouse_delta[0] += delta.0 as f32;
        fly.pending_mouse_delta[1] += delta.1 as f32;
    }
}

pub fn camera_to_object(world: &mut World) {
    let state = world.resource::<EditorState>();
    let (Some(selected), Some(camera_entity)) =
        (state.selected, state.editor_camera)
    else {
        return;
    };
    let Some(global) = world.get::<GlobalTransform>(selected).copied() else {
        return;
    };

    let bounds =
        world
            .get::<MeshRenderer>(selected)
            .copied()
            .and_then(|renderer| {
                world
                    .resource::<AssetServer>()
                    .meshes
                    .get(renderer.mesh)
                    .and_then(crate::editor::overlay::mesh_bounds)
            });
    let (target, radius) = world_bounds(global.matrix, bounds);

    let Some(camera_transform) = world.get::<Transform>(camera_entity).copied()
    else {
        return;
    };
    let projection = world
        .get::<Camera>(camera_entity)
        .map(|camera| camera.projection)
        .unwrap_or_default();
    let rotation = Rotation3::from_euler_angles(
        camera_transform.rotation[0],
        camera_transform.rotation[1],
        camera_transform.rotation[2],
    );
    let forward = rotation * Vector3::new(0.0, 0.0, -1.0);
    let distance = match projection {
        Projection::Perspective {
            vertical_fov_radians,
            near,
            ..
        } => {
            let half_fov = (vertical_fov_radians * 0.5).clamp(0.05, 1.5);
            (radius / half_fov.sin() * 1.2).max(near * 2.0)
        }
        Projection::Orthographic { near, .. } => radius.max(near * 2.0),
    };
    let position = Vector3::from(target) - forward * distance;
    if let Some(mut camera_transform) =
        world.get_mut::<Transform>(camera_entity)
    {
        camera_transform.position = position.into();
    }
}

/// Returns the world-space center and enclosing radius of an optional mesh.
/// Entities without render geometry still focus as a one-unit point of interest.
fn world_bounds(
    matrix: [[f32; 4]; 4],
    local_bounds: Option<([f32; 3], [f32; 3])>,
) -> ([f32; 3], f32) {
    let Some((minimum, maximum)) = local_bounds else {
        return ([matrix[3][0], matrix[3][1], matrix[3][2]], 1.0);
    };
    let local_center = [
        (minimum[0] + maximum[0]) * 0.5,
        (minimum[1] + maximum[1]) * 0.5,
        (minimum[2] + maximum[2]) * 0.5,
    ];
    let local_half_extent = [
        (maximum[0] - minimum[0]) * 0.5,
        (maximum[1] - minimum[1]) * 0.5,
        (maximum[2] - minimum[2]) * 0.5,
    ];
    let transform_point = |point: [f32; 3]| {
        [
            matrix[0][0] * point[0]
                + matrix[1][0] * point[1]
                + matrix[2][0] * point[2]
                + matrix[3][0],
            matrix[0][1] * point[0]
                + matrix[1][1] * point[1]
                + matrix[2][1] * point[2]
                + matrix[3][1],
            matrix[0][2] * point[0]
                + matrix[1][2] * point[1]
                + matrix[2][2] * point[2]
                + matrix[3][2],
        ]
    };
    let center = transform_point(local_center);
    let center_vector = Vector3::from(center);
    let mut radius: f32 = 0.0;
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                let corner = transform_point([
                    local_center[0] + local_half_extent[0] * x,
                    local_center[1] + local_half_extent[1] * y,
                    local_center[2] + local_half_extent[2] * z,
                ]);
                radius = radius
                    .max(Vector3::from(corner).metric_distance(&center_vector));
            }
        }
    }
    let radius = radius.max(0.1);
    (center, radius)
}
/// Applies one frame of pointer look and WASD/vertical movement.
pub fn update_fly_camera(world: &mut World, delta: Duration) {
    let (active, actions, mouse_delta, speed, sprint_multiplier, sensitivity) = {
        let mut fly = world.resource_mut::<EditorFlyCamera>();
        let mouse_delta = std::mem::take(&mut fly.pending_mouse_delta);
        (
            fly.active,
            fly.pressed_actions.clone(),
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
    if actions.contains(&SceneViewAction::FlyForward) {
        movement += forward;
    }
    if actions.contains(&SceneViewAction::FlyBackward) {
        movement -= forward;
    }
    if actions.contains(&SceneViewAction::FlyRight) {
        movement += right;
    }
    if actions.contains(&SceneViewAction::FlyLeft) {
        movement -= right;
    }
    if actions.contains(&SceneViewAction::FlyUp) {
        movement.y += 1.0;
    }
    if actions.contains(&SceneViewAction::FlyDown) {
        movement.y -= 1.0;
    }
    if movement.norm_squared() > f32::EPSILON {
        let sprinting = actions.contains(&SceneViewAction::FlySprint);
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
    fly.pressed_actions.clear();
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
            shortcuts
                .get(ShortcutAction::SceneView(SceneViewAction::ToggleFly)),
            Some(KeyBinding {
                key: KeyCode::Numpad0,
            }),
        );
    }

    #[test]
    fn every_scene_view_action_has_a_working_default_binding() {
        let shortcuts = EditorShortcuts::default();
        for action in SceneViewAction::ALL {
            let action = ShortcutAction::SceneView(action);
            let binding = shortcuts
                .get(action)
                .unwrap_or_else(|| panic!("{action:?} has no default binding"));
            let ShortcutAction::SceneView(scene_action) = action;
            assert_eq!(
                shortcuts
                    .scene_view_action(scene_action.context(), binding.key,),
                Some(scene_action)
            );
        }
    }

    #[test]
    fn rebinding_moves_a_key_to_only_one_action() {
        let mut shortcuts = EditorShortcuts::default();
        let replaced = shortcuts.set(
            ShortcutAction::SceneView(SceneViewAction::FlyForward),
            KeyBinding {
                key: KeyCode::Space,
            },
        );

        assert_eq!(
            replaced,
            Some(ShortcutAction::SceneView(SceneViewAction::FlyUp))
        );
        assert_eq!(
            shortcuts
                .scene_view_action(ShortcutContext::FlyCamera, KeyCode::Space,),
            Some(SceneViewAction::FlyForward)
        );
        assert_eq!(
            shortcuts.get(ShortcutAction::SceneView(SceneViewAction::FlyUp)),
            None
        );
    }

    #[test]
    fn same_key_resolves_differently_in_scene_and_fly_contexts() {
        let shortcuts = EditorShortcuts::default();

        assert_eq!(
            shortcuts
                .scene_view_action(ShortcutContext::SceneView, KeyCode::KeyS,),
            Some(SceneViewAction::TransformModes(TransformModes::Scale))
        );
        assert_eq!(
            shortcuts
                .scene_view_action(ShortcutContext::FlyCamera, KeyCode::KeyS,),
            Some(SceneViewAction::FlyBackward)
        );
    }

    #[test]
    fn rebinding_only_replaces_an_action_in_the_same_context() {
        let mut shortcuts = EditorShortcuts::default();
        let replaced = shortcuts.set(
            ShortcutAction::SceneView(SceneViewAction::TransformModes(
                TransformModes::Move,
            )),
            KeyBinding { key: KeyCode::KeyS },
        );

        assert_eq!(
            replaced,
            Some(ShortcutAction::SceneView(SceneViewAction::TransformModes(
                TransformModes::Scale
            )))
        );
        assert_eq!(
            shortcuts
                .scene_view_action(ShortcutContext::FlyCamera, KeyCode::KeyS,),
            Some(SceneViewAction::FlyBackward)
        );
    }

    #[test]
    fn axis_constraints_start_single_then_build_a_plane() {
        let mut transform = EditorTransformMode::default();

        transform.select_only_or_toggle(GizmoAxis::X);
        assert_eq!(transform.axis_mask, [true, false, false]);

        transform.select_only_or_toggle(GizmoAxis::Y);
        assert_eq!(transform.axis_mask, [true, true, false]);

        transform.select_only_or_toggle(GizmoAxis::X);
        assert_eq!(transform.axis_mask, [false, true, false]);

        transform.select_only_or_toggle(GizmoAxis::Y);
        assert_eq!(transform.axis_mask, [true; 3]);
    }

    #[test]
    fn fly_camera_moves_the_editor_camera_only() {
        let mut world = World::new();
        let camera = world.spawn(Transform::default()).id();
        let state = EditorState {
            editor_camera: Some(camera),
            ..EditorState::default()
        };
        world.insert_resource(state);
        let mut fly = EditorFlyCamera {
            active: true,
            ..EditorFlyCamera::default()
        };
        fly.pressed_actions.insert(SceneViewAction::FlyForward);
        world.insert_resource(fly);

        update_fly_camera(&mut world, Duration::from_secs(1));

        let transform = world.get::<Transform>(camera).unwrap();
        assert!(transform.position[2] < -5.9);
    }

    #[test]
    fn focus_places_selected_object_in_front_of_camera() {
        let mut world = World::new();
        let selected = world
            .spawn(GlobalTransform {
                matrix: Transform::new([10.0, 2.0, -3.0]).to_matrix(),
            })
            .id();
        let camera =
            world.spawn((Transform::default(), Camera::default())).id();
        world.insert_resource(EditorState {
            selected: Some(selected),
            editor_camera: Some(camera),
            ..EditorState::default()
        });

        camera_to_object(&mut world);

        let camera = world.get::<Transform>(camera).unwrap();
        assert_eq!(camera.position[0], 10.0);
        assert_eq!(camera.position[1], 2.0);
        assert!(camera.position[2] > -3.0);
    }
}
