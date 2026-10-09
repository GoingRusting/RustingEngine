//! Turret, from `rusting recipe apply turret`.
//!
//! `Turret` shoots the nearest object in class `enemy` within `RANGE` that
//! it can see, once every `COOLDOWN_SECONDS`: the shot removes it and adds
//! one to the counter `turret_kills`. Anything in the way, the player
//! included, blocks the shot. Wire it up with `mod turret;` in
//! `src/main.rs` and `turret::turret(scene);` in `update`.

use rusting_engine::prelude::*;

/// How far the turret sees, in metres.
pub const RANGE: f32 = 12.0;
/// Time between shots.
pub const COOLDOWN_SECONDS: f32 = 0.5;

/// Runs the turret recipe once a frame.
pub fn turret(scene: &mut GameScene<'_>) {
    if !scene.cooldown_ready("turret_cooldown") {
        return;
    }
    let Some(at) = scene.try_object("Turret").map(|turret| turret.position())
    else {
        return;
    };
    // Nearest enemy in range; ties go to the first name, so runs repeat.
    let mut nearest: Option<(String, [f32; 3], f32)> = None;
    for name in scene.in_class("enemy") {
        let p = scene.object(&name).position();
        let to = [p[0] - at[0], p[1] - at[1], p[2] - at[2]];
        let distance = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        if distance > 0.0
            && distance <= RANGE
            && nearest.as_ref().is_none_or(|(_, _, d)| distance < *d)
        {
            nearest = Some((name, to, distance));
        }
    }
    let Some((name, to, distance)) = nearest else {
        return;
    };
    let direction = to.map(|axis| axis / distance);
    let seen = scene
        .raycast_skipping(at, direction, distance + 1.0, &["turret"])
        .is_some_and(|hit| hit.name == name);
    if !seen {
        return;
    }
    scene.despawn(&name);
    scene.add_to_counter("turret_kills", 1);
    scene.start_cooldown("turret_cooldown", COOLDOWN_SECONDS);
}
