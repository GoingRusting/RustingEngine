//! Runtime-only sample player. Native projects use the same runner with their
//! own Rust plugin.

use std::error::Error;
use std::path::PathBuf;

use rusting_engine::demo::DemoPlugin;
use rusting_engine::project_runner::{run_project, run_project_headless};

/// Fixed number of ticks run by `--headless`.
const HEADLESS_TICKS: u32 = 60;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1).peekable();
    let headless = args.peek().is_some_and(|arg| arg == "--headless");
    if headless {
        args.next();
    }
    let scene_path = args.next().map_or_else(
        || PathBuf::from("testGame/build/main.rscene.bin"),
        PathBuf::from,
    );
    if headless {
        run_project_headless(scene_path, DemoPlugin, HEADLESS_TICKS)
    } else {
        run_project("RustingEngine Game", scene_path, DemoPlugin)
    }
}
