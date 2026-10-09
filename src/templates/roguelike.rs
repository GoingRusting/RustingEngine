//! Crypt, from `rusting new --template roguelike`: descend three floors.
//!
//! The arrow keys, WASD or the d-pad step `Player` one cell, and every
//! step is a turn. Each floor is made from the run's seed: rubble (copies
//! of the hidden `Rubble Template`, class `rubble`), enemies (copies of
//! `Enemy Template`, class `enemy`) and the `Stairs`. Walking into an
//! enemy defeats it. After each turn every enemy next to the player
//! strikes, adding to `wounds`, and the others step toward the player.
//! Reaching the stairs builds the next floor and adds to `depth`. `depth`
//! reaching its target shows `You escaped!`, `wounds` reaching its target
//! shows `Game over`, and `restart` (R) starts over. `rusting test
//! tests/descend.json` plays a run on a fixed seed.

use rusting_engine::prelude::*;

/// Each move action and its step on the X-Z grid.
const MOVES: [(&str, (i32, i32)); 4] = [
    ("up", (0, -1)),
    ("down", (0, 1)),
    ("left", (-1, 0)),
    ("right", (1, 0)),
];
/// The floor's last cell; cells run from 1 to these on X and Z, inside
/// the outer walls.
const WIDTH: i32 = 9;
const DEPTH: i32 = 7;
const START: (i32, i32) = (1, 1);
/// One in this many pillar cells gets rubble.
const RUBBLE_ODDS: u64 = 2;
/// Enemies on the first floor; each floor adds one.
const ENEMIES: i32 = 2;

fn cell(position: [f32; 3]) -> (i32, i32) {
    (position[0].round() as i32, position[2].round() as i32)
}

fn place(scene: &mut GameScene<'_>, name: &str, at: (i32, i32)) {
    let y = scene.object(name).position()[1];
    scene
        .object(name)
        .set_position([at.0 as f32, y, at.1 as f32]);
}

/// The object in `class` standing on `at`, if any.
fn on_cell(
    scene: &mut GameScene<'_>,
    class: &str,
    at: (i32, i32),
) -> Option<String> {
    scene
        .in_class(class)
        .into_iter()
        .find(|name| cell(scene.object(name).position()) == at)
}

/// Whether `at` is inside the walls and free of rubble.
fn open(scene: &mut GameScene<'_>, at: (i32, i32)) -> bool {
    (1..=WIDTH).contains(&at.0)
        && (1..=DEPTH).contains(&at.1)
        && on_cell(scene, "rubble", at).is_none()
}

/// A number drawn from the run's seed, the floor and `n`.
fn roll(scene: &GameScene<'_>, floor: i32, n: u64) -> u64 {
    // SplitMix64.
    let mut z = scene
        .seed()
        .wrapping_add((floor as u64) << 32 | n)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Clears the last floor and lays out floor `floor`. Rubble only goes on
/// cells with both coordinates even, so every other cell stays connected.
fn build(scene: &mut GameScene<'_>, floor: i32) {
    for name in scene
        .in_class("rubble")
        .into_iter()
        .chain(scene.in_class("enemy"))
    {
        scene.despawn(&name);
    }
    place(scene, "Player", START);
    let mut draw = 0;
    let mut next = |scene: &GameScene<'_>| {
        draw += 1;
        roll(scene, floor, draw)
    };
    for x in (2..WIDTH).step_by(2) {
        for z in (2..DEPTH).step_by(2) {
            if next(scene) % RUBBLE_ODDS == 0 {
                let name = format!("Rubble {x} {z}");
                scene.spawn_copy(
                    "Rubble Template",
                    &name,
                    [x as f32, 0.5, z as f32],
                );
                scene.set_visible(&name, true);
                scene.add_class(&name, "rubble");
            }
        }
    }
    // The stairs go far from the start, enemies anywhere off the start's
    // corner; both stay off the pillar cells.
    let mut free = |scene: &GameScene<'_>, far: i32| loop {
        let (x, z) = (
            1 + (next(scene) % WIDTH as u64) as i32,
            1 + (next(scene) % DEPTH as u64) as i32,
        );
        if (x % 2 == 1 || z % 2 == 1) && (x - START.0) + (z - START.1) >= far {
            break (x, z);
        }
    };
    let stairs = free(scene, WIDTH + DEPTH - 6);
    place(scene, "Stairs", stairs);
    for n in 1..=ENEMIES + floor {
        let at = free(scene, 4);
        if at == stairs || on_cell(scene, "enemy", at).is_some() {
            continue;
        }
        let name = format!("Enemy {n}");
        scene.spawn_copy(
            "Enemy Template",
            &name,
            [at.0 as f32, 0.4, at.1 as f32],
        );
        scene.set_visible(&name, true);
        scene.add_class(&name, "enemy");
    }
}

/// Runs once a frame: one turn per key press.
pub fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("depth") || scene.counter_complete("wounds") {
        return;
    }
    let floor = scene.counter_value("depth");
    if scene.counter_or("built", -1) != floor {
        build(scene, floor);
        scene.set_counter("built", floor);
    }
    let Some((dx, dz)) = MOVES
        .iter()
        .find(|(action, _)| scene.pressed(action))
        .map(|(_, step)| *step)
    else {
        return;
    };
    let (x, z) = cell(scene.object("Player").position());
    let target = (x + dx, z + dz);
    if !open(scene, target) {
        return;
    }
    if let Some(enemy) = on_cell(scene, "enemy", target) {
        scene.despawn(&enemy);
    } else {
        place(scene, "Player", target);
        if cell(scene.object("Stairs").position()) == target {
            scene.add_to_counter("depth", 1);
            return;
        }
    }

    // The enemies' turn.
    let player = cell(scene.object("Player").position());
    for enemy in scene.in_class("enemy") {
        let (ex, ez) = cell(scene.object(&enemy).position());
        let (gx, gz) = (player.0 - ex, player.1 - ez);
        if gx.abs() + gz.abs() == 1 {
            scene.add_to_counter("wounds", 1);
            scene.flash(&enemy);
            scene.add_trauma("Game Camera", 0.4);
            continue;
        }
        // Step along the longer gap first, else the other.
        let steps = [(gx.signum(), 0), (0, gz.signum())];
        let order = if gx.abs() >= gz.abs() { [0, 1] } else { [1, 0] };
        for (sx, sz) in order.map(|i| steps[i]) {
            let to = (ex + sx, ez + sz);
            if (sx, sz) != (0, 0)
                && open(scene, to)
                && on_cell(scene, "enemy", to).is_none()
            {
                place(scene, &enemy, to);
                break;
            }
        }
    }
}

rusting_game!(update);
