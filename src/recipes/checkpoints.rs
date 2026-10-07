//! Checkpoints, from `rusting recipe apply checkpoints`.
//!
//! Touching an object in class `checkpoint` moves `Checkpoint Respawn` to
//! it. Falling below `FALL_Y` puts `Player` back at `Checkpoint Respawn`.
//! The respawn point lives in the scene, so snapshots and restarts keep it.
//! Wire it up with `mod checkpoints;` in `src/main.rs` and
//! `checkpoints::checkpoints(scene);` in `update`.

use rusting_engine::prelude::*;

/// Height below which the player respawns.
pub const FALL_Y: f32 = -10.0;

/// Runs the checkpoints recipe once a frame.
pub fn checkpoints(scene: &mut GameScene<'_>) {
    let checkpoints = scene.in_class("checkpoint");
    for name in scene.touching("Player") {
        if checkpoints.contains(&name) {
            let at = scene.object(&name).position();
            scene.object("Checkpoint Respawn").set_position(at);
        }
    }
    let Some(y) = scene
        .try_object("Player")
        .map(|player| player.position()[1])
    else {
        return;
    };
    if y < FALL_Y {
        let at = scene.object("Checkpoint Respawn").position();
        scene.object("Player").set_position(at);
    }
}
