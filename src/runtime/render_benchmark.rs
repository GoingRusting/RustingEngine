//! Fixed render benchmark: one scene and one camera path.
//!
//! The scene is a pure function of nothing: the same ECS entities, bit for
//! bit, on every machine. It exercises every stage the vertical slice uses:
//! a shadowed sun, point lights past the Eco budget, PBR materials, instanced
//! cubes and spheres, and GPU physics bodies falling onto a fixed ground. The
//! camera orbits the scene once in [`RENDER_BENCHMARK_FRAMES`] frames, moving
//! in and out so culling and shadow distance see changing work.

use std::f32::consts::TAU;

use bevy_ecs::prelude::{Entity, World};

use super::physics_benchmark::grid;
use super::{
    Camera, DirectionalLight, GpuCondition, GpuEventMode, GpuEventPayload,
    GpuPhysicsClassWatches, GpuPhysicsRule, MeshRenderer, Name, ObjectClasses,
    PhysicsBenchmark, PhysicsBody, PointLight, RigidBody, RigidBodyKind,
    SimulationClass, Visibility,
};
use crate::assets::{
    procedural_sphere_mesh, AssetServer, Handle, MaterialAsset, MeshAsset,
};
use crate::Transform;

/// Frames in one pass of the camera path: ten seconds at 60 FPS.
pub const RENDER_BENCHMARK_FRAMES: u32 = 600;
/// GPU physics bodies in the benchmark scene.
pub const RENDER_BENCHMARK_BODIES: usize = 1_000;
/// Point lights in the benchmark scene; more than Eco's light budget.
pub const RENDER_BENCHMARK_POINT_LIGHTS: usize = 24;
/// Pillars, each topped by a sphere.
pub const RENDER_BENCHMARK_PILLARS: usize = 40;
/// Class of the benchmark's dynamic bodies.
pub const RENDER_BENCHMARK_BODY_CLASS: &str = "benchmark_bodies";

/// Spawns the benchmark scene into `world` and returns its active camera.
/// Move the camera with [`render_benchmark_camera`] each frame. Add
/// `HybridPhysicsPlugin` first to also watch body landings.
pub fn spawn_render_benchmark(world: &mut World) -> Entity {
    let (cube, sphere, materials) = {
        let mut assets =
            world.get_resource_or_insert_with(AssetServer::default);
        let sphere = assets.meshes.insert(procedural_sphere_mesh(16));
        let materials: Vec<_> = (0..5)
            .map(|index| {
                let t = index as f32 / 4.0;
                assets.materials.insert(MaterialAsset {
                    base_color: [0.9 - 0.5 * t, 0.5, 0.2 + 0.6 * t, 1.0],
                    metallic: t,
                    roughness: 0.9 - 0.8 * t,
                    ..MaterialAsset::default()
                })
            })
            .collect();
        (assets.fallback_mesh, sphere, materials)
    };
    let mut spawn_mesh =
        |name: String, transform, mesh: Handle<MeshAsset>, material| {
            world
                .spawn((
                    Name(name),
                    transform,
                    MeshRenderer {
                        mesh,
                        material,
                        cast_shadows: true,
                        receive_shadows: true,
                    },
                    Visibility::default(),
                ))
                .id()
        };

    let ground = spawn_mesh(
        "Benchmark Ground".into(),
        Transform {
            position: [0.0, -0.5, 0.0],
            scale: [80.0, 1.0, 80.0],
            ..Transform::default()
        },
        cube,
        materials[0],
    );
    // A 7 x 7 grid, 8 m apart, without the centre 3 x 3 where bodies fall.
    let pillars = (0..49)
        .map(|cell| grid(cell, 7, 8.0))
        .filter(|[x, z]| x.abs() > 8.0 || z.abs() > 8.0);
    for (index, [x, z]) in pillars.enumerate() {
        let material = materials[index % materials.len()];
        spawn_mesh(
            format!("Benchmark Pillar {index}"),
            Transform {
                position: [x, 3.0, z],
                scale: [1.0, 6.0, 1.0],
                ..Transform::default()
            },
            cube,
            material,
        );
        spawn_mesh(
            format!("Benchmark Sphere {index}"),
            Transform {
                position: [x, 7.0, z],
                scale: [2.0; 3],
                ..Transform::default()
            },
            sphere,
            material,
        );
    }
    let bodies = PhysicsBenchmark::Mixed.bodies(RENDER_BENCHMARK_BODIES);
    let mut body_entities = Vec::with_capacity(bodies.len());
    for (index, body) in bodies.into_iter().enumerate() {
        let material = materials[index % materials.len()];
        let entity = spawn_mesh(
            format!("Benchmark Body {index}"),
            body.transform,
            cube,
            material,
        );
        body_entities.push((entity, body.solver));
    }

    let physics = |solver, kind| {
        (
            PhysicsBody {
                simulation: SimulationClass::Gpu,
                solver,
                custom_shader: None,
            },
            RigidBody {
                kind,
                ..RigidBody::default()
            },
            super::Collider::default(),
        )
    };
    world
        .entity_mut(ground)
        .insert(physics(super::PhysicsSolver::Full, RigidBodyKind::Fixed));
    for (entity, solver) in body_entities {
        world.entity_mut(entity).insert((
            physics(solver, RigidBodyKind::Dynamic),
            ObjectClasses::new([RENDER_BENCHMARK_BODY_CLASS]),
        ));
    }
    // Landing events give the benchmark event readback traffic.
    if let Some(mut watches) =
        world.get_resource_mut::<GpuPhysicsClassWatches>()
    {
        watches.add(
            RENDER_BENCHMARK_BODY_CLASS,
            GpuPhysicsRule::new(
                "benchmark_body_landed",
                GpuCondition::position_y().less_than(1.0),
            )
            .mode(GpuEventMode::OnEnter)
            .payload(GpuEventPayload::Position),
        );
    }

    world.spawn((
        Name("Benchmark Sun".into()),
        Transform {
            rotation: crate::engine::rotation_facing([-0.4, -1.0, -0.3]),
            ..Transform::default()
        },
        DirectionalLight {
            illuminance: 20_000.0,
            ..DirectionalLight::default()
        },
    ));
    for index in 0..RENDER_BENCHMARK_POINT_LIGHTS {
        let angle = TAU * index as f32 / RENDER_BENCHMARK_POINT_LIGHTS as f32;
        let mut color = [0.3; 3];
        color[index % 3] = 1.0;
        world.spawn((
            Name(format!("Benchmark Light {index}")),
            Transform::new([12.0 * angle.cos(), 3.0, 12.0 * angle.sin()]),
            PointLight {
                color,
                intensity: 400.0,
                range: 10.0,
            },
        ));
    }
    world
        .spawn((
            Name("Benchmark Camera".into()),
            render_benchmark_camera(0),
            Camera {
                active: true,
                ..Camera::default()
            },
        ))
        .id()
}

/// Camera transform at `frame` of the benchmark path. The path is closed:
/// frame [`RENDER_BENCHMARK_FRAMES`] matches frame 0.
#[must_use]
pub fn render_benchmark_camera(frame: u32) -> Transform {
    let turn = (frame % RENDER_BENCHMARK_FRAMES) as f32
        / RENDER_BENCHMARK_FRAMES as f32;
    let angle = TAU * turn;
    // Two dives per orbit: from 40 m out to 16 m, low over the pillars.
    let reach = 28.0 + 12.0 * (2.0 * angle).cos();
    let height = 10.0 + 6.0 * (2.0 * angle).cos();
    let position = [reach * angle.sin(), height, reach * angle.cos()];
    let target = [0.0, 2.0, 0.0];
    Transform {
        position,
        rotation: crate::engine::rotation_facing(
            [0, 1, 2].map(|axis| target[axis] - position[axis]),
        ),
        ..Transform::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_path_is_closed_repeatable_and_faces_the_scene() {
        assert_eq!(
            render_benchmark_camera(0),
            render_benchmark_camera(RENDER_BENCHMARK_FRAMES)
        );
        for frame in 0..RENDER_BENCHMARK_FRAMES {
            let camera = render_benchmark_camera(frame);
            assert_eq!(camera, render_benchmark_camera(frame));
            let [x, y, z] = camera.position;
            let reach = (x * x + z * z).sqrt();
            assert!((15.9..=40.1).contains(&reach), "{frame}: {reach}");
            assert!(y > 3.9, "{frame}: {y}");
            // Pitch looks down toward the target below the camera.
            assert!(camera.rotation[0] < 0.0, "{frame}");
        }
        let quarter = render_benchmark_camera(RENDER_BENCHMARK_FRAMES / 4);
        assert!(quarter.position[0] > 15.0, "{quarter:?}");
    }

    #[test]
    fn scene_spawns_the_same_fixed_content_every_time() {
        let counts = || {
            let mut world = World::new();
            let camera = spawn_render_benchmark(&mut world);
            assert!(world.get::<Camera>(camera).unwrap().active);
            let mut meshes =
                world.query::<(&Name, &Transform, &MeshRenderer)>();
            let layout: Vec<_> = meshes
                .iter(&world)
                .map(|(name, transform, _)| (name.0.clone(), *transform))
                .collect();
            let bodies = world.query::<&PhysicsBody>().iter(&world).count();
            let points = world.query::<&PointLight>().iter(&world).count();
            let suns = world.query::<&DirectionalLight>().iter(&world).count();
            (layout, bodies, points, suns)
        };
        let (layout, bodies, points, suns) = counts();
        assert_eq!(
            layout.len(),
            1 + 2 * RENDER_BENCHMARK_PILLARS + RENDER_BENCHMARK_BODIES
        );
        assert_eq!(bodies, 1 + RENDER_BENCHMARK_BODIES);
        assert_eq!((points, suns), (RENDER_BENCHMARK_POINT_LIGHTS, 1));
        assert_eq!(counts().0, layout);
    }
}
