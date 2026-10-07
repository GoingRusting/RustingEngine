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
//! With `camera_distance` above zero the controller is third person: the
//! `Camera` children orbit behind the body at that distance, around a point
//! `camera_height` above the body center, and pitch swings them up and down.
//! A solid collider between that point and the camera pulls the camera in
//! in front of it, so walls behind the player do not hide the view.
//!
//! The body moves as its `Collider` shape (unscaled), or as
//! [`DEFAULT_PLAYER_SHAPE`] without one. Give it a `Collider`, a CPU
//! `PhysicsBody`, and a kinematic `RigidBody` to make sensors report it and
//! let it push dynamic bodies; `push_bodies: false` keeps it from moving
//! them. Surfaces steeper than `max_slope` are walls, and ledges up to
//! `max_step_height` are stepped onto.
//!
//! Movement reads the named actions in [`PLAYER_ACTIONS`] from the
//! [`ActionMap`]. `App::new` binds them to WASD/arrow keys, Space, and Shift;
//! a game can add more bindings to the same names.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, Query, Res, ResMut, With, Without};
use serde::{Deserialize, Serialize};

use super::cpu_physics::world_position;
use super::sim_math;
use super::{
    ActionMap, Camera, CharacterMove, Children, Collider, ColliderShape,
    FrameTime, GlobalTransform, InputBinding, KeyCode, MouseButton, PadButton,
    Parent, PhysicsWorld, RigidBody, RigidBodyKind, RuntimeInput, Stick,
};
use crate::Transform;

/// Room the third-person camera keeps from walls behind it.
const CAMERA_RADIUS: f32 = 0.2;

pub const PLAYER_FORWARD: &str = "player.forward";
pub const PLAYER_BACK: &str = "player.back";
pub const PLAYER_LEFT: &str = "player.left";
pub const PLAYER_RIGHT: &str = "player.right";
pub const PLAYER_JUMP: &str = "player.jump";
pub const PLAYER_SPRINT: &str = "player.sprint";
pub const PLAYER_CROUCH: &str = "player.crouch";
/// Every action name the controller reads.
pub const PLAYER_ACTIONS: [&str; 7] = [
    PLAYER_FORWARD,
    PLAYER_BACK,
    PLAYER_LEFT,
    PLAYER_RIGHT,
    PLAYER_JUMP,
    PLAYER_SPRINT,
    PLAYER_CROUCH,
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
    /// Walking speed in metres per second.
    pub walk_speed: f32,
    /// Speed factor while sprint is held.
    pub sprint_multiplier: f32,
    /// Upward speed in metres per second when a jump starts.
    pub jump_speed: f32,
    /// Extra jumps allowed in the air before landing: 1 is a double jump.
    pub air_jumps: u32,
    /// Downward acceleration in metres per second squared.
    pub gravity: f32,
    /// Radians of turn per pixel of mouse motion.
    pub look_sensitivity: f32,
    /// Whether a left click captures the cursor for mouse look. Off, the
    /// mouse stays free (for a second player or a UI) and only the right
    /// stick or game code (`set_look`) turns the view.
    pub mouse_look: bool,
    /// Collision layers the body stops against.
    pub collision_mask: u32,
    /// Heading in radians around +Y; 0 looks toward -Z.
    pub yaw: f32,
    /// Up/down look in radians, kept within `pitch_limits`.
    pub pitch: f32,
    /// Lowest and highest pitch in radians, within ±89°. `[-0.7, 0.7]`
    /// limits a seated view to ±40°.
    pub pitch_limits: [f32; 2],
    /// Lowest and highest yaw in radians, for a seated or turret view;
    /// `None` turns freely. Applied after every mouse, stick and `set_look`
    /// change, before the view is drawn.
    pub yaw_limits: Option<[f32; 2]>,
    /// 0 is first person. Above 0, the `Camera` children orbit behind the
    /// body at this distance in metres. The controller owns the transform
    /// of its direct `Camera` children: it sets their rotation to the pitch
    /// and, in third person, their position. Put a camera under an empty
    /// child to place it yourself.
    pub camera_distance: f32,
    /// Added to the camera position, in the body's frame: `[0.6, 0, 0]` is
    /// an over-the-shoulder camera 0.6 m to the right.
    pub camera_offset: [f32; 3],
    /// Height of the third-person orbit center above the body center.
    /// Unused in first person (`camera_distance` 0): there the camera
    /// child keeps its own local position, or takes `camera_offset` when
    /// that is not zero. Neither adds to the other.
    pub camera_height: f32,
    /// Steepest ground in radians the body stands on and walks up.
    pub max_slope: f32,
    /// Highest ledge in metres the body steps onto while walking.
    pub max_step_height: f32,
    /// Whether the body pushes dynamic bodies it walks into. Off, they
    /// still block it but never move.
    pub push_bodies: bool,
    /// Radians per second the body's non-camera children (the visible rig)
    /// turn toward the walking direction. 0 leaves them alone.
    pub turn_speed: f32,
    /// Full body height in metres while `player.crouch` is held; 0 turns
    /// crouching off. The body shrinks from the top, so its feet stay put,
    /// and first-person cameras drop by the height it loses. Releasing the
    /// action stands up only when there is room above. Works for capsule
    /// and box shapes.
    pub crouch_height: f32,
    /// Speed factor while crouched, instead of sprinting.
    pub crouch_multiplier: f32,
    /// Whether the body is crouched now.
    #[serde(skip)]
    pub crouched: bool,
    /// How far the crouch lowered the body center (0 standing); cameras
    /// drop this much more.
    #[serde(skip)]
    pub crouch_drop: f32,
    /// The part of `crouch_drop` already taken off first-person cameras.
    #[serde(skip)]
    pub camera_drop: f32,
    /// Current up/down speed in metres per second; negative while falling.
    #[serde(skip)]
    pub vertical_speed: f32,
    /// Whether the body stood on ground after the last fixed step.
    #[serde(skip)]
    pub grounded: bool,
    /// Set when jump is pressed; the next fixed step consumes it.
    #[serde(skip)]
    pub jump_requested: bool,
    /// Air jumps taken since the body last stood on ground.
    #[serde(skip)]
    pub air_jumps_used: u32,
    /// The floor body under the controller after the last step and where
    /// it was, so a moving platform carries the controller.
    #[serde(skip)]
    pub floor: Option<(Entity, [f32; 3])>,
    /// The body the last step walked into, if any (not the floor). A
    /// pushed crate shows here too.
    #[serde(skip)]
    pub wall: Option<Entity>,
    /// How far the body moved in the last fixed step, per second: the
    /// walking speed it really reached, including slides and platforms.
    #[serde(skip)]
    pub velocity: [f32; 3],
}

impl Default for PlayerController {
    fn default() -> Self {
        Self {
            walk_speed: 4.0,
            sprint_multiplier: 1.8,
            jump_speed: 5.0,
            air_jumps: 0,
            gravity: 9.81,
            look_sensitivity: 0.002,
            mouse_look: true,
            collision_mask: u32::MAX,
            yaw: 0.0,
            pitch: 0.0,
            pitch_limits: [-MAX_PITCH, MAX_PITCH],
            yaw_limits: None,
            camera_distance: 0.0,
            camera_height: 0.6,
            camera_offset: [0.0; 3],
            max_slope: 45.0_f32.to_radians(),
            max_step_height: 0.3,
            push_bodies: true,
            turn_speed: 0.0,
            crouch_height: 0.0,
            crouch_multiplier: 0.5,
            crouched: false,
            crouch_drop: 0.0,
            camera_drop: 0.0,
            vertical_speed: 0.0,
            grounded: false,
            jump_requested: false,
            air_jumps_used: 0,
            floor: None,
            wall: None,
            velocity: [0.0; 3],
        }
    }
}

const MAX_PITCH: f32 = 89.0_f32.to_radians();

pub(super) fn bind_default_actions(map: &mut ActionMap) {
    let bindings: [(&str, &[KeyCode]); 7] = [
        (PLAYER_FORWARD, &[KeyCode::KeyW, KeyCode::ArrowUp]),
        (PLAYER_BACK, &[KeyCode::KeyS, KeyCode::ArrowDown]),
        (PLAYER_LEFT, &[KeyCode::KeyA, KeyCode::ArrowLeft]),
        (PLAYER_RIGHT, &[KeyCode::KeyD, KeyCode::ArrowRight]),
        (PLAYER_JUMP, &[KeyCode::Space]),
        (PLAYER_SPRINT, &[KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        (PLAYER_CROUCH, &[KeyCode::KeyC, KeyCode::ControlLeft]),
    ];
    for (action, keys) in bindings {
        for key in keys {
            map.bind(action, InputBinding::Key(*key));
        }
    }
    let pads: [(&str, &[PadButton]); 7] = [
        (PLAYER_FORWARD, &[PadButton::LeftStickUp, PadButton::DpadUp]),
        (
            PLAYER_BACK,
            &[PadButton::LeftStickDown, PadButton::DpadDown],
        ),
        (
            PLAYER_LEFT,
            &[PadButton::LeftStickLeft, PadButton::DpadLeft],
        ),
        (
            PLAYER_RIGHT,
            &[PadButton::LeftStickRight, PadButton::DpadRight],
        ),
        (PLAYER_JUMP, &[PadButton::South]),
        (PLAYER_SPRINT, &[PadButton::LeftStick]),
        (PLAYER_CROUCH, &[PadButton::East]),
    ];
    for (action, buttons) in pads {
        for button in buttons {
            map.bind(action, InputBinding::Pad(*button));
        }
    }
}

/// Right-stick look speed at full tilt, in radians per second.
const PAD_LOOK_SPEED: f32 = 3.0;
/// Stick tilt below this is ignored, so a worn stick does not drift.
const PAD_DEAD_ZONE: f32 = 0.15;

/// Per rendered frame: cursor capture, mouse look, and jump requests.
pub(super) fn player_look(
    mut input: ResMut<RuntimeInput>,
    actions: Res<ActionMap>,
    physics: Res<PhysicsWorld>,
    time: Res<FrameTime>,
    mut players: Query<(
        Entity,
        &mut PlayerController,
        &mut Transform,
        Option<&Children>,
    )>,
    mut cameras: Query<
        &mut Transform,
        (With<Camera>, Without<PlayerController>),
    >,
) {
    let Some(mouse_look) = players
        .iter()
        .map(|(_, player, ..)| player.mouse_look)
        .reduce(|a, b| a || b)
    else {
        return;
    };
    if input.key_just_pressed(KeyCode::Escape) || !mouse_look {
        if input.cursor_captured() {
            input.set_cursor_captured(false);
        }
    } else if input.mouse_just_pressed(MouseButton::Left) {
        input.set_cursor_captured(true);
    }
    let motion = input.mouse_motion();
    let stick = input.stick(Stick::Right).map(|axis| {
        if axis.abs() < PAD_DEAD_ZONE {
            0.0
        } else {
            axis
        }
    });
    let pad_turn = PAD_LOOK_SPEED * time.real_delta.as_secs_f32();
    let jump = actions.just_pressed(&input, PLAYER_JUMP);
    for (entity, mut player, mut transform, children) in &mut players {
        if input.cursor_captured() && player.mouse_look {
            let sensitivity = player.look_sensitivity;
            player.yaw -= motion[0] * sensitivity;
            player.pitch -= motion[1] * sensitivity;
        }
        // Stick up looks up; it needs no captured cursor.
        player.yaw -= stick[0] * pad_turn;
        player.pitch += stick[1] * pad_turn;
        let [low, high] = player.pitch_limits;
        player.pitch = player
            .pitch
            .max(low.max(-MAX_PITCH))
            .min(high.min(MAX_PITCH));
        if let Some([low, high]) = player.yaw_limits {
            player.yaw = player.yaw.max(low).min(high);
        }
        player.jump_requested |= jump;
        transform.rotation = [0.0, player.yaw, 0.0];
        // Behind the orbit center along the view direction, which is -Z
        // tilted up by pitch.
        let (sin, cos) = sim_math::sin_cos(player.pitch);
        let orbit = player.camera_distance > 0.0;
        let mut distance = player.camera_distance;
        if orbit {
            // Stop a camera-sized sphere at the first solid collider behind
            // the orbit center, ignoring the player's own body.
            let [x, y, z] = transform.position;
            let (yaw_sin, yaw_cos) = sim_math::sin_cos(player.yaw);
            let back = [cos * yaw_sin, -sin, cos * yaw_cos];
            if let Some(hit) = physics.shape_cast(
                ColliderShape::Sphere {
                    radius: CAMERA_RADIUS,
                },
                [x, y + player.camera_height, z],
                back,
                distance,
                u32::MAX,
                Some(entity),
            ) {
                distance = hit.distance;
            }
        }
        for child in children.into_iter().flat_map(|children| &children.0) {
            if let Ok(mut camera) = cameras.get_mut(*child) {
                camera.rotation = [player.pitch, 0.0, 0.0];
                let [x, y, z] = player.camera_offset;
                if orbit {
                    camera.position = [
                        x,
                        y + player.camera_height - sin * distance,
                        z + cos * distance,
                    ];
                } else if player.camera_offset != [0.0; 3] {
                    camera.position = player.camera_offset;
                    camera.position[1] -= player.crouch_drop;
                } else {
                    camera.position[1] -=
                        player.crouch_drop - player.camera_drop;
                }
            }
        }
        if !orbit && player.camera_offset == [0.0; 3] {
            player.camera_drop = player.crouch_drop;
        }
    }
}

/// The shape of a body crouched to `height` metres tall and how far its
/// center drops to keep the feet in place, or `None` when the shape cannot
/// crouch or is already that low.
fn crouch_shape(
    shape: ColliderShape,
    height: f32,
) -> Option<(ColliderShape, f32)> {
    let half = height / 2.0;
    match shape {
        ColliderShape::Capsule {
            half_height,
            radius,
        } if half_height + radius > half.max(radius) => {
            let half = half.max(radius);
            Some((
                ColliderShape::Capsule {
                    half_height: half - radius,
                    radius,
                },
                half_height + radius - half,
            ))
        }
        ColliderShape::Box { half_extents } if half_extents[1] > half => {
            Some((
                ColliderShape::Box {
                    half_extents: [half_extents[0], half, half_extents[2]],
                },
                half_extents[1] - half,
            ))
        }
        _ => None,
    }
}

/// Gives a dynamic body the walker ran into at least the walker's speed into
/// it, along the contact normal. The slide stops the walker a skin short of
/// the body, so without this the solver never sees them overlap.
// ponytail: ignores the body's mass, so a heavy crate moves as fast as a
// light one; scale by mass if games need heavy things to resist.
fn push(
    bodies: &mut Query<&mut RigidBody, Without<PlayerController>>,
    wall: Entity,
    normal: [f32; 3],
    motion: [f32; 3],
    dt: f32,
) {
    let Ok(mut body) = bodies.get_mut(wall) else {
        return;
    };
    let length = normal[0].hypot(normal[2]);
    if body.kind != RigidBodyKind::Dynamic || length < 1e-3 || dt <= 0.0 {
        return;
    }
    // Horizontal direction into the body.
    let into = [-normal[0] / length, -normal[2] / length];
    let speed = (motion[0] * into[0] + motion[2] * into[1]) / dt;
    let velocity = &mut body.linear_velocity;
    let current = velocity[0] * into[0] + velocity[2] * into[1];
    if speed > current {
        velocity[0] += into[0] * (speed - current);
        velocity[2] += into[1] * (speed - current);
    }
}

/// Per fixed step: walking, gravity, jumping, and collision sliding.
#[allow(clippy::type_complexity)]
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
    floors: Query<
        (&Transform, Option<&Parent>, Option<&GlobalTransform>),
        Without<PlayerController>,
    >,
    mut pushed: Query<&mut RigidBody, Without<PlayerController>>,
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
    let crouch = actions.held(&input, PLAYER_CROUCH);
    for (entity, mut player, mut transform, collider) in &mut players {
        let mut shape =
            collider.map_or(DEFAULT_PLAYER_SHAPE, |collider| collider.shape);
        match (player.crouch_height > 0.0)
            .then(|| crouch_shape(shape, player.crouch_height))
            .flatten()
        {
            Some((low, drop)) => {
                if crouch && !player.crouched {
                    player.crouched = true;
                    transform.position[1] -= drop;
                } else if !crouch && player.crouched {
                    // Stand only when the full shape fits above.
                    let blocked = physics
                        .shape_cast(
                            low,
                            transform.position,
                            [0.0, 1.0, 0.0],
                            2.0 * drop,
                            player.collision_mask,
                            Some(entity),
                        )
                        .is_some();
                    if !blocked {
                        player.crouched = false;
                        transform.position[1] += drop;
                    }
                }
                if player.crouched {
                    shape = low;
                }
                player.crouch_drop = if player.crouched { drop } else { 0.0 };
            }
            None => {
                player.crouched = false;
                player.crouch_drop = 0.0;
            }
        }
        let (sin, cos) = sim_math::sin_cos(player.yaw);
        // Forward is -Z turned by yaw; right is +X turned by yaw.
        let mut motion = [
            -sin * forward + cos * right,
            0.0,
            -cos * forward - sin * right,
        ];
        let length = motion[0].hypot(motion[2]);
        let speed = player.walk_speed
            * if player.crouched {
                player.crouch_multiplier
            } else if sprint {
                player.sprint_multiplier
            } else {
                1.0
            };
        if length > 0.0 {
            motion = motion.map(|value| value / length * speed * dt);
        }
        if player.jump_requested && !player.crouched {
            if player.grounded {
                player.vertical_speed = player.jump_speed;
            } else if player.air_jumps_used < player.air_jumps {
                player.air_jumps_used += 1;
                player.vertical_speed = player.jump_speed;
            }
        }
        player.jump_requested = false;
        player.vertical_speed -= player.gravity * dt;
        motion[1] = player.vertical_speed * dt;
        let start = CharacterMove::ride(
            &physics,
            shape,
            transform.position,
            player.floor,
            player.collision_mask,
            entity,
            |floor| {
                floors.get(floor).ok().map(|(t, parent, global)| {
                    world_position(t, parent, global)
                })
            },
        );

        let moved = physics.move_character_on_foot(
            shape,
            start,
            motion,
            player.collision_mask,
            Some(entity),
            player.max_slope,
            player.max_step_height,
        );
        if let Some((wall, normal)) = moved.wall.filter(|_| player.push_bodies)
        {
            push(&mut pushed, wall, normal, motion, dt);
        }
        player.grounded = moved.grounded;
        if moved.grounded {
            player.air_jumps_used = 0;
        }
        if (moved.grounded && player.vertical_speed < 0.0)
            || (moved.ceiling && player.vertical_speed > 0.0)
        {
            player.vertical_speed = 0.0;
        }
        player.floor = moved.floor.and_then(|floor| {
            let (t, parent, global) = floors.get(floor).ok()?;
            Some((floor, world_position(t, parent, global)))
        });
        player.wall = moved.wall.map(|(wall, _)| wall);
        if dt > 0.0 {
            player.velocity = std::array::from_fn(|axis| {
                (moved.position[axis] - transform.position[axis]) / dt
            });
        }
        transform.position = moved.position;
    }
}

/// Per fixed step: turns the non-camera children of a walking player toward
/// its horizontal velocity at `turn_speed`, in the body's local yaw.
pub(super) fn player_face(
    time: Res<FrameTime>,
    players: Query<(&PlayerController, Option<&Children>)>,
    mut rigs: Query<
        &mut Transform,
        (Without<Camera>, Without<PlayerController>),
    >,
) {
    use std::f32::consts::{PI, TAU};
    let dt = time.fixed_delta.as_secs_f32();
    for (player, children) in &players {
        let [vx, _, vz] = player.velocity;
        if player.turn_speed <= 0.0 || vx.hypot(vz) < 0.1 {
            continue;
        }
        // Model forward is -Z, as for yaw.
        let heading = sim_math::atan2(-vx, -vz) - player.yaw;
        for child in children.into_iter().flat_map(|children| &children.0) {
            if let Ok(mut rig) = rigs.get_mut(*child) {
                let turn =
                    (heading - rig.rotation[1] + PI).rem_euclid(TAU) - PI;
                let step = player.turn_speed * dt;
                rig.rotation[1] += turn.clamp(-step, step);
            }
        }
    }
}
