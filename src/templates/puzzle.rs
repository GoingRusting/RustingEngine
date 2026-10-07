//! Box Push, from `rusting new --template puzzle`: push every box onto a
//! goal.
//!
//! The arrow keys, WASD or the d-pad step `Player` one cell. Walking into
//! a box (class `box`) pushes it when the cell behind it is free; walls
//! (class `wall`) and other boxes block. The counter `boxes` counts boxes
//! on goals (class `goal`), and the `Solved` text shows once it reaches
//! its target. Every object sits on whole-number X and Z, so new levels
//! are made by moving, copying or deleting objects in the editor.
//! `rusting test tests/solve.json` plays the level.

use rusting_engine::prelude::*;

/// Each move action and its step on the X-Z grid.
const MOVES: [(&str, (i32, i32)); 4] = [
    ("up", (0, -1)),
    ("down", (0, 1)),
    ("left", (-1, 0)),
    ("right", (1, 0)),
];

fn cell(position: [f32; 3]) -> (i32, i32) {
    (position[0].round() as i32, position[2].round() as i32)
}

/// The object in `class` standing on `at`, if any.
fn on_cell(
    scene: &mut GameScene<'_>,
    class: &str,
    at: (i32, i32),
) -> Option<String> {
    scene
        .in_class(class)
        .into_iter()
        .find(|name| cell(scene.object(name).position()) == at)
}

/// Runs once a frame: one move per key press.
pub fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    let Some((dx, dz)) = MOVES
        .iter()
        .find(|(action, _)| scene.pressed(action))
        .map(|(_, step)| *step)
    else {
        return;
    };
    let player = scene.object("Player").position();
    let (x, z) = cell(player);
    let next = (x + dx, z + dz);
    if on_cell(scene, "wall", next).is_some() {
        return;
    }
    if let Some(pushed) = on_cell(scene, "box", next) {
        let beyond = (next.0 + dx, next.1 + dz);
        if on_cell(scene, "wall", beyond).is_some()
            || on_cell(scene, "box", beyond).is_some()
        {
            return;
        }
        let y = scene.object(&pushed).position()[1];
        scene.object(&pushed).set_position([
            beyond.0 as f32,
            y,
            beyond.1 as f32,
        ]);
    }
    scene.object("Player").set_position([
        next.0 as f32,
        player[1],
        next.1 as f32,
    ]);
    let goals: Vec<_> = scene
        .in_class("goal")
        .into_iter()
        .map(|goal| cell(scene.object(&goal).position()))
        .collect();
    let solved = scene
        .in_class("box")
        .into_iter()
        .filter(|name| goals.contains(&cell(scene.object(name).position())))
        .count();
    scene.set_counter("boxes", solved as i32);
}

rusting_game!(update);
