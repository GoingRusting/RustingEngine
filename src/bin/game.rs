//! Runtime-only sample player. Native projects use the same runner with their
//! own Rust plugin.

use std::error::Error;
use std::path::PathBuf;

use rusting_engine::demo::DemoPlugin;
use rusting_engine::project_runner::run_project;

fn main() -> Result<(), Box<dyn Error>> {
    let scene_path = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from("testGame/build/main.rscene.bin"),
        PathBuf::from,
    );
    run_project("RustingEngine Game", scene_path, DemoPlugin)
}
