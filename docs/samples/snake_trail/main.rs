use rusting_engine::prelude::*;

/// Ticks between two moves at the start, and the fastest pace later on.
const FIRST_STEP: u64 = 9;
const LAST_STEP: u64 = 5;
/// Right, down, left, up, in grid cells (rows grow downward).
const HEADINGS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
/// Where the apples appear, in order.
const APPLES: [(i32, i32); 10] = [
    (12, 6),
    (12, 2),
    (4, 3),
    (15, 8),
    (8, 9),
    (17, 2),
    (2, 10),
    (10, 5),
    (16, 6),
    (6, 2),
];

/// World position of the center of cell (`column`, `row`) of the Arena,
/// whose top-left corner is at (-10, 6).
fn center((column, row): (i32, i32)) -> [f32; 3] {
    [-9.5 + column as f32, 5.5 - row as f32, 0.1]
}

fn cell_of(position: [f32; 3]) -> (i32, i32) {
    (
        (position[0] + 10.0).floor() as i32,
        (6.0 - position[1]).floor() as i32,
    )
}

fn part(index: i32) -> String {
    if index == 0 {
        "Head".to_owned()
    } else {
        format!("Segment {index}")
    }
}

fn steer(scene: &mut GameScene<'_>) {
    for (heading, action) in ["right", "down", "left", "up"].iter().enumerate()
    {
        if scene.pressed(action) {
            scene.set_counter("turn", heading as i32);
        }
    }
}

/// Moves the snake one cell. Returns false when it crashed.
fn step(scene: &mut GameScene<'_>) -> bool {
    let heading = scene.counter_value("heading");
    let turn = scene.counter_value("turn");
    // A snake cannot turn back into itself.
    let heading = if (turn - heading).abs() == 2 {
        heading
    } else {
        turn
    };
    scene.set_counter("heading", heading);
    let (dx, dy) = HEADINGS[heading as usize];
    let length = scene.counter_value("length");
    let body: Vec<(i32, i32)> = (0..=length)
        .map(|index| cell_of(scene.object(&part(index)).position()))
        .collect();
    let next = (body[0].0 + dx, body[0].1 + dy);
    // The tail moves out of the way this step, so the head may follow it.
    let bites = body[1..body.len() - 1].contains(&next);
    if bites || scene.tile("Arena", center(next)) == Some('#') {
        return false;
    }
    for index in (1..=length).rev() {
        scene
            .object(&part(index))
            .set_position(center(body[index as usize - 1]));
    }
    scene.object("Head").set_position(center(next));

    if cell_of(scene.object("Apple").position()) == next {
        let grown = length + 1;
        scene.spawn_copy(
            "Segment",
            part(grown).as_str(),
            center(body[length as usize]),
        );
        scene.set_visible(&part(grown), true);
        scene.add_to_counter("length", 1);
        scene.add_to_counter("apples", 1);
        let eaten = scene.counter_value("apples") as usize;
        let mut occupied = body;
        occupied.push(next);
        // ponytail: the next free spot in a fixed list; a snake covering
        // every listed spot leaves the apple where it was.
        if let Some(spot) = (0..APPLES.len())
            .map(|offset| APPLES[(eaten + offset) % APPLES.len()])
            .find(|spot| !occupied.contains(spot))
        {
            scene.object("Apple").set_position(center(spot));
        }
    }
    true
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if scene.counter_complete("apples") || scene.counter_complete("crashed") {
        return;
    }
    steer(scene);
    let tick = time.fixed_tick;
    let next_step = scene.counter_value("next_step") as u64;
    if tick < next_step {
        return;
    }
    let pace = FIRST_STEP
        .saturating_sub(scene.counter_value("apples") as u64 / 2)
        .max(LAST_STEP);
    scene.set_counter("next_step", (tick + pace) as i32);
    if next_step > 0 && !step(scene) {
        scene.add_to_counter("crashed", 1);
    }
}

rusting_game!(update);
