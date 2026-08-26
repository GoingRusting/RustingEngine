//! Project creation, validation, opening, and recent-project storage.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::{
    SceneCamera, SceneDocument, SceneEntity, SceneMaterial, SceneMaterialData,
    SceneMaterialModel, SceneMesh, SceneMeshRenderer, SceneProjection,
    SceneTransform, SCENE_FORMAT_VERSION,
};

/// Current version of `project.json` written by the editor.
pub const PROJECT_FORMAT_VERSION: u32 = 1;

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
    let result =
        write_project_template(&temporary, name.trim()).and_then(|()| {
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
        std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
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

fn write_project_template(root: &Path, name: &str) -> Result<(), ProjectError> {
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
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nrusting_engine = {{ {engine_dependency}, default-features = false, features = [\"window\"] }}\n\n[workspace]\n"
    );
    std::fs::write(root.join("Cargo.toml"), cargo)?;

    let manifest = ProjectManifest {
        format_version: PROJECT_FORMAT_VERSION,
        name: name.into(),
        main_scene: "scenes/main.rscene".into(),
        cooked_scene: "build/main.rscene.bin".into(),
        binary_name: package_name.clone(),
    };
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    std::fs::write(root.join("src/main.rs"), default_game_source())?;
    std::fs::write(
        root.join("scenes/main.rscene"),
        serde_json::to_vec_pretty(&default_scene(name))?,
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
                        model: SceneMaterialModel::Pbr,
                        base_color: [0.1, 0.45, 0.95, 1.0],
                        emissive: [0.0; 3],
                        metallic: 0.0,
                        roughness: 0.5,
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
                }),
                visible: None,
                physics_body: None,
                rigid_body: None,
                collider: None,
                collision_layers: None,
                gpu_physics_watch: None,
                components: BTreeMap::new(),
            },
        ],
    }
}

/// Uses the process folder only for editor settings, never project creation.
fn default_project_parent() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn recent_projects_path() -> PathBuf {
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
        .join("rusting_engine/recent_projects.json")
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
    std::fs::write(path, serde_json::to_vec_pretty(recent)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_creation_writes_a_complete_openable_template() {
        let parent = std::env::temp_dir()
            .join(format!("rusting-project-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project = create_project(&parent, "Example Game").unwrap();

        assert_eq!(project.manifest.name, "Example Game");
        assert!(project.root.join("Cargo.toml").is_file());
        assert!(project.root.join("src/main.rs").is_file());
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
}
