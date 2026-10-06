//! Fixed render benchmark with stored baselines.
//!
//! ```sh
//! cargo run --release --example render_bench -- eco benchmarks/my-gpu-eco.json
//! cargo run --release --example render_bench -- eco --screens 6 --bears 5000 --bear-updates 500
//! ```
//!
//! `--screens N` adds N camera screens at `--screen-size WxH` (320x180 by
//! default); `--bears N` adds N static spheres sharing one mesh and
//! material, `--bear-updates N` moves that many of them every frame, and
//! `--screen-every N` draws each feed every N frames, `--no-bodies` drops
//! the 1,000 GPU physics bodies, and `--extent WxH` changes the frame size.
//!
//! Renders the benchmark scene along its camera path (600 frames at
//! 1920x1080, offscreen) and prints the report as JSON. With a baseline path:
//! a missing file is written from this run; an existing one is compared, and
//! any material regression is printed and exits with status 1.

use rusting_engine::rendering::benchmark::{
    run_render_benchmark_with, RenderBenchmarkReport,
};
use rusting_engine::rendering::init_vulkan_headless;
use rusting_engine::runtime::{
    QualityProfile, RenderBenchmarkExtras, RENDER_BENCHMARK_FRAMES,
};

fn main() {
    let mut extras = RenderBenchmarkExtras {
        screen_size: [320, 180],
        ..RenderBenchmarkExtras::default()
    };
    let mut extent = [1920, 1080];
    let mut positional = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .unwrap_or_else(|| panic!("{argument} needs a value"))
        };
        let size = |text: String| -> [u32; 2] {
            let (w, h) = text.split_once('x').expect("size is WxH");
            [w.parse().expect("width"), h.parse().expect("height")]
        };
        match argument.as_str() {
            "--screens" => extras.screens = value().parse().expect("count"),
            "--screen-size" => extras.screen_size = size(value()),
            "--bears" => extras.bears = value().parse().expect("count"),
            "--bear-updates" => {
                extras.bear_updates = value().parse().expect("count");
            }
            "--screen-every" => {
                extras.screen_every = value().parse().expect("frames");
            }
            "--no-bodies" => extras.without_bodies = true,
            "--extent" => extent = size(value()),
            _ => positional.push(argument),
        }
    }
    let mut arguments = positional.into_iter();
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
    let report = run_render_benchmark_with(
        base.queue.clone(),
        extent,
        quality,
        RENDER_BENCHMARK_FRAMES,
        extras,
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
