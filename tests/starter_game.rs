//! Repeatable acceptance exercise for the starter game. It turns the request
//! in `samples/starter_game/request.md` into a game with the public CLI
//! only: create, modify, run, inspect, test, and export. The report records
//! each command's time, the time to the first playable frame, failed edits,
//! and manual interventions.
//!
//! It runs real cargo builds, so it is ignored by default:
//! `cargo test --test starter_game -- --ignored --nocapture`. Set
//! `RUSTING_ACCEPTANCE_REPORT` to also write the report to a file.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde_json::{json, Value};

struct Exercise {
    target: PathBuf,
    steps: Vec<Value>,
    failed_edits: usize,
}

impl Exercise {
    /// Runs one CLI command with `--json` and records it.
    fn cli(&mut self, step: &str, args: &[&str]) -> Value {
        let started = Instant::now();
        let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
            .args(args)
            .arg("--json")
            .env("CARGO_TARGET_DIR", &self.target)
            .output()
            .unwrap();
        let ms = started.elapsed().as_millis();
        let value: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| {
                panic!(
                    "{step}: no JSON\n{}",
                    String::from_utf8_lossy(&output.stderr)
                )
            });
        let ok = output.status.success() && value["ok"] == true;
        self.steps.push(json!({"step": step, "ok": ok, "ms": ms}));
        value
    }

    /// An edit command: counts a failure instead of stopping.
    fn edit(&mut self, step: &str, args: &[&str]) -> Value {
        let value = self.cli(step, args);
        if value["ok"] != true {
            self.failed_edits += 1;
        }
        value
    }
}

#[test]
#[ignore = "runs real cargo builds of a generated project"]
fn starter_game_acceptance_exercise() {
    let started = Instant::now();
    let parent = std::env::temp_dir()
        .join(format!("rusting-starter-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&parent).unwrap();
    let samples =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("samples/starter_game");
    let mut run = Exercise {
        // Shared with tests/cli.rs so later runs reuse the engine build.
        target: Path::new(env!("CARGO_MANIFEST_DIR")).join("target/cli-games"),
        steps: Vec::new(),
        failed_edits: 0,
    };

    // Create.
    let created = run.cli(
        "create",
        &[
            "new",
            parent.to_str().unwrap(),
            "Coin Run",
            "--template",
            "starter",
        ],
    );
    assert_eq!(created["ok"], true, "{created}");
    let root = parent.join("Coin Run");
    let scene = root.join("scenes/main.rscene");
    let (root, scene) = (root.to_str().unwrap(), scene.to_str().unwrap());

    // Modify: the sunset look first, then pulsing coins and a win message
    // that stays large and gold after the preset's HUD styling. The first
    // run of this exercise showed that the preset's ACES tone mapping and
    // light sky wash out unlit 2D art, so the patch also sets linear tone
    // mapping and a deeper dusk background.
    let preset = run.edit("preset", &["preset", "apply", scene, "golden_hour"]);
    assert_eq!(preset["ok"], true, "{preset}");
    let coins =
        run.cli("find coins", &["scene", "query", scene, "--class", "coin"]);
    let win = run.cli("find win", &["scene", "query", scene, "--name", "Win"]);
    let tone = run.cli(
        "find tone mapping",
        &[
            "scene",
            "query",
            scene,
            "--component",
            "rusting.tone_mapping",
        ],
    );
    let coin_ids: Vec<_> = coins["data"]["entities"]
        .as_array()
        .unwrap_or_else(|| panic!("{coins}"))
        .iter()
        .map(|coin| coin["id"].clone())
        .collect();
    assert_eq!(coin_ids.len(), 5, "{coins}");
    let mut operations: Vec<Value> = coin_ids
        .iter()
        .map(|id| {
            json!({"op": "set", "id": id, "path": "/components/rusting.tween", "value": {
                "property": "Scale", "from": [0.4, 0.4, 1.0], "to": [0.5, 0.5, 1.0],
                "duration": 0.5, "delay": 0.0, "easing": "SineInOut", "repeat": "PingPong"
            }})
        })
        .collect();
    let win_id = &win["data"]["entities"][0]["id"];
    operations.push(json!({"op": "set", "id": win_id,
        "path": "/components/rusting.hud/font_size", "value": 56.0}));
    operations.push(json!({"op": "set", "id": win_id,
        "path": "/components/rusting.hud/color", "value": [1.0, 0.8, 0.2, 1.0]}));
    operations.push(json!({"op": "set", "id": win_id,
        "path": "/components/rusting.background/color", "value": [0.3, 0.1, 0.05, 1.0]}));
    operations
        .push(json!({"op": "set", "id": tone["data"]["entities"][0]["id"],
        "path": "/components/rusting.tone_mapping/mapper", "value": "Linear"}));
    let patch = parent.join("patch.json");
    std::fs::write(&patch, json!({"operations": operations}).to_string())
        .unwrap();
    let patch = patch.to_str().unwrap();
    let preview = run.edit(
        "patch preview",
        &["scene", "patch", scene, patch, "--dry-run"],
    );
    assert_eq!(preview["ok"], true, "{preview}");
    let patched = run.edit("patch", &["scene", "patch", scene, patch]);
    assert_eq!(patched["ok"], true, "{patched}");
    let valid = run.cli("validate", &["validate", root]);
    assert_eq!(valid["ok"], true, "{valid}");

    // Run: a windowless build and 120 ticks.
    let ran = run.cli("run", &["run", root, "--ticks", "120"]);
    assert_eq!(ran["ok"], true, "{ran}");
    let timings = ran["data"]["timings"].clone();

    // Inspect: the first frame, with a pick on the first coin.
    let shot = parent.join("start.png");
    let captured = run.cli(
        "capture",
        &[
            "capture",
            scene,
            shot.to_str().unwrap(),
            "--tick",
            "30",
            "--size",
            "960x540",
            "--pick",
            "367,310",
        ],
    );
    let coin_seen = captured["data"]["picks"][0]["name"] == "Coin 1";

    // Test: play the level to the win inside the real game build.
    let tests = Path::new(root).join("tests");
    std::fs::create_dir_all(&tests).unwrap();
    let scenario = tests.join("win.scenario.json");
    std::fs::copy(samples.join("win.scenario.json"), &scenario).unwrap();
    let tested = run.cli("test", &["test", root, scenario.to_str().unwrap()]);
    let won = tested["data"]["scenario"]["passed"] == true;
    let win_frame = tests.join("shots/win.png");

    // Export.
    let exports = parent.join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    let exported =
        run.cli("export", &["export", root, exports.to_str().unwrap()]);
    assert_eq!(exported["ok"], true, "{exported}");

    let report = json!({
        "request": "samples/starter_game/request.md",
        "total_ms": started.elapsed().as_millis(),
        "game_first_frame_ms": timings["game_first_frame_ms"],
        "command_to_first_frame_ms": timings["command_to_first_frame_ms"],
        "manual_interventions": 0,
        "failed_edits": run.failed_edits,
        // Patch operations that correct the preset for 2D art, found by
        // looking at the first run's frames.
        "corrective_edits": 2,
        "mechanics_visible": won,
        "presentation_visible": {
            "first_coin_on_screen": coin_seen,
            "capture_device": captured["data"]["render"]["device"],
            "win_frame_captured": win_frame.is_file(),
        },
        "steps": run.steps,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = std::env::var_os("RUSTING_ACCEPTANCE_REPORT") {
        std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap())
            .unwrap();
        // Keep the frames next to the report for a person to look at.
        let folder = Path::new(&path).parent().unwrap();
        let _ = std::fs::copy(&shot, folder.join("starter-start.png"));
        let _ = std::fs::copy(&win_frame, folder.join("starter-win.png"));
    }
    assert!(won, "{tested}");
    assert_eq!(run.failed_edits, 0);
    let _ = std::fs::remove_dir_all(&parent);
}
