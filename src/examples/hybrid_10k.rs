//! Native-runtime example with 10,000 visible GPU-owned cubes.

use rusting_engine::prelude::*;

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    scene.once("create_10k_cubes", |scene| {
        let cube = CubeSpawn::new().class("gravity").class("falling_cubes");

        for x in 0..20 {
            for y in 0..25 {
                for z in 0..20 {
                    let index = x * 500 + y * 20 + z;
                    scene.spawn_cube(
                        format!("Physics Cube {index}"),
                        Transform::new([
                            (x as f32 - 10.0) * 1.2,
                            y as f32 * 1.2 + 5.0,
                            (z as f32 - 10.0) * 1.2 - 15.0,
                        ]),
                        &cube,
                    );
                }
            }
        }

        let count = scene
            .apply_gpu_physics_to_class("gravity", &GpuBodySettings::default());
        println!("Created {count} GPU physics cubes");
    });

    scene.watch_gpu_class(
        "falling_cubes",
        GpuPhysicsRule::new(
            "body_fell",
            GpuCondition::position_y().less_than(-100.0),
        )
        .mode(GpuEventMode::OnEnter)
        .payload(GpuEventPayload::Position),
    );

    // Printing all 10,000 events separately would make terminal I/O the
    // benchmark bottleneck, so report one summary for this frame instead.
    let events = scene.gpu_events("body_fell");
    if let Some(first) = events.first() {
        println!(
            "{} cubes crossed Y=-100; first ID {:?}, position {:?}",
            events.len(),
            first.physics_id,
            first.payload,
        );
    }
}

rusting_game!("testGame/build/main.rscene.bin", update);
