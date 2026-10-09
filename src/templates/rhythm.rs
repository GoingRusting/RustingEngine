//! Beat, from `rusting new --template rhythm`: hit the notes on time.
//!
//! Notes, copies of the hidden `Note Template`, slide down three lanes to
//! the hit line on the beats in `CHART`. Press a lane's action (`lane_1`
//! on D or the left arrow, `lane_2` on F or the down arrow, `lane_3` on J
//! or the right arrow) as its note crosses the line: within `PERFECT_MS`
//! scores 100, within `GOOD_MS` 50, and a note let past is a miss. The
//! `Judgement` text shows the last call. `played` reaching its target, every
//! note hit or missed, shows `Song clear!`; `misses` reaching its target
//! shows `Game over`; `restart` (R) starts over. Note times follow from
//! the `clock_ms` counter, so the code keeps no per-note state. There is
//! no music yet: play a track with `scene.play_sound` when the clock
//! starts, and set `BEAT_MS` to its tempo. `rusting test tests/song.json`
//! plays the song.

use rusting_engine::prelude::*;

/// 120 beats a minute.
const BEAT_MS: i32 = 500;
/// Each note's beat and lane; keep the `played` target equal to its length.
const CHART: [(i32, i32); 16] = [
    (4, 1),
    (5, 2),
    (6, 3),
    (7, 2),
    (8, 1),
    (9, 1),
    (10, 3),
    (11, 3),
    (12, 2),
    (13, 1),
    (14, 2),
    (15, 3),
    (16, 1),
    (17, 3),
    (18, 2),
    (19, 2),
];
const PERFECT_MS: i32 = 60;
const GOOD_MS: i32 = 140;
/// How long a note takes from the top of its lane to the line.
const TRAVEL_MS: i32 = 1500;
/// Lane X positions, and the hit line's Z.
const LANE_X: [f32; 3] = [-1.5, 0.0, 1.5];
const HIT_Z: f32 = 2.0;
const NOTE_SPEED: f32 = 8.0;

fn judge(scene: &mut GameScene<'_>, text: &str, color: [f32; 4]) {
    scene.set_hud("Judgement", |hud| {
        hud.text = text.into();
        hud.color = color;
    });
}

/// Runs once a frame.
pub fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("played") || scene.counter_complete("misses") {
        return;
    }
    let ms = (time.delta.as_secs_f32() * 1000.0).round() as i32;
    let clock = scene.add_to_counter("clock_ms", ms);
    if clock / BEAT_MS != (clock - ms) / BEAT_MS {
        scene.flash("Hit Line");
    }

    let spawned = scene.counter_or("spawned", 0);
    let next = CHART.get(spawned as usize).copied();
    if let Some((_, lane)) =
        next.filter(|(beat, _)| clock >= beat * BEAT_MS - TRAVEL_MS)
    {
        let name = format!("Note {}", spawned + 1);
        let x = LANE_X[lane as usize - 1];
        scene.spawn_copy("Note Template", &name, [x, 0.2, -20.0]);
        scene.set_visible(&name, true);
        scene.add_class(&name, "note");
        scene.set_counter("spawned", spawned + 1);
    }

    // Each live note's index in the chart and how early it is, in ms.
    let notes: Vec<(String, usize, i32)> = scene
        .in_class("note")
        .into_iter()
        .map(|name| {
            let n: usize = name["Note ".len()..].parse().unwrap_or(1);
            let early = CHART[n - 1].0 * BEAT_MS - clock;
            (name, n - 1, early)
        })
        .collect();
    let mut hit = Vec::new();
    for lane in 1..=3 {
        if !scene.pressed(&format!("lane_{lane}")) {
            continue;
        }
        scene.flash(&format!("Pad {lane}"));
        // The lane's earliest note within reach.
        let Some((name, _, early)) = notes
            .iter()
            .filter(|(_, n, early)| {
                CHART[*n].1 == lane && early.abs() <= GOOD_MS
            })
            .min_by_key(|(_, n, _)| *n)
        else {
            continue;
        };
        let (points, text, color) = if early.abs() <= PERFECT_MS {
            (100, "PERFECT", [1.0, 0.85, 0.2, 1.0])
        } else {
            (50, "GOOD", [0.4, 0.9, 1.0, 1.0])
        };
        scene.despawn(name);
        scene.add_to_counter("score", points);
        scene.add_to_counter("played", 1);
        judge(scene, text, color);
        hit.push(name.clone());
    }
    for (name, _, early) in notes {
        if hit.contains(&name) {
            continue;
        }
        if early < -GOOD_MS {
            scene.despawn(&name);
            scene.add_to_counter("misses", 1);
            scene.add_to_counter("played", 1);
            scene.add_trauma("Game Camera", 0.3);
            judge(scene, "MISS", [1.0, 0.3, 0.3, 1.0]);
            continue;
        }
        let mut at = scene.object(&name).position();
        at[2] = HIT_Z - early as f32 / 1000.0 * NOTE_SPEED;
        scene.object(&name).set_position(at);
    }
}

rusting_game!(update);
