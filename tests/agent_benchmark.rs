//! The agent benchmark task suite in `benchmarks/agent/` (see its README).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Task {
    kind: String,
    template: String,
    request: String,
    seed: Vec<Edit>,
    remove: Vec<String>,
    reference: Vec<Edit>,
    hidden: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    file: String,
    find: String,
    replace: String,
}

fn tasks() -> Vec<(PathBuf, Task)> {
    let suite = Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/agent");
    let mut tasks: Vec<_> = std::fs::read_dir(&suite)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("task.json").is_file())
        .map(|folder| {
            let text =
                std::fs::read_to_string(folder.join("task.json")).unwrap();
            let task = serde_json::from_str(&text).unwrap_or_else(|error| {
                panic!("{}: {error}", folder.display())
            });
            (folder, task)
        })
        .collect();
    tasks.sort_by(|a, b| a.0.cmp(&b.0));
    tasks
}

fn rusting(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Applies `edits` in order; each `find` must occur exactly once.
fn apply(root: &Path, edits: &[Edit], task: &Path) {
    for edit in edits {
        let path = root.join(&edit.file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text.matches(&edit.find).count(),
            1,
            "{}: `{}` must occur once in {}",
            task.display(),
            edit.find,
            edit.file
        );
        std::fs::write(&path, text.replacen(&edit.find, &edit.replace, 1))
            .unwrap();
    }
}

/// The task's starting project: its template with the seed applied.
fn seeded(folder: &Path, task: &Task) -> (PathBuf, PathBuf) {
    let parent = std::env::temp_dir()
        .join(format!("rusting-agent-bench-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&parent).unwrap();
    let created = rusting(&[
        "new",
        parent.to_str().unwrap(),
        "Task",
        "--template",
        &task.template,
    ]);
    assert_eq!(created["ok"], true, "{}: {created}", folder.display());
    let root = parent.join("Task");
    apply(&root, &task.seed, folder);
    for file in &task.remove {
        std::fs::remove_file(root.join(file)).unwrap();
    }
    (parent, root)
}

#[test]
fn every_task_is_well_formed_and_applies_to_its_template() {
    let tasks = tasks();
    assert!(tasks.len() >= 3);
    for (folder, task) in &tasks {
        let name = folder.display();
        assert!(
            ["new-game", "feature", "bug-fix", "performance"]
                .contains(&task.kind.as_str()),
            "{name}: unknown kind `{}`",
            task.kind
        );
        assert!(
            task.request.len() > 40,
            "{name}: the request is a paragraph"
        );
        assert!(!task.hidden.is_empty(), "{name}: needs a hidden scenario");
        for hidden in &task.hidden {
            let text = std::fs::read_to_string(folder.join(hidden))
                .unwrap_or_else(|error| panic!("{name}: {hidden}: {error}"));
            serde_json::from_str::<rusting_engine::scenario::Scenario>(&text)
                .unwrap_or_else(|error| panic!("{name}: {hidden}: {error}"));
        }
        let (parent, root) = seeded(folder, task);
        apply(&root, &task.reference, folder);
        std::fs::remove_dir_all(parent).unwrap();
    }
}

/// Builds every task's game twice, so it takes minutes.
#[test]
#[ignore = "builds each task's game twice; run with --ignored"]
fn hidden_scenarios_fail_when_seeded_and_pass_with_the_reference() {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/cli-games");
    let test = |root: &Path, scenario: &Path| {
        let output = Command::new(env!("CARGO_BIN_EXE_rusting"))
            .args(["test", root.to_str().unwrap(), scenario.to_str().unwrap()])
            .arg("--json")
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value
    };
    for (folder, task) in tasks() {
        let (parent, root) = seeded(&folder, &task);
        let hidden: Vec<_> = task
            .hidden
            .iter()
            .map(|hidden| folder.join(hidden))
            .collect();
        assert!(
            hidden
                .iter()
                .any(|scenario| test(&root, scenario)["ok"] == false),
            "{}: the seeded project already passes",
            folder.display()
        );
        apply(&root, &task.reference, &folder);
        for scenario in &hidden {
            let result = test(&root, scenario);
            assert_eq!(result["ok"], true, "{}: {result}", folder.display());
        }
        std::fs::remove_dir_all(parent).unwrap();
    }
}
