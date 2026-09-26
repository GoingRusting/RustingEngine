//! Repeatable GPU physics benchmark.
//!
//! ```sh
//! cargo run --release --example physics_bench -- stacking 10000
//! ```
//!
//! Scenes: `falling`, `stacking`, `debris`, `mixed` (default); body count
//! defaults to 10000. The layout comes from
//! [`PhysicsBenchmark::bodies`], so every run starts from the same state.
//! Prints the average frame time once per second; the editor profiler shows
//! the per-pass GPU timings and physics traffic.

use rusting_engine::prelude::*;
use rusting_engine::runtime::{
    PhysicsBenchmark, PhysicsSolver, RigidBody, RigidBodyKind,
};

fn arguments() -> (PhysicsBenchmark, usize) {
    let mut arguments = std::env::args().skip(1);
    let scene = arguments
        .next()
        .map(|name| {
            PhysicsBenchmark::from_name(&name).unwrap_or_else(|| {
                panic!("unknown scene `{name}`: falling, stacking, debris or mixed")
            })
        })
        .unwrap_or(PhysicsBenchmark::Mixed);
    let count = arguments
        .next()
        .map_or(10_000, |count| count.parse().expect("body count"));
    (scene, count)
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    scene.once("create_benchmark", |scene| {
        let (benchmark, count) = arguments();
        let bodies = benchmark.bodies(count);
        let reach = bodies
            .iter()
            .flat_map(|body| {
                [body.transform.position[0], body.transform.position[2]]
            })
            .fold(0.0_f32, |reach, value| reach.max(value.abs()))
            + 20.0;
        let mut ground = Transform::new([0.0, -0.5, 0.0]);
        ground.scale = [2.0 * reach, 1.0, 2.0 * reach];
        scene.spawn_cube(
            "Benchmark Ground",
            ground,
            &CubeSpawn::new().class("benchmark_ground"),
        );
        scene.apply_gpu_physics_to_class(
            "benchmark_ground",
            &GpuBodySettings {
                rigid_body: RigidBody {
                    kind: RigidBodyKind::Fixed,
                    ..RigidBody::default()
                },
                // The unit-cube collider scales with the transform.
                ..GpuBodySettings::default()
            },
        );
        let solvers = [
            PhysicsSolver::Full,
            PhysicsSolver::Simplified,
            PhysicsSolver::Space,
            PhysicsSolver::NoCollision,
        ];
        for (index, body) in bodies.iter().enumerate() {
            let class = format!("bench_{:?}", body.solver);
            scene.spawn_cube(
                format!("Bench {index}"),
                body.transform,
                &CubeSpawn::new().class(class),
            );
        }
        for solver in solvers {
            scene.apply_gpu_physics_to_class(
                &format!("bench_{solver:?}"),
                &GpuBodySettings {
                    solver,
                    ..GpuBodySettings::default()
                },
            );
        }
        for (index, body) in bodies.iter().enumerate() {
            if body.linear_velocity != [0.0; 3] {
                scene.set_linear_velocity(
                    &format!("Bench {index}"),
                    body.linear_velocity,
                );
            }
        }
        println!("{} with {count} bodies", benchmark.name());
    });

    // Frame-time summary once per second of real time.
    FRAMES.with(|frames| {
        let (count, seconds) = frames.get();
        let (count, seconds) = (count + 1, seconds + time.delta_seconds());
        if seconds >= 1.0 {
            println!(
                "{:.2} ms/frame over {count} frames",
                1000.0 * seconds / count as f32
            );
            frames.set((0, 0.0));
        } else {
            frames.set((count, seconds));
        }
    });
}

thread_local! {
    static FRAMES: std::cell::Cell<(u32, f32)> = const { std::cell::Cell::new((0, 0.0)) };
}

rusting_game!("testGame/build/main.rscene.bin", update);
