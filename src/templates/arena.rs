//! Arena, from `rusting new --template top-down`: a top-down action game.
//!
//! The arrow keys, WASD or the d-pad move `Player` around the arena, and
//! Space or the pad's south button attacks every enemy (class `enemy`)
//! within reach. Enemies walk toward the player; one that touches the
//! player costs a hit and is knocked back. The counter `kills` reaching
//! its target shows `You win!`, and `hits` reaching its target shows
//! `Game over`; either one stops play, and attacking again restarts.
//! Add enemies by copying one in the editor. `rusting test
//! tests/fight.json` plays a round.

use rusting_engine::prelude::*;

const PLAYER_SPEED: f32 = 5.0;
const ENEMY_SPEED: f32 = 1.5;
/// Enemies this close to the player when it attacks are defeated.
const ATTACK_REACH: f32 = 1.8;
/// Enemies this close to the player hit it.
const HIT_REACH: f32 = 0.9;
/// How far from the player a hit knocks the enemy back.
const KNOCKBACK: f32 = 3.0;
/// Half the arena's width; the player stays inside it.
const ARENA: f32 = 7.0;

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// Runs once a frame.
pub fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.counter_complete("kills") || scene.counter_complete("hits") {
        if scene.pressed("attack") {
            scene.restart();
        }
        return;
    }
    let dt = time.delta.as_secs_f32();
    let axis = |scene: &GameScene<'_>, minus: &str, plus: &str| {
        f32::from(u8::from(scene.held(plus)))
            - f32::from(u8::from(scene.held(minus)))
    };
    let (dx, dz) = (axis(scene, "left", "right"), axis(scene, "up", "down"));
    let length = dx.hypot(dz);
    let mut player = scene.object("Player").position();
    if length > 0.0 {
        let step = PLAYER_SPEED * dt / length;
        player[0] = (player[0] + dx * step).clamp(-ARENA, ARENA);
        player[2] = (player[2] + dz * step).clamp(-ARENA, ARENA);
        scene.object("Player").set_position(player);
    }
    if scene.pressed("attack") {
        scene.flash("Player");
        for enemy in scene.in_class("enemy") {
            if distance(scene.object(&enemy).position(), player) <= ATTACK_REACH
            {
                scene.despawn(&enemy);
                scene.add_to_counter("kills", 1);
            }
        }
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
