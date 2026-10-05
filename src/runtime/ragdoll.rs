//! Ragdolls: hand a character's bones from animation to CPU physics and
//! back.
//!
//! Going limp spawns one capsule body per [`RagdollBone`], placed on the
//! bone and moving as the animation moved it last tick, joined to the
//! nearest ancestor bone's body. While limp, each bone follows its body;
//! bones without one keep their animated local pose. Recovering moves the
//! character under its hips, removes the bodies, and blends every bone from
//! the limp pose back to the animation over `blend_time`.
//!
//! With `muscle` above 0 the ragdoll is active: the bodies exist from the
//! start and muscles turn each body toward its bone's animated pose
//! relative to its parent body, while the top body is held to the animated
//! hips. Hits push the bodies and the muscles pull them back; a hit at
//! `hit_speed` drops the muscles until the character recovers, after which
//! they regain strength over `blend_time`.
//!
//! Runs at the end of the animation step, after IK, in scene ID then entity
//! order, so bodies spawn in a stable order.

use std::collections::{HashMap, HashSet};

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, World};
use nalgebra::{UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};

use super::cpu_physics::next_spawn_order;
use super::ik::{position, rotation_of, world_matrix};
use super::{
    find_target, sim_math, Children, Collider, ColliderShape, Joint, JointKind,
    Parent, PhysicsBody, PhysicsWorld, RigidBody, RigidBodyKind, SceneId,
};
use crate::Transform;

/// Lets a character go limp when hit or told to, and get back up.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ragdoll {
    pub bones: Vec<RagdollBone>,
    /// A CPU body closing on the character's colliders at least this fast
    /// (m/s) makes it go limp; 0 only on command.
    pub hit_speed: f32,
    /// Seconds limp before getting back up; 0 waits for game code.
    pub recover_after: f32,
    /// Seconds to blend from the limp pose back to the animation, or for
    /// an active ragdoll's muscles to regain full strength.
    pub blend_time: f32,
    /// Muscle stiffness as a frequency in Hz; above 0 makes the ragdoll
    /// active. 5 is loose, 15 is stiff.
    pub muscle: f32,
    /// Set by game code: `true` goes limp, `false` gets back up.
    #[serde(skip)]
    pub command: Option<bool>,
}

impl Default for Ragdoll {
    fn default() -> Self {
        Self {
            bones: Vec::new(),
            hit_speed: 0.0,
            recover_after: 0.0,
            blend_time: 0.5,
            muscle: 0.0,
            command: None,
        }
    }
}

/// One limp body: a capsule from the bone's origin along its local +Y.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RagdollBone {
    /// Bone path from the character.
    pub path: String,
    /// Capsule length in metres, ends included.
    pub length: f32,
    pub radius: f32,
    /// kg.
    pub mass: f32,
    /// Joint to the nearest ancestor bone's body.
    pub joint: JointKind,
    /// Joint axes in the bone's frame, as Euler angles; the default turns
    /// X (the hinge and twist axis) along the bone.
    pub frame: [f32; 3],
}

impl Default for RagdollBone {
    fn default() -> Self {
        Self {
            path: String::new(),
            length: 0.3,
            radius: 0.06,
            mass: 2.0,
            joint: JointKind::ConeTwist {
                swing: 0.7,
                twist: [-0.4, 0.4],
            },
            frame: [0.0, 0.0, std::f32::consts::FRAC_PI_2],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RagdollPhase {
    #[default]
    Animated,
    /// Bodies driven toward the animation by muscles.
    Active,
    Limp,
    Blending,
}

type Pose = (Vector3<f32>, UnitQuaternion<f32>);

/// A ragdoll's runtime state. Not saved.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct RagdollState {
    pub phase: RagdollPhase,
    /// Seconds in this phase.
    pub time: f32,
    /// Body per bone; `Entity::PLACEHOLDER` for a missing bone.
    pub parts: Vec<Entity>,
    /// Bone world poses at the end of last tick, for the handoff velocity.
    previous: Vec<Option<Pose>>,
    /// Bone local transforms when the character went limp.
    before: Vec<Transform>,
    /// What this module last wrote to each bone.
    written: Vec<Transform>,
    /// The limp pose blending starts from.
    from: Vec<Transform>,
    /// The character's own collider, taken while limp.
    collider: Option<Collider>,
}

/// Steps every ragdoll; see the module docs.
pub fn step_ragdolls(world: &mut World, dt: f32) {
    let mut roots: Vec<_> = world
        .query::<(Entity, &Ragdoll, Option<&SceneId>)>()
        .iter(world)
        .map(|(entity, _, id)| (id.map(|id| id.0.as_u128()), entity))
        .collect();
    roots.sort_unstable_by_key(|(id, entity)| (*id, entity.to_bits()));
    for (_, root) in roots {
        let command = world
            .get_mut::<Ragdoll>(root)
            .and_then(|mut ragdoll| ragdoll.command.take());
        let ragdoll = world.get::<Ragdoll>(root).expect("queried").clone();
        let mut state =
            world.get::<RagdollState>(root).cloned().unwrap_or_default();
        let bones: Vec<Entity> = ragdoll
            .bones
            .iter()
            .map(|bone| {
                find_target(world, root, &bone.path)
                    .unwrap_or(Entity::PLACEHOLDER)
            })
            .collect();
        let order = depth_order(&ragdoll);
        let active = ragdoll.muscle > 0.0;
        if state.phase != RagdollPhase::Limp
            && (command == Some(true) || hit(world, root, &ragdoll, &state))
        {
            if state.parts.is_empty() {
                spawn_parts(
                    world, root, &ragdoll, &bones, &order, &mut state, dt,
                );
            }
            state.phase = RagdollPhase::Limp;
            state.time = 0.0;
        } else if state.phase == RagdollPhase::Active && !active {
            // Muscles switched off: limp, then back to plain animation.
            state.phase = RagdollPhase::Limp;
            state.time = 0.0;
        } else if state.phase == RagdollPhase::Animated && active {
            spawn_parts(world, root, &ragdoll, &bones, &order, &mut state, dt);
            state.phase = RagdollPhase::Active;
            state.time = ragdoll.blend_time;
        } else {
            state.time += dt;
            match state.phase {
                RagdollPhase::Limp => {
                    follow(world, &ragdoll, &bones, &order, &mut state);
                    let timed = ragdoll.recover_after > 0.0
                        && state.time >= ragdoll.recover_after;
                    if command == Some(false) || timed {
                        move_under_hips(
                            world, root, &ragdoll, &bones, &order, &state,
                        );
                        if active {
                            state.phase = RagdollPhase::Active;
                            state.time = 0.0;
                        } else {
                            recover(
                                world, root, &ragdoll, &bones, &order,
                                &mut state,
                            );
                        }
                    }
                }
                RagdollPhase::Active => {
                    drive(world, &ragdoll, &bones, &order, &mut state, dt);
                    follow(world, &ragdoll, &bones, &order, &mut state);
                }
                RagdollPhase::Blending => {
                    blend(world, &ragdoll, &bones, &mut state);
                }
                RagdollPhase::Animated => {}
            }
        }
        if state.phase != RagdollPhase::Limp {
            state.previous = bones.iter().map(|&b| pose(world, b)).collect();
        }
        world.entity_mut(root).insert(state);
    }
}

/// Bone indices, shallowest path first, so parents come before children.
fn depth_order(ragdoll: &Ragdoll) -> Vec<usize> {
    let mut order: Vec<usize> = (0..ragdoll.bones.len()).collect();
    order.sort_by_key(|&i| ragdoll.bones[i].path.split('/').count());
    order
}

fn pose(world: &World, bone: Entity) -> Option<Pose> {
    let model = world_matrix(world, bone)?;
    let rotation = UnitQuaternion::from_rotation_matrix(&rotation_of(&model));
    Some((position(&model), rotation))
}

/// Middle of the bone's capsule, in the bone's frame.
fn half(bone: &RagdollBone) -> Vector3<f32> {
    Vector3::new(0.0, bone.length / 2.0, 0.0)
}

fn euler(rotation: &UnitQuaternion<f32>) -> [f32; 3] {
    let (roll, pitch, yaw) =
        sim_math::euler_from_rotation(&rotation.to_rotation_matrix());
    [roll, pitch, yaw]
}

fn quaternion([roll, pitch, yaw]: [f32; 3]) -> UnitQuaternion<f32> {
    UnitQuaternion::from_rotation_matrix(&sim_math::rotation_from_euler(
        roll, pitch, yaw,
    ))
}

/// Whether a CPU body closed on the character's own colliders at
/// `hit_speed` or faster in this tick's physics step.
fn hit(
    world: &World,
    root: Entity,
    ragdoll: &Ragdoll,
    state: &RagdollState,
) -> bool {
    if ragdoll.hit_speed <= 0.0 {
        return false;
    }
    let Some(physics) = world.get_resource::<PhysicsWorld>() else {
        return false;
    };
    let mut own = HashSet::from([root]);
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        for &child in world.get::<Children>(at).map_or(&[][..], |c| &c.0) {
            own.insert(child);
            stack.push(child);
        }
    }
    own.extend(&state.parts);
    physics
        .contacts()
        .iter()
        .chain(physics.impacts())
        .any(|contact| {
            !contact.sensor
                && own.contains(&contact.a) != own.contains(&contact.b)
                && contact.speed >= ragdoll.hit_speed
        })
}

/// Spawns a body per bone, moving as the animation moved it, and takes the
/// character's own collider.
fn spawn_parts(
    world: &mut World,
    root: Entity,
    ragdoll: &Ragdoll,
    bones: &[Entity],
    order: &[usize],
    state: &mut RagdollState,
    dt: f32,
) {
    let index: HashMap<Entity, usize> =
        bones.iter().enumerate().map(|(i, &b)| (b, i)).collect();
    state.before = bones
        .iter()
        .map(|&b| world.get::<Transform>(b).copied().unwrap_or_default())
        .collect();
    state.written.clone_from(&state.before);
    state.parts = vec![Entity::PLACEHOLDER; bones.len()];
    let mut centres: Vec<Option<Pose>> = vec![None; bones.len()];
    for &i in order {
        let bone = &ragdoll.bones[i];
        let Some((at, turn)) = pose(world, bones[i]) else {
            continue;
        };
        let offset = turn * half(bone);
        let centre = at + offset;
        let (linear, angular) = match state.previous.get(i).copied().flatten() {
            Some((was, was_turn)) if dt > 0.0 => {
                let spin = (turn * was_turn.inverse()).scaled_axis() / dt;
                ((at - was) / dt + spin.cross(&offset), spin)
            }
            _ => (Vector3::zeros(), Vector3::zeros()),
        };
        let order = next_spawn_order(world);
        let part = world
            .spawn((
                Transform {
                    position: centre.into(),
                    rotation: euler(&turn),
                    scale: [1.0; 3],
                },
                Collider {
                    shape: ColliderShape::Capsule {
                        half_height: (bone.length / 2.0 - bone.radius).max(0.0),
                        radius: bone.radius,
                    },
                    ..Collider::default()
                },
                PhysicsBody::default(),
                RigidBody {
                    kind: RigidBodyKind::Dynamic,
                    mass: bone.mass,
                    linear_velocity: linear.into(),
                    angular_velocity: angular.into(),
                    ..RigidBody::default()
                },
                order,
            ))
            .id();
        // Hang it from the nearest ancestor bone that has a body.
        let mut at_bone = bones[i];
        let mut parent = None;
        while let Some(up) = world.get::<Parent>(at_bone).map(|p| p.0) {
            if up == root {
                break;
            }
            at_bone = up;
            if let Some(&j) = index.get(&up) {
                parent = centres[j].map(|pose| (j, pose));
                break;
            }
        }
        if let Some((j, (parent_at, parent_turn))) = parent {
            let frame = quaternion(bone.frame);
            world.entity_mut(part).insert(Joint {
                target: state.parts[j],
                kind: bone.joint,
                anchor: (-half(bone)).into(),
                frame: bone.frame,
                target_anchor: (parent_turn.inverse() * (at - parent_at))
                    .into(),
                target_frame: euler(&(parent_turn.inverse() * turn * frame)),
                collide_connected: false,
                ..Joint::default()
            });
        }
        state.parts[i] = part;
        centres[i] = Some((centre, turn));
    }
    state.collider = world.entity_mut(root).take::<Collider>();
}

/// Turns each body toward its bone's animated pose relative to its parent
/// body, and pulls the top body toward the animated hips, with a critically
/// damped spring at `muscle` Hz solved implicitly (stable at any step).
// ponytail: muscles push only their own body, with no reaction on the
// parent; add equal and opposite impulses if whole-body momentum matters.
fn drive(
    world: &mut World,
    ragdoll: &Ragdoll,
    bones: &[Entity],
    order: &[usize],
    state: &mut RagdollState,
    dt: f32,
) {
    // Bones this module wrote last tick take their pose from before the
    // handoff; the rest hold this tick's animation.
    for (i, &bone) in bones.iter().enumerate() {
        if let Some(mut transform) = world.get_mut::<Transform>(bone) {
            if state.written.get(i) == Some(&*transform) {
                *transform = state.before[i];
            }
        }
    }
    let targets: Vec<Option<Pose>> =
        bones.iter().map(|&b| pose(world, b)).collect();
    let strength = if ragdoll.blend_time > 0.0 {
        (state.time / ragdoll.blend_time).min(1.0)
    } else {
        1.0
    };
    let omega = std::f32::consts::TAU * ragdoll.muscle;
    let (k, c) = (omega * omega, 2.0 * omega);
    let spring = |velocity: Vector3<f32>, error: Vector3<f32>| {
        let driven =
            (velocity + error * (k * dt)) / (1.0 + c * dt + k * dt * dt);
        velocity + (driven - velocity) * strength
    };
    let body = |world: &World, part: Entity| {
        let transform = world.get::<Transform>(part)?;
        let rigid = world.get::<RigidBody>(part)?;
        Some((
            Vector3::from(transform.position),
            quaternion(transform.rotation),
            Vector3::from(rigid.linear_velocity),
            Vector3::from(rigid.angular_velocity),
        ))
    };
    for &i in order {
        let part = state.parts[i];
        let (Some((at, turn)), Some((centre, now, linear, angular))) =
            (targets[i], body(world, part))
        else {
            continue;
        };
        let parent = world.get::<Joint>(part).and_then(|joint| {
            let j = state.parts.iter().position(|&p| p == joint.target)?;
            Some((targets[j]?, body(world, joint.target)?))
        });
        let (linear, angular) = match parent {
            Some(((_, parent_target), (_, parent_now, _, parent_spin))) => {
                let want = parent_now * parent_target.inverse() * turn;
                let error = (want * now.inverse()).scaled_axis();
                (linear, parent_spin + spring(angular - parent_spin, error))
            }
            None => {
                let goal = at + turn * half(&ragdoll.bones[i]);
                let error = (turn * now.inverse()).scaled_axis();
                (spring(linear, goal - centre), spring(angular, error))
            }
        };
        if let Some(mut physics) = world.get_resource_mut::<PhysicsWorld>() {
            physics.keep_awake(part);
        }
        let mut entity = world.entity_mut(part);
        entity.remove::<super::Sleeping>();
        if let Some(mut rigid) = entity.get_mut::<RigidBody>() {
            rigid.linear_velocity = linear.into();
            rigid.angular_velocity = angular.into();
        }
    }
}

/// Puts each bone with a body where its body is.
fn follow(
    world: &mut World,
    ragdoll: &Ragdoll,
    bones: &[Entity],
    order: &[usize],
    state: &mut RagdollState,
) {
    for &i in order {
        let (bone, part) = (bones[i], state.parts[i]);
        let Some(body) = world.get::<Transform>(part).copied() else {
            continue;
        };
        let turn = quaternion(body.rotation);
        let at = Vector3::from(body.position) - turn * half(&ragdoll.bones[i]);
        let parent = world
            .get::<Parent>(bone)
            .and_then(|parent| world_matrix(world, parent.0));
        let (at, turn) =
            match parent.and_then(|m| m.try_inverse().map(|i| (m, i))) {
                Some((model, inverse)) => (
                    inverse.transform_point(&at.into()).coords,
                    UnitQuaternion::from_rotation_matrix(&rotation_of(&model))
                        .inverse()
                        * turn,
                ),
                None => (at, turn),
            };
        let Some(mut transform) = world.get_mut::<Transform>(bone) else {
            continue;
        };
        transform.position = at.into();
        transform.rotation = euler(&turn);
        state.written[i] = *transform;
    }
}

/// Moves the character across the ground so its hips stand where its
/// top body lies.
fn move_under_hips(
    world: &mut World,
    root: Entity,
    ragdoll: &Ragdoll,
    bones: &[Entity],
    order: &[usize],
    state: &RagdollState,
) {
    // ponytail: moves the character only across the ground (x, z); it
    // keeps its height and facing, so a game that needs a get-up turn or a
    // floor probe moves the character itself before recovering.
    if let Some(&first) = order.first() {
        let lies = world.get::<Transform>(state.parts[first]).map(|body| {
            Vector3::from(body.position)
                - quaternion(body.rotation) * half(&ragdoll.bones[first])
        });
        let stood = world
            .get::<Parent>(bones[first])
            .and_then(|parent| world_matrix(world, parent.0))
            .map(|model| {
                let local = state.before[first].position;
                model.transform_point(&local.into()).coords
            });
        if world.get::<Parent>(root).is_none() {
            if let (Some(lies), Some(stood), Some(mut transform)) =
                (lies, stood, world.get_mut::<Transform>(root))
            {
                transform.position[0] += lies.x - stood.x;
                transform.position[2] += lies.z - stood.z;
            }
        }
    }
}

/// Removes the bodies and starts blending back.
fn recover(
    world: &mut World,
    root: Entity,
    ragdoll: &Ragdoll,
    bones: &[Entity],
    order: &[usize],
    state: &mut RagdollState,
) {
    follow(world, ragdoll, bones, order, state);
    state.from.clone_from(&state.written);
    for part in std::mem::take(&mut state.parts) {
        if part != Entity::PLACEHOLDER {
            world.despawn(part);
        }
    }
    if let Some(collider) = state.collider.take() {
        world.entity_mut(root).insert(collider);
    }
    state.phase = RagdollPhase::Blending;
    state.time = 0.0;
}

/// Mixes each bone from the limp pose toward this tick's animated pose (or
/// its pose before going limp when no clip writes it).
fn blend(
    world: &mut World,
    ragdoll: &Ragdoll,
    bones: &[Entity],
    state: &mut RagdollState,
) {
    let weight = if ragdoll.blend_time > 0.0 {
        (state.time / ragdoll.blend_time).min(1.0)
    } else {
        1.0
    };
    for (i, &bone) in bones.iter().enumerate() {
        let (Some(from), Some(before), Some(written)) =
            (state.from.get(i), state.before.get(i), state.written.get(i))
        else {
            continue;
        };
        let Some(mut transform) = world.get_mut::<Transform>(bone) else {
            continue;
        };
        let target = if *transform == *written {
            *before
        } else {
            *transform
        };
        let (a, b) = (quaternion(from.rotation), quaternion(target.rotation));
        let b = if a.coords.dot(&b.coords) < 0.0 {
            UnitQuaternion::new_unchecked(-b.into_inner())
        } else {
            b
        };
        let mix = |x: f32, y: f32| x + (y - x) * weight;
        *transform = Transform {
            position: std::array::from_fn(|k| {
                mix(from.position[k], target.position[k])
            }),
            rotation: euler(&a.nlerp(&b, weight)),
            scale: target.scale,
        };
        state.written[i] = *transform;
    }
    if weight >= 1.0 {
        *state = RagdollState {
            previous: std::mem::take(&mut state.previous),
            ..RagdollState::default()
        };
    }
}
