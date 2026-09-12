//! CPU-side collision events for `SimulationClass::Gameplay` bodies.
//!
//! GPU-simulated bodies already report contacts through
//! `GpuCondition::colliding()` and the `GpuPhysicsEvent` pipeline. CPU
//! ("Gameplay") bodies have no physics solver driving them at all, so this
//! module only detects overlaps — it does not resolve them — using each
//! [`Collider`]'s bounding sphere as a simple first-pass broad/narrow phase.
//! `Collider::sensor` marks a pair as trigger-style (no expected physical
//! response) versus a solid contact, matching the GPU sensor convention.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::World;
use nalgebra::Vector3;

use crate::runtime::{
    Collider, ColliderShape, EventQueue, GlobalTransform, PhysicsBody,
    SimulationClass,
};

/// Fired once per overlapping pair of CPU ("Gameplay") colliders detected in
/// a `FixedUpdate` step. `sensor` is true when either collider is a sensor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionEvent {
    pub a: Entity,
    pub b: Entity,
    pub sensor: bool,
}

/// Detects overlaps between every pair of CPU-authoritative bodies.
///
/// Every collider shape is reduced to a conservative bounding-sphere radius,
/// which keeps this a simple O(n^2) broad phase rather than shape-specific
/// narrow-phase math; that is enough for "an event fired when two gameplay
/// objects touch" without requiring a full CPU physics solver.
pub(super) fn detect_collisions(world: &mut World) {
    let bodies = {
        let mut query =
            world
                .query::<(Entity, &GlobalTransform, &Collider, &PhysicsBody)>();
        query
            .iter(world)
            .filter(|(.., body)| body.simulation == SimulationClass::Gameplay)
            .map(|(entity, transform, collider, _)| {
                let matrix = transform.matrix;
                let position =
                    Vector3::new(matrix[3][0], matrix[3][1], matrix[3][2]);
                (
                    entity,
                    position,
                    bounding_radius(collider.shape),
                    collider.sensor,
                )
            })
            .collect::<Vec<_>>()
    };

    let mut overlaps = Vec::new();
    for (index, &(entity_a, position_a, radius_a, sensor_a)) in
        bodies.iter().enumerate()
    {
        for &(entity_b, position_b, radius_b, sensor_b) in &bodies[index + 1..]
        {
            let contact_radius = radius_a + radius_b;
            if (position_a - position_b).norm_squared()
                <= contact_radius * contact_radius
            {
                overlaps.push(CollisionEvent {
                    a: entity_a,
                    b: entity_b,
                    sensor: sensor_a || sensor_b,
                });
            }
        }
    }

    let mut events = world.resource_mut::<EventQueue<CollisionEvent>>();
    for overlap in overlaps {
        events.send(overlap);
    }
}

/// Returns the radius of the smallest sphere containing the collider shape.
fn bounding_radius(shape: ColliderShape) -> f32 {
    match shape {
        ColliderShape::Box { half_extents } => {
            Vector3::from(half_extents).norm()
        }
        ColliderShape::Sphere { radius } => radius,
        ColliderShape::Capsule {
            half_height,
            radius,
        } => half_height + radius,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_bounding_radius_is_the_half_extent_diagonal() {
        let radius = bounding_radius(ColliderShape::Box {
            half_extents: [1.0, 0.0, 0.0],
        });
        assert_eq!(radius, 1.0);
    }

    #[test]
    fn capsule_bounding_radius_covers_both_caps() {
        let radius = bounding_radius(ColliderShape::Capsule {
            half_height: 1.0,
            radius: 0.5,
        });
        assert_eq!(radius, 1.5);
    }
}
