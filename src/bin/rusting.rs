//! Agent-friendly window-free engine command line.

use std::path::Path;
use std::time::Duration;

use rusting_engine::cli::{self, CliResult, SceneFilter};
use rusting_engine::project::ProjectTemplate;
use rusting_engine::schema::OPERATIONS;
use uuid::Uuid;

fn help() -> String {
    let mut text = String::from(
        "rusting — RustingEngine project and scene tools\n\nUsage:\n",
    );
    for operation in OPERATIONS {
        text += &format!("  rusting {}\n", operation.usage);
    }
    text + "\nExit codes: 0 success, 1 validation or operation error, 2 usage error.\nJSON output has schema_version, ok, data, and diagnostics.\n"
}

fn help_for(command: &[String]) -> String {
    let topic = command
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    let topic = match topic.as_str() {
        "project" => "project inspect",
        "scene" => "scene inspect",
        "asset" => "asset import",
        "preset" => "preset apply",
        topic => topic,
    };
    match OPERATIONS.iter().find(|operation| operation.name == topic) {
        Some(operation) => format!("{}\n{}\n", help(), operation.summary),
        None => help(),
    }
}

fn usage(message: impl Into<String>) -> CliResult {
    CliResult::failure("CLI_USAGE", message, None)
}

/// Inserts `.` for a left-out project root, so `rusting check` works inside
/// a project. `extra` is how many positional arguments follow the root.
fn default_root(mut args: Vec<&str>) -> Vec<&str> {
    const COMMANDS: &[(&[&str], usize)] = &[
        (&["check"], 0),
        (&["validate"], 0),
        (&["cook"], 0),
        (&["run"], 0),
        (&["determinism"], 0),
        (&["project", "inspect"], 0),
        (&["asset", "list"], 0),
        (&["test"], 1),
    ];
    for (command, extra) in COMMANDS {
        if !args.starts_with(command) {
            continue;
        }
        let given = args[command.len()..]
            .iter()
            .take_while(|arg| !arg.starts_with("--"))
            .count();
        let project_only = *command == ["test"]
            && given == 1
            && Path::new(args[1]).join("project.json").is_file();
        if project_only {
            // `rusting test <project>` runs that project's tests folder.
            args.insert(2, "tests");
        } else if given == *extra {
            args.insert(command.len(), ".");
        } else if *command == ["test"] && given == 0 {
            args.splice(1..1, [".", "tests"]);
        }
        break;
    }
    args
}

fn execute(args: &[String]) -> CliResult {
    let positional = default_root(
        args.iter()
            .filter(|arg| arg.as_str() != "--json")
            .map(String::as_str)
            .collect(),
    );
    match positional.as_slice() {
        ["doctor"] => cli::doctor(),
        ["new", parent, name] => {
            cli::new_project(Path::new(parent), name, ProjectTemplate::Basic3d)
        }
        ["new", parent, name, "--template", template] => {
            match ProjectTemplate::parse(template) {
                Some(template) => {
                    cli::new_project(Path::new(parent), name, template)
                }
                None => usage(
                    "--template takes 3d, first-person, third-person, sandbox, 2d, or starter",
                ),
            }
        }
        ["project", "inspect", root] => cli::inspect_project(Path::new(root)),
        ["scene", "inspect", scene] => cli::inspect_scene(Path::new(scene)),
        ["scene", "query", scene] => {
            cli::query_scene(Path::new(scene), &SceneFilter::All)
        }
        ["scene", "query", scene, flag, value] => {
            let filter = match *flag {
                "--id" => match Uuid::parse_str(value) {
                    Ok(id) => SceneFilter::Id(id),
                    Err(_) => return usage("--id requires a valid UUID"),
                },
                "--name" => SceneFilter::Name((*value).to_owned()),
                "--class" => SceneFilter::Class((*value).to_owned()),
                "--component" => SceneFilter::Component((*value).to_owned()),
                _ => {
                    return usage(format!(
                        "unknown scene query filter `{flag}`"
                    ))
                }
            };
            cli::query_scene(Path::new(scene), &filter)
        }
        ["scene", "patch", scene, patch] => {
            cli::patch_scene(Path::new(scene), Path::new(patch), false)
        }
        ["scene", "patch", scene, patch, "--dry-run"]
        | ["scene", "patch", scene, "--dry-run", patch] => {
            cli::patch_scene(Path::new(scene), Path::new(patch), true)
        }
        ["determinism", root] => {
            cli::check_game_determinism(Path::new(root), 600)
        }
        ["determinism", root, "--ticks", ticks] => match ticks.parse() {
            Ok(ticks) => cli::check_game_determinism(Path::new(root), ticks),
            Err(_) => usage("--ticks requires a tick count"),
        },
        ["validate", root] => cli::validate_project(Path::new(root)),
        ["cook", root] => cli::cook_project(Path::new(root)),
        ["schema"] => CliResult::success(rusting_engine::schema::catalog()),
        ["check", root] => cli::check_project(Path::new(root)),
        ["run", root, flags @ ..] | ["test", root, _, flags @ ..] => {
            let mut options = cli::RunOptions::default();
            if let ["test", _, scenario, ..] = positional.as_slice() {
                // A scenario path is looked up in the project root first.
                let in_root = Path::new(root).join(scenario);
                options.scenario = Some(if in_root.exists() {
                    in_root
                } else {
                    Path::new(scenario).to_path_buf()
                });
            }
            let mut flags = flags.iter();
            while let Some(flag) = flags.next() {
                match *flag {
                    "--release" => options.release = true,
                    "--ticks" => match flags.next().map(|v| v.parse()) {
                        Some(Ok(ticks)) => options.headless_ticks = Some(ticks),
                        _ => return usage("--ticks requires a tick count"),
                    },
                    "--timeout" => match flags.next().map(|v| v.parse()) {
                        Some(Ok(seconds)) => {
                            options.timeout =
                                Some(Duration::from_secs_f64(seconds));
                        }
                        _ => return usage("--timeout requires seconds"),
                    },
                    _ => return usage(format!("unknown run flag `{flag}`")),
                }
            }
            match &options.scenario {
                Some(folder) if folder.is_dir() => {
                    let folder = folder.clone();
                    cli::test_game_folder(Path::new(root), &folder, options)
                }
                _ => cli::run_game_project(Path::new(root), options),
            }
        }
        ["export", root, parent] => {
            cli::export_game_project(Path::new(root), Path::new(parent), None)
        }
        ["export", root, parent, "--target", target] => {
            cli::export_game_project(
                Path::new(root),
                Path::new(parent),
                Some(target),
            )
        }
        ["preset", "list"] => cli::list_presets(),
        ["preset", "apply", scene, name] => {
            cli::apply_preset(Path::new(scene), name, false)
        }
        ["preset", "apply", scene, name, "--dry-run"] => {
            cli::apply_preset(Path::new(scene), name, true)
        }
        ["asset", "list", root] => cli::list_assets(Path::new(root)),
        ["asset", action @ ("import" | "reimport"), root, target, flags @ ..] =>
        {
            let mut options = cli::AssetImportFlags::default();
            let mut flags = flags.iter();
            while let Some(flag) = flags.next() {
                if *flag == "--dry-run" {
                    options.dry_run = true;
                    continue;
                }
                let Some(value) = flags.next().map(|value| (*value).to_owned())
                else {
                    return usage(format!("`{flag}` needs a value"));
                };
                let source = &mut options.provenance;
                match *flag {
                    "--to" if *action == "import" => {
                        options.folder = value.into()
                    }
                    "--from" if *action == "reimport" => {
                        options.from = Some(value.into());
                    }
                    "--author" => source.author = Some(value),
                    "--license" => source.license = Some(value),
                    "--url" => source.url = Some(value),
                    "--generator" => source.generator = Some(value),
                    "--notes" => source.notes = Some(value),
                    "--max-size" => match value.parse() {
                        Ok(size) => options.max_size = Some(size),
                        Err(_) => return usage("--max-size requires pixels"),
                    },
                    _ => {
                        return usage(format!(
                            "unknown asset {action} flag `{flag}`"
                        ))
                    }
                }
            }
            if *action == "import" {
                cli::import_asset(Path::new(root), Path::new(target), &options)
            } else {
                cli::reimport_asset(Path::new(root), target, &options)
            }
        }
        ["asset", "generate", root, hook, prompt, flags @ ..] => {
            let mut options = cli::AssetImportFlags::default();
            let mut replace = None;
            let mut flags = flags.iter();
            while let Some(flag) = flags.next() {
                match (*flag, flags.clone().next()) {
                    ("--dry-run", _) => {
                        options.dry_run = true;
                        continue;
                    }
                    ("--to", Some(folder)) => {
                        options.folder = folder.into();
                    }
                    ("--replace", Some(asset)) => replace = Some(*asset),
                    _ => {
                        return usage(format!(
                            "unknown or incomplete asset generate flag `{flag}`"
                        ))
                    }
                }
                flags.next();
            }
            cli::generate_asset(
                Path::new(root),
                hook,
                prompt,
                replace,
                &options,
            )
        }
        ["capture", scene, output, flags @ ..] => {
            let mut options = cli::CaptureOptions {
                camera: None,
                tick: 0,
                extent: [1280, 720],
                output: (*output).into(),
                picks: Vec::new(),
            };
            let pair = |value: &str, separator| {
                let (a, b) = value.split_once(separator)?;
                Some([a.parse().ok()?, b.parse().ok()?])
            };
            let mut flags = flags.iter();
            while let Some(flag) = flags.next() {
                let value = flags.next().copied();
                match (*flag, value) {
                    ("--camera", Some(camera)) => {
                        options.camera = Some(camera.to_owned());
                    }
                    ("--tick", Some(tick)) => match tick.parse() {
                        Ok(tick) => options.tick = tick,
                        Err(_) => {
                            return usage("--tick requires a tick number")
                        }
                    },
                    ("--size", Some(size)) => match pair(size, 'x') {
                        Some(extent) => options.extent = extent,
                        None => return usage("--size requires WIDTHxHEIGHT"),
                    },
                    ("--pick", Some(pixel)) => match pair(pixel, ',') {
                        Some(pixel) => options.picks.push(pixel),
                        None => return usage("--pick requires X,Y"),
                    },
                    _ => {
                        return usage(format!(
                            "unknown or incomplete capture flag `{flag}`"
                        ))
                    }
                }
            }
            cli::capture_scene(Path::new(scene), &options)
        }
        _ => usage("invalid command or arguments; run `rusting --help`"),
    }
}

fn render_human(result: &CliResult) -> String {
    let mut lines = Vec::new();
    for run in result
        .data
        .get("scenarios")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        lines.push(format!(
            "{} {}: {}",
            if run["ok"] == true { "PASS" } else { "FAIL" },
            run["file"].as_str().unwrap_or("?"),
            run["message"].as_str().unwrap_or("")
        ));
    }
    if result.ok {
        let data = &result.data;
        if let Some(version) = data.get("engine_version") {
            lines.push(format!("RustingEngine {version}"));
        }
        if let Some(root) = data.get("root").and_then(|v| v.as_str()) {
            lines.push(format!("Project: {root}"));
        }
        if let Some(path) = data.get("path").and_then(|v| v.as_str()) {
            lines.push(format!("Scene: {path}"));
        }
        for (key, label) in [
            ("binary_name", "Binary"),
            ("manifest_path", "Manifest"),
            ("main_scene", "Main scene"),
            ("cargo_manifest", "Cargo manifest"),
            ("code_path", "Code"),
        ] {
            if let Some(value) = data.get(key).and_then(|v| v.as_str()) {
                lines.push(format!("{label}: {value}"));
            }
        }
        if let Some(version) = data.get("scene_version") {
            lines.push(format!("Scene version: {version}"));
        }
        if let Some(count) = data.get("entity_count") {
            lines.push(format!("Entities: {count}"));
        }
        if let Some(count) = data.get("count") {
            lines.push(format!("Matches: {count}"));
        }
        if let Some(entities) = data.get("entities").and_then(|v| v.as_array())
        {
            for entity in entities {
                lines.push(format!(
                    "{} {}",
                    entity["id"].as_str().unwrap_or("?"),
                    entity["name"].as_str().unwrap_or("(unnamed)")
                ));
            }
        }
        if let Some(revision) = data.get("revision").and_then(|v| v.as_str()) {
            lines.push(format!("Revision: {revision}"));
        }
        if let Some(patch) = data.get("patch") {
            lines.push(format!(
                "{} {} -> {}: {} created, {} deleted, {} field changes",
                if patch["written"] == true {
                    "Patched"
                } else {
                    "Dry run"
                },
                patch["revision_before"].as_str().unwrap_or(""),
                patch["revision_after"].as_str().unwrap_or(""),
                patch["created"].as_array().map_or(0, Vec::len),
                patch["deleted"].as_array().map_or(0, Vec::len),
                patch["changes"].as_array().map_or(0, Vec::len),
            ));
            for change in patch["changes"].as_array().into_iter().flatten() {
                lines.push(format!(
                    "  {}{}: {} -> {}",
                    match (change["name"].as_str(), change["id"].as_str()) {
                        (Some(name), _) => name,
                        (None, Some(id)) if id == Uuid::nil().to_string() => {
                            "(scene)"
                        }
                        (None, id) => id.unwrap_or("?"),
                    },
                    change["path"].as_str().unwrap_or(""),
                    change["before"],
                    change["after"]
                ));
            }
        }
        if let Some(path) = data.get("export_path").and_then(|v| v.as_str()) {
            lines.push(format!("Exported: {path}"));
        }
        if let Some(game) = data.get("game").or(data.get("verified")) {
            if !game.is_null() {
                lines.push(format!(
                    "Game exit code: {}, timed out: {}",
                    game["exit_code"], game["timed_out"]
                ));
            }
        }
        if let Some(path) = data.get("final_scene").and_then(|v| v.as_str()) {
            lines.push(format!(
                "Final state: {path} (inspect with `rusting scene query`)"
            ));
        }
        if let Some(operations) =
            data.get("operations").and_then(|v| v.as_array())
        {
            lines.push(format!(
                "Schema catalog {}: {} operations; use --json for the full catalog",
                data["catalog_version"],
                operations.len()
            ));
            for operation in operations {
                lines.push(format!(
                    "  {}  [GPU: {}]",
                    operation["usage"].as_str().unwrap_or(""),
                    operation["gpu_cost"].as_str().unwrap_or("")
                ));
            }
        }
        if let Some(scenario) =
            data.get("scenario").filter(|v| v.get("passed").is_some())
        {
            lines.push(format!(
                "Scenario {}: {} after {} ticks, seed {}",
                scenario["name"],
                if scenario["passed"] == true {
                    "passed"
                } else {
                    "failed"
                },
                scenario["ticks_run"],
                scenario["seed"]
            ));
        }
        if let Some(picks) = data.get("picks").and_then(|v| v.as_array()) {
            for pick in picks {
                lines.push(format!(
                    "Pick {}: {} {}",
                    pick["pixel"],
                    pick["id"].as_str().unwrap_or("(nothing)"),
                    pick["name"].as_str().unwrap_or("")
                ));
            }
        }
        if let Some(output) = data.get("output").and_then(|v| v.as_str()) {
            lines.push(format!("Output: {output}"));
        }
        if let Some(vulkan) = data.get("vulkan") {
            lines.push(format!("Vulkan available: {}", vulkan["available"]));
            if let Some(devices) = vulkan["devices"].as_array() {
                for device in devices {
                    lines.push(format!(
                        "Device: {}",
                        device.as_str().unwrap_or("?")
                    ));
                }
            }
        }
        if let Some(tools) = data.get("build_tools").and_then(|v| v.as_object())
        {
            for (name, available) in tools {
                lines.push(format!("{name}: {available}"));
            }
        }
        for (key, label) in
            [("classes", "Classes"), ("referenced_assets", "Assets")]
        {
            if let Some(items) = data.get(key).and_then(|v| v.as_array()) {
                lines.push(format!(
                    "{label}: {}",
                    items
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        for asset in data
            .get("assets")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .chain(data.get("asset"))
        {
            lines.push(format!(
                "Asset: {} ({}, license {}){}",
                asset["path"].as_str().unwrap_or("?"),
                asset["id"].as_str().unwrap_or("?"),
                asset["source"]["license"].as_str().unwrap_or("none"),
                if asset["dry_run"] == true {
                    " [preview, nothing written]"
                } else {
                    ""
                }
            ));
        }
        if let Some(cameras) = data.get("cameras").and_then(|v| v.as_array()) {
            for camera in cameras {
                lines.push(format!(
                    "Camera: {} ({})",
                    camera["name"].as_str().unwrap_or("unnamed"),
                    camera["id"].as_str().unwrap_or("?")
                ));
            }
        }
        if lines.is_empty() {
            lines.push("OK".to_owned());
        }
    }
    for diagnostic in &result.diagnostics {
        lines.push(format!(
            "{} [{}]: {}{}",
            diagnostic.severity,
            diagnostic.code,
            diagnostic.message,
            diagnostic
                .file
                .as_ref()
                .map(|path| format!(" ({})", path.display()))
                .unwrap_or_default()
        ));
    }
    lines.join("\n")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h")
    {
        print!("{}", help_for(&args));
        return;
    }
    let json = args.iter().any(|arg| arg == "--json");
    let result = execute(&args);
    if json {
        println!(
            "{}",
            serde_json::to_string(&result).expect("serializable CLI result")
        );
    } else if result.ok {
        println!("{}", render_human(&result));
    } else {
        eprintln!("{}", render_human(&result));
    }
    std::process::exit(result.exit_code());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_has_json_error_and_exit_code() {
        let result = execute(&[
            "scene".into(),
            "query".into(),
            "missing.rscene".into(),
            "--id".into(),
            "bad".into(),
            "--json".into(),
        ]);
        assert_eq!(result.exit_code(), 2);
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["diagnostics"][0]["code"], "CLI_USAGE");
    }

    #[test]
    fn project_root_defaults_to_the_current_folder() {
        assert_eq!(default_root(vec!["check"]), ["check", "."]);
        assert_eq!(
            default_root(vec!["run", "--ticks", "5"]),
            ["run", ".", "--ticks", "5"]
        );
        assert_eq!(default_root(vec!["run", "game"]), ["run", "game"]);
        assert_eq!(
            default_root(vec!["test", "a.json"]),
            ["test", ".", "a.json"]
        );
        assert_eq!(
            default_root(vec!["test", "game", "a.json"]),
            ["test", "game", "a.json"]
        );
        assert_eq!(
            default_root(vec!["test", "samples/putt_course"]),
            ["test", "samples/putt_course", "tests"]
        );
        assert_eq!(
            default_root(vec!["test", "--release"]),
            ["test", ".", "tests", "--release"]
        );
        assert_eq!(
            default_root(vec!["project", "inspect"]),
            ["project", "inspect", "."]
        );
    }
}
