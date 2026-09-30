use rusting_engine::prelude::*;

/// Balls leave the camera this fast, in metres per second.
const THROW_SPEED: f32 = 22.0;
/// Upward share added to the throw, so a ball aimed at the crosshair drops
/// onto it at the stands' distance instead of short of it.
const LOFT: f32 = 0.15;
/// A block whose center drops below its stand's top has been knocked off.
const STAND_TOP: f32 = 0.9;
/// Ticks to wait after the last ball before the run counts as over, so a
/// rolling ball can still knock a block down.
const SETTLE_TICKS: i32 = 180;

fn value(scene: &mut GameScene<'_>, name: &str) -> i32 {
    scene.counter(name).map_or(0, |counter| counter.value)
}

fn set(scene: &mut GameScene<'_>, name: &str, value: i32) {
    if let Some(mut counter) = scene.counter(name) {
        counter.value = value;
    }
}

fn throw(scene: &mut GameScene<'_>, tick: i32) {
    let Some((origin, forward)) = scene.camera_ray() else {
        return;
    };
    let balls = value(scene, "balls");
    let name = format!("Ball {}", 11 - balls);
    // Start in front of the camera so the ball clears the player's capsule.
    let start = std::array::from_fn(|axis| origin[axis] + forward[axis] * 0.8);
    scene.spawn_copy("Ball", name.as_str(), start);
    scene.set_visible(&name, true);
    scene.set_body_kind(&name, RigidBodyKind::Dynamic);
    let velocity = [forward[0], forward[1] + LOFT, forward[2]];
    scene.set_linear_velocity(&name, velocity.map(|axis| axis * THROW_SPEED));
    set(scene, "balls", balls - 1);
    set(scene, "last_throw", tick);
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    let tick = time.fixed_tick as i32;
    let toppled = scene
        .in_class("block")
        .iter()
        .filter(|block| scene.object(block).position()[1] < STAND_TOP)
        .count() as i32;
    set(scene, "toppled", toppled);
    let won = scene.counter("toppled").is_some_and(|counter| counter.complete());
    let balls = value(scene, "balls");
    let over = won || (balls == 0 && tick - value(scene, "last_throw") > SETTLE_TICKS);
    set(scene, "out", i32::from(over && !won));
    if scene.pressed("throw") {
        if over {
            scene.restart();
        } else if balls > 0 {
            throw(scene, tick);
        }
    }
}

rusting_game!(update);
