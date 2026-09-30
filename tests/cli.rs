use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

fn run(args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

fn temporary_parent() -> PathBuf {
    let path = std::env::temp_dir()
        .join(format!("rusting-cli-test-{}", Uuid::new_v4()));
    std::fs::create_dir(&path).unwrap();
    path
}

fn generated_project(parent: &Path) -> PathBuf {
    let (output, result) = run(&["new", parent.to_str().unwrap(), "CLI Test"]);
    assert!(output.status.success(), "{result}");
    parent.join("CLI Test")
}

#[test]
fn doctor_reports_capabilities_without_requiring_a_gpu() {
    let (output, result) = run(&["doctor"]);
    assert!(output.status.success(), "{result}");
    assert!(result["data"]["engine_version"].is_string());
    assert!(result["data"]["platform"]["os"].is_string());
    assert!(result["data"]["build_tools"]["cargo"].is_boolean());
    assert!(result["data"]["vulkan"]["available"].is_boolean());
}

#[test]
fn repository_project_inspects_validates_and_cooks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testGame");
    let root = root.to_str().unwrap();
    let (output, inspected) = run(&["project", "inspect", root]);
    assert!(output.status.success(), "{inspected}");
    assert_eq!(inspected["data"]["binary_name"], "project1");
    let (output, scene) =
        run(&["scene", "inspect", &format!("{root}/scenes/main.rscene")]);
    assert!(output.status.success(), "{scene}");
    assert!(scene["data"]["entity_count"].as_u64().unwrap() > 0);
    assert_eq!(scene["data"]["scene_version"], 3);
    assert_eq!(
        scene["data"]["loaded_scene_version"],
        rusting_engine::runtime::SCENE_FORMAT_VERSION
    );
    let (output, validated) = run(&["validate", root]);
    assert!(output.status.success(), "{validated}");
}

#[test]
fn generated_project_queries_in_persistent_id_order() {
    let parent = temporary_parent();
    let root = generated_project(&parent);
    let scene = root.join("scenes/main.rscene");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&scene).unwrap()).unwrap();
    document["entities"].as_array_mut().unwrap().reverse();
    std::fs::write(&scene, serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    let path = scene.to_str().unwrap();
    let (output, queried) = run(&["scene", "query", path]);
    assert!(output.status.success(), "{queried}");
    let ids: Vec<_> = queried["data"]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entity| entity["id"].as_str().unwrap())
        .collect();
    assert!(ids.windows(2).all(|pair| pair[0] <= pair[1]));
    let id = ids[0];
    let (output, filtered) = run(&["scene", "query", path, "--id", id]);
    assert!(output.status.success(), "{filtered}");
    assert_eq!(filtered["data"]["count"], 1);
    assert!(filtered["data"]["entities"][0]["transform"].is_object());
    let (output, validated) = run(&["validate", root.to_str().unwrap()]);
    assert!(output.status.success(), "{validated}");
    let (output, cooked) = run(&["cook", root.to_str().unwrap()]);
    assert!(output.status.success(), "{cooked}");
    assert!(Path::new(cooked["data"]["output"].as_str().unwrap()).is_file());
    std::fs::remove_dir_all(parent).unwrap();
}

#[test]
fn scene_patch_dry_runs_writes_and_reports_revision_conflicts() {
    let parent = temporary_parent();
    let root = generated_project(&parent);
    let scene = root.join("scenes/main.rscene");
    let path = scene.to_str().unwrap();
    let (_, inspected) = run(&["scene", "inspect", path]);
    let revision = inspected["data"]["revision"].as_str().unwrap().to_owned();
    let (_, queried) = run(&["scene", "query", path]);
    let id = queried["data"]["entities"][0]["id"].as_str().unwrap();
    let patch_path = parent.join("patch.json");
    let patch = serde_json::json!({"expected_revision": revision, "operations": [
        {"op": "set", "id": id, "path": "/name", "value": "Patched"}]});
    std::fs::write(&patch_path, patch.to_string()).unwrap();
    let patch_arg = patch_path.to_str().unwrap();

    let original = std::fs::read(&scene).unwrap();
    let (output, dry) = run(&["scene", "patch", path, patch_arg, "--dry-run"]);
    assert!(output.status.success(), "{dry}");
    assert_eq!(dry["data"]["patch"]["written"], false);
    assert_eq!(dry["data"]["patch"]["changes"][0]["path"], "/name");
    assert_eq!(dry["data"]["patch"]["changes"][0]["after"], "Patched");
    assert_eq!(std::fs::read(&scene).unwrap(), original);

    let (output, applied) = run(&["scene", "patch", path, patch_arg]);
    assert!(output.status.success(), "{applied}");
    let (_, renamed) = run(&["scene", "query", path, "--name", "Patched"]);
    assert_eq!(renamed["data"]["count"], 1);
    assert_eq!(
        renamed["data"]["revision"],
        applied["data"]["patch"]["revision_after"]
    );

    // The same patch now targets a stale revision.
    let (output, conflict) = run(&["scene", "patch", path, patch_arg]);
    assert_eq!(output.status.code(), Some(1), "{conflict}");
    assert_eq!(conflict["diagnostics"][0]["code"], "SCENE_CONFLICT");
    assert_eq!(
        conflict["data"]["revision"],
        applied["data"]["patch"]["revision_after"]
    );
    std::fs::remove_dir_all(parent).unwrap();
}

#[test]
fn malformed_and_missing_inputs_have_json_diagnostics_and_nonzero_exit() {
    let parent = temporary_parent();
    let root = generated_project(&parent);
    let manifest = root.join("project.json");
    std::fs::write(&manifest, "{").unwrap();
    let (output, result) = run(&["validate", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["diagnostics"][0]["code"], "PROJECT_MANIFEST_JSON");
    std::fs::write(&manifest, serde_json::to_vec(&serde_json::json!({"format_version":1,"name":"CLI Test","main_scene":"scenes/main.rscene","cooked_scene":"build/main.rscene.bin","binary_name":"cli_test"})).unwrap()).unwrap();
    let scene = root.join("scenes/main.rscene");
    std::fs::write(&scene, "{").unwrap();
    let (output, result) = run(&["scene", "inspect", scene.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["diagnostics"][0]["code"], "SCENE_JSON");
    let duplicate = serde_json::json!({"format_version":6,"name":"Broken","entities":[
        {"id":Uuid::nil(),"parent":null,"name":"A","transform":null,"mesh_renderer":null,"camera":null,"visible":null},
        {"id":Uuid::nil(),"parent":null,"name":"B","transform":null,"mesh_renderer":null,"camera":null,"visible":null}
    ]});
    std::fs::write(&scene, serde_json::to_vec(&duplicate).unwrap()).unwrap();
    let (output, result) = run(&["scene", "inspect", scene.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["diagnostics"][0]["code"], "SCENE_STRUCTURE");
    std::fs::remove_file(&scene).unwrap();
    let (output, result) = run(&["validate", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["diagnostics"][0]["code"], "PROJECT_MISSING_FILE");
    let (output, result) =
        run(&["scene", "query", "missing.rscene", "--id", "bad"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(result["diagnostics"][0]["code"], "CLI_USAGE");
    std::fs::remove_dir_all(parent).unwrap();
}

#[test]
fn missing_asset_reference_fails_validation_with_location() {
    let parent = temporary_parent();
    let root = generated_project(&parent);
    let scene = root.join("scenes/main.rscene");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&scene).unwrap()).unwrap();
    document["entities"][0]["mesh_renderer"]["mesh"] =
        serde_json::json!({"AssetPath":"../assets/missing.rmesh"});
    std::fs::write(&scene, serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    let (output, result) = run(&["validate", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(result["diagnostics"][0]["code"], "SCENE_MISSING_ASSET");
    assert!(result["diagnostics"][0]["scene_location"]
        .as_str()
        .unwrap()
        .starts_with("/entities/"));
    std::fs::remove_dir_all(parent).unwrap();
}

#[test]
fn run_and_export_reject_malformed_flags_as_usage_errors() {
    for args in [
        &["run", "project", "--ticks"][..],
        &["run", "project", "--ticks", "many"],
        &["run", "project", "--fast"],
        &["export", "project"],
        &["export", "project", "out", "--target"],
    ] {
        let (output, result) = run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {result}");
        assert_eq!(result["diagnostics"][0]["code"], "CLI_USAGE");
    }
}

/// Builds a generated project three times with cargo, so it is slow. Run it
/// with `cargo test --test cli -- --ignored`.
#[test]
#[ignore = "runs real cargo builds of a generated project"]
fn generated_project_checks_runs_and_exports_a_verified_game() {
    let parent = temporary_parent();
    let root = generated_project(&parent);
    let root = root.to_str().unwrap();
    // Share one target folder across runs so later runs reuse the build.
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/cli-games");
    let cli = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
            .args(args)
            .arg("--json")
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        (output, value)
    };

    let (output, checked) = cli(&["check", root]);
    assert!(output.status.success(), "{checked}");

    let (output, ran) = cli(&["run", root, "--ticks", "30"]);
    assert!(output.status.success(), "{ran}");
    assert_eq!(ran["data"]["game"]["exit_code"], 0);
    assert_eq!(ran["data"]["game"]["timed_out"], false);
    let timings = &ran["data"]["timings"];
    assert!(timings["game_first_frame_ms"].is_u64(), "{timings}");
    assert!(
        timings["command_to_first_frame_ms"].as_u64()
            >= timings["game_first_frame_ms"].as_u64()
    );

    let exports = parent.join("exports");
    std::fs::create_dir(&exports).unwrap();
    let (output, exported) = cli(&["export", root, exports.to_str().unwrap()]);
    assert!(output.status.success(), "{exported}");
    assert_eq!(exported["data"]["verified"]["exit_code"], 0);
    let files = exported["data"]["files"].as_array().unwrap();
    assert!(files
        .iter()
        .any(|file| file.as_str().unwrap().ends_with(".rscene.bin")));

    // A Rust error fails the build with the file and line attached.
    let (_, project) = cli(&["project", "inspect", root]);
    let code = project["data"]["code_path"].as_str().unwrap().to_owned();
    let source = std::fs::read_to_string(&code).unwrap();
    std::fs::write(
        &code,
        format!("{source}\nfn broken() {{ missing_value }}\n"),
    )
    .unwrap();
    let (output, failed) = cli(&["run", root, "--ticks", "1"]);
    assert_eq!(output.status.code(), Some(1), "{failed}");
    let rust = failed["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "RUST_BUILD_ERROR")
        .unwrap_or_else(|| panic!("no Rust diagnostic: {failed}"));
    assert!(Path::new(rust["file"].as_str().unwrap()).ends_with("src/main.rs"));
    assert!(rust["message"].as_str().unwrap().contains("missing_value"));
    std::fs::write(&code, source).unwrap();

    // A broken game fails the run with the game's exit reported.
    std::fs::write(Path::new(root).join("scenes/main.rscene"), "not a scene")
        .unwrap();
    let (output, broken) = cli(&["run", root, "--ticks", "30"]);
    assert_eq!(output.status.code(), Some(1), "{broken}");
    let _ = std::fs::remove_dir_all(&parent);
}

/// Runs scenarios inside a real game build. Ignored like the test above.
#[test]
#[ignore = "runs real cargo builds of a generated project"]
fn generated_game_passes_and_fails_scenarios_with_a_tick_trace() {
    let parent = temporary_parent();
    let (scene, cube) = falling_cube_scene(&parent);
    let root = scene.parent().unwrap().parent().unwrap();
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/cli-games");
    let test = |steps: Value| {
        let file = parent.join("scenario.json");
        let scenario = serde_json::json!({
            "name": "falls", "seed": 3, "ticks": 60,
            "capture_size": [64, 36], "steps": steps
        });
        std::fs::write(&file, scenario.to_string()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
            .args(["test", root.to_str().unwrap(), file.to_str().unwrap()])
            .arg("--json")
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        (output, value)
    };

    let (output, passed) = test(serde_json::json!([
        {"tick": 30, "expect": {"entity": cube,
            "path": "/transform/position/1", "less_than": 0.0}},
        {"tick": 30, "capture": "shots/tick30.png"},
    ]));
    assert!(output.status.success(), "{passed}");
    let report = &passed["data"]["scenario"];
    assert_eq!(report["passed"], true);
    assert_eq!(report["ticks_run"], 60);

    let (output, failed) = test(serde_json::json!([
        {"tick": 0, "until": 60, "expect": {"entity": "Cube",
            "path": "/transform/position/1", "greater_than": -0.5}},
    ]));
    assert_eq!(output.status.code(), Some(1), "{failed}");
    assert_eq!(failed["diagnostics"][0]["code"], "SCENARIO_FAILED");
    let tick = failed["data"]["scenario"]["first_failure"]["tick"]
        .as_u64()
        .unwrap();
    assert!(tick > 0 && tick < 60, "{failed}");
    let _ = std::fs::remove_dir_all(&parent);
}

/// A generated project whose cube falls under CPU physics.
fn falling_cube_scene(parent: &Path) -> (PathBuf, String) {
    let scene = generated_project(parent).join("scenes/main.rscene");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&scene).unwrap()).unwrap();
    let cube = document["entities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entity| entity["name"] == "Cube")
        .unwrap();
    cube["physics_body"] = serde_json::json!({
        "simulation": "Cpu", "solver": "Full", "custom_shader": null
    });
    cube["rigid_body"] = serde_json::json!({
        "kind": "Dynamic", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0],
        "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0
    });
    cube["collider"] = serde_json::json!({
        "shape": {"Box": {"half_extents": [0.5, 0.5, 0.5]}},
        "friction": 0.5, "restitution": 0.0, "sensor": false
    });
    let id = cube["id"].as_str().unwrap().to_owned();
    std::fs::write(&scene, serde_json::to_vec_pretty(&document).unwrap())
        .unwrap();
    (scene, id)
}

fn capture(args: &[&str], device: Option<&str>) -> (Output, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rusting"));
    command.arg("capture").args(args).arg("--json");
    if let Some(device) = device {
        command.env("RUSTING_VULKAN_DEVICE", device);
    }
    let output = command.output().unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

#[test]
fn capture_without_vulkan_still_reports_camera_and_picks_per_tick() {
    let parent = temporary_parent();
    let (scene, cube) = falling_cube_scene(&parent);
    let scene = scene.to_str().unwrap();
    let png = parent.join("shot.png");
    let png = png.to_str().unwrap();
    let pick = |tick: &str| {
        let (output, result) = capture(
            &[
                scene, png, "--size", "320x180", "--pick", "160,150", "--tick",
                tick,
            ],
            Some("no-such-vulkan-device"),
        );
        assert_eq!(output.status.code(), Some(1), "{result}");
        assert_eq!(result["diagnostics"][0]["code"], "VULKAN_UNAVAILABLE");
        assert_eq!(result["data"]["camera"]["name"], "Game Camera");
        assert!(result["data"]["output"].is_null());
        assert!(result["data"]["picks"][0]["color"].is_null());
        result["data"]["picks"][0]["id"].clone()
    };
    // The cube is under the pixel at the start and has fallen away a
    // second later.
    assert_eq!(pick("0"), cube.as_str());
    assert!(pick("60").is_null());
    assert!(!Path::new(png).exists());

    let (output, missing) = capture(
        &[scene, png, "--camera", "Nobody"],
        Some("no-such-vulkan-device"),
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(missing["diagnostics"][0]["code"], "CAMERA_NOT_FOUND");
    let (output, outside) = capture(&[scene, png, "--pick", "5000,1"], None);
    assert_eq!(output.status.code(), Some(2), "{outside}");
    let _ = std::fs::remove_dir_all(&parent);
}

#[cfg(feature = "gpu-tests")]
#[test]
fn capture_renders_a_png_and_maps_pixels_to_scene_ids() {
    let parent = temporary_parent();
    let (scene, cube) = falling_cube_scene(&parent);
    let scene = scene.to_str().unwrap();
    let png = parent.join("shot.png");
    let (output, result) = capture(
        &[
            scene,
            png.to_str().unwrap(),
            "--size",
            "320x180",
            "--camera",
            "Game Camera",
            "--pick",
            "160,150",
            "--pick",
            "10,10",
        ],
        None,
    );
    assert!(output.status.success(), "{result}");
    let picks = &result["data"]["picks"];
    assert_eq!(picks[0]["id"], cube.as_str());
    assert!(picks[1]["id"].is_null());
    assert_ne!(picks[0]["color"], picks[1]["color"]);
    assert!(result["data"]["render"]["draws"].as_u64().unwrap() > 0);
    let image = image::open(&png).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (320, 180));
    assert_eq!(
        image.get_pixel(160, 150).0.to_vec(),
        picks[0]["color"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() as u8)
            .collect::<Vec<_>>()
    );
    // The camera's persistent ID selects the same camera as its name.
    let id = result["data"]["camera"]["id"].as_str().unwrap();
    let (output, by_id) =
        capture(&[scene, png.to_str().unwrap(), "--camera", id], None);
    assert!(output.status.success(), "{by_id}");
    assert_eq!(by_id["data"]["camera"]["name"], "Game Camera");
    let _ = std::fs::remove_dir_all(&parent);
}

#[test]
fn schema_catalog_examples_parse_and_help_lists_every_operation() {
    let (output, schema) = run(&["schema"]);
    assert!(output.status.success(), "{schema}");
    let operations = schema["data"]["operations"].as_array().unwrap();
    let help = Command::new(env!("CARGO_BIN_EXE_rusting"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8(help.stdout).unwrap();
    for operation in operations {
        let usage = operation["usage"].as_str().unwrap();
        assert!(help.contains(usage), "help misses `{usage}`");
        // In an empty folder, a valid example fails on the missing project
        // at worst (exit 1); only an argument error exits 2.
        let example = operation["example"].as_str().unwrap();
        let folder = temporary_parent();
        let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
            .args(example.split_whitespace().skip(1))
            .current_dir(&folder)
            .output()
            .unwrap();
        assert_ne!(output.status.code(), Some(2), "`{example}` is rejected");
        let _ = std::fs::remove_dir_all(&folder);
    }
}

#[test]
fn assets_import_list_and_reimport_with_a_stable_id() {
    let parent = temporary_parent();
    let project = generated_project(&parent);
    let root = project.to_str().unwrap();
    let art = parent.join("crate.png");
    image::RgbaImage::from_pixel(4, 2, image::Rgba([9, 9, 9, 255]))
        .save(&art)
        .unwrap();

    let (output, imported) = run(&[
        "asset",
        "import",
        root,
        art.to_str().unwrap(),
        "--to",
        "props",
        "--author",
        "Ada",
    ]);
    assert!(output.status.success(), "{imported}");
    let asset = &imported["data"]["asset"];
    assert_eq!(asset["path"], "assets/props/crate.png");
    assert_eq!(asset["size"], serde_json::json!([4, 2]));
    assert_eq!(imported["diagnostics"][0]["code"], "ASSET_NO_LICENSE");
    let id = asset["id"].as_str().unwrap();

    let (output, listed) = run(&["asset", "list", root]);
    assert!(output.status.success(), "{listed}");
    assert_eq!(listed["data"]["assets"][0]["id"], id);

    let (output, replaced) = run(&[
        "asset",
        "reimport",
        root,
        id,
        "--license",
        "CC0-1.0",
        "--max-size",
        "2",
    ]);
    assert!(output.status.success(), "{replaced}");
    assert_eq!(replaced["data"]["asset"]["id"], id);
    assert_eq!(replaced["data"]["asset"]["size"], serde_json::json!([2, 1]));
    assert_eq!(replaced["data"]["asset"]["source"]["author"], "Ada");
    assert!(replaced["diagnostics"].as_array().unwrap().is_empty());

    std::fs::remove_file(project.join("assets/props/crate.png")).unwrap();
    let (output, broken) = run(&["asset", "list", root]);
    assert_eq!(output.status.code(), Some(1), "{broken}");
    assert_eq!(broken["diagnostics"][0]["code"], "ASSET_MISSING");
    let (output, invalid) = run(&["validate", root]);
    assert_eq!(output.status.code(), Some(1), "{invalid}");
    assert_eq!(invalid["diagnostics"][0]["code"], "ASSET_MISSING");
    let (output, usage) = run(&["asset", "import", root, "x.png", "--to"]);
    assert_eq!(output.status.code(), Some(2), "{usage}");
    std::fs::remove_dir_all(parent).unwrap();
}

#[cfg(unix)]
#[test]
fn generator_hooks_preview_and_import_through_the_cli() {
    let parent = temporary_parent();
    let project = generated_project(&parent);
    let root = project.to_str().unwrap();
    let art = parent.join("made.png");
    image::RgbaImage::new(8, 8).save(&art).unwrap();
    let manifest_path = project.join("project.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap())
            .unwrap();
    manifest["generators"] = serde_json::json!({"stub": {
        "command": ["sh", "-c",
            "cp \"$0\" \"$RUSTING_OUTPUT_DIR/icon.png\" && echo '{\"file\": \"icon.png\", \"author\": \"stub\"}'",
            art],
        "license": "CC0-1.0"
    }});
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();

    let (output, preview) = run(&[
        "asset",
        "generate",
        root,
        "stub",
        "a red crate",
        "--to",
        "ui",
        "--dry-run",
    ]);
    assert!(output.status.success(), "{preview}");
    assert_eq!(preview["data"]["asset"]["dry_run"], true);
    assert!(!project.join("assets/ui").exists());

    let (output, made) = run(&[
        "asset",
        "generate",
        root,
        "stub",
        "a red crate",
        "--to",
        "ui",
    ]);
    assert!(output.status.success(), "{made}");
    let asset = &made["data"]["asset"];
    assert_eq!(asset["path"], "assets/ui/icon.png");
    assert_eq!(asset["source"]["original"], "generator:stub");
    assert_eq!(asset["source"]["notes"], "prompt: a red crate");
    assert!(project.join("assets/ui/icon.png.rmeta").is_file());

    let (output, preview) = run(&[
        "asset",
        "reimport",
        root,
        "assets/ui/icon.png",
        "--from",
        art.to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(output.status.success(), "{preview}");
    assert_eq!(preview["data"]["asset"]["id"], asset["id"]);

    let (output, unknown) = run(&["asset", "generate", root, "missing", "x"]);
    assert_eq!(output.status.code(), Some(1), "{unknown}");
    assert_eq!(unknown["diagnostics"][0]["code"], "GENERATOR_UNKNOWN");
    std::fs::remove_dir_all(parent).unwrap();
}

#[test]
fn art_presets_list_and_apply_as_scene_patches() {
    let (output, listed) = run(&["preset", "list"]);
    assert!(output.status.success(), "{listed}");
    let names: Vec<_> = listed["data"]["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|preset| preset["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"golden_hour".to_owned()), "{names:?}");

    let parent = temporary_parent();
    let project = generated_project(&parent);
    let scene = project.join("scenes/main.rscene");
    let scene = scene.to_str().unwrap();
    let (output, dry) =
        run(&["preset", "apply", scene, "golden_hour", "--dry-run"]);
    assert!(output.status.success(), "{dry}");
    assert_eq!(dry["data"]["patch"]["written"], false);
    assert_eq!(dry["data"]["patch"]["created"].as_array().unwrap().len(), 1);
    let (output, applied) = run(&["preset", "apply", scene, "golden_hour"]);
    assert!(output.status.success(), "{applied}");
    let (_, found) = run(&["scene", "query", scene, "--name", "Sun"]);
    assert_eq!(found["data"]["count"], 1, "{found}");
    let (output, unknown) = run(&["preset", "apply", scene, "neon"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(unknown["diagnostics"][0]["code"], "PRESET_UNKNOWN");
    std::fs::remove_dir_all(parent).unwrap();
}

#[test]
fn validate_names_parts_below_the_project_determinism_mode() {
    let parent = temporary_parent();
    let root = generated_project(&parent);
    let manifest_path = root.join("project.json");
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap())
            .unwrap();
    let scene = root.join("scenes/main.rscene");
    let mut document: Value =
        serde_json::from_slice(&std::fs::read(&scene).unwrap()).unwrap();
    document["entities"][0]["physics_body"] = serde_json::json!(
        {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null}
    );
    std::fs::write(&scene, document.to_string()).unwrap();
    let path = root.to_str().unwrap();
    for (mode, ok) in [("Local", true), ("CrossPlatform", false)] {
        manifest["determinism"] = mode.into();
        std::fs::write(&manifest_path, manifest.to_string()).unwrap();
        let (output, validated) = run(&["validate", path]);
        assert_eq!(output.status.success(), ok, "{validated}");
        if !ok {
            let diagnostic = &validated["diagnostics"][0];
            assert_eq!(diagnostic["code"], "DETERMINISM_UNSUPPORTED");
            assert!(diagnostic["message"]
                .as_str()
                .unwrap()
                .contains("cpu_physics"));
        }
    }
    std::fs::remove_dir_all(parent).unwrap();
}
