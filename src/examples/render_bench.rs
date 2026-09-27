//! Fixed render benchmark with stored baselines.
//!
//! ```sh
//! cargo run --release --example render_bench -- eco benchmarks/my-gpu-eco.json
//! ```
//!
//! Renders the benchmark scene along its camera path (600 frames at
//! 1920x1080, offscreen) and prints the report as JSON. With a baseline path:
//! a missing file is written from this run; an existing one is compared, and
//! any material regression is printed and exits with status 1.

use rusting_engine::rendering::benchmark::{
    run_render_benchmark, RenderBenchmarkReport,
};
use rusting_engine::rendering::init_vulkan_headless;
use rusting_engine::runtime::{QualityProfile, RENDER_BENCHMARK_FRAMES};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let quality = match arguments.next().as_deref().unwrap_or("auto") {
        "auto" => QualityProfile::Auto,
        "eco" => QualityProfile::Eco,
        "balanced" => QualityProfile::Balanced,
        "high" => QualityProfile::High,
        other => {
            panic!("unknown quality `{other}`: auto, eco, balanced or high")
        }
    };
    let baseline = arguments.next();

    let base = init_vulkan_headless();
    let report = run_render_benchmark(
        base.queue.clone(),
        [1920, 1080],
        quality,
        RENDER_BENCHMARK_FRAMES,
    )
    .unwrap_or_else(|error| panic!("benchmark failed: {error}"));
    let json = serde_json::to_string_pretty(&report).unwrap();
    println!("{json}");

    let Some(path) = baseline else { return };
    match std::fs::read_to_string(&path) {
        Ok(stored) => {
            let stored: RenderBenchmarkReport = serde_json::from_str(&stored)
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            let regressions = report.regressions(&stored);
            if regressions.is_empty() {
                eprintln!("no regressions against {path}");
            } else {
                for regression in &regressions {
                    eprintln!("regression: {regression}");
                }
                std::process::exit(1);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::write(&path, json + "\n")
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            eprintln!("wrote baseline {path}");
        }
        Err(error) => panic!("{path}: {error}"),
    }
}
