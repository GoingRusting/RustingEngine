//! Duel, from `rusting new --template card-game`: a card battle.
//!
//! You hold three cards from a ten-card deck shuffled from the run's
//! seed. Keys 1 to 3, the face buttons, or clicking a card's button
//! (`Card 1` to `Card 3`) plays it: Strike adds to `foe_damage`, Guard
//! blocks this turn's attack, and Heal takes back `damage`. Then the foe
//! attacks for `intent`, shown before you pick, and draws its next
//! attack, and you draw a card in place of the one played. `foe_damage`
//! reaching its target shows `You win!`, `damage` reaching its target
//! shows `Game over`, and `restart` (R) starts over. Hand and deck state
//! live in counters made on first write (`drawn`, `turn`, `hand_1` to `hand_3`).
//! `rusting test tests/duel.json` plays a duel on a fixed seed.

use rusting_engine::prelude::*;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Strike,
    Guard,
    Heal,
}

/// The deck: each card's kind and power.
const DECK: [(Kind, i32); 10] = [
    (Kind::Strike, 6),
    (Kind::Strike, 6),
    (Kind::Strike, 6),
    (Kind::Strike, 6),
    (Kind::Guard, 6),
    (Kind::Guard, 6),
    (Kind::Guard, 6),
    (Kind::Heal, 5),
    (Kind::Heal, 5),
    (Kind::Heal, 5),
];
/// The foe attacks for this much plus up to `FOE_SPREAD - 1` more.
const FOE_MIN: i32 = 2;
const FOE_SPREAD: u64 = 6;

/// A number drawn from the run's seed, `stream` and `n`.
fn roll(scene: &GameScene<'_>, stream: u64, n: u64) -> u64 {
    // SplitMix64.
    let mut z = scene
        .seed()
        .wrapping_add(stream << 32 | n)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Draws the next card; the deck is shuffled again each time through.
fn draw(scene: &mut GameScene<'_>) -> i32 {
    let drawn = scene.add_to_counter("drawn", 1) - 1;
    let (round, at) = (drawn as u64 / 10, drawn as usize % 10);
    let mut order: Vec<i32> = (0..10).collect();
    for i in (1..order.len()).rev() {
        let j = roll(scene, round + 1, i as u64) as usize % (i + 1);
        order.swap(i, j);
    }
    order[at]
}

/// Shows hand slot `slot`: its button text and its card's color.
fn show(scene: &mut GameScene<'_>, slot: usize) {
    let (kind, power) =
        DECK[scene.counter_value(&format!("hand_{slot}")) as usize];
    let (label, color) = match kind {
        Kind::Strike => ("Strike", [0.85, 0.25, 0.2, 1.0]),
        Kind::Guard => ("Guard", [0.25, 0.45, 0.9, 1.0]),
        Kind::Heal => ("Heal", [0.3, 0.8, 0.35, 1.0]),
    };
    scene.set_hud(&format!("Card {slot}"), |hud| {
        hud.text = format!("{slot}: {label} {power}");
    });
    scene.set_color(&format!("Slot {slot}"), color);
}

/// Runs once a frame: one turn per card played.
pub fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("foe_damage") || scene.counter_complete("damage")
    {
        return;
    }
    if scene.counter_or("drawn", 0) == 0 {
        for slot in 1..=3 {
            let card = draw(scene);
            scene.set_counter(&format!("hand_{slot}"), card);
            show(scene, slot);
        }
        let intent = FOE_MIN + (roll(scene, 0, 0) % FOE_SPREAD) as i32;
        scene.set_counter("intent", intent);
    }
    let clicked = scene.clicked();
    let Some(slot) = (1..=3).find(|slot| {
        scene.pressed(&format!("play_{slot}"))
            || clicked.contains(&format!("Card {slot}"))
    }) else {
        return;
    };

    let (kind, power) =
        DECK[scene.counter_value(&format!("hand_{slot}")) as usize];
    scene.flash(&format!("Slot {slot}"));
    let mut block = 0;
    match kind {
        Kind::Strike => {
            scene.add_to_counter("foe_damage", power);
            scene.flash("Foe");
        }
        Kind::Guard => block = power,
        Kind::Heal => {
            let healed = power.min(scene.counter_value("damage"));
            scene.add_to_counter("damage", -healed);
        }
    }
    if scene.counter_complete("foe_damage") {
        return;
    }
    let hit = (scene.counter_value("intent") - block).max(0);
    if hit > 0 {
        scene.add_to_counter("damage", hit);
        scene.add_trauma("Game Camera", 0.1 * hit as f32);
    }
    let turn = scene.add_to_counter("turn", 1) as u64;
    let intent = FOE_MIN + (roll(scene, 0, turn) % FOE_SPREAD) as i32;
    scene.set_counter("intent", intent);
    let card = draw(scene);
    scene.set_counter(&format!("hand_{slot}"), card);
    show(scene, slot);
}

rusting_game!(update);
