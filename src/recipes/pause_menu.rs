//! Pause menu, from `rusting recipe apply pause_menu`.
//!
//! The action `menu` (Escape or the gamepad Start button) pauses the game
//! and its sounds and shows the HUD objects in `MENU`; pressing it again,
//! or clicking `Pause Resume`, carries on. `Pause Quit` closes the game.
//! Wire it up with `mod pause_menu;` in `src/main.rs` and
//! `pause_menu::pause_menu(scene);` in `update`.

use rusting_engine::prelude::*;

/// The HUD objects shown while paused.
pub const MENU: [&str; 3] = ["Pause Title", "Pause Resume", "Pause Quit"];

/// Runs the pause menu recipe once a frame.
pub fn pause_menu(scene: &mut GameScene<'_>) {
    let clicked = scene.clicked();
    let paused = scene.paused();
    if paused && clicked.iter().any(|name| name == "Pause Quit") {
        scene.quit();
    }
    let resume = paused && clicked.iter().any(|name| name == "Pause Resume");
    if scene.pressed("menu") || resume {
        scene.set_paused(!paused);
        match paused {
            true => scene.resume_sounds(None),
            false => scene.pause_sounds(None),
        }
        for name in MENU {
            scene.set_visible(name, !paused);
        }
    }
}
