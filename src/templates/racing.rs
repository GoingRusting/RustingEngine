//! Circuit, from `rusting new --template racing`: drive three laps.
//!
//! `up` (W, the up arrow, the d-pad) accelerates `Car`,
//! `down` brakes and reverses, and `left` and `right` steer. The car is
//! slow off the road: objects of class `road` are the track, so new
//! track is made by copying a road piece in the editor. Lap by driving
//! through `Checkpoint 1`, `Checkpoint 2` and so on in order; the last
//! one is the finish line. The counter `laps` reaching its target shows
//! `Finished!`, and `restart` (R) starts over. `rusting test
//! tests/lap.json` drives a lap.

use rusting_engine::prelude::*;

const TOP_SPEED: f32 = 14.0;
const OFF_ROAD_SPEED: f32 = 5.0;
const REVERSE_SPEED: f32 = 4.0;
const ACCELERATION: f32 = 10.0;
const BRAKING: f32 = 20.0;
const DRAG: f32 = 4.0;
/// Turn rate in radians a second at speeds above 4 m/s.
const STEERING: f32 = 2.2;
/// How close to a checkpoint's center the car must pass.
const CHECKPOINT_REACH: f32 = 5.0;

/// Whether `at` is over a `road` object, read as an unrotated box.
// ponytail: ignores road rotation; turn roads by swapping X and Z scale.
fn on_road(scene: &mut GameScene<'_>, at: [f32; 3]) -> bool {
    scene.in_class("road").into_iter().any(|road| {
        let road = scene.object(&road);
        let (center, size) = (road.position(), road.scale());
        (at[0] - center[0]).abs() <= size[0] / 2.0
            && (at[2] - center[2]).abs() <= size[2] / 2.0
    })
}

/// Runs once a frame.
pub fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("laps") {
        return;
    }
    let dt = time.delta.as_secs_f32();
    let car = scene.object("Car");
    let (mut position, mut rotation) = (car.position(), car.rotation());
    // Speed lives in a counter, in centimeters a second.
    let mut speed = scene.counter_value("speed") as f32 / 100.0;
    if scene.held("up") {
        speed += ACCELERATION * dt;
    } else if scene.held("down") {
        speed -= BRAKING * dt;
    } else {
        speed -= speed.signum() * (DRAG * dt).min(speed.abs());
    }
    let top = if on_road(scene, position) {
        TOP_SPEED
    } else {
        OFF_ROAD_SPEED
    };
    speed = speed.clamp(-REVERSE_SPEED, top);
    let steer = f32::from(u8::from(scene.held("left")))
        - f32::from(u8::from(scene.held("right")));
    rotation[1] += steer * STEERING * (speed / 4.0).clamp(-1.0, 1.0) * dt;
    position[0] -= rotation[1].sin() * speed * dt;
    position[2] -= rotation[1].cos() * speed * dt;
    scene
        .object("Car")
        .set_position(position)
        .set_rotation(rotation);
    scene.set_counter("speed", (speed * 100.0).round() as i32);

    let checkpoints = scene.in_class("checkpoint").len() as i32;
    let next = scene.counter_value("checkpoint");
    let target = scene.object(&format!("Checkpoint {}", next + 1)).position();
    if (position[0] - target[0]).hypot(position[2] - target[2])
        < CHECKPOINT_REACH
    {
        if next + 1 == checkpoints {
            scene.set_counter("checkpoint", 0);
            scene.add_to_counter("laps", 1);
        } else {
            scene.set_counter("checkpoint", next + 1);
        }
    }
}

rusting_game!(update);
