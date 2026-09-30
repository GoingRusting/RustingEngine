use rusting_engine::prelude::*;

/// Embers enter through these gates, in turn.
const GATES: [[f32; 3]; 4] = [
    [0.0, 0.6, -13.0],
    [13.0, 0.6, 0.0],
    [0.0, 0.6, 13.0],
    [-13.0, 0.6, 0.0],
];
const START: [f32; 3] = [0.0, 1.0, 0.0];
/// Ticks between embers at the start, and the shortest gap later on.
const FIRST_GAP: u64 = 150;
const LAST_GAP: u64 = 40;
/// Ticks the pulse needs to recharge; the `charge` counter's target.
const RECHARGE: i32 = 60;
const PULSE_RADIUS: f32 = 3.5;

fn value(scene: &mut GameScene<'_>, name: &str) -> i32 {
    scene.counter(name).map_or(0, |counter| counter.value)
}

fn set(scene: &mut GameScene<'_>, name: &str, value: i32) {
    if let Some(mut counter) = scene.counter(name) {
        counter.value = value;
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// The tick the next ember enters on: the gap shrinks by 10 ticks per ember.
fn spawn_tick(spawned: u64) -> u64 {
    (0..spawned)
        .map(|index| FIRST_GAP.saturating_sub(10 * index).max(LAST_GAP))
        .sum::<u64>()
        + 60
}

fn restart(scene: &mut GameScene<'_>, tick: i32) {
    for ember in scene.in_class("ember") {
        if ember != "Ember" {
            scene.despawn(&ember);
        }
    }
    for name in ["score", "burns", "spawned"] {
        set(scene, name, 0);
    }
    set(scene, "pulse_tick", tick);
    scene.object("Player").set_position(START);
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    let tick = time.fixed_tick as i32;
    let over = value(scene, "burns") >= 3;
    let charge = (tick - value(scene, "pulse_tick")).clamp(0, RECHARGE);
    set(scene, "charge", charge);
    if over {
        if scene.pressed("pulse") {
            restart(scene, tick);
        }
        return;
    }

    let spawned = value(scene, "spawned");
    if time.fixed_tick >= spawn_tick(spawned as u64) {
        let name = format!("Ember {}", spawned + 1);
        let gate = GATES[spawned as usize % GATES.len()];
        scene.spawn_copy("Ember", name.as_str(), gate);
        scene.set_visible(&name, true);
        set(scene, "spawned", spawned + 1);
    }

    let player = scene.object("Player").position();
    let pulse = charge == RECHARGE && scene.pressed("pulse");
    if pulse {
        set(scene, "pulse_tick", tick);
        scene.trigger("Pulse");
    }
    // Embers speed up as the score grows.
    let speed = (1.6 + 0.08 * value(scene, "score") as f32).min(4.5);
    let step = speed * time.delta.as_secs_f32();
    let burned = scene.touching("Player");
    for ember in scene.in_class("ember") {
        if ember == "Ember" {
            continue;
        }
        let position = scene.object(&ember).position();
        let hit = burned.contains(&ember);
        if hit || (pulse && distance(position, player) < PULSE_RADIUS) {
            scene.despawn(&ember);
            let counter = if hit { "burns" } else { "score" };
            let count = value(scene, counter);
            set(scene, counter, count + 1);
            continue;
        }
        let (dx, dz) = (player[0] - position[0], player[2] - position[2]);
        let length = dx.hypot(dz).max(1e-3);
        let step = step.min(length);
        scene
            .object(&ember)
            .move_by([dx / length * step, 0.0, dz / length * step]);
    }
}

rusting_game!(update);
