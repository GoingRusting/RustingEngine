use rusting_engine::prelude::*;

/// Each guard walks from `from` to `to` and back, taking `ticks` each way.
struct Patrol {
    guard: &'static str,
    from: [f32; 3],
    to: [f32; 3],
    ticks: u64,
}

const PATROLS: [Patrol; 2] = [
    Patrol {
        guard: "Guard A",
        from: [-9.0, 0.9, 2.0],
        to: [9.0, 0.9, 2.0],
        ticks: 540,
    },
    Patrol {
        guard: "Guard B",
        from: [-12.0, 0.9, -6.0],
        to: [-2.0, 0.9, -6.0],
        ticks: 300,
    },
];
/// A guard sees this far, within this angle of where it faces (the lamp's
/// outer cone), when no wall or hedge is in the way.
const SIGHT_RANGE: f32 = 9.0;
const SIGHT_ANGLE: f32 = 0.5;
/// Guards lose track of you twice as slowly as they notice you.
const COOL_DOWN_EVERY: u64 = 2;
const CALM: [f32; 3] = [0.0; 3];
const ALARMED: [f32; 3] = [1.6, 0.15, 0.1];

/// Where the guard stands at `tick` and the yaw it faces.
fn patrol_pose(patrol: &Patrol, tick: u64) -> ([f32; 3], f32) {
    let phase = tick % (2 * patrol.ticks);
    let (from, to, step) = if phase < patrol.ticks {
        (patrol.from, patrol.to, phase)
    } else {
        (patrol.to, patrol.from, phase - patrol.ticks)
    };
    let t = step as f32 / patrol.ticks as f32;
    let position =
        [0, 1, 2].map(|axis| from[axis] + (to[axis] - from[axis]) * t);
    // Forward is -Z at yaw 0, so the yaw facing (dx, dz) is atan2(-dx, -dz).
    let yaw = (-(to[0] - from[0])).atan2(-(to[2] - from[2]));
    (position, yaw)
}

/// True when the guard at `eye` facing `yaw` can see `target`.
fn sees(
    scene: &GameScene<'_>,
    eye: [f32; 3],
    yaw: f32,
    target: [f32; 3],
) -> bool {
    let offset = [0, 1, 2].map(|axis| target[axis] - eye[axis]);
    let distance = offset.iter().map(|value| value * value).sum::<f32>().sqrt();
    if distance > SIGHT_RANGE || distance < 1e-3 {
        return false;
    }
    let facing = [-yaw.sin(), -yaw.cos()];
    let flat = offset[0].hypot(offset[2]).max(1e-3);
    let cosine = (facing[0] * offset[0] + facing[1] * offset[2]) / flat;
    if cosine < SIGHT_ANGLE.cos() {
        return false;
    }
    let direction = offset.map(|value| value / distance);
    scene
        .raycast(eye, direction, SIGHT_RANGE)
        .is_some_and(|hit| hit.name == "Player")
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("caught") || scene.counter_complete("escaped") {
        return;
    }
    let tick = time.fixed_tick;
    let player = scene.object("Player").position();
    let mut seen = false;
    for patrol in &PATROLS {
        let (position, yaw) = patrol_pose(patrol, tick);
        let mut guard = scene.object(patrol.guard);
        guard.set_position(position);
        guard.set_rotation([0.0, yaw, 0.0]);
        let eye = [position[0], 1.5, position[2]];
        let spotted = sees(scene, eye, yaw, player);
        scene.set_emissive(patrol.guard, if spotted { ALARMED } else { CALM });
        seen |= spotted;
    }

    if seen {
        if scene.add_to_counter("alert", 1) >= 40 {
            scene.set_counter("caught", 1);
        }
    } else if tick % COOL_DOWN_EVERY == 0 && scene.counter_value("alert") > 0 {
        scene.add_to_counter("alert", -1);
    }

    let at_door = scene
        .touching("Player")
        .iter()
        .any(|name| name == "Vault Door");
    if at_door && scene.counter_complete("keys") {
        scene.set_counter("escaped", 1);
    }
}

rusting_game!(update);
