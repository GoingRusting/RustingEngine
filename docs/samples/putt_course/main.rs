use rusting_engine::prelude::*;

/// The ball leaves the putter at this speed at full power, in m/s.
const MAX_PUTT_SPEED: f32 = 12.0;
/// Full power after this many ticks of charging; it then falls back.
const CHARGE_TICKS: i32 = 60;
/// Grass slows a rolling ball by this fraction of its speed per second...
const ROLL_DRAG: f32 = 0.2;
/// ...and by this much in m/s every second.
const ROLL_FRICTION: f32 = 0.35;
/// Below this speed the ball stops and can be putted again.
const REST_SPEED: f32 = 0.1;
/// A ball faster than this rolls over the cup instead of dropping in.
const CUP_SPEED: f32 = 4.0;
const TEE: [f32; 3] = [0.0, 0.15, 8.0];

/// Power in percent after `charge` ticks: up to 100, then back down, so
/// holding too long costs power.
fn power(charge: i32) -> i32 {
    let phase = charge % (2 * CHARGE_TICKS);
    let rising = if phase <= CHARGE_TICKS {
        phase
    } else {
        2 * CHARGE_TICKS - phase
    };
    rising * 100 / CHARGE_TICKS
}

/// Unit direction on the ground from the ball toward the floor point under
/// the mouse cursor.
fn aim_direction(
    scene: &mut GameScene<'_>,
    ball: [f32; 3],
) -> Option<[f32; 2]> {
    let (origin, direction) = scene.pointer_ray()?;
    let hit = scene.raycast(origin, direction, 100.0)?;
    let (dx, dz) = (hit.point[0] - ball[0], hit.point[2] - ball[2]);
    let length = dx.hypot(dz);
    (length > 0.2).then(|| [dx / length, dz / length])
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("sunk") {
        return;
    }
    let step = time.delta.as_secs_f32();
    let ball = scene.object("Ball").position();
    let [vx, vy, vz] = scene.linear_velocity("Ball").unwrap_or([0.0; 3]);
    let speed = vx.hypot(vz);

    if ball[1] < -2.0 {
        // Off the green: back to the tee with a penalty stroke.
        scene.set_linear_velocity("Ball", [0.0; 3]);
        scene.object("Ball").set_position(TEE);
        scene.add_to_counter("strokes", 1);
        return;
    }

    if speed < CUP_SPEED
        && scene.touching("Ball").iter().any(|name| name == "Cup")
    {
        scene.set_counter("sunk", 1);
        scene.set_body_kind("Ball", RigidBodyKind::Kinematic);
        scene.set_linear_velocity("Ball", [0.0; 3]);
        scene.set_visible("Ball", false);
        scene.set_visible("Aim", false);
        return;
    }

    let at_rest = speed < REST_SPEED;
    if !at_rest {
        // The grass slows the ball; gravity keeps its vertical speed.
        let slowed =
            (speed * (1.0 - ROLL_DRAG * step) - ROLL_FRICTION * step).max(0.0);
        let scale = slowed / speed;
        scene.set_linear_velocity("Ball", [vx * scale, vy, vz * scale]);
        scene.set_visible("Aim", false);
        return;
    }
    if speed > 0.0 {
        scene.set_linear_velocity("Ball", [0.0, vy, 0.0]);
    }

    let Some([dx, dz]) = aim_direction(scene, ball) else {
        scene.set_visible("Aim", false);
        return;
    };
    let charge = scene.counter_value("charge");
    if scene.held("putt") {
        scene.add_to_counter("charge", 1);
    } else if charge > 0 {
        let putt = power(charge) as f32 / 100.0 * MAX_PUTT_SPEED;
        scene.set_linear_velocity("Ball", [dx * putt, vy, dz * putt]);
        scene.add_to_counter("strokes", 1);
        scene.set_counter("charge", 0);
    }
    let percent = power(scene.counter_value("charge"));
    scene.set_counter("power", percent);

    // The arrow starts at the ball and grows with the power.
    let length = 0.5 + 2.0 * percent as f32 / 100.0;
    scene.set_visible("Aim", true);
    let mut aim = scene.object("Aim");
    aim.set_position([
        ball[0] + dx * (0.25 + length * 0.5),
        0.05,
        ball[2] + dz * (0.25 + length * 0.5),
    ]);
    aim.set_rotation([0.0, (-dx).atan2(-dz), 0.0]);
    aim.set_scale([0.08, 0.03, length]);
}

rusting_game!(update);
