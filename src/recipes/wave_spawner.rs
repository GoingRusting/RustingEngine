//! Wave spawner, from `rusting recipe apply wave_spawner`.
//!
//! When no enemies are left, the counter `wave` goes up and `WAVE_SIZE`
//! times that many copies of the hidden `Wave Enemy` appear in a row from
//! `Wave Spawn`, named `Wave Enemy <wave>-<n>`. Copies are in class
//! `enemy`; game code defeats one by despawning it, and one that falls
//! below `FALL_Y` is removed. `wave_enemies_left` counts the rest. Wire it
//! up with `mod wave_spawner;` in `src/main.rs` and
//! `wave_spawner::wave_spawner(scene);` in `update`.

use rusting_engine::prelude::*;

/// Enemies in the first wave; wave `n` has `n` times as many.
pub const WAVE_SIZE: i32 = 2;
/// Height below which an enemy is removed.
pub const FALL_Y: f32 = -10.0;
/// Space between enemies of one wave, along +X.
pub const SPACING: f32 = 1.5;

/// Runs the wave spawner recipe once a frame.
pub fn wave_spawner(scene: &mut GameScene<'_>) {
    let mut left = 0;
    for name in scene.in_class("enemy") {
        if name == "Wave Enemy" {
            continue;
        }
        if scene.object(&name).position()[1] < FALL_Y {
            scene.despawn(&name);
        } else {
            left += 1;
        }
    }
    if left == 0 {
        let wave = scene.add_to_counter("wave", 1);
        let spawn = scene.object("Wave Spawn").position();
        for n in 0..wave * WAVE_SIZE {
            let name = format!("Wave Enemy {wave}-{}", n + 1);
            let at = [spawn[0] + SPACING * n as f32, spawn[1], spawn[2]];
            scene.spawn_copy("Wave Enemy", name.as_str(), at);
            scene.set_visible(&name, true);
            left += 1;
        }
    }
    scene.set_counter("wave_enemies_left", left);
}
