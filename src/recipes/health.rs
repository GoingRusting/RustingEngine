//! Health and damage, from `rusting recipe apply health`.
//!
//! The counter `health` starts at `MAX_HEALTH` each round. Touching an
//! object in class `hazard` costs one point, flashes `Player` and makes it
//! safe for `SAFE_SECONDS`. At 0 the round restarts, which refills health.
//! Wire it up with `mod health;` in `src/main.rs` and
//! `health::health(scene);` in `update`.

use rusting_engine::prelude::*;

/// Health at the start of a round.
pub const MAX_HEALTH: i32 = 3;
/// Time after a hit during which hazards do no damage.
pub const SAFE_SECONDS: f32 = 1.0;

/// Runs the health recipe once a frame.
pub fn health(scene: &mut GameScene<'_>) {
    scene.once("health", |scene| {
        scene.set_counter("health", MAX_HEALTH);
        scene.set_counter("health_safe", 0);
    });
    if !scene.cooldown_ready("health_safe") {
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
    scene.start_cooldown("health_safe", SAFE_SECONDS);
    scene.flash("Player");
    if scene.add_to_counter("health", -1) <= 0 {
        scene.restart();
    }
}
