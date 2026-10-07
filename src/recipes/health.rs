//! Health and damage, from `rusting recipe apply health`.
//!
//! The counter `health` starts at `MAX_HEALTH` each round. Touching an
//! object in class `hazard` costs one point, flashes `Player` and makes it
//! safe for `SAFE_SECONDS`. At 0 the round restarts, which refills health.
//! Wire it up with `mod health;` in `src/main.rs` and
//! `health::health(scene, time);` in `update`.

use rusting_engine::prelude::*;

/// Health at the start of a round.
pub const MAX_HEALTH: i32 = 3;
/// Time after a hit during which hazards do no damage.
pub const SAFE_SECONDS: f64 = 1.0;

/// Runs the health recipe once a frame.
pub fn health(scene: &mut GameScene<'_>, time: &FrameTime) {
    scene.once("health", |scene| {
        scene.set_counter("health", MAX_HEALTH);
        scene.set_counter("health_safe_until_ms", 0);
    });
    let now_ms = i32::try_from(time.elapsed.as_millis()).unwrap_or(i32::MAX);
    if now_ms < scene.counter_value("health_safe_until_ms") {
        return;
    }
    let hazards = scene.in_class("hazard");
    if !scene
        .touching("Player")
        .iter()
        .any(|name| hazards.contains(name))
    {
        return;
    }
    let safe_ms = (SAFE_SECONDS * 1000.0) as i32;
    scene.set_counter("health_safe_until_ms", now_ms.saturating_add(safe_ms));
    scene.flash("Player");
    if scene.add_to_counter("health", -1) <= 0 {
        scene.restart();
    }
}
