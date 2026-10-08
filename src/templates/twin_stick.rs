//! Swarm, from `rusting new --template twin-stick`: a twin-stick shooter.
//!
//! WASD or the left stick move `Player`; the arrow keys or the right
//! stick aim and fire. Enemies, copied from the hidden `Enemy Template`,
//! come in from the four sides one after another and walk toward the
//! player. A shot (a copy of `Bullet Template`) defeats the enemy it
//! touches; an enemy that touches the player costs a hit and is knocked
//! back. The counter `kills` reaching its target shows `You win!`, `hits`
//! reaching its target shows `Game over`, and `restart` (R) starts over.
//! Game state lives in counters made on first write (`reload`,
//! `spawned`, `spawn_timer`, `shots`). `rusting test tests/wave.json`
//! plays the first wave.

use rusting_engine::prelude::*;

const PLAYER_SPEED: f32 = 6.0;
const SHOT_SPEED: f32 = 18.0;
const ENEMY_SPEED: f32 = 2.0;
/// Milliseconds between shots while aiming.
const RELOAD_MS: i32 = 150;
/// Milliseconds between enemies.
const SPAWN_MS: i32 = 1000;
/// How many enemies come; keep the `kills` target equal to it.
const WAVE: i32 = 12;
/// Where enemies come in, in turn.
const SPAWNS: [[f32; 2]; 4] =
    [[7.0, 0.0], [-7.0, 0.0], [0.0, -7.0], [0.0, 7.0]];
/// A shot this close to an enemy defeats it.
const SHOT_REACH: f32 = 0.6;
/// An enemy this close to the player hits it.
const HIT_REACH: f32 = 0.8;
/// How far from the player a hit knocks the enemy back.
const KNOCKBACK: f32 = 3.0;
/// Half the arena's width; the player stays inside it and shots end
/// past its walls.
const ARENA: f32 = 7.0;

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// -1, 0 or 1 from two held actions.
fn axis(scene: &GameScene<'_>, minus: &str, plus: &str) -> f32 {
    f32::from(u8::from(scene.held(plus)))
        - f32::from(u8::from(scene.held(minus)))
}

/// Copies `template` as `name` at `at`, shown and in `class`.
fn spawn(
    scene: &mut GameScene<'_>,
    template: &str,
    name: &str,
    class: &str,
    at: [f32; 3],
) {
    scene.spawn_copy(template, name, at);
    scene.set_visible(name, true);
    scene.add_class(name, class);
}

/// Runs once a frame.
pub fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("kills") || scene.counter_complete("hits") {
        return;
    }
    let dt = time.delta.as_secs_f32();
    let ms = (dt * 1000.0).round() as i32;

    let (dx, dz) = (axis(scene, "left", "right"), axis(scene, "up", "down"));
    let mut player = scene.object("Player").position();
    let length = dx.hypot(dz);
    if length > 0.0 {
        let step = PLAYER_SPEED * dt / length;
        player[0] = (player[0] + dx * step).clamp(-ARENA, ARENA);
        player[2] = (player[2] + dz * step).clamp(-ARENA, ARENA);
        scene.object("Player").set_position(player);
    }

    // Aiming fires; a shot's heading is its Y rotation.
    let (ax, az) = (
        axis(scene, "aim_left", "aim_right"),
        axis(scene, "aim_up", "aim_down"),
    );
    let mut reload = (scene.counter_or("reload", 0) - ms).max(0);
    if (ax, az) != (0.0, 0.0) && reload == 0 {
        let name = format!("Shot {}", scene.add_to_counter("shots", 1));
        spawn(scene, "Bullet Template", &name, "shot", player);
        scene
            .object(&name)
            .set_rotation([0.0, (-ax).atan2(-az), 0.0]);
        reload = RELOAD_MS;
    }
    scene.set_counter("reload", reload);

    let spawned = scene.counter_or("spawned", 0);
    if spawned < WAVE {
        let mut timer = scene.counter_or("spawn_timer", 0) + ms;
        if timer >= SPAWN_MS {
            timer -= SPAWN_MS;
            let [x, z] = SPAWNS[spawned as usize % SPAWNS.len()];
            let name = format!("Enemy {}", spawned + 1);
            spawn(scene, "Enemy Template", &name, "enemy", [x, 0.4, z]);
            scene.set_counter("spawned", spawned + 1);
        }
        scene.set_counter("spawn_timer", timer);
    }

    for shot in scene.in_class("shot") {
        let (mut position, heading) = {
            let shot = scene.object(&shot);
            (shot.position(), shot.rotation()[1])
        };
        position[0] -= heading.sin() * SHOT_SPEED * dt;
        position[2] -= heading.cos() * SHOT_SPEED * dt;
        if position[0].abs() > ARENA + 1.0 || position[2].abs() > ARENA + 1.0 {
            scene.despawn(&shot);
            continue;
        }
        let hit = scene.in_class("enemy").into_iter().find(|enemy| {
            distance(scene.object(enemy).position(), position) < SHOT_REACH
        });
        if let Some(enemy) = hit {
            scene.despawn(&enemy);
            scene.despawn(&shot);
            scene.add_to_counter("kills", 1);
            continue;
        }
        scene.object(&shot).set_position(position);
    }

    for enemy in scene.in_class("enemy") {
        let mut position = scene.object(&enemy).position();
        let gap = distance(position, player);
        if gap == 0.0 {
            continue;
        }
        let (ux, uz) = (
            (player[0] - position[0]) / gap,
            (player[2] - position[2]) / gap,
        );
        let step = (ENEMY_SPEED * dt).min(gap);
        position[0] += ux * step;
        position[2] += uz * step;
        if distance(position, player) < HIT_REACH {
            scene.add_to_counter("hits", 1);
            scene.add_trauma("Game Camera", 0.4);
            position[0] = player[0] - ux * KNOCKBACK;
            position[2] = player[2] - uz * KNOCKBACK;
        }
        scene.object(&enemy).set_position(position);
    }
}

rusting_game!(update);
