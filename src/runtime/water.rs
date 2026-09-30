//! Water you can build seas, lakes and rivers with: a flat rectangle of
//! animated waves that dynamic bodies float on and a current can carry.
//!
//! Waves are a sum of three sine waves, a function of position and the fixed
//! tick, so the surface is deterministic and needs no particle solver.
//! Use [`super::FluidBlock`] for small splashing volumes instead.

// The per-axis loops index several small arrays at once; iterators read worse.
#![allow(clippy::needless_range_loop)]

use super::{ColliderShape, FrameTime, RigidBodyKind};
use crate::assets::{
    AlphaMode, AssetServer, Handle, MaterialAsset, MeshAsset, MeshVertex,
};
use bevy_ecs::prelude::{
    Commands, Component, Entity, Query, Res, ResMut, Without,
};
use nalgebra::Vector3;
use std::f32::consts::TAU;

/// A rectangle of water centered on the entity. It is axis aligned: the
/// entity's rotation and scale are ignored, only its position counts.
/// `flow_direction` turns both the waves and the current, so a long thin
/// water with a `flow_speed` makes a river.
#[derive(
    Component,
    Clone,
    Copy,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(default)]
pub struct WaterBody {
    /// Size along x and z in meters.
    pub size: [f32; 2],
    /// Grid cells along the longer side; more cells make finer waves.
    pub resolution: u32,
    /// Crest height above the rest level in meters.
    pub wave_height: f32,
    /// Distance between crests in meters.
    pub wave_length: f32,
    /// How fast crests travel, in meters per second.
    pub wave_speed: f32,
    /// Direction waves and current travel, in degrees around +Y (0 is +X).
    pub flow_direction: f32,
    /// Speed of the current that carries floating bodies, in meters per
    /// second. 0 is still water.
    pub flow_speed: f32,
    /// Linear RGBA of the surface.
    pub color: [f32; 4],
}

impl Default for WaterBody {
    fn default() -> Self {
        Self {
            size: [20.0, 20.0],
            resolution: 64,
            wave_height: 0.25,
            wave_length: 4.0,
            wave_speed: 1.0,
            flow_direction: 0.0,
            flow_speed: 0.0,
            color: [0.1, 0.4, 0.7, 0.7],
        }
    }
}

/// `(amplitude share, length share, speed share, direction offset)`.
const WAVES: [[f32; 4]; 3] = [
    [0.57, 1.0, 1.0, 0.0],
    [0.29, 0.63, 0.8, 0.6],
    [0.14, 0.41, 0.6, -0.9],
];

impl WaterBody {
    /// Height above the rest level and the surface slope `[dh/dx, dh/dz]` at
    /// a world position `time` seconds into the simulation.
    pub fn wave(&self, x: f32, z: f32, time: f32) -> (f32, [f32; 2]) {
        let base = self.flow_direction.to_radians();
        let mut height = 0.0;
        let mut slope = [0.0; 2];
        for [amplitude, length, speed, turn] in WAVES {
            let angle = base + turn;
            let dir = [angle.cos(), angle.sin()];
            let k = TAU / (self.wave_length.max(0.05) * length);
            let phase =
                k * (dir[0] * x + dir[1] * z - self.wave_speed * speed * time);
            let a = amplitude * self.wave_height;
            height += a * phase.sin();
            let d = a * k * phase.cos();
            slope[0] += d * dir[0];
            slope[1] += d * dir[1];
        }
        (height, slope)
    }

    fn contains(&self, center: [f32; 3], x: f32, z: f32) -> bool {
        (x - center[0]).abs() <= self.size[0] / 2.0
            && (z - center[2]).abs() <= self.size[1] / 2.0
    }
}

/// The surface entity and mesh a [`WaterBody`] owns, made by [`sync_water`].
#[derive(Component, Clone, Copy, Debug)]
pub struct WaterMesh {
    mesh: Handle<MeshAsset>,
    material: Handle<MaterialAsset>,
    entity: Entity,
}

impl WaterMesh {
    pub(super) fn entity(&self) -> Entity {
        self.entity
    }
}

fn water_material(
    assets: &mut AssetServer,
    color: [f32; 4],
) -> Handle<MaterialAsset> {
    assets.materials.insert(MaterialAsset {
        name: "River Water".into(),
        alpha_mode: AlphaMode::Blend,
        base_color: color,
        roughness: 0.05,
        ..MaterialAsset::default()
    })
}

fn build_mesh(water: &WaterBody, center: [f32; 3], time: f32) -> MeshAsset {
    let longer = water.size[0].max(water.size[1]).max(0.01);
    let cells = water.resolution.clamp(2, 256) as f32;
    let nx = ((water.size[0] / longer * cells).ceil() as usize).max(1);
    let nz = ((water.size[1] / longer * cells).ceil() as usize).max(1);
    let mut vertices = Vec::with_capacity((nx + 1) * (nz + 1));
    for j in 0..=nz {
        for i in 0..=nx {
            let x = center[0] + (i as f32 / nx as f32 - 0.5) * water.size[0];
            let z = center[2] + (j as f32 / nz as f32 - 0.5) * water.size[1];
            let (height, slope) = water.wave(x, z, time);
            let n = Vector3::new(-slope[0], 1.0, -slope[1]).normalize();
            vertices.push(MeshVertex {
                position: [x, center[1] + height, z],
                normal: [n.x, n.y, n.z],
                uv: [i as f32 / nx as f32, j as f32 / nz as f32],
                tangent: [1.0, 0.0, 0.0, 1.0],
            });
        }
    }
    let mut indices = Vec::with_capacity(nx * nz * 6);
    let at = |i: usize, j: usize| (j * (nx + 1) + i) as u32;
    for j in 0..nz {
        for i in 0..nx {
            // Counter-clockwise seen from above (+Y).
            let (a, b, c, d) =
                (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            indices.extend([a, d, c, a, c, b]);
        }
    }
    MeshAsset { vertices, indices }
}

/// Per fixed step: gives each [`WaterBody`] its surface entity and rewrites
/// the surface mesh for the current tick. The surface entity carries a
/// `FluidParticle` marker, so [`super::fluid::sync_fluid_visuals`] despawns
/// it when the body goes.
/// ponytail: the grid uploads again each tick; move the waves to the vertex
/// shader if large seas show in profiles.
#[allow(clippy::type_complexity)]
pub(super) fn sync_water(
    mut commands: Commands,
    time: Res<FrameTime>,
    assets: Option<ResMut<AssetServer>>,
    mut waters: Query<(
        Entity,
        &WaterBody,
        &crate::Transform,
        Option<&super::Parent>,
        Option<&super::GlobalTransform>,
        Option<&mut WaterMesh>,
    )>,
    markers: Query<&super::FluidParticle>,
) {
    let seconds =
        (time.fixed_tick as f64 * time.fixed_delta.as_secs_f64()) as f32;
    let Some(mut assets) = assets else {
        return;
    };
    for (owner, water, transform, parent, global, mesh) in &mut waters {
        let position =
            super::cpu_physics::world_position(transform, parent, global);
        let built = build_mesh(water, position, seconds);
        let existing = mesh.as_deref().filter(|mesh| {
            markers
                .get(mesh.entity)
                .is_ok_and(|marker| marker.0 == owner)
        });
        match existing {
            Some(mesh) => {
                if let Some(slot) = assets.meshes.get_mut(mesh.mesh) {
                    *slot = built;
                }
                if assets
                    .materials
                    .get(mesh.material)
                    .is_some_and(|material| material.base_color != water.color)
                {
                    if let Some(material) =
                        assets.materials.get_mut(mesh.material)
                    {
                        material.base_color = water.color;
                    }
                }
            }
            None => {
                let handle = assets.meshes.insert(built);
                let material = water_material(&mut assets, water.color);
                let entity = commands
                    .spawn((
                        crate::Transform::default(),
                        super::MeshRenderer {
                            mesh: handle,
                            material,
                            cast_shadows: false,
                            receive_shadows: true,
                        },
                        super::FluidParticle(owner),
                        super::fluid::OwnedSurface {
                            mesh: handle,
                            material: Some(material),
                        },
                    ))
                    .id();
                commands.entity(owner).insert(WaterMesh {
                    mesh: handle,
                    material,
                    entity,
                });
            }
        }
    }
}

/// Per fixed step: floats dynamic bodies with sphere, box or capsule
/// colliders. Lift grows with the share of the body under the surface, drag
/// slows it, and the current pulls it along. Each body reads only its own
/// state and the waters in `SpawnOrder`, so the result does not depend on
/// body order.
/// ponytail: the body is treated as a column of its vertical extent with
/// its volume spread evenly over it; no torque, so boxes do not tilt with
/// the waves.
#[allow(clippy::type_complexity)]
pub(super) fn float_in_water(
    time: Res<FrameTime>,
    physics: Res<super::PhysicsSettings>,
    waters: Query<(
        Entity,
        Option<&super::SpawnOrder>,
        &WaterBody,
        &crate::Transform,
        Option<&super::Parent>,
        Option<&super::GlobalTransform>,
    )>,
    // A child body is placed by its parent, so buoyancy skips it.
    mut bodies: Query<
        (
            &crate::Transform,
            &mut super::RigidBody,
            &super::Collider,
            Option<&super::PhysicsBody>,
        ),
        Without<super::Parent>,
    >,
) {
    if waters.is_empty() {
        return;
    }
    let dt = time.fixed_delta.as_secs_f32();
    let seconds =
        (time.fixed_tick as f64 * time.fixed_delta.as_secs_f64()) as f32;
    let gravity = physics.gravity;
    let gravity_size = Vector3::from(gravity).norm();
    let mut waters: Vec<_> = waters.iter().collect();
    waters.sort_by_key(|(entity, order, ..)| {
        (order.is_none(), order.map_or(0, |order| order.0), *entity)
    });
    for (transform, mut body, collider, physics) in &mut bodies {
        // The GPU solver never reads back CPU velocities.
        if body.kind != RigidBodyKind::Dynamic
            || body.mass <= 0.0
            || !physics
                .is_some_and(|p| p.simulation == super::SimulationClass::Cpu)
        {
            continue;
        }
        let rotation = super::sim_math::rotation_from_euler(
            transform.rotation[0],
            transform.rotation[1],
            transform.rotation[2],
        );
        // Vertical half extent and volume.
        let (reach, volume) = match collider.shape {
            ColliderShape::Sphere { radius } => {
                let radius =
                    radius * transform.scale.into_iter().fold(0.0, f32::max);
                (radius, 4.0 / 3.0 * std::f32::consts::PI * radius.powi(3))
            }
            ColliderShape::Box { half_extents } => {
                let half: [f32; 3] = std::array::from_fn(|axis| {
                    half_extents[axis] * transform.scale[axis]
                });
                let up = rotation.inverse() * Vector3::y();
                (
                    (0..3).map(|axis| up[axis].abs() * half[axis]).sum(),
                    8.0 * half[0] * half[1] * half[2],
                )
            }
            ColliderShape::Capsule {
                half_height,
                radius,
            } => {
                let radius =
                    radius * transform.scale[0].max(transform.scale[2]);
                let half = half_height * transform.scale[1];
                let axis = rotation * Vector3::y();
                (
                    half * axis.y.abs() + radius,
                    std::f32::consts::PI * radius * radius * 2.0 * half
                        + 4.0 / 3.0 * std::f32::consts::PI * radius.powi(3),
                )
            }
            _ => continue,
        };
        let center = transform.position;
        for (_, _, water, water_transform, parent, global) in &waters {
            let origin = super::cpu_physics::world_position(
                water_transform,
                *parent,
                *global,
            );
            if !water.contains(origin, center[0], center[2]) {
                continue;
            }
            let (height, _) = water.wave(center[0], center[2], seconds);
            let surface = origin[1] + height;
            let submerged = ((surface - (center[1] - reach))
                / (2.0 * reach).max(1e-4))
            .clamp(0.0, 1.0);
            if submerged <= 0.0 {
                continue;
            }
            if gravity_size > 0.0 {
                let lift =
                    1000.0 * submerged * volume * gravity_size / body.mass;
                for axis in 0..3 {
                    body.linear_velocity[axis] -=
                        gravity[axis] / gravity_size * lift * dt;
                }
            }
            let angle = water.flow_direction.to_radians();
            let current = [
                angle.cos() * water.flow_speed,
                0.0,
                angle.sin() * water.flow_speed,
            ];
            for axis in 0..3 {
                // Drag pulls the body toward the current, which is still
                // water (zero) for a lake.
                body.linear_velocity[axis] += (current[axis]
                    - body.linear_velocity[axis])
                    * (2.0 * submerged * dt).min(1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_are_reproducible_and_bounded() {
        let water = WaterBody::default();
        let a = water.wave(1.3, -2.1, 4.0);
        assert_eq!(a, water.wave(1.3, -2.1, 4.0));
        assert!(a.0.abs() <= water.wave_height);
    }

    #[test]
    fn the_mesh_grid_matches_the_rectangle() {
        let water = WaterBody {
            size: [10.0, 5.0],
            resolution: 10,
            ..WaterBody::default()
        };
        let mesh = build_mesh(&water, [0.0; 3], 0.0);
        assert_eq!(mesh.vertices.len(), 11 * 6);
        assert_eq!(mesh.indices.len(), 10 * 5 * 6);
    }
}
