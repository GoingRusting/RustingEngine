//! First-person player camera and controller.
//!
//! Put [`PlayerController`] on a body entity whose `Transform` position is
//! the capsule center, and parent an entity with a `Camera` to it at eye
//! height. Every fixed step the body walks with
//! [`PhysicsWorld::move_character`], so it slides along CPU colliders and
//! lands on floors. Mouse look runs once per rendered frame while the cursor
//! is captured: yaw turns the body and pitch tilts its `Camera` children. A
//! left click captures the cursor and Escape releases it.
//!
//! The body moves as its `Collider` shape (unscaled), or as
//! [`DEFAULT_PLAYER_SHAPE`] without one. Give it a `Collider`, a CPU
//! `PhysicsBody`, and a kinematic `RigidBody` to make sensors report it and
//! let it push dynamic bodies.
//!
//! Movement reads the named actions in [`PLAYER_ACTIONS`] from the
//! [`ActionMap`]. `App::new` binds them to WASD/arrow keys, Space, and Shift;
//! a game can add more bindings to the same names.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, Query, Res, ResMut, With, Without};
use serde::{Deserialize, Serialize};

use super::sim_math;
use super::{
    ActionMap, Camera, Children, Collider, ColliderShape, FrameTime,
    InputBinding, KeyCode, MouseButton, PhysicsWorld, RuntimeInput,
};
use crate::Transform;

pub const PLAYER_FORWARD: &str = "player.forward";
pub const PLAYER_BACK: &str = "player.back";
pub const PLAYER_LEFT: &str = "player.left";
pub const PLAYER_RIGHT: &str = "player.right";
pub const PLAYER_JUMP: &str = "player.jump";
pub const PLAYER_SPRINT: &str = "player.sprint";
/// Every action name the controller reads.
pub const PLAYER_ACTIONS: [&str; 6] = [
    PLAYER_FORWARD,
    PLAYER_BACK,
    PLAYER_LEFT,
    PLAYER_RIGHT,
    PLAYER_JUMP,
    PLAYER_SPRINT,
];

/// Shape of a player body without a `Collider`: 1.8 m tall.
pub const DEFAULT_PLAYER_SHAPE: ColliderShape = ColliderShape::Capsule {
    half_height: 0.6,
    radius: 0.3,
};

/// Walk, jump, and mouse-look settings plus the controller's live state.
/// Speeds are in metres per second, `gravity` in metres per second squared,
/// and `look_sensitivity` in radians per pixel of mouse motion.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerController {
    pub walk_speed: f32,
    pub sprint_multiplier: f32,
    pub jump_speed: f32,
    pub gravity: f32,
    pub look_sensitivity: f32,
    /// Collision layers the body stops against.
    pub collision_mask: u32,
    /// Heading in radians around +Y; 0 looks toward -Z.
    pub yaw: f32,
    /// Up/down look in radians, kept within ±89°.
    pub pitch: f32,
    #[serde(skip)]
    pub vertical_speed: f32,
    #[serde(skip)]
    pub grounded: bool,
    /// Set when jump is pressed; the next grounded fixed step consumes it.
    #[serde(skip)]
    pub jump_requested: bool,
}

impl Default for PlayerController {
    fn default() -> Self {
        Self {
            walk_speed: 4.0,
            sprint_multiplier: 1.8,
            jump_speed: 5.0,
            gravity: 9.81,
            look_sensitivity: 0.002,
            collision_mask: u32::MAX,
            yaw: 0.0,
            pitch: 0.0,
            vertical_speed: 0.0,
            grounded: false,
            jump_requested: false,
        }
    }
}

const MAX_PITCH: f32 = 89.0_f32.to_radians();

pub(super) fn bind_default_actions(map: &mut ActionMap) {
    let bindings: [(&str, &[KeyCode]); 6] = [
        (PLAYER_FORWARD, &[KeyCode::KeyW, KeyCode::ArrowUp]),
        (PLAYER_BACK, &[KeyCode::KeyS, KeyCode::ArrowDown]),
        (PLAYER_LEFT, &[KeyCode::KeyA, KeyCode::ArrowLeft]),
        (PLAYER_RIGHT, &[KeyCode::KeyD, KeyCode::ArrowRight]),
        (PLAYER_JUMP, &[KeyCode::Space]),
        (PLAYER_SPRINT, &[KeyCode::ShiftLeft, KeyCode::ShiftRight]),
    ];
    for (action, keys) in bindings {
        for key in keys {
            map.bind(action, InputBinding::Key(*key));
        }
    }
}

/// Per rendered frame: cursor capture, mouse look, and jump requests.
pub(super) fn player_look(
    mut input: ResMut<RuntimeInput>,
    actions: Res<ActionMap>,
    mut players: Query<(
        &mut PlayerController,
        &mut Transform,
        Option<&Children>,
    )>,
    mut cameras: Query<
        &mut Transform,
        (With<Camera>, Without<PlayerController>),
    >,
) {
    if players.is_empty() {
        return;
    }
    if input.key_just_pressed(KeyCode::Escape) {
        input.set_cursor_captured(false);
    } else if input.mouse_just_pressed(MouseButton::Left) {
        input.set_cursor_captured(true);
    }
    let motion = input.mouse_motion();
    let jump = actions.just_pressed(&input, PLAYER_JUMP);
    for (mut player, mut transform, children) in &mut players {
        if input.cursor_captured() {
            let sensitivity = player.look_sensitivity;
            player.yaw -= motion[0] * sensitivity;
            player.pitch = (player.pitch - motion[1] * sensitivity)
                .clamp(-MAX_PITCH, MAX_PITCH);
        }
        player.jump_requested |= jump;
        transform.rotation = [0.0, player.yaw, 0.0];
        for child in children.into_iter().flat_map(|children| &children.0) {
            if let Ok(mut camera) = cameras.get_mut(*child) {
                camera.rotation = [player.pitch, 0.0, 0.0];
            }
        }
    }
}

/// Per fixed step: walking, gravity, jumping, and collision sliding.
pub(super) fn player_move(
    time: Res<FrameTime>,
    input: Res<RuntimeInput>,
    actions: Res<ActionMap>,
    physics: Res<PhysicsWorld>,
    mut players: Query<(
        Entity,
        &mut PlayerController,
        &mut Transform,
        Option<&Collider>,
    )>,
) {
    let dt = time.fixed_delta.as_secs_f32();
    let axis = |positive, negative| {
        f32::from(u8::from(actions.held(&input, positive)))
            - f32::from(u8::from(actions.held(&input, negative)))
    };
    let (forward, right) = (
        axis(PLAYER_FORWARD, PLAYER_BACK),
        axis(PLAYER_RIGHT, PLAYER_LEFT),
    );
    let sprint = actions.held(&input, PLAYER_SPRINT);
    for (entity, mut player, mut transform, collider) in &mut players {
        let (sin, cos) = sim_math::sin_cos(player.yaw);
        // Forward is -Z turned by yaw; right is +X turned by yaw.
        let mut motion = [
            -sin * forward + cos * right,
            0.0,
            -cos * forward - sin * right,
        ];
        let length = motion[0].hypot(motion[2]);
        let speed = player.walk_speed
            * if sprint {
                player.sprint_multiplier
            } else {
                1.0
            };
        if length > 0.0 {
            motion = motion.map(|value| value / length * speed * dt);
        }
        if player.grounded && player.jump_requested {
            player.vertical_speed = player.jump_speed;
        }
        player.jump_requested = false;
        player.vertical_speed -= player.gravity * dt;
        motion[1] = player.vertical_speed * dt;

        let moved = physics.move_character(
            collider.map_or(DEFAULT_PLAYER_SHAPE, |collider| collider.shape),
            transform.position,
            motion,
            player.collision_mask,
            Some(entity),
        );
        player.grounded = moved.grounded;
        if moved.grounded && player.vertical_speed < 0.0 {
            player.vertical_speed = 0.0;
        }
        transform.position = moved.position;
    }
}
