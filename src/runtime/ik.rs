//! Inverse kinematics on joint chains, solved each fixed step after the
//! animation pose is written.
//!
//! An [`Ik`] sits on the end of a chain (a hand, a foot, a head) and names
//! the `target` object it reaches for. `LookAt` turns the object itself so
//! its `forward` axis points at the target. `TwoBone` bends the object's
//! parent and grandparent, like an arm or leg, so the object's origin lands
//! on the target; the optional `pole` object picks which way the middle
//! joint (elbow, knee) points. World poses are read from the `Transform`
//! chain, so the solve sees this tick's animation. `Foot` also turns the
//! foot to the ground slope and lowers the hips (the leg's grandparent's
//! parent) when the ground under a foot is out of the leg's reach.
//!
//! Each joint IK writes remembers its pose from before the solve in an
//! [`IkPose`]. Next tick, a joint nothing else wrote goes back to that pose
//! before solving again, so results never pile up tick after tick.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, World};
use nalgebra::{Matrix3, Matrix4, Point3, Rotation3, UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};

use super::{sim_math, Parent, SceneId};
use crate::Transform;

/// How an [`Ik`] moves its chain.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum IkKind {
    /// Turns this object so `forward` points at the target.
    #[default]
    LookAt,
    /// Bends the parent and grandparent so this object reaches the target.
    TwoBone,
    /// `TwoBone` toward the ground under this object instead of a target:
    /// the foot keeps its animated height above the character's floor.
    Foot,
    /// Bends `joints` ancestors, like a tail, spine or tentacle, so this
    /// object reaches the target (FABRIK).
    Chain,
}

/// Inverse kinematics from this object to `target`. See the module docs.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ik {
    pub kind: IkKind,
    /// Object to reach for or look at; null turns the solve off.
    pub target: Entity,
    /// `TwoBone` only: the middle joint bends toward this object. Null
    /// keeps the bend plane the pose already has.
    pub pole: Entity,
    /// 0 keeps the animated pose (skips the solve), 1 solves fully.
    pub weight: f32,
    /// `LookAt` only: the axis in this object's local space that points
    /// at the target.
    pub forward: [f32; 3],
    /// `Foot` only: how far above and below the character's floor to look
    /// for ground, in metres. About one step height.
    pub reach: f32,
    /// `Chain` only: how many joints above this object bend.
    pub joints: u32,
}

impl Default for Ik {
    fn default() -> Self {
        Self {
            kind: IkKind::default(),
            target: Entity::PLACEHOLDER,
            pole: Entity::PLACEHOLDER,
            weight: 1.0,
            forward: [0.0, 0.0, -1.0],
            reach: 0.5,
            joints: 3,
        }
    }
}

/// A foot's target, the ground normal there, and the foot's character.
type Ground = (Vector3<f32>, Vector3<f32>, Entity);

/// A joint's local transform before and after last tick's IK, so the next
/// tick can undo it. Runtime only; not saved.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct IkPose {
    before: Transform,
    after: Transform,
}

/// Undoes last tick's IK, then solves every [`Ik`] in scene ID, then entity
/// order. Feet find their ground first, then hips lower, then chains solve.
pub fn solve_ik(world: &mut World) {
    let mut poses = world.query::<(Entity, &IkPose, &mut Transform)>();
    let mut undone = Vec::new();
    for (entity, pose, mut transform) in poses.iter_mut(world) {
        if *transform == pose.after {
            *transform = pose.before;
        }
        undone.push(entity);
    }
    for entity in undone {
        world.entity_mut(entity).remove::<IkPose>();
    }
    let mut query = world.query::<(Entity, &Ik, Option<&SceneId>)>();
    let mut solves: Vec<_> = query
        .iter(world)
        .filter(|(_, ik, _)| {
            (ik.kind == IkKind::Foot || ik.target != Entity::PLACEHOLDER)
                && ik.weight > 0.0
        })
        .map(|(entity, ik, id)| {
            ((id.map(|id| id.0.as_u128()), entity.to_bits()), entity, *ik)
        })
        .collect();
    solves.sort_by_key(|solve| solve.0);
    let grounds: Vec<_> = solves
        .iter()
        .map(|(_, entity, ik)| {
            (ik.kind == IkKind::Foot)
                .then(|| ground(world, *entity, ik.reach))
                .flatten()
        })
        .collect();
    let feet: Vec<_> = solves
        .iter()
        .zip(&grounds)
        .filter_map(|((_, foot, ik), ground)| {
            Some((*foot, ik.weight, (*ground)?))
        })
        .collect();
    lower_hips(world, &feet);
    for ((_, entity, ik), ground) in solves.into_iter().zip(grounds) {
        match ik.kind {
            IkKind::LookAt | IkKind::TwoBone | IkKind::Chain => {
                let Some(target) = world_matrix(world, ik.target) else {
                    continue;
                };
                let target = position(&target);
                match ik.kind {
                    IkKind::LookAt => look_at(world, entity, &ik, target),
                    IkKind::TwoBone => two_bone(world, entity, &ik, target),
                    _ => chain(world, entity, &ik, target),
                }
            }
            IkKind::Foot => {
                let Some((target, normal, _)) = ground else {
                    continue;
                };
                // The foot keeps its animated world turn, tilted to the
                // slope, however the leg bends above it.
                let Some(model) = world_matrix(world, entity) else {
                    continue;
                };
                let tilted = arc(Vector3::y(), normal) * rotation_of(&model);
                two_bone(world, entity, &ik, target);
                set_world_rotation(world, entity, tilted, ik.weight);
            }
        }
    }
}

/// Lowers each hip (the parent of a foot's leg) by the most any of its
/// feet needs to reach the ground, times that foot's weight. A hip that is
/// the character itself stays put.
fn lower_hips(world: &mut World, feet: &[(Entity, f32, Ground)]) {
    let mut drops = std::collections::BTreeMap::<Entity, f32>::new();
    for &(foot, weight, (target, _, character)) in feet {
        let parent =
            |entity| world.get::<Parent>(entity).map(|parent| parent.0);
        let Some(mid) = parent(foot) else { continue };
        let Some(root) = parent(mid) else { continue };
        let Some(hip) = parent(root).filter(|hip| *hip != character) else {
            continue;
        };
        let (Some(a), Some(b), Some(c)) = (
            world_matrix(world, root),
            world_matrix(world, mid),
            world_matrix(world, foot),
        ) else {
            continue;
        };
        let (a, b, c) = (position(&a), position(&b), position(&c));
        let leg = 0.999 * ((b - a).norm() + (c - b).norm());
        let side = Vector3::new(target.x - a.x, 0.0, target.z - a.z).norm();
        if side >= leg {
            continue;
        }
        let drop = (a.y - target.y - sim_math::sqrt(leg * leg - side * side))
            * weight.clamp(0.0, 1.0);
        let most = drops.entry(hip).or_insert(0.0);
        *most = most.max(drop);
    }
    for (hip, drop) in drops {
        if drop <= 0.0 {
            continue;
        }
        let inverse = world
            .get::<Parent>(hip)
            .and_then(|parent| world_matrix(world, parent.0))
            .and_then(|model| model.try_inverse())
            .unwrap_or_else(Matrix4::identity);
        let down = inverse.transform_vector(&Vector3::new(0.0, -drop, 0.0));
        write(world, hip, |transform| {
            for (axis, step) in transform.position.iter_mut().zip(down.iter()) {
                *axis += step;
            }
        });
    }
}

/// Where a `Foot` should go: its animated position, raised or lowered by
/// the ground height under it relative to the character's floor; then the
/// ground normal and the character. The character is the nearest ancestor
/// with an `Animation`, else the top one; its own colliders are skipped.
fn ground(world: &World, foot: Entity, reach: f32) -> Option<Ground> {
    let physics = world.get_resource::<super::PhysicsWorld>()?;
    let mut character = foot;
    while let Some(parent) = world.get::<Parent>(character) {
        character = parent.0;
        if world.get::<super::Animation>(character).is_some() {
            break;
        }
    }
    let inside = |mut entity: Entity| loop {
        if entity == character {
            return true;
        }
        match world.get::<Parent>(entity) {
            Some(parent) => entity = parent.0,
            None => return false,
        }
    };
    let floor = position(&world_matrix(world, character)?).y;
    let tip = position(&world_matrix(world, foot)?);
    let hit = physics.raycast_where(
        [tip.x, floor + reach, tip.z],
        [0.0, -1.0, 0.0],
        2.0 * reach,
        u32::MAX,
        |entity| !inside(entity),
    )?;
    Some((
        tip + Vector3::y() * (hit.point[1] - floor),
        Vector3::from(hit.normal),
        character,
    ))
}

/// FABRIK: alternately pins the tip to the target and the top joint to its
/// place, moving each joint along the line to its neighbour, then turns
/// each joint, top first, to point at its new child position.
fn chain(world: &mut World, tip: Entity, ik: &Ik, target: Vector3<f32>) {
    let mut joints = vec![tip];
    for _ in 0..ik.joints {
        let Some(parent) = world.get::<Parent>(*joints.last().unwrap()) else {
            break;
        };
        joints.push(parent.0);
    }
    joints.reverse();
    let Some(mut points) = joints
        .iter()
        .map(|joint| world_matrix(world, *joint).map(|model| position(&model)))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    if points.len() < 2 {
        return;
    }
    let lengths: Vec<f32> = points
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).norm())
        .collect();
    let top = points[0];
    let total: f32 = lengths.iter().sum();
    if (target - top).norm() >= total {
        for (index, length) in lengths.iter().enumerate() {
            points[index + 1] = points[index] + unit(target - top) * *length;
        }
    } else {
        let last = points.len() - 1;
        for _ in 0..16 {
            if (points[last] - target).norm() < 1e-4 * total {
                break;
            }
            points[last] = target;
            for index in (0..last).rev() {
                let toward = unit(points[index] - points[index + 1]);
                points[index] = points[index + 1] + toward * lengths[index];
            }
            points[0] = top;
            for index in 0..last {
                let toward = unit(points[index + 1] - points[index]);
                points[index + 1] = points[index] + toward * lengths[index];
            }
        }
    }
    for (index, joint) in joints.iter().enumerate().take(joints.len() - 1) {
        let (Some(model), Some(child)) = (
            world_matrix(world, *joint),
            world_matrix(world, joints[index + 1]),
        ) else {
            return;
        };
        let current = position(&child) - position(&model);
        let wanted = points[index + 1] - position(&model);
        let turned = arc(current, wanted) * rotation_of(&model);
        set_world_rotation(world, *joint, turned, ik.weight);
    }
}

fn look_at(world: &mut World, entity: Entity, ik: &Ik, target: Vector3<f32>) {
    let Some(model) = world_matrix(world, entity) else {
        return;
    };
    let rotation = rotation_of(&model);
    let current = rotation * Vector3::from(ik.forward);
    let wanted = target - position(&model);
    let turned = arc(current, wanted) * rotation;
    set_world_rotation(world, entity, turned, ik.weight);
}

fn two_bone(world: &mut World, tip: Entity, ik: &Ik, t: Vector3<f32>) {
    let parent = |entity| world.get::<Parent>(entity).map(|parent| parent.0);
    let Some(mid) = parent(tip) else { return };
    let Some(root) = parent(mid) else { return };
    let (Some(ma), Some(mb), Some(mc)) = (
        world_matrix(world, root),
        world_matrix(world, mid),
        world_matrix(world, tip),
    ) else {
        return;
    };
    let (a, b, c) = (position(&ma), position(&mb), position(&mc));
    let (ra, rb) = (rotation_of(&ma), rotation_of(&mb));
    let (lab, lcb) = ((b - a).norm(), (c - b).norm());
    if lab < 1e-6 || lcb < 1e-6 {
        return;
    }
    let reach = 1e-4 * (lab + lcb);
    let lat = (t - a).norm().clamp(reach, lab + lcb - reach);
    let angle = |u: Vector3<f32>, v: Vector3<f32>| acos(unit(u).dot(&unit(v)));
    let (ac_ab, ba_bc) = (angle(c - a, b - a), angle(a - b, c - b));
    let ac_at = angle(c - a, t - a);
    let ac_ab_wanted =
        acos((lcb * lcb - lab * lab - lat * lat) / (-2.0 * lab * lat));
    let ba_bc_wanted =
        acos((lat * lat - lab * lab - lcb * lcb) / (-2.0 * lab * lcb));
    // A straight chain has no bend plane; bend toward the pole, or any
    // side.
    let mut bend = (c - a).cross(&(b - a));
    if bend.norm_squared() < 1e-12 {
        let side = match world_matrix(world, ik.pole) {
            Some(pole) if ik.pole != Entity::PLACEHOLDER => position(&pole) - a,
            _ => ra * Vector3::z(),
        };
        bend = (c - a).cross(&side);
        if bend.norm_squared() < 1e-12 {
            bend = (c - a).cross(&Vector3::x());
        }
        if bend.norm_squared() < 1e-12 {
            bend = (c - a).cross(&Vector3::y());
        }
    }
    let bend = unit(bend);
    let r0 = turn(bend, ac_ab_wanted - ac_ab);
    let r1 = turn(bend, ba_bc_wanted - ba_bc);
    let r2 = arc_angle(c - a, t - a, ac_at);
    let mut root_rotation = r2 * r0 * ra;
    if ik.pole != Entity::PLACEHOLDER {
        if let Some(pole) = world_matrix(world, ik.pole) {
            let axis = unit(t - a);
            let flat = |v: Vector3<f32>| v - axis * axis.dot(&v);
            let elbow = flat(r2 * r0 * (b - a));
            let wanted = flat(position(&pole) - a);
            let twist = sim_math::atan2(
                axis.dot(&elbow.cross(&wanted)),
                elbow.dot(&wanted),
            );
            root_rotation = turn(axis, twist) * root_rotation;
        }
    }
    let mid_local = ra.inverse() * r1 * rb;
    set_world_rotation(world, root, root_rotation, ik.weight);
    set_local_rotation(world, mid, mid_local, ik.weight);
}

/// The model matrix from the `Transform` chain; `GlobalTransform` is a
/// frame stale in the fixed step.
pub(super) fn world_matrix(
    world: &World,
    entity: Entity,
) -> Option<Matrix4<f32>> {
    let mut model = Matrix4::from(sim_math::transform_matrix(
        world.get::<Transform>(entity)?,
    ));
    let mut at = entity;
    while let Some(parent) = world.get::<Parent>(at) {
        at = parent.0;
        let Some(transform) = world.get::<Transform>(at) else {
            break;
        };
        model = Matrix4::from(sim_math::transform_matrix(transform)) * model;
    }
    Some(model)
}

pub(super) fn position(model: &Matrix4<f32>) -> Vector3<f32> {
    model.transform_point(&Point3::origin()).coords
}

// ponytail: drops shear from non-uniform parent scale; bones rarely have it.
pub(super) fn rotation_of(model: &Matrix4<f32>) -> Rotation3<f32> {
    let axis = |index: usize| unit(model.fixed_view::<3, 1>(0, index).into());
    Rotation3::from_matrix_unchecked(Matrix3::from_columns(&[
        axis(0),
        axis(1),
        axis(2),
    ]))
}

fn set_world_rotation(
    world: &mut World,
    entity: Entity,
    rotation: Rotation3<f32>,
    weight: f32,
) {
    let parent = world
        .get::<Parent>(entity)
        .and_then(|parent| world_matrix(world, parent.0))
        .map_or_else(Rotation3::identity, |model| rotation_of(&model));
    set_local_rotation(world, entity, parent.inverse() * rotation, weight);
}

fn set_local_rotation(
    world: &mut World,
    entity: Entity,
    rotation: Rotation3<f32>,
    weight: f32,
) {
    let Some(transform) = world.get::<Transform>(entity) else {
        return;
    };
    let [roll, pitch, yaw] = transform.rotation;
    let from = UnitQuaternion::from_rotation_matrix(
        &sim_math::rotation_from_euler(roll, pitch, yaw),
    );
    let mut to = UnitQuaternion::from_rotation_matrix(&rotation);
    if from.coords.dot(&to.coords) < 0.0 {
        to = UnitQuaternion::new_unchecked(-to.into_inner());
    }
    let mixed = from.nlerp(&to, weight.clamp(0.0, 1.0));
    let (roll, pitch, yaw) =
        sim_math::euler_from_rotation(&mixed.to_rotation_matrix());
    write(world, entity, |transform| {
        transform.rotation = [roll, pitch, yaw];
    });
}

/// Changes a joint's transform, keeping its pose from before this tick's
/// first IK write in [`IkPose`].
fn write(
    world: &mut World,
    entity: Entity,
    change: impl FnOnce(&mut Transform),
) {
    let Some(mut transform) = world.get_mut::<Transform>(entity) else {
        return;
    };
    let before = *transform;
    change(&mut transform);
    let after = *transform;
    let before = world
        .get::<IkPose>(entity)
        .map_or(before, |pose| pose.before);
    world.entity_mut(entity).insert(IkPose { before, after });
}

fn unit(v: Vector3<f32>) -> Vector3<f32> {
    Vector3::from(sim_math::normalize(v.into()))
}

fn acos(x: f32) -> f32 {
    let x = x.clamp(-1.0, 1.0);
    sim_math::atan2(sim_math::sqrt(1.0 - x * x), x)
}

fn turn(axis: Vector3<f32>, angle: f32) -> Rotation3<f32> {
    sim_math::rotation_from_scaled_axis(axis * angle)
}

/// The shortest turn taking direction `from` to `to`.
fn arc(from: Vector3<f32>, to: Vector3<f32>) -> Rotation3<f32> {
    let (from, to) = (unit(from), unit(to));
    let cross = from.cross(&to);
    arc_angle(from, to, sim_math::atan2(cross.norm(), from.dot(&to)))
}

fn arc_angle(
    from: Vector3<f32>,
    to: Vector3<f32>,
    angle: f32,
) -> Rotation3<f32> {
    let axis = from.cross(&to);
    if axis.norm_squared() < 1e-12 {
        return Rotation3::identity();
    }
    turn(unit(axis), angle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Children, Parent};

    fn spawn(
        world: &mut World,
        parent: Option<Entity>,
        position: [f32; 3],
    ) -> Entity {
        let entity = world
            .spawn(Transform {
                position,
                ..Transform::default()
            })
            .id();
        if let Some(parent) = parent {
            world.entity_mut(entity).insert(Parent(parent));
            let mut children = world
                .get::<Children>(parent)
                .map(|c| c.0.clone())
                .unwrap_or_default();
            children.push(entity);
            world.entity_mut(parent).insert(Children(children));
        }
        entity
    }

    fn arm(world: &mut World, bent: bool) -> (Entity, Entity) {
        let root = spawn(world, None, [0.0, 0.0, 0.0]);
        let mid = spawn(world, Some(root), [0.0, 1.0, 0.0]);
        let tip = spawn(
            world,
            Some(mid),
            if bent {
                [0.1, 1.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            },
        );
        (root, tip)
    }

    fn reach(world: &World, entity: Entity) -> Vector3<f32> {
        position(&world_matrix(world, entity).unwrap())
    }

    #[test]
    fn two_bone_reaches_the_target_bent_or_straight() {
        for bent in [true, false] {
            for wanted in [[1.0, 1.0, 0.0], [0.0, 1.2, 0.8], [-0.5, -0.5, 0.3]]
            {
                let mut world = World::new();
                let (_, tip) = arm(&mut world, bent);
                let target = spawn(&mut world, None, wanted);
                world.entity_mut(tip).insert(Ik {
                    kind: IkKind::TwoBone,
                    target,
                    ..Ik::default()
                });
                solve_ik(&mut world);
                let miss = (reach(&world, tip) - Vector3::from(wanted)).norm();
                assert!(
                    miss < 1e-3,
                    "bent {bent} target {wanted:?} misses by {miss}"
                );
            }
        }
    }

    #[test]
    fn two_bone_bends_toward_the_pole_and_clamps_far_targets() {
        let mut world = World::new();
        let (root, tip) = arm(&mut world, true);
        let target = spawn(&mut world, None, [0.0, 1.0, 0.0]);
        let pole = spawn(&mut world, None, [0.0, 0.5, -5.0]);
        world.entity_mut(tip).insert(Ik {
            kind: IkKind::TwoBone,
            target,
            pole,
            ..Ik::default()
        });
        solve_ik(&mut world);
        let mid = world.get::<Parent>(tip).unwrap().0;
        assert!(
            (reach(&world, tip) - Vector3::new(0.0, 1.0, 0.0)).norm() < 1e-3
        );
        assert!(reach(&world, mid).z < -0.5, "elbow points at the pole");

        world.get_mut::<Transform>(target).unwrap().position = [0.0, 9.0, 0.0];
        solve_ik(&mut world);
        let end = reach(&world, tip) - reach(&world, root);
        assert!(end.y > 1.99 && end.x.abs() < 1e-2, "{end:?}");
    }

    #[test]
    fn chain_bends_every_joint_to_reach_or_straightens_toward_far_targets() {
        let mut world = World::new();
        let mut at = spawn(&mut world, None, [0.0, 0.0, 0.0]);
        let top = at;
        for _ in 0..4 {
            at = spawn(&mut world, Some(at), [0.0, 0.5, 0.0]);
        }
        let target = spawn(&mut world, None, [1.0, 0.8, 0.3]);
        world.entity_mut(at).insert(Ik {
            kind: IkKind::Chain,
            target,
            joints: 4,
            ..Ik::default()
        });
        solve_ik(&mut world);
        let miss = (reach(&world, at) - Vector3::new(1.0, 0.8, 0.3)).norm();
        assert!(miss < 1e-3, "misses by {miss}");
        assert_eq!(reach(&world, top), Vector3::zeros(), "top stays put");

        world.get_mut::<Transform>(target).unwrap().position = [5.0, 0.0, 0.0];
        solve_ik(&mut world);
        let end = reach(&world, at);
        assert!((end - Vector3::new(2.0, 0.0, 0.0)).norm() < 1e-3, "{end:?}");
    }

    #[test]
    fn look_at_points_forward_at_the_target_with_weight() {
        let mut world = World::new();
        let parent = spawn(&mut world, None, [1.0, 0.0, 0.0]);
        world.get_mut::<Transform>(parent).unwrap().rotation = [0.0, 0.0, 0.7];
        let head = spawn(&mut world, Some(parent), [0.0, 1.0, 0.0]);
        let target = spawn(&mut world, None, [4.0, 3.0, -2.0]);
        world.entity_mut(head).insert(Ik {
            target,
            ..Ik::default()
        });
        solve_ik(&mut world);
        let model = world_matrix(&world, head).unwrap();
        let forward = rotation_of(&model) * Vector3::new(0.0, 0.0, -1.0);
        let wanted = unit(Vector3::new(4.0, 3.0, -2.0) - position(&model));
        assert!(forward.dot(&wanted) > 0.9999);

        world.get_mut::<Ik>(head).unwrap().weight = 0.0;
        world.get_mut::<Transform>(target).unwrap().position = [-4.0, 0.0, 0.0];
        solve_ik(&mut world);
        assert_eq!(world.get::<Transform>(head).unwrap().rotation, [0.0; 3]);
    }
}
