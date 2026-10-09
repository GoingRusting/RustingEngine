//! Outpost, from `rusting new --template tower-defense`: hold the base.
//!
//! Enemies, copied from the hidden `Enemy Template`, walk the path
//! through `Waypoint 1`, `Waypoint 2` and so on to the base at the last
//! one. Keys 1 to 4 build a tower (a copy of `Tower Template`) on `Pad 1`
//! to `Pad 4` for 50 `gold`. Every 1.5 seconds each tower defeats the enemy
//! furthest along the path within its reach, for 10 gold. An enemy that
//! reaches the base adds to `leaks`. `cleared` reaching its target, every
//! enemy defeated or leaked, shows `You win!`; `leaks` reaching its target
//! shows `Game over`; `restart` (R) starts over. Move waypoints and pads
//! in the editor to change the map. `rusting test tests/defend.json`
//! plays the wave.

use rusting_engine::prelude::*;

const ENEMY_SPEED: f32 = 2.5;
/// Milliseconds between enemies.
const SPAWN_MS: i32 = 500;
/// How many enemies come; keep the `cleared` target equal to it.
const WAVE: i32 = 10;
const TOWER_COST: i32 = 50;
const TOWER_REACH: f32 = 3.5;
/// Milliseconds between tower volleys.
const VOLLEY_MS: i32 = 1500;
const BOUNTY: i32 = 10;

/// The path's points, from `Waypoint 1` until a number is missing.
fn path(scene: &mut GameScene<'_>) -> Vec<[f32; 3]> {
    (1..)
        .map_while(|n| {
            scene
                .try_object(&format!("Waypoint {n}"))
                .map(|w| w.position())
        })
        .collect()
}

/// The point `distance` meters along `path`, or `None` past its end.
fn along(path: &[[f32; 3]], mut distance: f32) -> Option<[f32; 3]> {
    for leg in path.windows(2) {
        let (from, to) = (leg[0], leg[1]);
        let length = (to[0] - from[0]).hypot(to[2] - from[2]);
        if distance <= length {
            let t = distance / length;
            return Some([
                from[0] + (to[0] - from[0]) * t,
                0.4,
                from[2] + (to[2] - from[2]) * t,
            ]);
        }
        distance -= length;
    }
    None
}

/// Runs once a frame.
pub fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("cleared") || scene.counter_complete("leaks") {
        return;
    }
    // The round's clock; enemy `Enemy n` set off at n * SPAWN_MS, so its
    // place on the path follows from the clock alone.
    let ms = (time.delta.as_secs_f32() * 1000.0).round() as i32;
    let clock = scene.add_to_counter("clock_ms", ms);
    let path = path(scene);

    for n in 1..=4 {
        let (tower, pad) = (format!("Tower {n}"), format!("Pad {n}"));
        if scene.pressed(&format!("build_{n}"))
            && scene.counter_value("gold") >= TOWER_COST
            && scene.try_object(&tower).is_none()
        {
            let mut at = scene.object(&pad).position();
            at[1] = 0.6;
            scene.spawn_copy("Tower Template", &tower, at);
            scene.set_visible(&tower, true);
            scene.add_class(&tower, "tower");
            scene.add_to_counter("gold", -TOWER_COST);
        }
    }

    let spawned = scene.counter_or("spawned", 0);
    if spawned < WAVE && clock >= (spawned + 1) * SPAWN_MS {
        let name = format!("Enemy {}", spawned + 1);
        scene.spawn_copy("Enemy Template", &name, path[0]);
        scene.set_visible(&name, true);
        scene.add_class(&name, "enemy");
        scene.set_counter("spawned", spawned + 1);
    }

    // Each enemy's distance along the path, furthest first.
    let mut enemies: Vec<(f32, String)> = scene
        .in_class("enemy")
        .into_iter()
        .map(|name| {
            let n: i32 = name["Enemy ".len()..].parse().unwrap_or(0);
            ((clock - n * SPAWN_MS) as f32 / 1000.0 * ENEMY_SPEED, name)
        })
        .collect();
    enemies.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut alive = Vec::new();
    for (distance, enemy) in enemies {
        match along(&path, distance) {
            Some(at) => {
                scene.object(&enemy).set_position(at);
                alive.push((at, enemy));
            }
            None => {
                scene.despawn(&enemy);
                scene.add_to_counter("leaks", 1);
                scene.add_to_counter("cleared", 1);
                scene.add_trauma("Game Camera", 0.5);
            }
        }
    }

    // Towers fire each time the clock passes a whole volley.
    if clock / VOLLEY_MS == (clock - ms) / VOLLEY_MS {
        return;
    }
    for tower in scene.in_class("tower") {
        let from = scene.object(&tower).position();
        let Some(index) = alive.iter().position(|(at, _)| {
            (at[0] - from[0]).hypot(at[2] - from[2]) <= TOWER_REACH
        }) else {
            continue;
        };
        let (_, enemy) = alive.remove(index);
        scene.despawn(&enemy);
        scene.flash(&tower);
        scene.add_to_counter("gold", BOUNTY);
        scene.add_to_counter("cleared", 1);
    }
}

rusting_game!(update);
