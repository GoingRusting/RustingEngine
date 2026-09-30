use rusting_engine::prelude::*;

/// Grid steps for each move action, in world X and Y.
const MOVES: [(&str, [f32; 2]); 4] = [
    ("player.forward", [0.0, 1.0]),
    ("player.back", [0.0, -1.0]),
    ("player.left", [-1.0, 0.0]),
    ("player.right", [1.0, 0.0]),
];

fn value(scene: &mut GameScene<'_>, name: &str) -> i32 {
    scene.counter(name).map_or(0, |counter| counter.value)
}

fn set(scene: &mut GameScene<'_>, name: &str, value: i32) {
    if let Some(mut counter) = scene.counter(name) {
        counter.value = value;
    }
}

fn step(position: [f32; 3], [dx, dy]: [f32; 2]) -> [f32; 3] {
    [position[0] + dx, position[1] + dy, position[2]]
}

fn same_cell(a: [f32; 3], b: [f32; 3]) -> bool {
    (a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5
}

fn wall(scene: &mut GameScene<'_>, position: [f32; 3]) -> bool {
    scene.tile("Level", position).is_none_or(|tile| tile == '#')
}

fn crate_at(scene: &mut GameScene<'_>, position: [f32; 3]) -> Option<String> {
    scene
        .in_class("crate")
        .into_iter()
        .find(|name| same_cell(scene.object(name).position(), position))
}

/// Moves the player one cell, pushing a crate ahead when the cell past it
/// is free. Returns whether the player moved.
fn try_move(scene: &mut GameScene<'_>, direction: [f32; 2]) -> bool {
    let target = step(scene.object("Player").position(), direction);
    if wall(scene, target) {
        return false;
    }
    if let Some(pushed) = crate_at(scene, target) {
        let beyond = step(target, direction);
        if wall(scene, beyond) || crate_at(scene, beyond).is_some() {
            return false;
        }
        let depth = scene.object(&pushed).position()[2];
        scene
            .object(&pushed)
            .set_position([beyond[0], beyond[1], depth]);
    }
    scene.object("Player").set_position(target);
    true
}

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    let solved = scene.counter("placed").is_some_and(|placed| placed.complete());
    if solved {
        return;
    }
    for (action, direction) in MOVES {
        if scene.pressed(action) && try_move(scene, direction) {
            let moves = value(scene, "moves");
            set(scene, "moves", moves + 1);
        }
    }
    let placed = scene
        .in_class("crate")
        .iter()
        .filter(|name| {
            let position = scene.object(name).position();
            scene.tile("Level", position) == Some('o')
        })
        .count() as i32;
    set(scene, "placed", placed);
}

rusting_game!(update);
