use rusting_engine::prelude::*;

const START: [f32; 3] = [0.0, 1.0, 3.0];

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    let hit = scene
        .touching("Player")
        .iter()
        .any(|name| name.starts_with("Sweeper"));
    let mut player = scene.object("Player");
    if hit || player.position()[1] < -5.0 {
        player.set_position(START);
    }
}

rusting_game!(update);
