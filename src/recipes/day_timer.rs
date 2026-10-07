//! Day timer, from `rusting recipe apply day_timer`.
//!
//! The counter `day` starts at 1 each round and goes up every
//! `DAY_SECONDS`; `day_seconds_left` counts down to the next day. The HUD
//! label `Day Clock` shows both. Wire it up with `mod day_timer;` in
//! `src/main.rs` and `day_timer::day_timer(scene, time);` in `update`.

use rusting_engine::prelude::*;

/// Length of one day.
pub const DAY_SECONDS: f64 = 30.0;

/// Runs the day timer recipe once a frame.
pub fn day_timer(scene: &mut GameScene<'_>, time: &FrameTime) {
    let now_ms = i32::try_from(time.elapsed.as_millis()).unwrap_or(i32::MAX);
    scene.once("day_timer", |scene| {
        scene.set_counter("day", 1);
        scene.set_counter("day_started_ms", now_ms);
    });
    let day_ms = (DAY_SECONDS * 1000.0) as i32;
    let mut started = scene.counter_value("day_started_ms");
    while now_ms - started >= day_ms {
        started += day_ms;
        scene.add_to_counter("day", 1);
    }
    scene.set_counter("day_started_ms", started);
    let left_ms = day_ms - (now_ms - started);
    scene.set_counter("day_seconds_left", (left_ms + 999) / 1000);
}
