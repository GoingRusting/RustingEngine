use rusting_engine::prelude::*;

/// Ball speed in metres per second. Bounces keep it constant.
const BALL_SPEED: f32 = 8.0;
const PADDLE_SPEED: f32 = 10.0;
const PADDLE_HALF_WIDTH: f32 = 1.0;
/// The walls' inner faces are at x = ±6.
const PADDLE_LIMIT: f32 = 6.0 - PADDLE_HALF_WIDTH;
/// Height of the ball's center while it waits on the paddle.
const BALL_REST_Y: f32 = -4.45;
/// Below this height the ball is lost.
const FLOOR_Y: f32 = -6.5;
/// The steepest paddle bounce leans this far from straight up.
const MAX_BOUNCE_ANGLE: f32 = 1.05;

fn value(scene: &mut GameScene<'_>, name: &str) -> i32 {
    scene.counter(name).map_or(0, |counter| counter.value)
}

fn add(scene: &mut GameScene<'_>, name: &str, amount: i32) {
    if let Some(mut counter) = scene.counter(name) {
        counter.value += amount;
    }
}

fn complete(scene: &mut GameScene<'_>, name: &str) -> bool {
    scene
        .counter(name)
        .is_some_and(|counter| counter.complete())
}

/// Puts the ball back on the paddle, waiting for a launch.
fn park_ball(scene: &mut GameScene<'_>, paddle_x: f32) {
    scene.set_body_kind("Ball", RigidBodyKind::Kinematic);
    scene.set_linear_velocity("Ball", [0.0; 3]);
    scene
        .object("Ball")
        .set_position([paddle_x, BALL_REST_Y, 0.0]);
}

/// Direction off the paddle: the farther from its center the ball hits,
/// the more it leans that way.
fn paddle_bounce(ball_x: f32, paddle_x: f32) -> [f32; 3] {
    let offset = ((ball_x - paddle_x) / PADDLE_HALF_WIDTH).clamp(-1.0, 1.0);
    let angle = offset * MAX_BOUNCE_ANGLE;
    [angle.sin() * BALL_SPEED, angle.cos() * BALL_SPEED, 0.0]
}

/// Scales `velocity` to the ball speed and stops it from going nearly
/// flat, where it would bounce between the side walls for a long time.
fn steady(velocity: [f32; 3]) -> [f32; 3] {
    let [x, y, _] = velocity;
    let min_y = 0.3;
    let length = x.hypot(y).max(f32::EPSILON);
    let (mut x, mut y) = (x / length, y / length);
    if y.abs() < min_y {
        y = min_y.copysign(if y == 0.0 { -1.0 } else { y });
        x = (1.0 - y * y).sqrt().copysign(x);
    }
    [x * BALL_SPEED, y * BALL_SPEED, 0.0]
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if complete(scene, "score") || complete(scene, "lost") {
        return;
    }

    let steer = f32::from(u8::from(scene.held("player.right")))
        - f32::from(u8::from(scene.held("player.left")));
    let mut paddle = scene.object("Paddle");
    let [x, y, z] = paddle.position();
    let paddle_x = (x + steer * PADDLE_SPEED * time.delta.as_secs_f32())
        .clamp(-PADDLE_LIMIT, PADDLE_LIMIT);
    paddle.set_position([paddle_x, y, z]);

    let velocity = scene.linear_velocity("Ball").unwrap_or([0.0; 3]);
    if velocity == [0.0; 3] {
        park_ball(scene, paddle_x);
        if scene.pressed("launch") {
            scene.set_body_kind("Ball", RigidBodyKind::Dynamic);
            scene.set_linear_velocity(
                "Ball",
                paddle_bounce(paddle_x + 0.3, paddle_x),
            );
        }
        return;
    }

    let touching = scene.touching("Ball");
    for brick in touching.iter().filter(|name| name.starts_with("Brick ")) {
        if scene.despawn(brick) {
            add(scene, "score", 1);
        }
    }
    let ball = scene.object("Ball").position();
    let velocity = if touching.iter().any(|name| name == "Paddle") {
        paddle_bounce(ball[0], paddle_x)
    } else {
        steady(velocity)
    };
    scene.set_linear_velocity("Ball", velocity);

    if ball[1] < FLOOR_Y {
        add(scene, "lives", -1);
        add(scene, "lost", 1);
        park_ball(scene, paddle_x);
    }
}

rusting_game!(update);
