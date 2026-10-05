use rusting_engine::prelude::*;

const START: [f32; 3] = [2.5, 0.51, 0.0];

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    let caught = scene.touching("Player").iter().any(|name| name == "Crawler");
    let fell = scene.object("Player").position()[1] < -6.0;
    if caught || fell {
        scene.object("Player").set_position(START);
        if let Some(mut falls) = scene.counter("falls") {
            falls.value += 1;
        }
    }
}

rusting_game!(update);
