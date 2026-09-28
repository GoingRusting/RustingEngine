//! Edit-to-running-game latency for the starter game, the representative
//! project. It times `rusting run` (cook, Debug build, start, first frame)
//! after no change and after a Rust edit, and fails when the Rust edit
//! misses the target in `docs/dev-environment.md` "Iteration speed".
//!
//! It runs real cargo builds, so it is ignored by default:
//! `cargo test --test iteration_latency -- --ignored --nocapture`.
//! Set `RUSTING_LATENCY_REPORT` to also write the report to a file.

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

/// Incremental Debug build plus first headless frame after a Rust edit.
const RUST_EDIT_TARGET_MS: u64 = 3_000;

/// Samples per case; the report uses the median.
const SAMPLES: usize = 3;

fn cli(target: &Path, args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
        .args(args)
        .arg("--json")
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap();
    let value: Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!("no JSON\n{}", String::from_utf8_lossy(&output.stderr))
        });
    assert_eq!(value["ok"], true, "{value}");
    value
}

/// `build_ms` and `command_to_first_frame_ms` of one `rusting run`.
fn run(target: &Path, root: &str) -> (u64, u64) {
    let ran = cli(target, &["run", root, "--ticks", "1"]);
    let timings = &ran["data"]["timings"];
    let ms = |key: &str| {
        timings[key]
            .as_u64()
            .unwrap_or_else(|| panic!("no {key}: {ran}"))
    };
    (ms("build_ms"), ms("command_to_first_frame_ms"))
}

fn median(mut samples: Vec<(u64, u64)>) -> Value {
    samples.sort_by_key(|&(_, total)| total);
    let (build_ms, total_ms) = samples[samples.len() / 2];
    json!({"build_ms": build_ms, "command_to_first_frame_ms": total_ms})
}

#[test]
#[ignore = "runs real cargo builds of a generated project"]
fn rust_edit_reaches_the_first_frame_within_the_target() {
    let parent = std::env::temp_dir()
        .join(format!("rusting-latency-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&parent).unwrap();
    // Shared with tests/cli.rs so the engine build is reused.
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/cli-games");
    let name = "Coin Run";
    cli(
        &target,
        &[
            "new",
            parent.to_str().unwrap(),
            name,
            "--template",
            "starter",
        ],
    );
    let root = parent.join(name);
    let code = root.join("src/main.rs");
    let root = root.to_str().unwrap();

    // The first run builds the game once; it is not part of the report.
    let (cold_build_ms, cold_ms) = run(&target, root);
    let unchanged = median((0..SAMPLES).map(|_| run(&target, root)).collect());
    let source = std::fs::read_to_string(&code).unwrap();
    let rust_edit = median(
        (0..SAMPLES)
            .map(|sample| {
                std::fs::write(
                    &code,
                    format!(
                        "{source}\n#[allow(dead_code)]\nfn edited() -> u32 {{ {sample} }}\n"
                    ),
                )
                .unwrap();
                run(&target, root)
            })
            .collect(),
    );

    let report = json!({
        "project": "starter template (Coin Run), Debug, headless",
        "target_ms": RUST_EDIT_TARGET_MS,
        "first_run": {"build_ms": cold_build_ms, "command_to_first_frame_ms": cold_ms},
        "no_change": unchanged,
        "rust_edit": rust_edit,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = std::env::var_os("RUSTING_LATENCY_REPORT") {
        std::fs::write(path, report.to_string()).unwrap();
    }
    let _ = std::fs::remove_dir_all(&parent);
    let total = rust_edit["command_to_first_frame_ms"].as_u64().unwrap();
    assert!(
        total <= RUST_EDIT_TARGET_MS,
        "a Rust edit took {total} ms to the first frame; the target is \
         {RUST_EDIT_TARGET_MS} ms"
    );
}
