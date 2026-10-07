//! Add a level select.
//!
//! HUD buttons named `Level 1`, `Level 2` and so on load
//! `scenes/level_<n>.rscene` when clicked (level 1 is `scenes/main.rscene`).
//! The score counter carries over, since counters live in the scene. Make
//! the buttons `rusting.hud` objects with `"button": true`, and call
//! `level_select::level_select(scene);` first in `update`, returning when
//! it does: objects looked up before a scene load are gone.

use rusting_engine::prelude::*;

/// Loads the level whose button was clicked; true when a level loaded.
pub fn level_select(scene: &mut GameScene<'_>) -> bool {
    let clicked = scene.clicked();
    let Some(level) =
        clicked.iter().find_map(|name| name.strip_prefix("Level "))
    else {
        return false;
    };
    let path = match level {
        "1" => "scenes/main.rscene".to_owned(),
        n => format!("scenes/level_{n}.rscene"),
    };
    let score = scene.counter_or("score", 0);
    if scene.load_scene(&path).is_err() {
        return false;
    }
    scene.set_counter("score", score);
    true
}
