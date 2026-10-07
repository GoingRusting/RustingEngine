//! Make an enemy follow the player.
//!
//! Every object in class `chaser` turns to face `Player` and walks toward
//! it at `SPEED` on the ground plane, stopping `STOP_DISTANCE` away. Give a
//! chaser a `Kinematic` body so physics does not push it back, and call
//! `follow_player::follow_player(scene, time);` from `update`.

use rusting_engine::prelude::*;

/// Walking speed, in metres per second.
pub const SPEED: f32 = 3.0;
/// How close a chaser gets before it stops.
pub const STOP_DISTANCE: f32 = 1.5;

/// Moves every chaser one frame toward the player.
pub fn follow_player(scene: &mut GameScene<'_>, time: &FrameTime) {
    let Some(target) = scene.try_object("Player").map(|p| p.position()) else {
        return;
    };
    let step = SPEED * time.delta.as_secs_f32();
    for name in scene.in_class("chaser") {
        let at = scene.object(&name).position();
        let (dx, dz) = (target[0] - at[0], target[2] - at[2]);
        let distance = (dx * dx + dz * dz).sqrt();
        if distance <= STOP_DISTANCE {
            continue;
        }
        let step = step.min(distance - STOP_DISTANCE);
        let to = [
            at[0] + dx / distance * step,
            at[1],
            at[2] + dz / distance * step,
        ];
        // Objects face -Z at yaw 0, like the player controllers.
        let yaw = (-dx).atan2(-dz);
        let mut chaser = scene.object(&name);
        chaser.set_position(to);
        chaser.set_rotation([0.0, yaw, 0.0]);
    }
}
