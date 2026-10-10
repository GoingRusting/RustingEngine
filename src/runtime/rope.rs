//! Ropes: a chain of small CPU bodies between two bodies.
//!
//! The first fixed step a [`Rope`] is seen it spawns `segments` sphere
//! beads on the straight line between its two anchors. Each bead hangs from
//! the one before it with a [`JointKind::Distance`] joint of `min` 0, so the
//! rope goes slack but never stretches past `length`, and pulls on both ends
//! through the joints. The first bead hangs from the target; the rope's own
//! body gets the joint to the last bead, so it cannot carry another
//! [`Joint`]. Ropes spawn in scene ID then entity order.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, World};
use nalgebra::{Point3, Vector3};
use serde::{Deserialize, Serialize};

use super::cpu_physics::next_spawn_order;
use super::{
    sim_math, Collider, ColliderShape, Joint, JointKind, PhysicsBody,
    RigidBody, RigidBodyKind, SceneId,
};
use crate::Transform;

/// A rope from this entity's body to `target`'s. See the module docs.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rope {
    /// Body at the far end. `Entity::PLACEHOLDER` (null in scenes) ties it
    /// to the world; then `target_anchor` is in world space.
    pub target: Entity,
    /// Rope end in this body's local space, in metres.
    pub anchor: [f32; 3],
    /// Rope end in the target's local space.
    pub target_anchor: [f32; 3],
    /// Longest the rope stretches, in metres.
    pub length: f32,
    /// Bead count; more bend smoother and cost more.
    pub segments: u32,
    /// Bead radius; keep it under half of `length / (segments + 1)`.
    pub radius: f32,
    /// kg per bead.
    pub mass: f32,
}

impl Default for Rope {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
            anchor: [0.0; 3],
            target_anchor: [0.0; 3],
            length: 2.0,
            segments: 8,
            radius: 0.05,
            mass: 0.1,
        }
    }
}

/// The beads a [`Rope`] spawned, from the target end to this body. Not
/// saved; read their transforms to draw the rope.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct RopeState {
    pub beads: Vec<Entity>,
}

/// Marks a bead with the rope that spawned it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RopeBead {
    pub rope: Entity,
}

fn world_point(world: &World, entity: Entity, local: [f32; 3]) -> Point3<f32> {
    match world.get::<Transform>(entity) {
        Some(transform) if entity != Entity::PLACEHOLDER => {
            let [roll, pitch, yaw] = transform.rotation;
            let turn = sim_math::rotation_from_euler(roll, pitch, yaw);
            Point3::from(transform.position) + turn * Vector3::from(local)
        }
        _ => local.into(),
    }
}

// ponytail: beads outlive a despawned rope or target; despawn RopeState's
// beads with it if games remove ropes during play.
pub(super) fn build_ropes(world: &mut World) {
    let mut ropes: Vec<_> = world
        .query::<(Entity, &Rope, Option<&SceneId>, Option<&RopeState>)>()
        .iter(world)
        .filter(|(.., state)| state.is_none())
        .map(|(entity, rope, id, _)| {
            (id.map(|id| id.0.as_u128()), entity, *rope)
        })
        .collect();
    ropes.sort_unstable_by_key(|(id, entity, _)| (*id, entity.to_bits()));
    for (_, owner, rope) in ropes {
        let count = rope.segments.max(1);
        let link = rope.length / (count + 1) as f32;
        let from = world_point(world, rope.target, rope.target_anchor);
        let to = world_point(world, owner, rope.anchor);
        let mut beads = Vec::with_capacity(count as usize);
        let mut previous = (rope.target, rope.target_anchor);
        for i in 1..=count {
            let at = from + (to - from) * (i as f32 / (count + 1) as f32);
            let order = next_spawn_order(world);
            let bead = world
                .spawn((
                    Transform::new(at.into()),
                    Collider {
                        shape: ColliderShape::Sphere {
                            radius: rope.radius,
                        },
                        ..Collider::default()
                    },
                    PhysicsBody::default(),
                    RigidBody {
                        kind: RigidBodyKind::Dynamic,
                        mass: rope.mass,
                        ..RigidBody::default()
                    },
                    rope_joint(previous, [0.0; 3], link),
                    RopeBead { rope: owner },
                    order,
                ))
                .id();
            beads.push(bead);
            previous = (bead, [0.0; 3]);
        }
        let mut entity = world.entity_mut(owner);
        if !entity.contains::<Joint>() {
            entity.insert(rope_joint(previous, rope.anchor, link));
        }
        entity.insert(RopeState { beads });
    }
}

fn rope_joint(
    (target, target_anchor): (Entity, [f32; 3]),
    anchor: [f32; 3],
    link: f32,
) -> Joint {
    Joint {
        target,
        kind: JointKind::Distance {
            min: 0.0,
            max: link,
        },
        anchor,
        target_anchor,
        collide_connected: false,
        ..Joint::default()
    }
}
