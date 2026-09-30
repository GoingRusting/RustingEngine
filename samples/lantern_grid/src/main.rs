use rusting_engine::prelude::*;

/// The board is SIZE x SIZE lanterns named "Cell <column> <row>".
const SIZE: i32 = 5;
/// The puzzle starts from a full board flipped at these cells, so pressing
/// the same cells again solves it.
const SCRAMBLE: [(i32, i32); 3] = [(1, 1), (3, 2), (2, 4)];
const LIT: ([f32; 4], [f32; 3]) = ([1.0, 0.72, 0.3, 1.0], [2.4, 1.5, 0.45]);
const DARK: ([f32; 4], [f32; 3]) = ([0.12, 0.13, 0.18, 1.0], [0.0; 3]);

fn cell(column: i32, row: i32) -> String {
    format!("Cell {column} {row}")
}

fn is_lit(scene: &mut GameScene<'_>, name: &str) -> bool {
    scene.color(name).is_some_and(|color| color[0] > 0.5)
}

/// Flips the lantern at (`column`, `row`) and its four edge neighbours.
fn press(scene: &mut GameScene<'_>, column: i32, row: i32) {
    for (dc, dr) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
        let (c, r) = (column + dc, row + dr);
        if !(0..SIZE).contains(&c) || !(0..SIZE).contains(&r) {
            continue;
        }
        let name = cell(c, r);
        let (color, emissive) = if is_lit(scene, &name) { DARK } else { LIT };
        scene.set_color(&name, color);
        scene.set_emissive(&name, emissive);
    }
}

/// The cell under the mouse cursor, if any.
fn clicked_cell(scene: &mut GameScene<'_>) -> Option<(i32, i32)> {
    let (origin, direction) = scene.pointer_ray()?;
    let hit = scene.raycast(origin, direction, 100.0)?;
    let mut parts = hit.name.strip_prefix("Cell ")?.split(' ');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn count_lit(scene: &mut GameScene<'_>) -> i32 {
    let mut lit = 0;
    for row in 0..SIZE {
        for column in 0..SIZE {
            lit += i32::from(is_lit(scene, &cell(column, row)));
        }
    }
    lit
}

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    scene.once("scramble", |scene| {
        for (column, row) in SCRAMBLE {
            press(scene, column, row);
        }
    });
    if !scene.counter_complete("lit") && scene.pressed("select") {
        if let Some((column, row)) = clicked_cell(scene) {
            press(scene, column, row);
            scene.add_to_counter("moves", 1);
        }
    }
    let lit = count_lit(scene);
    scene.set_counter("lit", lit);
}

rusting_game!(update);
