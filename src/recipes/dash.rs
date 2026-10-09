//! Dash, from `rusting recipe apply dash`.
//!
//! The action `dash` (Q, gamepad West) sends `Player` `DASH_SPEED` metres
//! per second for `DASH_SECONDS`, in place of walking, jumping and gravity;
//! walls still stop it. A 3D player dashes the way the movement keys point,
//! or forward when none is held; a platformer dashes the way it last ran.
//! A new dash waits `COOLDOWN_SECONDS`. Wire it up with `mod dash;` in
//! `src/main.rs` and `dash::dash(scene);` in `update`.

use rusting_engine::prelude::*;

/// Dash speed in metres per second.
pub const DASH_SPEED: f32 = 15.0;
/// How long one dash lasts.
pub const DASH_SECONDS: f32 = 0.2;
/// Time from one dash to the next.
pub const COOLDOWN_SECONDS: f32 = 0.6;

/// Runs the dash recipe once a frame.
pub fn dash(scene: &mut GameScene<'_>) {
    let axis = |scene: &GameScene<'_>, plus: &str, minus: &str| {
        f32::from(u8::from(scene.held(plus)))
            - f32::from(u8::from(scene.held(minus)))
    };
    let run = axis(scene, "player.right", "player.left");
    if run != 0.0 {
        scene.set_counter("dash_facing", run as i32);
    }
    if !scene.pressed("dash") || !scene.cooldown_ready("dash_cooldown") {
        return;
    }
    let direction = if let Some(player) = scene.player("Player") {
        // Forward is -Z turned by yaw, as the controller walks.
        let mut forward = axis(scene, "player.forward", "player.back");
        if forward == 0.0 && run == 0.0 {
            forward = 1.0;
        }
        let (sin, cos) = player.yaw.sin_cos();
        let way = [-sin * forward + cos * run, 0.0, -cos * forward - sin * run];
        let length = way[0].hypot(way[2]);
        way.map(|value| value / length)
    } else {
        [scene.counter_or("dash_facing", 1) as f32, 0.0, 0.0]
    };
    if scene.dash(
        "Player",
        direction.map(|value| value * DASH_SPEED),
        DASH_SECONDS,
    ) {
        scene.start_cooldown("dash_cooldown", COOLDOWN_SECONDS);
    }
}
