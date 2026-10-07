//! Project creation, validation, opening, and recent-project storage.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::{
    SceneAlphaMode, SceneCamera, SceneDocument, SceneEntity, SceneMaterial,
    SceneMaterialData, SceneMaterialModel, SceneMesh, SceneMeshRenderer,
    SceneProjection, SceneTransform, SCENE_FORMAT_VERSION,
};

/// Current version of `project.json` written by the editor.
pub const PROJECT_FORMAT_VERSION: u32 = 1;

/// Environment variable that makes a game run this many ticks headless and
/// exit, for smoke tests of built and exported games.
pub const HEADLESS_TICKS_ENV: &str = "RUSTING_HEADLESS_TICKS";
/// Where `rusting test` saves its newest folder run, relative to the
/// project root; the editor's Agent panel reads it.
pub const TEST_RESULTS_FILE: &str = "build/test-results.json";

/// Environment variable naming the folder where game code keeps settings
/// and saves (`GameScene::save_data`). `rusting test` points it at an empty
/// `build/test-userdata/<scenario>/` folder for each scenario.
pub const USER_DATA_ENV: &str = "RUSTING_USER_DATA";

/// Folder for a game's settings and saves: [`USER_DATA_ENV`] when set, else
/// a folder named after the game's executable in the user's data folder
/// (`~/.local/share/<game>` on Linux, `~/Library/Application Support/<game>`
/// on macOS, `%APPDATA%\<game>` on Windows).
#[must_use]
pub fn user_data_folder() -> PathBuf {
    if let Some(folder) = std::env::var_os(USER_DATA_ENV) {
        return PathBuf::from(folder);
    }
    let game = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(ToOwned::to_owned))
        .unwrap_or_else(|| "rusting-game".into());
    let env = |name| std::env::var_os(name).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|home| home.join("Library/Application Support"))
    } else {
        env("XDG_DATA_HOME")
            .or_else(|| env("HOME").map(|home| home.join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from("userdata")).join(game)
}

/// Environment variable naming a file where a headless run writes its
/// `StateHashReport` as JSON, for determinism checks across processes.
pub const STATE_HASH_OUT_ENV: &str = "RUSTING_STATE_HASH_OUT";

/// Environment variable naming a file where a headless run saves the scene
/// as it stands after the last tick, so tools can query the end state.
pub const FINAL_SCENE_OUT_ENV: &str = "RUSTING_FINAL_SCENE_OUT";

/// Environment variable naming a file where a windowed game writes a
/// `Replay` of the session as JSON when it exits.
pub const REPLAY_OUT_ENV: &str = "RUSTING_REPLAY_OUT";

/// Environment variable with the [`crate::runtime::RandomSeed`] a game run
/// starts with, from `rusting run --seed`. A scenario's `seed` and a
/// replay's recorded seed still win.
pub const SEED_ENV: &str = "RUSTING_SEED";

/// Milliseconds after which a windowed game closes itself as if its window
/// closed, so `rusting run --record --timeout` still saves the replay.
pub const QUIT_AFTER_MS_ENV: &str = "RUSTING_QUIT_AFTER_MS";

/// Environment variable naming a `Replay` JSON file that a game plays back
/// headless instead of opening a window, failing if any tick's hash differs.
pub const REPLAY_PLAY_ENV: &str = "RUSTING_REPLAY_PLAY";

/// Environment variable the editor sets on Play: a file that carries the
/// game's state across a code reload. The game writes its scene there and
/// exits when it reads [`CODE_RELOAD_SAVE_COMMAND`] on standard input, and
/// starts from that file, then deletes it, when the file exists.
pub const CODE_RELOAD_STATE_ENV: &str = "RUSTING_CODE_RELOAD_STATE";

/// Standard input line that asks a game started with
/// [`CODE_RELOAD_STATE_ENV`] to save its state and exit.
pub const CODE_RELOAD_SAVE_COMMAND: &str = "save-state";

/// Line prefix a game prints to standard error when a code reload kept the
/// scene.
pub const CODE_RELOAD_KEPT_MARKER: &str =
    "[rusting] code reload kept the scene";

/// Line prefix a game prints to standard error when a code reload could not
/// keep the scene, followed by the reason.
pub const CODE_RELOAD_CLEAN_MARKER: &str =
    "[rusting] code reload restarted clean:";

/// Line prefix a game prints to standard error once its first playable frame
/// is done, followed by the milliseconds since the game started.
pub const FIRST_FRAME_MARKER: &str = "[rusting] first playable frame after";

/// Milliseconds the game reported with [`FIRST_FRAME_MARKER`].
#[must_use]
pub fn first_frame_ms(output: &str) -> Option<u64> {
    output.lines().find_map(|line| {
        line.strip_prefix(FIRST_FRAME_MARKER)?
            .trim()
            .strip_suffix("ms")?
            .trim()
            .parse()
            .ok()
    })
}

/// Printed by a headless run with the mean wall time of one fixed tick.
pub const TICK_TIME_MARKER: &str = "[rusting] headless ms per tick";

/// Frames a windowed game measures before it closes itself and prints
/// [`BENCH_MARKER`]; set by `rusting run --bench FRAMES`.
pub const BENCH_FRAMES_ENV: &str = "RUSTING_BENCH_FRAMES";

/// Comma-separated scene component names (`collider`, `rigid_body`, or a
/// registered component) that every scene load leaves out, set by
/// `rusting test --without`.
pub const WITHOUT_ENV: &str = "RUSTING_WITHOUT";

/// Frames a bench run skips before measuring: the first frames build
/// pipelines and upload assets.
pub const BENCH_WARMUP_FRAMES: u32 = 60;

/// Prefix of the line holding a bench run's frame times as JSON.
pub const BENCH_MARKER: &str = "[rusting] bench";

/// A bench line for frame lengths in milliseconds: frame count, mean, p50,
/// p95, p99 and max (nearest rank). Sorts `lengths`.
#[must_use]
pub fn bench_line(lengths: &mut [f64]) -> String {
    lengths.sort_by(f64::total_cmp);
    let rank = |p: f64| {
        let index = (p * lengths.len() as f64).ceil() as usize;
        lengths.get(index.saturating_sub(1)).copied().unwrap_or(0.0)
    };
    let mean = lengths.iter().sum::<f64>() / lengths.len().max(1) as f64;
    let summary = serde_json::json!({
        "frames": lengths.len(),
        "mean_ms": mean,
        "p50_ms": rank(0.5),
        "p95_ms": rank(0.95),
        "p99_ms": rank(0.99),
        "max_ms": rank(1.0),
    });
    format!("{BENCH_MARKER} {summary}")
}

/// The frame times a game printed with [`BENCH_MARKER`].
#[must_use]
pub fn bench_result(output: &str) -> Option<serde_json::Value> {
    output.lines().find_map(|line| {
        serde_json::from_str(line.strip_prefix(BENCH_MARKER)?.trim()).ok()
    })
}

/// Milliseconds per tick the game reported with [`TICK_TIME_MARKER`].
#[must_use]
pub fn tick_time_ms(output: &str) -> Option<f64> {
    output.lines().find_map(|line| {
        line.strip_prefix(TICK_TIME_MARKER)?.trim().parse().ok()
    })
}

/// One failed Rust, shader, or asset reload, attached to its file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReloadDiagnostic {
    /// `rust`, `shader`, or `asset`.
    pub kind: &'static str,
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub message: String,
}

/// Collects errors from `cargo --message-format short` output and asset hot
/// reload failures printed by a running game. Shader errors are reported by
/// `vulkano_shaders` at build time, so they arrive as Rust build errors that
/// name a shader file.
#[must_use]
pub fn reload_diagnostics(output: &str) -> Vec<ReloadDiagnostic> {
    const SHADERS: [&str; 5] = [".comp", ".vert", ".frag", ".glsl", ".geom"];
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(failure) = line.strip_prefix("hot reload failed: ") {
                let file = failure.split('`').nth(1).map(PathBuf::from);
                return Some(ReloadDiagnostic {
                    kind: "asset",
                    file,
                    line: None,
                    message: failure.to_owned(),
                });
            }
            let (location, message) = match line.find(": error") {
                Some(index) => (&line[..index], &line[index + 2..]),
                None if line.starts_with("error") => ("", line),
                None => return None,
            };
            if message.starts_with("error: could not compile")
                || message.starts_with("error: aborting")
            {
                return None;
            }
            let mut parts = location.rsplitn(3, ':');
            let (column, number, file) =
                (parts.next(), parts.next(), parts.next());
            let (file, number) = match (file, number, column) {
                (Some(file), Some(number), Some(_)) => {
                    (Some(PathBuf::from(file)), number.parse().ok())
                }
                _ => (None, None),
            };
            let shader = SHADERS.iter().any(|extension| {
                message.contains(&format!("{extension}:"))
                    || file.as_ref().is_some_and(|file| {
                        file.to_string_lossy().ends_with(extension)
                    })
            });
            Some(ReloadDiagnostic {
                kind: if shader { "shader" } else { "rust" },
                file,
                line: number,
                message: message.to_owned(),
            })
        })
        .collect()
}

/// Small file that tells the editor how a game project is arranged.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectManifest {
    /// Version used to reject project files that are too new.
    #[serde(default)]
    pub format_version: u32,
    /// Human-readable name shown in the Project Manager.
    pub name: String,
    /// Scene opened when this project starts.
    pub main_scene: PathBuf,
    /// Compact scene file loaded by the built game.
    pub cooked_scene: PathBuf,
    /// Cargo binary copied into exported game folders.
    #[serde(default)]
    pub binary_name: String,
    /// Optional asset generator hooks by name; see
    /// [`crate::asset_import::GeneratorHook`]. Projects need none.
    #[serde(
        default,
        skip_serializing_if = "std::collections::BTreeMap::is_empty"
    )]
    pub generators:
        std::collections::BTreeMap<String, crate::asset_import::GeneratorHook>,
    /// How strictly the simulation must reproduce itself; see
    /// [`crate::runtime::DeterminismMode`]. `cook` copies it into the
    /// cooked scene.
    #[serde(default, skip_serializing_if = "is_default")]
    pub determinism: crate::runtime::DeterminismMode,
    /// Window size in pixels, `[width, height]`, asked for when the game
    /// opens from the project folder; `None` keeps the default window.
    /// Game code can still change it with `GameScene::set_window_size`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<[u32; 2]>,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Valid project paths returned after create or open succeeds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenProject {
    /// Absolute folder containing `project.json` and `Cargo.toml`.
    pub root: PathBuf,
    /// Checked project settings loaded from `project.json`.
    pub manifest: ProjectManifest,
    /// Absolute path to the main editable scene.
    pub scene_path: PathBuf,
    /// Absolute path opened by Code Editor first.
    pub code_path: PathBuf,
}

/// One project displayed in the recent-project list.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentProject {
    /// Name last read from the project manifest.
    pub name: String,
    /// Absolute path to the project folder.
    pub path: PathBuf,
}

/// Project Manager values that survive between GUI frames.
#[derive(Resource, Clone, Debug)]
pub struct ProjectManagerState {
    /// True while the Project Manager covers the main editor.
    pub open: bool,
    /// Name typed into the Create Project form.
    pub project_name: String,
    /// Parent folder selected for the new project.
    pub parent_directory: PathBuf,
    /// Starting content of the new project.
    pub template: ProjectTemplate,
    /// Projects loaded from the editor settings file.
    pub recent_projects: Vec<RecentProject>,
    /// Last create, open, or validation message.
    pub message: Option<String>,
}

impl Default for ProjectManagerState {
    fn default() -> Self {
        Self {
            open: true,
            project_name: "MyGame".into(),
            // An empty path forces the user to choose where the project lives.
            parent_directory: PathBuf::new(),
            template: ProjectTemplate::default(),
            recent_projects: load_recent_projects().unwrap_or_default(),
            message: None,
        }
    }
}

/// Errors shown by the Project Manager instead of crashing the editor.
#[derive(Debug)]
pub enum ProjectError {
    /// Project name is empty or cannot be used as a folder name.
    InvalidName,
    /// Cargo executable name is unsafe or invalid.
    InvalidBinaryName,
    /// Selected parent folder does not exist.
    MissingParent(PathBuf),
    /// Target project folder already exists, so nothing was overwritten.
    AlreadyExists(PathBuf),
    /// Required project file is missing.
    MissingFile(PathBuf),
    /// Project file was created by a newer editor.
    UnsupportedVersion(u32),
    /// File-system operation failed.
    Io(std::io::Error),
    /// JSON project or scene data could not be read.
    Json(serde_json::Error),
}

impl Display for ProjectError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName => formatter.write_str(
                "Project name must contain letters, numbers, spaces, - or _",
            ),
            Self::InvalidBinaryName => formatter.write_str(
                "Project binary name must contain only letters, numbers, - or _",
            ),
            Self::MissingParent(path) => write!(
                formatter,
                "Parent directory `{}` does not exist",
                path.display()
            ),
            Self::AlreadyExists(path) => write!(
                formatter,
                "Project folder `{}` already exists; no files were changed",
                path.display()
            ),
            Self::MissingFile(path) => {
                write!(formatter, "Required file `{}` is missing", path.display())
            }
            Self::UnsupportedVersion(version) => write!(
                formatter,
                "Project format {version} is newer than supported format {PROJECT_FORMAT_VERSION}"
            ),
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Json(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Creates a complete game project without overwriting an existing folder.
///
/// # Arguments
/// * `parent` - Existing folder that will contain the project.
/// * `name` - Project and folder name selected by the user.
pub fn create_project(
    parent: &Path,
    name: &str,
) -> Result<OpenProject, ProjectError> {
    create_project_from(parent, name, ProjectTemplate::Basic3d)
}

/// Starting content of a new project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProjectTemplate {
    /// A lit cube and a perspective camera.
    #[default]
    Basic3d,
    /// A playable side-view level: tile map, platformer player with a
    /// following orthographic camera, goal burst, and HUD text.
    Platformer2d,
    /// Coin Run, a complete small game on the 2D template: collect every
    /// coin, then touch the flag to win.
    Starter,
    /// A walkable 3D room with crates and a first-person player.
    FirstPerson3d,
    /// The first-person room with a visible player body and an orbiting
    /// camera behind it.
    ThirdPerson3d,
    /// A pile of dynamic boxes and balls that fall onto a floor and settle.
    PhysicsSandbox,
    /// The 3D template without its cube: only a camera, so game
    /// code can spawn any name.
    Empty,
    /// Box Push, a grid puzzle with game code: step on a grid and push
    /// every box onto a goal.
    Puzzle,
}

impl ProjectTemplate {
    /// Every template, in the order pickers list them.
    pub const ALL: [Self; 8] = [
        Self::Basic3d,
        Self::FirstPerson3d,
        Self::ThirdPerson3d,
        Self::PhysicsSandbox,
        Self::Platformer2d,
        Self::Starter,
        Self::Puzzle,
        Self::Empty,
    ];

    /// The name `rusting new --template` takes.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Basic3d => "3d",
            Self::Platformer2d => "2d",
            Self::Starter => "starter",
            Self::FirstPerson3d => "first-person",
            Self::ThirdPerson3d => "third-person",
            Self::PhysicsSandbox => "sandbox",
            Self::Empty => "empty",
            Self::Puzzle => "puzzle",
        }
    }

    /// A short label for pickers.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Basic3d => "Empty 3D",
            Self::Platformer2d => "2D platformer",
            Self::Starter => "Coin Run (2D game)",
            Self::FirstPerson3d => "3D first person",
            Self::ThirdPerson3d => "3D third person",
            Self::PhysicsSandbox => "Physics sandbox",
            Self::Empty => "Empty (camera only)",
            Self::Puzzle => "Box Push (grid puzzle)",
        }
    }

    /// Inverse of [`Self::name`].
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|template| template.name() == name)
    }
}

/// [`create_project`] with a chosen [`ProjectTemplate`].
pub fn create_project_from(
    parent: &Path,
    name: &str,
    template: ProjectTemplate,
) -> Result<OpenProject, ProjectError> {
    validate_project_name(name)?;
    if !parent.is_dir() {
        return Err(ProjectError::MissingParent(parent.to_owned()));
    }
    let root = parent.join(name.trim());
    if root.exists() {
        return Err(ProjectError::AlreadyExists(root));
    }

    // A temporary sibling keeps half-written projects out of the chosen path.
    let temporary = parent.join(format!(".rusting-project-{}", Uuid::new_v4()));
    let result = write_project_template(&temporary, name.trim(), template)
        .and_then(|()| {
            std::fs::rename(&temporary, &root).map_err(ProjectError::from)
        });
    if result.is_err() && temporary.exists() {
        // Cleanup is safe because the random folder was created by this call.
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result?;
    open_project(&root)
}

/// Opens and validates an existing RustingEngine project folder.
pub fn open_project(root: &Path) -> Result<OpenProject, ProjectError> {
    let root = root.canonicalize()?;
    let manifest_path = root.join("project.json");
    let cargo_path = root.join("Cargo.toml");
    for required in [&manifest_path, &cargo_path] {
        if !required.is_file() {
            return Err(ProjectError::MissingFile(required.to_path_buf()));
        }
    }
    let manifest_bytes = std::fs::read(&manifest_path)?;
    let mut manifest: ProjectManifest =
        serde_json::from_slice(&manifest_bytes)?;
    let stored_manifest = manifest.clone();
    if manifest.format_version > PROJECT_FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion(manifest.format_version));
    }
    if manifest.format_version == 0 {
        // Version 0 is the old unversioned project.json shape.
        let backup = root.join("project.json.v0.backup");
        if !backup.exists() {
            std::fs::copy(&manifest_path, &backup)?;
        }
        manifest.format_version = PROJECT_FORMAT_VERSION;
    }
    if manifest.binary_name.is_empty() {
        manifest.binary_name = cargo_package_name(&manifest.name);
    }
    if manifest.binary_name.is_empty()
        || manifest.binary_name.chars().any(|character| {
            !(character.is_ascii_alphanumeric()
                || matches!(character, '-' | '_'))
        })
    {
        return Err(ProjectError::InvalidBinaryName);
    }
    // Write fields added by migration only after every value was validated.
    if manifest != stored_manifest {
        crate::runtime::write_atomic(
            &manifest_path,
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
    }
    let scene_path = checked_project_path(&root, &manifest.main_scene)?;
    if !scene_path.is_file() {
        return Err(ProjectError::MissingFile(scene_path));
    }
    // The cooked file may not exist yet, but its configured destination must
    // still remain inside the project.
    checked_project_path(&root, &manifest.cooked_scene)?;
    let code_path = root.join("src/main.rs");
    if !code_path.is_file() {
        return Err(ProjectError::MissingFile(code_path));
    }
    Ok(OpenProject {
        root,
        manifest,
        scene_path,
        code_path,
    })
}

/// Adds a project to the top of the recent list and saves the list.
pub fn remember_project(
    recent: &mut Vec<RecentProject>,
    project: &OpenProject,
) -> Result<(), ProjectError> {
    recent.retain(|item| item.path != project.root);
    recent.insert(
        0,
        RecentProject {
            name: project.manifest.name.clone(),
            path: project.root.clone(),
        },
    );
    recent.truncate(12);
    save_recent_projects(recent)
}

fn validate_project_name(name: &str) -> Result<(), ProjectError> {
    let name = name.trim();
    let upper_name = name.to_ascii_uppercase();
    let windows_device_name = matches!(
        upper_name.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if name.is_empty()
        || name == "."
        || name == ".."
        || windows_device_name
        || name.chars().any(|character| {
            !(character.is_ascii_alphanumeric()
                || matches!(character, ' ' | '-' | '_'))
        })
    {
        return Err(ProjectError::InvalidName);
    }
    Ok(())
}

fn checked_project_path(
    root: &Path,
    relative: &Path,
) -> Result<PathBuf, ProjectError> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(ProjectError::MissingFile(root.join(relative)));
    }
    Ok(root.join(relative))
}

fn write_project_template(
    root: &Path,
    name: &str,
    template: ProjectTemplate,
) -> Result<(), ProjectError> {
    for folder in ["src", "scenes", "assets", "shaders", "build"] {
        std::fs::create_dir_all(root.join(folder))?;
    }

    let package_name = cargo_package_name(name);
    let engine_source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let engine_dependency = if engine_source.join("Cargo.toml").is_file() {
        let engine_path = engine_source.to_string_lossy().replace('\\', "\\\\");
        format!("path = \"{engine_path}\"")
    } else {
        // A downloaded editor has no source checkout beside its executable.
        // Use the matching GitHub release tag for generated game projects.
        format!(
            "git = \"https://github.com/GoingRusting/RustingEngine\", tag = \"v{}\"",
            env!("CARGO_PKG_VERSION")
        )
    };
    let cargo = format!(
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nrusting_engine = {{ {engine_dependency}, default-features = false, features = [\"ui\"] }}\n\n[workspace]\n\n# Debug builds of the engine are too slow to play: optimize dependencies\n# and keep the game's own code quick to rebuild.\n[profile.dev.package.\"*\"]\nopt-level = 3\n"
    );
    std::fs::write(root.join("Cargo.toml"), cargo)?;

    let manifest = ProjectManifest {
        format_version: PROJECT_FORMAT_VERSION,
        name: name.into(),
        main_scene: "scenes/main.rscene".into(),
        cooked_scene: "build/main.rscene.bin".into(),
        binary_name: package_name.clone(),
        generators: std::collections::BTreeMap::new(),
        determinism: Default::default(),
        window: None,
    };
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    std::fs::write(
        root.join("src/main.rs"),
        match template {
            ProjectTemplate::Puzzle => include_str!("templates/puzzle.rs"),
            _ => default_game_source(),
        },
    )?;
    if template == ProjectTemplate::Puzzle {
        std::fs::create_dir_all(root.join("tests"))?;
        std::fs::write(
            root.join("tests/solve.json"),
            include_str!("templates/puzzle_solve.json"),
        )?;
    }
    std::fs::write(root.join("AGENTS.md"), include_str!("project_agents.md"))?;
    std::fs::create_dir_all(root.join("skills/rusting-game"))?;
    std::fs::write(
        root.join("skills/rusting-game/SKILL.md"),
        include_str!("../skills/rusting-game/SKILL.md"),
    )?;
    std::fs::write(root.join(".gitignore"), "/target\n/build\n/tests/shots\n")?;
    std::fs::write(
        root.join("scenes/main.rscene"),
        serde_json::to_vec_pretty(&match template {
            ProjectTemplate::Basic3d => default_scene(name),
            ProjectTemplate::Empty => {
                let mut scene = default_scene(name);
                scene
                    .entities
                    .retain(|entity| entity.name.as_deref() != Some("Cube"));
                scene
            }
            ProjectTemplate::Platformer2d => platformer_scene(name),
            ProjectTemplate::Starter => starter_scene(name),
            ProjectTemplate::Puzzle => puzzle_scene(name),
            ProjectTemplate::FirstPerson3d
            | ProjectTemplate::ThirdPerson3d
            | ProjectTemplate::PhysicsSandbox => scene_3d(name, template),
        })?,
    )?;
    Ok(())
}

fn cargo_package_name(name: &str) -> String {
    let mut package = String::new();
    let mut previous_separator = false;
    for character in name.trim().chars() {
        if character.is_alphanumeric() {
            package.extend(character.to_lowercase());
            previous_separator = false;
        } else if !previous_separator {
            package.push('_');
            previous_separator = true;
        }
    }
    package.trim_matches('_').to_owned()
}

fn default_game_source() -> &'static str {
    "use rusting_engine::prelude::*;\n\nfn update(_scene: &mut GameScene<'_>, _time: &FrameTime) {\n    // Add game behaviour here.\n}\n\nrusting_game!(update);\n"
}

fn default_scene(name: &str) -> SceneDocument {
    let cube = Uuid::new_v4();
    let camera = Uuid::new_v4();
    SceneDocument {
        format_version: SCENE_FORMAT_VERSION,
        name: format!("{name} Main Scene"),
        entities: vec![
            SceneEntity {
                id: cube,
                parent: None,
                name: Some("Cube".into()),
                classes: Vec::new(),
                transform: Some(SceneTransform {
                    position: [0.0, 0.0, 0.0],
                    rotation: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                }),
                mesh_renderer: Some(SceneMeshRenderer {
                    mesh: SceneMesh::BuiltinCube,
                    material: SceneMaterial::Inline(SceneMaterialData {
                        name: String::new(),
                        model: SceneMaterialModel::Pbr,
                        alpha_mode: SceneAlphaMode::Opaque,
                        base_color: [0.1, 0.45, 0.95, 1.0],
                        emissive: [0.0; 3],
                        metallic: 0.0,
                        roughness: 0.5,
                        transmission: 0.0,
                        ior: crate::assets::MaterialAsset::default().ior,
                        thickness: 0.0,
                        uv_scale: [1.0; 2],
                        uv_offset: [0.0; 2],
                        base_color_texture: None,
                        normal_texture: None,
                        metallic_roughness_texture: None,
                        occlusion_texture: None,
                        emissive_texture: None,
                    }),
                    cast_shadows: true,
                    receive_shadows: true,
                }),
                camera: None,
                visible: Some(true),
                physics_body: None,
                rigid_body: None,
                collider: None,
                collision_layers: None,
                gpu_physics_watch: None,
                components: BTreeMap::new(),
                directional_light: None,
                point_light: None,
                spot_light: None,
            },
            SceneEntity {
                id: camera,
                parent: None,
                name: Some("Game Camera".into()),
                classes: Vec::new(),
                transform: Some(SceneTransform {
                    position: [0.0, 3.0, 8.0],
                    rotation: [0.0, 0.0, 0.0],
                    scale: [1.0; 3],
                }),
                mesh_renderer: None,
                camera: Some(SceneCamera {
                    projection: SceneProjection::Perspective {
                        vertical_fov_radians: std::f32::consts::FRAC_PI_3,
                        near: 0.1,
                        far: 1_000.0,
                    },
                    active: true,
                    priority: 10,
                    viewport: None,
                }),
                visible: None,
                physics_body: None,
                rigid_body: None,
                collider: None,
                collision_layers: None,
                gpu_physics_watch: None,
                components: BTreeMap::new(),
                directional_light: None,
                point_light: None,
                spot_light: None,
            },
        ],
        render: Default::default(),
        simulation: Default::default(),
    }
}

fn platformer_scene(name: &str) -> SceneDocument {
    use serde_json::json;

    use crate::runtime::{
        BurstEmitter, HudElement, PlatformerController, TileKind, TileMap,
    };

    fn component(value: &impl Serialize) -> String {
        serde_json::to_string(value).expect("components serialize")
    }
    let sprite = |color: [f32; 4]| {
        json!({
            "mesh": {"BuiltinPrimitive": "Quad"},
            "material": {"Inline": {
                "model": "Unlit", "alpha_mode": "Opaque", "base_color": color,
                "emissive": [0.0, 0.0, 0.0], "metallic": 0.0, "roughness": 1.0,
                "base_color_texture": null, "normal_texture": null,
                "metallic_roughness_texture": null,
                "occlusion_texture": null, "emissive_texture": null
            }},
            "cast_shadows": false, "receive_shadows": false
        })
    };
    let transform = |position: [f32; 3], scale: [f32; 3]| json!({"position": position, "rotation": [0.0, 0.0, 0.0], "scale": scale});
    let level = TileMap {
        tile_size: 1.0,
        rows: [
            "....................",
            "....................",
            "....................",
            "..............BBB...",
            "........BBB.........",
            "....................",
            "...BB...........###.",
            "#######...##########",
            "####################",
        ]
        .map(String::from)
        .to_vec(),
        tiles: BTreeMap::from([
            (
                "#".into(),
                TileKind {
                    color: [0.25, 0.55, 0.3, 1.0],
                    ..TileKind::default()
                },
            ),
            (
                "B".into(),
                TileKind {
                    color: [0.55, 0.35, 0.2, 1.0],
                    ..TileKind::default()
                },
            ),
        ]),
    };
    let goal_burst = BurstEmitter {
        count: 24,
        speed: 4.0,
        lifetime: 0.8,
        particle_scale: 0.25,
        ..BurstEmitter::default()
    };
    let help = HudElement {
        text: "A/D or arrows run, Space jumps. Reach the gold block.".into(),
        ..HudElement::default()
    };
    let player = Uuid::new_v4();
    let entities = json!([
        {
            "id": Uuid::new_v4(), "parent": null, "name": "Level",
            "transform": transform([-10.0, 6.0, 0.0], [1.0, 1.0, 1.0]),
            "components": {"rusting.tile_map": component(&level)}
        },
        {
            "id": player, "parent": null, "name": "Player",
            "transform": transform([-8.0, 0.5, 0.0], [1.0, 1.0, 1.0]),
            "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
            "rigid_body": {"kind": "Kinematic", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
            "collider": {"shape": {"Box": {"half_extents": [0.3, 0.5, 0.3]}}, "friction": 0.0, "restitution": 0.0, "sensor": false},
            "components": {
                "rusting.platformer_controller":
                    component(&PlatformerController::default())
            }
        },
        {
            "id": Uuid::new_v4(), "parent": player, "name": "Player Sprite",
            "transform": transform([0.0, 0.0, 0.1], [0.6, 1.0, 1.0]),
            "mesh_renderer": sprite([0.95, 0.45, 0.15, 1.0]),
            "visible": true
        },
        {
            "id": Uuid::new_v4(), "parent": player, "name": "Game Camera",
            "transform": transform([0.0, 1.0, 10.0], [1.0, 1.0, 1.0]),
            "camera": {
                "projection": {"Orthographic": {"vertical_size": 12.0, "near": 0.1, "far": 100.0}},
                "active": true, "priority": 10
            }
        },
        {
            "id": Uuid::new_v4(), "parent": null, "name": "Goal",
            "transform": transform([8.5, 0.5, 0.05], [0.6, 1.0, 1.0]),
            "mesh_renderer": sprite([1.0, 0.8, 0.1, 1.0]),
            "visible": true,
            "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
            "rigid_body": {"kind": "Fixed", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
            "collider": {"shape": {"Box": {"half_extents": [0.5, 0.5, 0.5]}}, "friction": 0.5, "restitution": 0.0, "sensor": true},
            "components": {"rusting.burst_emitter": component(&goal_burst)}
        },
        {
            "id": Uuid::new_v4(), "parent": null, "name": "Help",
            "components": {"rusting.hud": component(&help)}
        }
    ]);
    SceneDocument {
        format_version: SCENE_FORMAT_VERSION,
        name: format!("{name} Main Scene"),
        entities: serde_json::from_value(entities)
            .expect("the 2D template is a valid scene"),
        render: Default::default(),
        simulation: Default::default(),
    }
}

/// The first-person, third-person, and physics sandbox templates. They
/// share a lit floor; the player templates add crates and a
/// `PlayerController`, the sandbox a pile of dynamic bodies and a fixed
/// camera.
fn scene_3d(name: &str, template: ProjectTemplate) -> SceneDocument {
    use serde_json::{json, Value};

    use crate::runtime::{HudElement, PlayerController};

    fn component(value: &impl Serialize) -> String {
        serde_json::to_string(value).expect("components serialize")
    }
    let mesh = |shape: &str, color: [f32; 3]| {
        json!({
            "mesh": {"BuiltinPrimitive": shape},
            "material": {"Inline": {
                "model": "Pbr", "alpha_mode": "Opaque",
                "base_color": [color[0], color[1], color[2], 1.0],
                "emissive": [0.0, 0.0, 0.0], "metallic": 0.0, "roughness": 0.7,
                "base_color_texture": null, "normal_texture": null,
                "metallic_roughness_texture": null,
                "occlusion_texture": null, "emissive_texture": null
            }},
            "cast_shadows": true, "receive_shadows": true
        })
    };
    let transform = |position: [f32; 3], scale: [f32; 3]| json!({"position": position, "rotation": [0.0, 0.0, 0.0], "scale": scale});
    let physics = json!({"simulation": "Cpu", "solver": "Simplified", "custom_shader": null});
    let body = |kind: &str| json!({"kind": kind, "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0});
    let collider = |shape: Value| json!({"shape": shape, "friction": 0.5, "restitution": 0.0, "sensor": false});
    // A box of `size` whose mesh matches its collider.
    let block = |name: &str,
                 kind: &str,
                 position: [f32; 3],
                 size: [f32; 3],
                 color| {
        json!({
            "id": Uuid::new_v4(), "parent": null, "name": name,
            "transform": transform(position, size),
            "mesh_renderer": mesh("Cube", color), "visible": true,
            "physics_body": physics, "rigid_body": body(kind),
            "collider": collider(json!({"Box": {"half_extents": [0.5, 0.5, 0.5]}}))
        })
    };

    let mut entities = vec![
        // Top face at y = 0.
        block(
            "Floor",
            "Fixed",
            [0.0, -0.5, 0.0],
            [30.0, 1.0, 30.0],
            [0.35, 0.37, 0.4],
        ),
        json!({
            "id": Uuid::new_v4(), "parent": null, "name": "Sun",
            "transform": {"position": [0.0, 10.0, 0.0], "rotation": [-0.9, 0.5, 0.0], "scale": [1.0, 1.0, 1.0]},
            "directional_light": {"color": [1.0, 0.96, 0.9], "illuminance": 100_000.0, "shadows": true}
        }),
    ];
    let help = if template == ProjectTemplate::PhysicsSandbox {
        for (index, x) in [-1.1_f32, 0.0, 1.1].into_iter().enumerate() {
            for level in 0..3 {
                entities.push(block(
                    &format!("Box {}", level * 3 + index + 1),
                    "Dynamic",
                    [x, 0.5 + 1.05 * level as f32, 0.0],
                    [1.0; 3],
                    [0.9, 0.55 - 0.15 * level as f32, 0.2],
                ));
            }
        }
        // Low walls keep rolling balls on the floor.
        for (index, (position, size)) in [
            ([0.0, 0.5, -15.0], [30.0, 1.0, 0.5]),
            ([0.0, 0.5, 15.0], [30.0, 1.0, 0.5]),
            ([-15.0, 0.5, 0.0], [0.5, 1.0, 30.0]),
            ([15.0, 0.5, 0.0], [0.5, 1.0, 30.0]),
        ]
        .into_iter()
        .enumerate()
        {
            entities.push(block(
                &format!("Wall {}", index + 1),
                "Fixed",
                position,
                size,
                [0.3, 0.32, 0.35],
            ));
        }
        for (index, x) in [-0.6_f32, 0.6].into_iter().enumerate() {
            entities.push(json!({
                "id": Uuid::new_v4(), "parent": null,
                "name": format!("Ball {}", index + 1),
                "transform": transform([x, 6.0 + 2.0 * index as f32, 0.2], [1.0; 3]),
                "mesh_renderer": mesh("Sphere", [0.2, 0.6, 0.95]), "visible": true,
                "physics_body": physics, "rigid_body": body("Dynamic"),
                "collider": collider(json!({"Sphere": {"radius": 0.5}}))
            }));
        }
        entities.push(json!({
            "id": Uuid::new_v4(), "parent": null, "name": "Game Camera",
            "transform": {"position": [0.0, 4.0, 10.0], "rotation": [-0.3, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
            "camera": {
                "projection": {"Perspective": {"vertical_fov_radians": std::f32::consts::FRAC_PI_3, "near": 0.1, "far": 1000.0}},
                "active": true, "priority": 10
            }
        }));
        "Boxes and balls fall and settle. Press Play again to restart."
    } else {
        for (index, (position, size)) in [
            ([-3.0, 0.5, -3.0], [1.0, 1.0, 1.0]),
            ([-1.5, 0.25, -4.0], [1.0, 0.5, 1.0]),
            ([3.0, 0.75, -2.0], [2.0, 1.5, 2.0]),
            ([0.0, 1.5, -12.0], [12.0, 3.0, 1.0]),
        ]
        .into_iter()
        .enumerate()
        {
            entities.push(block(
                &format!("Crate {}", index + 1),
                "Fixed",
                position,
                size,
                [0.6, 0.45, 0.3],
            ));
        }
        let third_person = template == ProjectTemplate::ThirdPerson3d;
        let player = Uuid::new_v4();
        let controller = if third_person {
            // Start looking a little down, over the body's shoulder.
            PlayerController {
                camera_distance: 4.0,
                pitch: -0.3,
                ..PlayerController::default()
            }
        } else {
            PlayerController::default()
        };
        entities.push(json!({
            "id": player, "parent": null, "name": "Player",
            "transform": transform([0.0, 1.0, 4.0], [1.0; 3]),
            "physics_body": physics, "rigid_body": body("Kinematic"),
            "collider": collider(json!({"Capsule": {"half_height": 0.6, "radius": 0.3}})),
            "components": {"rusting.player_controller": component(&controller)}
        }));
        entities.push(json!({
            "id": Uuid::new_v4(), "parent": player, "name": "Game Camera",
            "transform": transform([0.0, 0.7, 0.0], [1.0; 3]),
            "camera": {
                "projection": {"Perspective": {"vertical_fov_radians": std::f32::consts::FRAC_PI_3, "near": 0.05, "far": 1000.0}},
                "active": true, "priority": 10
            }
        }));
        if third_person {
            entities.push(json!({
                "id": Uuid::new_v4(), "parent": player, "name": "Body",
                "transform": transform([0.0; 3], [0.6, 1.8, 0.6]),
                "mesh_renderer": mesh("Cylinder", [0.95, 0.45, 0.15]),
                "visible": true
            }));
        }
        "Click to look around, Esc frees the mouse. WASD moves, Shift runs, Space jumps."
    };
    let help = HudElement {
        text: help.into(),
        ..HudElement::default()
    };
    entities.push(json!({
        "id": Uuid::new_v4(), "parent": null, "name": "Help",
        "components": {"rusting.hud": component(&help)}
    }));
    SceneDocument {
        format_version: SCENE_FORMAT_VERSION,
        name: format!("{name} Main Scene"),
        entities: serde_json::from_value(Value::Array(entities))
            .expect("the 3D templates are valid scenes"),
        render: Default::default(),
        simulation: Default::default(),
    }
}

/// Number of coins in the starter game.
pub const STARTER_COINS: i32 = 5;

/// The 2D template turned into Coin Run: a flatter level with two blocks,
/// a pit, and a ledge, five coins, a coin counter on the HUD, and a flag
/// that wins once every coin is collected.
fn starter_scene(name: &str) -> SceneDocument {
    use serde_json::{json, Value};

    use crate::runtime::{
        BurstEmitter, Counter, HudAnchor, HudElement, Pickup, SceneBackground,
        SceneEntity, TileKind, TileMap,
    };

    fn component(value: &impl Serialize) -> String {
        serde_json::to_string(value).expect("components serialize")
    }
    fn find<'a>(
        scene: &'a mut SceneDocument,
        name: &str,
    ) -> &'a mut SceneEntity {
        scene
            .entities
            .iter_mut()
            .find(|entity| entity.name.as_deref() == Some(name))
            .expect("the 2D template has this entity")
    }
    let mut scene = platformer_scene(name);
    let level = find(&mut scene, "Level");
    let mut map: TileMap =
        serde_json::from_str(&level.components["rusting.tile_map"])
            .expect("the 2D template has a tile map");
    map.rows = [
        "......................",
        "......................",
        "......................",
        "#....................#",
        "#....................#",
        "#....................#",
        "#....B.......B....####",
        "#########...##########",
        "######################",
    ]
    .map(String::from)
    .to_vec();
    map.tiles.insert(
        "B".into(),
        TileKind {
            color: [0.55, 0.35, 0.2, 1.0],
            ..TileKind::default()
        },
    );
    level
        .components
        .insert("rusting.tile_map".into(), component(&map));
    level.transform.as_mut().expect("placed").position = [-11.0, 6.0, 0.0];
    find(&mut scene, "Player")
        .transform
        .as_mut()
        .expect("placed")
        .position = [-9.5, 0.5, 0.0];
    // Look ahead of the runner.
    find(&mut scene, "Game Camera")
        .transform
        .as_mut()
        .expect("placed")
        .position = [4.0, 1.0, 10.0];
    let goal = find(&mut scene, "Goal");
    goal.name = Some("Flag".into());
    goal.components.insert(
        "rusting.burst_emitter".into(),
        component(&BurstEmitter {
            count: 32,
            speed: 5.0,
            lifetime: 1.0,
            particle_scale: 0.25,
            on_collision: false,
            ..BurstEmitter::default()
        }),
    );
    goal.components.insert(
        "rusting.pickup".into(),
        component(&Pickup {
            counter: "won".into(),
            requires: Some("coins".into()),
            ..Pickup::default()
        }),
    );
    find(&mut scene, "Help").components.insert(
        "rusting.hud".into(),
        component(&HudElement {
            text: "A/D or arrows run, Space jumps. Grab every coin, then the flag."
                .into(),
            anchor: HudAnchor::BottomLeft,
            ..HudElement::default()
        }),
    );

    let mut extra = Vec::new();
    let coin_burst = component(&BurstEmitter {
        count: 8,
        speed: 2.5,
        lifetime: 0.4,
        particle_scale: 0.5,
        on_collision: false,
        ..BurstEmitter::default()
    });
    let coin = component(&Pickup {
        counter: "coins".into(),
        ..Pickup::default()
    });
    let mut coin_sprite =
        serde_json::to_value(&find(&mut scene, "Flag").mesh_renderer)
            .expect("mesh renderers serialize");
    coin_sprite["material"]["Inline"]["base_color"] =
        json!([1.0, 0.85, 0.2, 1.0]);
    // Ground, over the first block, over the pit, over the second block,
    // ground.
    for (index, [x, y]) in [
        [-8.0, -0.4],
        [-5.5, 1.2],
        [-0.5, 0.6],
        [2.5, 1.2],
        [5.0, -0.4],
    ]
    .into_iter()
    .enumerate()
    {
        extra.push(json!({
            "id": Uuid::new_v4(), "parent": null,
            "name": format!("Coin {}", index + 1), "classes": ["coin"],
            "transform": {"position": [x, y, 0.05], "rotation": [0.0, 0.0, 0.0], "scale": [0.4, 0.4, 1.0]},
            "mesh_renderer": coin_sprite,
            "visible": true,
            "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
            "rigid_body": {"kind": "Fixed", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
            "collider": {"shape": {"Box": {"half_extents": [0.5, 0.5, 0.5]}}, "friction": 0.5, "restitution": 0.0, "sensor": true},
            "components": {"rusting.pickup": coin, "rusting.burst_emitter": coin_burst}
        }));
    }
    let counter = |name: &str, target| {
        component(&Counter {
            name: name.into(),
            value: 0,
            target: Some(target),
        })
    };
    let hud = |text: &str, anchor, font_size, color, requires: Option<&str>| {
        component(&HudElement {
            text: text.into(),
            anchor,
            font_size,
            color,
            requires: requires.map(String::from),
            ..HudElement::default()
        })
    };
    extra.push(json!({
        "id": Uuid::new_v4(), "parent": null, "name": "Score",
        "components": {
            "rusting.counter": counter("coins", STARTER_COINS),
            "rusting.hud": hud(&format!("Coins {{coins}}/{STARTER_COINS}"), HudAnchor::TopLeft, 28.0, [1.0, 0.85, 0.2, 1.0], None),
        }
    }));
    extra.push(json!({
        "id": Uuid::new_v4(), "parent": null, "name": "Win",
        "components": {
            "rusting.counter": counter("won", 1),
            "rusting.hud": hud("You win!", HudAnchor::Center, 56.0, [1.0, 1.0, 1.0, 1.0], Some("won")),
            "rusting.background": component(&SceneBackground {
                color: [0.35, 0.55, 0.8, 1.0],
            }),
        }
    }));
    scene.entities.extend(
        extra
            .into_iter()
            .map(|entity: Value| serde_json::from_value(entity))
            .collect::<Result<Vec<_>, _>>()
            .expect("the starter template is a valid scene"),
    );
    scene
}

/// Box Push: walls, two boxes and two goals laid out from a map on whole
/// X and Z cells, a ball player, a camera looking down at the grid, and the
/// `boxes` counter. The game code is `src/templates/puzzle.rs`.
fn puzzle_scene(name: &str) -> SceneDocument {
    use serde_json::{json, Value};

    use crate::runtime::{Counter, HudAnchor, HudElement};

    fn component(value: &impl Serialize) -> String {
        serde_json::to_string(value).expect("components serialize")
    }
    const MAP: [&str; 6] = [
        "########", "#......#", "#.@.B..#", "#......#", "#.B.GG.#", "########",
    ];
    let mesh = |shape: &str, color: [f32; 3]| {
        json!({
            "mesh": {"BuiltinPrimitive": shape},
            "material": {"Inline": {
                "model": "Pbr", "alpha_mode": "Opaque",
                "base_color": [color[0], color[1], color[2], 1.0],
                "emissive": [0.0, 0.0, 0.0], "metallic": 0.0, "roughness": 0.7,
                "base_color_texture": null, "normal_texture": null,
                "metallic_roughness_texture": null,
                "occlusion_texture": null, "emissive_texture": null
            }},
            "cast_shadows": true, "receive_shadows": true
        })
    };
    let object = |name: &str,
                  class: Option<&str>,
                  shape: &str,
                  color,
                  position: [f32; 3],
                  scale: [f32; 3]| {
        json!({
            "id": Uuid::new_v4(), "parent": null, "name": name,
            "classes": class.into_iter().collect::<Vec<_>>(),
            "transform": {"position": position, "rotation": [0.0, 0.0, 0.0], "scale": scale},
            "mesh_renderer": mesh(shape, color), "visible": true
        })
    };
    let mut entities = vec![
        // Top face at y = 0, under every cell.
        object(
            "Floor",
            None,
            "Cube",
            [0.3, 0.32, 0.36],
            [-0.5, -0.1, -0.5],
            [8.0, 0.2, 6.0],
        ),
        json!({
            "id": Uuid::new_v4(), "parent": null, "name": "Sun",
            "transform": {"position": [0.0, 10.0, 0.0], "rotation": [-0.9, 0.5, 0.0], "scale": [1.0, 1.0, 1.0]},
            "directional_light": {"color": [1.0, 0.96, 0.9], "illuminance": 100_000.0, "shadows": true}
        }),
        json!({
            "id": Uuid::new_v4(), "parent": null, "name": "Game Camera",
            "transform": {"position": [-0.5, 8.0, 5.0], "rotation": [-1.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
            "camera": {
                "projection": {"Perspective": {"vertical_fov_radians": std::f32::consts::FRAC_PI_3, "near": 0.1, "far": 1000.0}},
                "active": true, "priority": 10
            }
        }),
    ];
    let mut count = std::collections::HashMap::new();
    let mut numbered = |kind: &str| {
        let n = count.entry(kind.to_owned()).or_insert(0);
        *n += 1;
        format!("{kind} {n}")
    };
    // Map column x and row z become cell (x - 4, z - 3).
    for (z, row) in MAP.iter().enumerate() {
        for (x, tile) in row.chars().enumerate() {
            let (x, z) = (x as f32 - 4.0, z as f32 - 3.0);
            entities.push(match tile {
                '#' => object(
                    &numbered("Wall"),
                    Some("wall"),
                    "Cube",
                    [0.45, 0.47, 0.52],
                    [x, 0.5, z],
                    [1.0; 3],
                ),
                'B' => object(
                    &numbered("Box"),
                    Some("box"),
                    "Cube",
                    [0.8, 0.55, 0.25],
                    [x, 0.4, z],
                    [0.8; 3],
                ),
                'G' => object(
                    &numbered("Goal"),
                    Some("goal"),
                    "Cube",
                    [0.25, 0.8, 0.35],
                    [x, 0.02, z],
                    [0.9, 0.04, 0.9],
                ),
                '@' => object(
                    "Player",
                    None,
                    "Sphere",
                    [0.25, 0.55, 0.95],
                    [x, 0.4, z],
                    [0.8; 3],
                ),
                _ => continue,
            });
        }
    }
    let hud = |text: &str, anchor, font_size, requires: Option<&str>| {
        component(&HudElement {
            text: text.into(),
            anchor,
            font_size,
            requires: requires.map(String::from),
            ..HudElement::default()
        })
    };
    let goals = MAP.concat().matches('G').count() as i32;
    entities.push(json!({
        "id": Uuid::new_v4(), "parent": null, "name": "Score",
        "components": {
            "rusting.counter": component(&Counter { name: "boxes".into(), value: 0, target: Some(goals) }),
            "rusting.hud": hud(&format!("Boxes {{boxes}}/{goals}"), HudAnchor::TopLeft, 28.0, None),
        }
    }));
    entities.push(json!({
        "id": Uuid::new_v4(), "parent": null, "name": "Solved",
        "components": {"rusting.hud": hud("Solved!", HudAnchor::Center, 56.0, Some("boxes"))}
    }));
    entities.push(json!({
        "id": Uuid::new_v4(), "parent": null, "name": "Help",
        "components": {"rusting.hud": hud("Arrows or WASD move. Push every box onto a green goal.", HudAnchor::BottomLeft, 20.0, None)}
    }));
    for (action, inputs) in [
        ("up", ["KeyW", "ArrowUp", "PadDpadUp"]),
        ("down", ["KeyS", "ArrowDown", "PadDpadDown"]),
        ("left", ["KeyA", "ArrowLeft", "PadDpadLeft"]),
        ("right", ["KeyD", "ArrowRight", "PadDpadRight"]),
    ] {
        entities.push(json!({
            "id": Uuid::new_v4(), "parent": null, "name": format!("Move {action}"),
            "components": {"rusting.input_action": component(&json!({"action": action, "inputs": inputs}))}
        }));
    }
    SceneDocument {
        format_version: SCENE_FORMAT_VERSION,
        name: format!("{name} Main Scene"),
        entities: entities
            .into_iter()
            .map(|entity: Value| serde_json::from_value(entity))
            .collect::<Result<Vec<_>, _>>()
            .expect("the puzzle template is a valid scene"),
        render: Default::default(),
        simulation: Default::default(),
    }
}

/// Uses the process folder only for editor settings, never project creation.
fn default_project_parent() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Per-user editor configuration folder.
pub(super) fn user_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config"))
        });
    base.unwrap_or_else(default_project_parent)
        .join("rusting_engine")
}

fn recent_projects_path() -> PathBuf {
    user_config_dir().join("recent_projects.json")
}

/// Editor settings that belong to the user, not to a project.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EditorPreferences {
    /// Whole-interface zoom (egui zoom factor); 1.0 is 100%.
    pub ui_scale: f32,
    /// Multiplier on egui's default text sizes; 1.0 is 100%.
    pub font_scale: f32,
}

impl Default for EditorPreferences {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            font_scale: 1.0,
        }
    }
}

impl EditorPreferences {
    /// UI scales offered in the View menu.
    pub const UI_SCALES: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];
    /// Text sizes offered in the View menu.
    pub const FONT_SCALES: [f32; 4] = [0.9, 1.0, 1.15, 1.3];

    fn path() -> PathBuf {
        user_config_dir().join("editor_preferences.json")
    }

    /// Reads the user's preferences; a missing or broken file gives defaults.
    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .map(|mut preferences| {
                preferences.ui_scale = preferences.ui_scale.clamp(0.5, 3.0);
                preferences.font_scale = preferences.font_scale.clamp(0.5, 3.0);
                preferences
            })
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), ProjectError> {
        self.save_to(&Self::path())
    }

    fn save_to(&self, path: &Path) -> Result<(), ProjectError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::runtime::write_atomic(path, &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

fn load_recent_projects() -> Result<Vec<RecentProject>, ProjectError> {
    let path = recent_projects_path();
    if !path.is_file() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn save_recent_projects(recent: &[RecentProject]) -> Result<(), ProjectError> {
    let path = recent_projects_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::runtime::write_atomic(path, &serde_json::to_vec_pretty(recent)?)?;
    Ok(())
}

/// File name of the game executable built for `target`.
pub fn executable_name(binary_name: &str, target: Option<&str>) -> String {
    let windows = target.map_or(cfg!(target_os = "windows"), |target| {
        target.contains("windows")
    });
    if windows {
        format!("{binary_name}.exe")
    } else {
        binary_name.to_owned()
    }
}

/// Creates a portable folder after Cargo has produced the release binary.
pub fn export_built_game(
    project_root: &std::path::Path,
    manifest: &std::path::Path,
    parent: &std::path::Path,
    project_name: &str,
    binary_name: &str,
    cooked_scene: &std::path::Path,
    target: Option<&str>,
) -> Result<PathBuf, String> {
    if !parent.is_dir() {
        return Err(format!("{} is not a folder", parent.display()));
    }
    let executable =
        built_executable(project_root, manifest, binary_name, target, true)?;
    package_game_files(
        project_root,
        &executable,
        parent,
        project_name,
        binary_name,
        cooked_scene,
        target,
    )
}

/// Copies a verified executable and runtime data into one new export folder.
pub fn package_game_files(
    project_root: &std::path::Path,
    executable: &std::path::Path,
    parent: &std::path::Path,
    project_name: &str,
    binary_name: &str,
    cooked_scene: &std::path::Path,
    target: Option<&str>,
) -> Result<PathBuf, String> {
    let executable_name = executable_name(binary_name, target);
    let cooked_source = project_root.join(cooked_scene);
    if !cooked_source.is_file() {
        return Err(format!(
            "Cooked scene {} is missing",
            cooked_source.display()
        ));
    }

    // A unique final name avoids replacing an older playable export.
    // Cross exports name their system, e.g. `game_windows_export`.
    let safe_name = match target.and_then(|target| target.split('-').nth(2)) {
        Some(system) => format!("{}_{system}", binary_name.replace('-', "_")),
        None => binary_name.replace('-', "_"),
    };
    let mut destination = parent.join(format!("{safe_name}_export"));
    for number in 2.. {
        if !destination.exists() {
            break;
        }
        destination = parent.join(format!("{safe_name}_export_{number}"));
    }
    let temporary =
        parent.join(format!(".rusting-export-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(&temporary)
            .map_err(|error| error.to_string())?;
        std::fs::copy(executable, temporary.join(&executable_name))
            .map_err(|error| error.to_string())?;
        let cooked_destination = temporary.join(cooked_scene);
        if let Some(folder) = cooked_destination.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|error| error.to_string())?;
        }
        std::fs::copy(&cooked_source, cooked_destination)
            .map_err(|error| error.to_string())?;
        let assets = project_root.join("assets");
        if assets.is_dir() {
            copy_directory(&assets, &temporary.join("assets"))?;
        }
        // Scenes a game loads later, next to the cooked main scene.
        let scenes = project_root.join("scenes");
        if scenes.is_dir() {
            copy_directory(&scenes, &temporary.join("scenes"))?;
        }
        let engine_license =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("LICENSE.md");
        if engine_license.is_file() {
            std::fs::copy(
                engine_license,
                temporary.join("RUSTING_ENGINE_LICENSE.md"),
            )
            .map_err(|error| error.to_string())?;
        }
        std::fs::write(
            temporary.join("README.txt"),
            format!(
                "{project_name}\n\nRun {executable_name} to start the game.\nThe system needs a Vulkan-capable graphics driver.\n\nSettings and saves live in the user data folder: ~/.local/share/{binary_name} on Linux,\n~/Library/Application Support/{binary_name} on macOS, %APPDATA%\\{binary_name} on Windows.\nSet RUSTING_USER_DATA=<folder> to use another one.\n\nRUSTING_HEADLESS_TICKS=<n> runs n ticks with no window and exits, for smoke tests;\ncommand-line arguments reach the game code through std::env::args().\n"
            ),
        )
        .map_err(|error| error.to_string())?;
        std::fs::rename(&temporary, &destination)
            .map_err(|error| error.to_string())?;
        Ok(())
    })();
    if result.is_err() && temporary.exists() {
        let _ = std::fs::remove_dir_all(&temporary);
    }
    result.map(|()| destination)
}

/// Recursively copies normal files and folders while ignoring symbolic links.
pub fn copy_directory(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), target)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Path of the executable Cargo built for the project, checked to exist.
pub fn built_executable(
    project_root: &std::path::Path,
    manifest: &std::path::Path,
    binary_name: &str,
    target: Option<&str>,
    release: bool,
) -> Result<PathBuf, String> {
    let metadata = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .args(["--manifest-path"])
        .arg(manifest)
        .current_dir(project_root)
        .output()
        .map_err(|error| format!("Could not read Cargo metadata: {error}"))?;
    if !metadata.status.success() {
        return Err(String::from_utf8_lossy(&metadata.stderr).into_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)
        .map_err(|error| format!("Invalid Cargo metadata: {error}"))?;
    let target_directory = metadata
        .get("target_directory")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Cargo metadata has no target directory".to_owned())?;
    let executable = PathBuf::from(target_directory)
        .join(target.unwrap_or_default())
        .join(if release { "release" } else { "debug" })
        .join(executable_name(binary_name, target));
    if !executable.is_file() {
        return Err(format!(
            "Executable {} was not produced",
            executable.display()
        ));
    }
    Ok(executable)
}

// `rusting_game!` adds a `main` the tests never call.
#[cfg(test)]
#[allow(dead_code)]
#[path = "templates/puzzle.rs"]
mod puzzle_template;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bench_line_round_trips_its_frame_times() {
        let mut lengths: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let line = bench_line(&mut lengths);
        assert!(line.starts_with(BENCH_MARKER), "{line}");
        let summary = bench_result(&format!("noise\n{line}\nmore")).unwrap();
        assert_eq!(summary["frames"], 100);
        assert_eq!(summary["mean_ms"], 50.5);
        assert_eq!(summary["p50_ms"], 50.0);
        assert_eq!(summary["p95_ms"], 95.0);
        assert_eq!(summary["p99_ms"], 99.0);
        assert_eq!(summary["max_ms"], 100.0);
        assert!(bench_result("no bench here").is_none());
    }

    #[test]
    fn reload_output_is_split_into_file_diagnostics_and_first_frame_time() {
        let output = "\
   Compiling game v0.1.0
src/main.rs:12:5: error[E0425]: cannot find value `x` in this scope
src/main.rs:3:1: warning: unused import
/engine/src/shaders/mod.rs:4:1: error: shader.comp:7: error: undeclared identifier
error: linking with `cc` failed
error: could not compile `game` (bin \"game\") due to 2 previous errors
hot reload failed: failed to load `assets/crate.rtexture`: bad header
[rusting] first playable frame after 412 ms
";
        let diagnostics = reload_diagnostics(output);
        let summary: Vec<_> = diagnostics
            .iter()
            .map(|d| (d.kind, d.file.clone(), d.line))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("rust", Some(PathBuf::from("src/main.rs")), Some(12)),
                (
                    "shader",
                    Some(PathBuf::from("/engine/src/shaders/mod.rs")),
                    Some(4)
                ),
                ("rust", None, None),
                ("asset", Some(PathBuf::from("assets/crate.rtexture")), None),
            ]
        );
        assert!(diagnostics[0].message.starts_with("error[E0425]"));
        assert_eq!(first_frame_ms(output), Some(412));
        assert_eq!(first_frame_ms("no marker"), None);
    }

    #[test]
    fn project_creation_writes_a_complete_openable_template() {
        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project = create_project(&parent, "Example Game").unwrap();

        assert_eq!(project.manifest.name, "Example Game");
        assert!(project.root.join("Cargo.toml").is_file());
        assert!(project.root.join("src/main.rs").is_file());
        assert!(project.root.join("AGENTS.md").is_file());
        assert!(project.root.join("skills/rusting-game/SKILL.md").is_file());
        assert!(project.root.join(".gitignore").is_file());
        assert!(project.root.join("scenes/main.rscene").is_file());
        assert!(project.root.join("assets").is_dir());
        assert!(project.root.join("shaders").is_dir());
        assert!(project.root.join("build").is_dir());

        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn project_creation_never_overwrites_an_existing_folder() {
        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        let existing = parent.join("Existing");
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("keep.txt"), "keep").unwrap();

        assert!(matches!(
            create_project(&parent, "Existing"),
            Err(ProjectError::AlreadyExists(_))
        ));
        assert_eq!(
            std::fs::read_to_string(existing.join("keep.txt")).unwrap(),
            "keep"
        );

        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn the_2d_template_is_a_playable_side_view_level() {
        use std::time::Duration;

        use crate::assets::AssetPlugin;
        use crate::runtime::{
            load_scene, App, Collider, KeyCode, PlatformerController,
            RuntimeInput, SceneLoadMode, TileOf,
        };

        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project = create_project_from(
            &parent,
            "Jumper",
            ProjectTemplate::Platformer2d,
        )
        .unwrap();
        let cargo =
            std::fs::read_to_string(project.root.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("features = [\"ui\"]"), "{cargo}");

        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        load_scene(
            app.world_mut(),
            project.root.join("scenes/main.rscene"),
            SceneLoadMode::Replace,
        )
        .unwrap();
        let tick = |app: &mut App, ticks| {
            for _ in 0..ticks {
                app.update(Duration::from_secs_f64(1.0 / 60.0)).unwrap();
                app.world_mut()
                    .resource_mut::<RuntimeInput>()
                    .clear_frame_edges();
            }
        };
        let player = |app: &mut crate::runtime::App| {
            let world = app.world_mut();
            let mut query =
                world.query::<(&PlatformerController, &crate::Transform)>();
            let (controller, transform) = query.single(world).unwrap();
            (*controller, transform.position)
        };
        tick(&mut app, 60);
        let (controller, start) = player(&mut app);
        assert!(controller.grounded, "{start:?}");
        // The ground's top edge is y = -1 and the body is 1 m tall.
        assert!((start[1] + 0.5).abs() < 0.02, "{start:?}");
        let world = app.world_mut();
        let tiles = world.query::<&TileOf>().iter(world).count();
        let colliders =
            world.query::<(&TileOf, &Collider)>().iter(world).count();
        assert_eq!(colliders, 7, "one collider per solid run");
        assert!(tiles > 40, "{tiles}");

        // Run right and jump over the two-tile step ahead.
        let mut input = app.world_mut().resource_mut::<RuntimeInput>();
        input.record_key(KeyCode::KeyD, true);
        input.record_key(KeyCode::Space, true);
        tick(&mut app, 20);
        assert!(!player(&mut app).0.grounded, "the jump left the ground");
        let mut frames = 20;
        while !player(&mut app).0.grounded {
            assert!(frames < 120, "never landed: {:?}", player(&mut app).1);
            tick(&mut app, 1);
            frames += 1;
        }
        let (_, moved) = player(&mut app);
        assert!(moved[0] > start[0] + 3.0, "{moved:?} after {frames}");

        std::fs::remove_dir_all(parent).unwrap();
    }

    /// Creates `template` in a temporary folder and loads its main scene.
    /// Returns the app, a ticker at 60 Hz, and the folder to remove.
    fn load_template(
        template: ProjectTemplate,
    ) -> (
        crate::runtime::App,
        impl Fn(&mut crate::runtime::App, u32),
        PathBuf,
    ) {
        use std::time::Duration;

        use crate::assets::AssetPlugin;
        use crate::runtime::{load_scene, App, RuntimeInput, SceneLoadMode};

        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project =
            create_project_from(&parent, "Template", template).unwrap();
        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        load_scene(
            app.world_mut(),
            project.root.join("scenes/main.rscene"),
            SceneLoadMode::Replace,
        )
        .unwrap();
        let tick = |app: &mut App, ticks| {
            for _ in 0..ticks {
                app.update(Duration::from_secs_f64(1.0 / 60.0)).unwrap();
                app.world_mut()
                    .resource_mut::<RuntimeInput>()
                    .clear_frame_edges();
            }
        };
        (app, tick, parent)
    }

    #[test]
    fn the_empty_template_has_no_named_objects_to_clash_with() {
        let (mut app, _, parent) = load_template(ProjectTemplate::Empty);
        let world = app.world_mut();
        let names: Vec<String> = world
            .query::<&rusting_core::components::Name>()
            .iter(world)
            .map(|name| name.0.clone())
            .collect();
        assert!(!names.iter().any(|name| name == "Cube"), "{names:?}");
        let cameras = world
            .query_filtered::<(), bevy_ecs::query::With<crate::runtime::Camera>>()
            .iter(world)
            .count();
        assert_eq!(cameras, 1);
        let _ = std::fs::remove_dir_all(parent);
    }

    #[test]
    fn template_names_round_trip() {
        for template in ProjectTemplate::ALL {
            assert_eq!(ProjectTemplate::parse(template.name()), Some(template));
        }
    }

    #[test]
    fn the_player_templates_stand_walk_and_frame_the_camera() {
        use crate::runtime::{Camera, KeyCode, PlayerController, RuntimeInput};

        for template in [
            ProjectTemplate::FirstPerson3d,
            ProjectTemplate::ThirdPerson3d,
        ] {
            let (mut app, tick, parent) = load_template(template);
            let player = |app: &mut crate::runtime::App| {
                let world = app.world_mut();
                let mut query =
                    world.query::<(&PlayerController, &crate::Transform)>();
                let (controller, transform) = query.single(world).unwrap();
                (*controller, transform.position)
            };
            tick(&mut app, 60);
            let (controller, start) = player(&mut app);
            assert!(controller.grounded, "{template:?} {start:?}");
            // The capsule is 1.8 m tall and the floor's top is y = 0.
            assert!((start[1] - 0.9).abs() < 0.05, "{template:?} {start:?}");

            app.world_mut()
                .resource_mut::<RuntimeInput>()
                .record_key(KeyCode::KeyW, true);
            tick(&mut app, 30);
            let (_, moved) = player(&mut app);
            assert!(moved[2] < start[2] - 1.0, "{template:?} {moved:?}");

            let world = app.world_mut();
            let camera = world
                .query_filtered::<&crate::Transform, bevy_ecs::query::With<Camera>>()
                .single(world)
                .unwrap()
                .position;
            if template == ProjectTemplate::ThirdPerson3d {
                assert!(camera[2] > 3.0 && camera[1] > 0.5, "{camera:?}");
            } else {
                assert_eq!(camera, [0.0, 0.7, 0.0]);
            }
            std::fs::remove_dir_all(parent).unwrap();
        }
    }

    #[test]
    fn the_sandbox_template_settles_its_pile_on_the_floor() {
        use crate::runtime::{Name, RigidBody, RigidBodyKind};

        let (mut app, tick, parent) =
            load_template(ProjectTemplate::PhysicsSandbox);
        let dynamic = |app: &mut crate::runtime::App| {
            let world = app.world_mut();
            let mut bodies: Vec<_> = world
                .query::<(&Name, &RigidBody, &crate::Transform)>()
                .iter(world)
                .filter(|(_, body, _)| body.kind == RigidBodyKind::Dynamic)
                .map(|(name, _, transform)| {
                    (name.0.clone(), transform.position)
                })
                .collect();
            bodies.sort_by(|a, b| a.0.cmp(&b.0));
            bodies
        };
        let start = dynamic(&mut app);
        assert_eq!(start.len(), 11);
        // Long enough for a rolling ball to reach a wall.
        tick(&mut app, 1200);
        let settled = dynamic(&mut app);
        tick(&mut app, 30);
        for ((name, before), (_, after)) in
            settled.iter().zip(dynamic(&mut app))
        {
            // Every body rests on the floor or the pile, not in it, and
            // inside the walls.
            assert!(before[1] > 0.45, "{name} sank: {before:?}");
            assert!(
                after[0].abs() < 15.0 && after[2].abs() < 15.0,
                "{name} left the floor: {after:?}"
            );
            if name.starts_with("Ball") {
                // Balls roll on with no rolling resistance.
                continue;
            }
            let moved = (0..3)
                .map(|axis| (after[axis] - before[axis]).abs())
                .fold(0.0, f32::max);
            assert!(moved < 0.02, "{name} still moves: {before:?} {after:?}");
        }
        let fell = start
            .iter()
            .zip(&settled)
            .any(|(a, b)| b.1[1] < a.1[1] - 1.0);
        assert!(fell, "the balls dropped onto the pile");
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn the_starter_template_wins_its_sample_scenario() {
        use crate::assets::AssetPlugin;
        use crate::runtime::{
            load_scene, App, RenderExtractPlugin, SceneLoadMode,
        };
        use crate::scenario::{run_scenario, Scenario};

        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project = create_project_from(
            &parent,
            "Coin Run",
            ProjectTemplate::parse("starter").unwrap(),
        )
        .unwrap();
        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        app.add_plugin(RenderExtractPlugin).unwrap();
        load_scene(
            app.world_mut(),
            project.root.join("scenes/main.rscene"),
            SceneLoadMode::Replace,
        )
        .unwrap();
        let scenario: Scenario = serde_json::from_str(include_str!(
            "../samples/starter_game/win.scenario.json"
        ))
        .unwrap();
        // The capture is skipped without Vulkan.
        let report = run_scenario(&mut app, &scenario, &parent);
        assert!(report.passed, "{:#?}", report.first_failure);
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn replays_of_the_starter_template_seek_through_snapshots() {
        use crate::assets::AssetPlugin;
        use crate::runtime::{
            load_scene, App, Counter, Name, RandomSeed, RenderExtractPlugin,
            ReplayError, ReplaySeeker, SceneLoadMode,
        };
        use crate::scenario::{run_scenario, Scenario, StepAction};

        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project = create_project_from(
            &parent,
            "Coin Run",
            ProjectTemplate::parse("starter").unwrap(),
        )
        .unwrap();
        let scene = project.root.join("scenes/main.rscene");
        let make_app = || {
            let mut app = App::new();
            app.add_plugin(AssetPlugin)?;
            app.add_plugin(RenderExtractPlugin)?;
            load_scene(app.world_mut(), &scene, SceneLoadMode::Replace)
                .expect("the starter scene loads");
            Ok(app)
        };
        // Record the sample scenario playing the level.
        let mut scenario: Scenario = serde_json::from_str(include_str!(
            "../samples/starter_game/win.scenario.json"
        ))
        .unwrap();
        scenario
            .steps
            .retain(|step| !matches!(step.action, StepAction::Capture(_)));
        let mut app = make_app().unwrap();
        app.world_mut().resource_mut::<RandomSeed>().0 = scenario.seed;
        app.start_recording();
        assert!(run_scenario(&mut app, &scenario, &parent).passed);
        let replay = app.finish_recording().unwrap();
        let counter = |app: &App, wanted: &str| {
            let world = app.world();
            let mut counters = world.try_query::<(&Name, &Counter)>().unwrap();
            counters
                .iter(world)
                .find(|(name, _)| name.0 == wanted)
                .map(|(_, counter)| counter.value)
                .unwrap()
        };

        let mut seeker =
            ReplaySeeker::new(replay.clone(), 50, make_app).unwrap();
        assert_eq!(seeker.seek(240).unwrap(), 240);
        assert_eq!(seeker.snapshot_count(), 4);
        assert_eq!(counter(seeker.app(), "Win"), 1);
        // Back to before the first checkpoint, then between checkpoints.
        assert_eq!(seeker.seek(10).unwrap(), 10);
        assert_eq!(counter(seeker.app(), "Score"), 0);
        assert_eq!(seeker.seek(190).unwrap(), 190);
        assert_eq!(counter(seeker.app(), "Score"), 5);
        assert_eq!(counter(seeker.app(), "Win"), 0);
        assert_eq!(seeker.seek(60).unwrap(), 60);
        assert_eq!(seeker.seek(240).unwrap(), 240);
        assert_eq!(counter(seeker.app(), "Win"), 1);
        assert_eq!(seeker.snapshot_count(), 4);

        // Past a checkpoint, each tick still checks the recorded hash.
        let mut tampered = replay;
        let index = tampered
            .hashes
            .iter()
            .position(|&(tick, _)| tick == 170)
            .unwrap();
        tampered.hashes[index].1 ^= 1;
        let mut seeker = ReplaySeeker::new(tampered, 50, make_app).unwrap();
        assert_eq!(seeker.seek(160).unwrap(), 160);
        assert!(matches!(
            seeker.seek(240),
            Err(ReplayError::Diverged { tick: 170 })
        ));
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn invalid_project_names_are_rejected() {
        for name in ["", "../game", "game/name", ".", "..", "CON", "game❤"] {
            assert!(matches!(
                validate_project_name(name),
                Err(ProjectError::InvalidName)
            ));
        }
    }

    #[test]
    fn unversioned_project_is_backed_up_and_migrated() {
        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project = create_project(&parent, "Legacy Game").unwrap();
        let manifest_path = project.root.join("project.json");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap())
                .unwrap();
        legacy.as_object_mut().unwrap().remove("format_version");
        legacy.as_object_mut().unwrap().remove("binary_name");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let migrated = open_project(&project.root).unwrap();

        assert_eq!(migrated.manifest.format_version, PROJECT_FORMAT_VERSION);
        assert_eq!(migrated.manifest.binary_name, "legacy_game");
        assert!(project.root.join("project.json.v0.backup").is_file());
        let stored: ProjectManifest =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap())
                .unwrap();
        assert_eq!(stored, migrated.manifest);
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn editor_preferences_round_trip_and_fall_back_to_defaults() {
        let folder = std::env::temp_dir()
            .join(format!("rusting_prefs_{}", Uuid::new_v4()));
        let path = folder.join("editor_preferences.json");
        assert_eq!(
            EditorPreferences::load_from(&path),
            EditorPreferences::default()
        );

        let preferences = EditorPreferences {
            ui_scale: 1.25,
            font_scale: 1.15,
        };
        preferences.save_to(&path).unwrap();
        assert_eq!(EditorPreferences::load_from(&path), preferences);

        // Files saved before a field existed keep their other settings.
        std::fs::write(&path, "{\"ui_scale\": 40.0}").unwrap();
        let old = EditorPreferences::load_from(&path);
        assert_eq!((old.ui_scale, old.font_scale), (3.0, 1.0));
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(
            EditorPreferences::load_from(&path),
            EditorPreferences::default()
        );
        std::fs::remove_dir_all(folder).unwrap();
    }

    /// Runs a template's own `update` as its game binary would.
    struct TemplateUpdate(fn(&mut bevy_ecs::prelude::World));

    impl crate::runtime::Plugin for TemplateUpdate {
        fn build(
            &self,
            app: &mut crate::App,
        ) -> Result<(), crate::runtime::AppError> {
            app.add_system(crate::runtime::ScheduleStage::Update, self.0);
            Ok(())
        }
    }

    #[test]
    fn the_puzzle_template_ships_code_whose_scenario_passes() {
        let parent = std::env::temp_dir()
            .join(format!("rusting-puzzle-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project =
            create_project_from(&parent, "Box Push", ProjectTemplate::Puzzle)
                .unwrap();
        assert_eq!(
            std::fs::read_to_string(&project.code_path).unwrap(),
            include_str!("templates/puzzle.rs")
        );
        crate::project_runner::run_project_scenario(
            project.scene_path.clone(),
            TemplateUpdate(|world| {
                let time = *world.resource::<crate::runtime::FrameTime>();
                super::puzzle_template::update(
                    &mut crate::project_runner::GameScene { world },
                    &time,
                );
            }),
            project.root.join("tests/solve.json"),
            Some(project.root.join("report.json")),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(parent);
    }
}
