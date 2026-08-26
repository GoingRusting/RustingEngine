//! Versioned, editor-authored scene files and compiled runtime scene data.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Resource, World};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::assets::{
    AssetServer, Handle, MaterialAsset, MaterialModel, TextureAsset,
};
use crate::Transform;

use super::{
    Camera, Collider, CollisionLayers, GpuPhysicsWatch, MeshRenderer, Name,
    ObjectClasses, Parent, PhysicsBody, Projection, RigidBody, SceneId,
    Visibility,
};

pub const SCENE_FORMAT_VERSION: u32 = 3;
const COMPILED_MAGIC: &[u8; 8] = b"RSCENE01";

#[derive(Debug)]
pub enum SceneIoError {
    Io(std::io::Error),
    Source(serde_json::Error),
    Compiled(Box<bincode::ErrorKind>),
    UnsupportedVersion(u32),
    DuplicateComponent(String),
    UnknownComponent(String),
    Component { name: String, message: String },
    MissingAssetServer,
    MissingAssetPath(PathBuf),
    AssetLoad { path: PathBuf, message: String },
    UnsavedMesh(u64),
    UnsavedTexture(u64),
    DuplicateEntity(Uuid),
    DuplicateName(String),
    MissingParent(Uuid),
    HierarchyCycle(Uuid),
    Runtime(super::AppError),
}

impl Display for SceneIoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Source(error) => Display::fmt(error, formatter),
            Self::Compiled(error) => Display::fmt(error, formatter),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported scene format version {version}")
            }
            Self::DuplicateComponent(name) => {
                write!(
                    formatter,
                    "scene component `{name}` is already registered"
                )
            }
            Self::UnknownComponent(name) => {
                write!(formatter, "scene uses unregistered component `{name}`")
            }
            Self::Component { name, message } => {
                write!(
                    formatter,
                    "failed to process component `{name}`: {message}"
                )
            }
            Self::MissingAssetServer => formatter
                .write_str("AssetPlugin must be installed before scene I/O"),
            Self::MissingAssetPath(path) => {
                write!(
                    formatter,
                    "scene asset `{}` is not loaded",
                    path.display()
                )
            }
            Self::AssetLoad { path, message } => {
                write!(
                    formatter,
                    "could not load scene asset `{}`: {message}",
                    path.display()
                )
            }
            Self::UnsavedMesh(key) => {
                write!(
                    formatter,
                    "mesh {key} has no asset path and cannot be saved"
                )
            }
            Self::UnsavedTexture(key) => {
                write!(
                    formatter,
                    "texture {key} has no asset path and cannot be saved"
                )
            }
            Self::DuplicateEntity(id) => {
                write!(formatter, "scene contains duplicate object ID {id}")
            }
            Self::DuplicateName(name) => {
                write!(
                    formatter,
                    "scene contains duplicate object name `{name}`"
                )
            }
            Self::MissingParent(id) => {
                write!(formatter, "scene parent {id} does not exist")
            }
            Self::HierarchyCycle(id) => {
                write!(formatter, "scene object {id} has a parent cycle")
            }
            Self::Runtime(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for SceneIoError {}

impl From<std::io::Error> for SceneIoError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for SceneIoError {
    fn from(error: serde_json::Error) -> Self {
        Self::Source(error)
    }
}

impl From<Box<bincode::ErrorKind>> for SceneIoError {
    fn from(error: Box<bincode::ErrorKind>) -> Self {
        Self::Compiled(error)
    }
}

impl From<super::AppError> for SceneIoError {
    fn from(error: super::AppError) -> Self {
        Self::Runtime(error)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneDocument {
    #[serde(default)]
    pub format_version: u32,
    pub name: String,
    pub entities: Vec<SceneEntity>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneEntity {
    pub id: Uuid,
    pub parent: Option<Uuid>,
    pub name: Option<String>,
    #[serde(default)]
    pub classes: Vec<String>,
    pub transform: Option<SceneTransform>,
    pub mesh_renderer: Option<SceneMeshRenderer>,
    pub camera: Option<SceneCamera>,
    pub visible: Option<bool>,
    #[serde(default)]
    pub physics_body: Option<PhysicsBody>,
    #[serde(default)]
    pub rigid_body: Option<RigidBody>,
    #[serde(default)]
    pub collider: Option<Collider>,
    #[serde(default)]
    pub collision_layers: Option<CollisionLayers>,
    #[serde(default)]
    pub gpu_physics_watch: Option<GpuPhysicsWatch>,
    #[serde(default)]
    pub components: BTreeMap<String, String>,
}

/// Cooked version 1 did not store programmable GPU physics watches.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct LegacySceneDocumentV1 {
    format_version: u32,
    name: String,
    entities: Vec<LegacySceneEntityV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct LegacySceneEntityV1 {
    id: Uuid,
    parent: Option<Uuid>,
    name: Option<String>,
    transform: Option<SceneTransform>,
    mesh_renderer: Option<SceneMeshRenderer>,
    camera: Option<SceneCamera>,
    visible: Option<bool>,
    physics_body: Option<PhysicsBody>,
    rigid_body: Option<RigidBody>,
    collider: Option<Collider>,
    collision_layers: Option<CollisionLayers>,
    components: BTreeMap<String, String>,
}

impl From<LegacySceneDocumentV1> for SceneDocument {
    fn from(document: LegacySceneDocumentV1) -> Self {
        Self {
            format_version: document.format_version,
            name: document.name,
            entities: document
                .entities
                .into_iter()
                .map(|entity| SceneEntity {
                    id: entity.id,
                    parent: entity.parent,
                    name: entity.name,
                    classes: Vec::new(),
                    transform: entity.transform,
                    mesh_renderer: entity.mesh_renderer,
                    camera: entity.camera,
                    visible: entity.visible,
                    physics_body: entity.physics_body,
                    rigid_body: entity.rigid_body,
                    collider: entity.collider,
                    collision_layers: entity.collision_layers,
                    gpu_physics_watch: None,
                    components: entity.components,
                })
                .collect(),
        }
    }
}

/// Cooked version 2 stored GPU watches but did not store object classes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct LegacySceneDocumentV2 {
    format_version: u32,
    name: String,
    entities: Vec<LegacySceneEntityV2>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct LegacySceneEntityV2 {
    id: Uuid,
    parent: Option<Uuid>,
    name: Option<String>,
    transform: Option<SceneTransform>,
    mesh_renderer: Option<SceneMeshRenderer>,
    camera: Option<SceneCamera>,
    visible: Option<bool>,
    physics_body: Option<PhysicsBody>,
    rigid_body: Option<RigidBody>,
    collider: Option<Collider>,
    collision_layers: Option<CollisionLayers>,
    gpu_physics_watch: Option<GpuPhysicsWatch>,
    components: BTreeMap<String, String>,
}

impl From<LegacySceneDocumentV2> for SceneDocument {
    fn from(document: LegacySceneDocumentV2) -> Self {
        Self {
            format_version: document.format_version,
            name: document.name,
            entities: document
                .entities
                .into_iter()
                .map(|entity| SceneEntity {
                    id: entity.id,
                    parent: entity.parent,
                    name: entity.name,
                    classes: Vec::new(),
                    transform: entity.transform,
                    mesh_renderer: entity.mesh_renderer,
                    camera: entity.camera,
                    visible: entity.visible,
                    physics_body: entity.physics_body,
                    rigid_body: entity.rigid_body,
                    collider: entity.collider,
                    collision_layers: entity.collision_layers,
                    gpu_physics_watch: entity.gpu_physics_watch,
                    components: entity.components,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl From<Transform> for SceneTransform {
    fn from(transform: Transform) -> Self {
        Self {
            position: transform.position,
            rotation: transform.rotation,
            scale: transform.scale,
        }
    }
}

impl From<SceneTransform> for Transform {
    fn from(transform: SceneTransform) -> Self {
        Self {
            position: transform.position,
            rotation: transform.rotation,
            scale: transform.scale,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SceneMesh {
    BuiltinCube,
    BuiltinSphere,
    AssetPath(PathBuf),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SceneMaterial {
    BuiltinError,
    Inline(SceneMaterialData),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneMaterialData {
    pub model: SceneMaterialModel,
    pub base_color: [f32; 4],
    pub emissive: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    pub base_color_texture: Option<PathBuf>,
    pub normal_texture: Option<PathBuf>,
    pub metallic_roughness_texture: Option<PathBuf>,
    pub occlusion_texture: Option<PathBuf>,
    pub emissive_texture: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SceneMaterialModel {
    Pbr,
    Unlit,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneMeshRenderer {
    pub mesh: SceneMesh,
    pub material: SceneMaterial,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneCamera {
    pub projection: SceneProjection,
    pub active: bool,
    pub priority: i32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum SceneProjection {
    Perspective {
        vertical_fov_radians: f32,
        near: f32,
        far: f32,
    },
    Orthographic {
        vertical_size: f32,
        near: f32,
        far: f32,
    },
}

#[derive(Clone, Copy)]
struct ComponentRegistration {
    capture: fn(&World, Entity) -> Result<Option<String>, SceneIoError>,
    restore: fn(&mut World, Entity, &str) -> Result<(), SceneIoError>,
    insert_default: fn(&mut World, Entity),
    remove: fn(&mut World, Entity),
}

/// Allowlist for game-defined, compiled Rust components stored in scenes.
#[derive(Resource, Default)]
pub struct SceneComponentRegistry {
    registrations: BTreeMap<String, ComponentRegistration>,
}

impl SceneComponentRegistry {
    pub fn register<T>(
        &mut self,
        name: impl Into<String>,
    ) -> Result<(), SceneIoError>
    where
        T: Component + Serialize + DeserializeOwned + Default,
    {
        let name = name.into();
        if self.registrations.contains_key(&name) {
            return Err(SceneIoError::DuplicateComponent(name));
        }
        self.registrations.insert(
            name,
            ComponentRegistration {
                capture: capture_component::<T>,
                restore: restore_component::<T>,
                insert_default: insert_default_component::<T>,
                remove: remove_component::<T>,
            },
        );
        Ok(())
    }

    #[must_use]
    pub fn names(&self) -> impl ExactSizeIterator<Item = &str> {
        self.registrations.keys().map(String::as_str)
    }
}

/// Returns serialized values for all registered components present on an
/// entity. The editor uses these strings as an initial generic inspector until
/// typed widgets are registered for a component.
pub fn registered_component_values(
    world: &World,
    entity: Entity,
) -> Result<Vec<(String, String)>, SceneIoError> {
    let registrations = world
        .resource::<SceneComponentRegistry>()
        .registrations
        .clone();
    registrations
        .into_iter()
        .filter_map(|(name, registration)| {
            (registration.capture)(world, entity)
                .transpose()
                .map(|result| result.map(|value| (name, value)))
        })
        .collect()
}

#[must_use]
pub fn registered_component_names(world: &World) -> Vec<String> {
    world
        .resource::<SceneComponentRegistry>()
        .registrations
        .keys()
        .cloned()
        .collect()
}

pub fn set_registered_component(
    world: &mut World,
    entity: Entity,
    name: &str,
    serialized: &str,
) -> Result<(), SceneIoError> {
    let registration = world
        .resource::<SceneComponentRegistry>()
        .registrations
        .get(name)
        .copied()
        .ok_or_else(|| SceneIoError::UnknownComponent(name.into()))?;
    (registration.restore)(world, entity, serialized)
}

pub fn add_registered_component(
    world: &mut World,
    entity: Entity,
    name: &str,
) -> Result<(), SceneIoError> {
    let registration = world
        .resource::<SceneComponentRegistry>()
        .registrations
        .get(name)
        .copied()
        .ok_or_else(|| SceneIoError::UnknownComponent(name.into()))?;
    (registration.insert_default)(world, entity);
    Ok(())
}

pub fn remove_registered_component(
    world: &mut World,
    entity: Entity,
    name: &str,
) -> Result<(), SceneIoError> {
    let registration = world
        .resource::<SceneComponentRegistry>()
        .registrations
        .get(name)
        .copied()
        .ok_or_else(|| SceneIoError::UnknownComponent(name.into()))?;
    (registration.remove)(world, entity);
    Ok(())
}

fn capture_component<T>(
    world: &World,
    entity: Entity,
) -> Result<Option<String>, SceneIoError>
where
    T: Component + Serialize,
{
    world
        .get::<T>(entity)
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| SceneIoError::Component {
            name: std::any::type_name::<T>().into(),
            message: error.to_string(),
        })
}

fn restore_component<T>(
    world: &mut World,
    entity: Entity,
    serialized: &str,
) -> Result<(), SceneIoError>
where
    T: Component + DeserializeOwned,
{
    let component = serde_json::from_str::<T>(serialized).map_err(|error| {
        SceneIoError::Component {
            name: std::any::type_name::<T>().into(),
            message: error.to_string(),
        }
    })?;
    world.entity_mut(entity).insert(component);
    Ok(())
}

fn insert_default_component<T>(world: &mut World, entity: Entity)
where
    T: Component + Default,
{
    world.entity_mut(entity).insert(T::default());
}

fn remove_component<T>(world: &mut World, entity: Entity)
where
    T: Component,
{
    world.entity_mut(entity).remove::<T>();
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SceneLoadMode {
    #[default]
    Replace,
    Additive,
}

pub fn scene_document(
    world: &mut World,
    name: impl Into<String>,
) -> Result<SceneDocument, SceneIoError> {
    let registrations = world
        .resource::<SceneComponentRegistry>()
        .registrations
        .clone();
    let mut query = world.query::<(
        Entity,
        &SceneId,
        Option<&Parent>,
        Option<&Name>,
        Option<&ObjectClasses>,
        Option<&Transform>,
        Option<&MeshRenderer>,
        Option<&Camera>,
        Option<&Visibility>,
        Option<&PhysicsBody>,
        Option<&RigidBody>,
        Option<&Collider>,
        Option<&CollisionLayers>,
        Option<&GpuPhysicsWatch>,
    )>();
    let raw = query
        .iter(world)
        .map(
            |(
                entity,
                id,
                parent,
                name,
                classes,
                transform,
                renderer,
                camera,
                visibility,
                physics_body,
                rigid_body,
                collider,
                collision_layers,
                gpu_physics_watch,
            )| {
                (
                    entity,
                    *id,
                    parent.copied(),
                    name.cloned(),
                    classes.cloned(),
                    transform.copied(),
                    renderer.copied(),
                    camera.copied(),
                    visibility.copied(),
                    physics_body.cloned(),
                    rigid_body.copied(),
                    collider.copied(),
                    collision_layers.copied(),
                    gpu_physics_watch.cloned(),
                )
            },
        )
        .collect::<Vec<_>>();
    let ids = raw
        .iter()
        .map(|(entity, id, ..)| (*entity, id.0))
        .collect::<HashMap<_, _>>();
    let assets = world
        .get_resource::<AssetServer>()
        .ok_or(SceneIoError::MissingAssetServer)?;
    let mut entities = Vec::with_capacity(raw.len());
    for (
        entity,
        id,
        parent,
        name,
        classes,
        transform,
        renderer,
        camera,
        visibility,
        physics_body,
        rigid_body,
        collider,
        collision_layers,
        gpu_physics_watch,
    ) in raw
    {
        let parent = parent
            .map(|parent| {
                ids.get(&parent.0)
                    .copied()
                    .ok_or(SceneIoError::MissingParent(id.0))
            })
            .transpose()?;
        let mesh_renderer = renderer
            .map(|renderer| scene_renderer(renderer, assets))
            .transpose()?;
        let mut components = BTreeMap::new();
        for (component_name, registration) in &registrations {
            if let Some(value) = (registration.capture)(world, entity)? {
                components.insert(component_name.clone(), value);
            }
        }
        entities.push(SceneEntity {
            id: id.0,
            parent,
            name: name.map(|name| name.0),
            classes: classes.unwrap_or_default().names,
            transform: transform.map(Into::into),
            mesh_renderer,
            camera: camera.map(scene_camera),
            visible: visibility.map(|visibility| visibility.visible),
            physics_body,
            rigid_body,
            collider,
            collision_layers,
            gpu_physics_watch,
            components,
        });
    }
    entities.sort_by_key(|entity| entity.id);
    let document = SceneDocument {
        format_version: SCENE_FORMAT_VERSION,
        name: name.into(),
        entities,
    };
    validate_scene_structure(&document)?;
    Ok(document)
}

pub fn save_scene(
    world: &mut World,
    path: impl AsRef<Path>,
    name: impl Into<String>,
) -> Result<(), SceneIoError> {
    let mut document = scene_document(world, name)?;
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        relativize_scene_assets(&mut document, parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&document)?)?;
    Ok(())
}

pub fn load_scene(
    world: &mut World,
    path: impl AsRef<Path>,
    mode: SceneLoadMode,
) -> Result<usize, SceneIoError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let mut document = decode_scene(&bytes)?;
    if let Some(parent) = path.parent() {
        absolutize_scene_assets(&mut document, parent)?;
    }
    load_scene_document(world, &document, mode)
}

pub fn cook_scene(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<(), SceneIoError> {
    let source = source.as_ref();
    let destination = destination.as_ref();
    let mut document: SceneDocument =
        serde_json::from_slice(&std::fs::read(source)?)?;
    migrate_scene_document(&mut document)?;
    validate_version(&document)?;
    validate_scene_structure(&document)?;
    if let Some(parent) = source.parent() {
        absolutize_scene_assets(&mut document, parent)?;
    }
    if let Some(parent) = destination.parent() {
        relativize_scene_assets(&mut document, parent)?;
    }
    let mut bytes = COMPILED_MAGIC.to_vec();
    bytes.extend(bincode::serialize(&document)?);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(destination, bytes)?;
    Ok(())
}

/// Applies one path conversion to every mesh and texture in a scene.
fn map_scene_asset_paths(
    document: &mut SceneDocument,
    mut convert: impl FnMut(&Path) -> Result<PathBuf, SceneIoError>,
) -> Result<(), SceneIoError> {
    for entity in &mut document.entities {
        let Some(renderer) = &mut entity.mesh_renderer else {
            continue;
        };
        if let SceneMesh::AssetPath(path) = &mut renderer.mesh {
            *path = convert(path)?;
        }
        let SceneMaterial::Inline(material) = &mut renderer.material else {
            continue;
        };
        for current in [
            &mut material.base_color_texture,
            &mut material.normal_texture,
            &mut material.metallic_roughness_texture,
            &mut material.occlusion_texture,
            &mut material.emissive_texture,
        ]
        .into_iter()
        .flatten()
        {
            *current = convert(current)?;
        }
    }
    Ok(())
}

/// Converts scene-relative asset paths to normalized absolute paths.
fn absolutize_scene_assets(
    document: &mut SceneDocument,
    scene_folder: &Path,
) -> Result<(), SceneIoError> {
    let base = absolute_path(scene_folder)?;
    map_scene_asset_paths(document, |path| {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(normalize_lexical(&base.join(path)))
        }
    })
}

/// Converts absolute paths to paths relative to the scene being written.
fn relativize_scene_assets(
    document: &mut SceneDocument,
    scene_folder: &Path,
) -> Result<(), SceneIoError> {
    let base = absolute_path(scene_folder)?;
    map_scene_asset_paths(document, |path| {
        let target = absolute_path(path)?;
        Ok(path_relative_to(&base, &target).unwrap_or(target))
    })
}

fn absolute_path(path: &Path) -> Result<PathBuf, SceneIoError> {
    if let Ok(path) = path.canonicalize() {
        return Ok(path);
    }
    if path.is_absolute() {
        Ok(normalize_lexical(path))
    } else {
        Ok(normalize_lexical(&std::env::current_dir()?.join(path)))
    }
}

/// Removes `.` and `..` without requiring the final file to exist.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// Builds a relative path when both paths use the same platform root.
fn path_relative_to(base: &Path, target: &Path) -> Option<PathBuf> {
    let base = base.components().collect::<Vec<_>>();
    let target = target.components().collect::<Vec<_>>();
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in &base[common..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &target[common..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

pub fn load_scene_document(
    world: &mut World,
    document: &SceneDocument,
    mode: SceneLoadMode,
) -> Result<usize, SceneIoError> {
    let mut migrated = document.clone();
    migrate_scene_document(&mut migrated)?;
    let document = &migrated;
    validate_version(document)?;
    validate_scene_structure(document)?;
    let registrations = world
        .resource::<SceneComponentRegistry>()
        .registrations
        .clone();
    for entity in &document.entities {
        for name in entity.components.keys() {
            if !registrations.contains_key(name) {
                return Err(SceneIoError::UnknownComponent(name.clone()));
            }
        }
    }

    // Resolve every referenced asset before replacing the current world. A
    // missing asset must not erase the scene that is already open.
    let prepared = prepare_assets(world, document)?;

    if mode == SceneLoadMode::Replace {
        let mut query =
            world.query_filtered::<Entity, bevy_ecs::query::With<SceneId>>();
        let entities = query.iter(world).collect::<Vec<_>>();
        for entity in entities {
            world.despawn(entity);
        }
    }

    let mut spawned = HashMap::new();
    for (scene_entity, renderer) in document.entities.iter().zip(prepared) {
        let mut entity = world.spawn(SceneId(scene_entity.id));
        if let Some(name) = &scene_entity.name {
            entity.insert(Name(name.clone()));
        }
        if !scene_entity.classes.is_empty() {
            entity.insert(ObjectClasses::new(scene_entity.classes.clone()));
        }
        if let Some(transform) = scene_entity.transform {
            entity.insert(Transform::from(transform));
        }
        if let Some(renderer) = renderer {
            entity.insert(renderer);
        }
        if let Some(camera) = scene_entity.camera {
            entity.insert(runtime_camera(camera));
        }
        if let Some(visible) = scene_entity.visible {
            entity.insert(Visibility { visible });
        }
        if let Some(physics_body) = &scene_entity.physics_body {
            entity.insert(physics_body.clone());
        }
        if let Some(rigid_body) = scene_entity.rigid_body {
            entity.insert(rigid_body);
        }
        if let Some(collider) = scene_entity.collider {
            entity.insert(collider);
        }
        if let Some(collision_layers) = scene_entity.collision_layers {
            entity.insert(collision_layers);
        }
        if let Some(gpu_physics_watch) = &scene_entity.gpu_physics_watch {
            entity.insert(gpu_physics_watch.clone());
        }
        spawned.insert(scene_entity.id, entity.id());
    }

    for scene_entity in &document.entities {
        let entity = spawned[&scene_entity.id];
        if let Some(parent) = scene_entity.parent {
            let parent = spawned
                .get(&parent)
                .copied()
                .ok_or(SceneIoError::MissingParent(parent))?;
            super::hierarchy::set_parent(world, entity, parent)?;
        }
        for (name, serialized) in &scene_entity.components {
            let registration = registrations
                .get(name)
                .ok_or_else(|| SceneIoError::UnknownComponent(name.clone()))?;
            (registration.restore)(world, entity, serialized)?;
        }
    }
    Ok(document.entities.len())
}

/// Checks every stable ID and parent chain before replacing the open scene.
fn validate_scene_structure(
    document: &SceneDocument,
) -> Result<(), SceneIoError> {
    let mut parents = HashMap::new();
    let mut names = HashSet::new();
    for entity in &document.entities {
        if parents.insert(entity.id, entity.parent).is_some() {
            return Err(SceneIoError::DuplicateEntity(entity.id));
        }
        if let Some(name) = entity.name.as_deref() {
            if !names.insert(name) {
                return Err(SceneIoError::DuplicateName(name.to_owned()));
            }
        }
    }
    for entity in &document.entities {
        let mut ancestor = entity.parent;
        let mut visited = HashSet::new();
        while let Some(id) = ancestor {
            if !visited.insert(id) || id == entity.id {
                return Err(SceneIoError::HierarchyCycle(entity.id));
            }
            ancestor = parents
                .get(&id)
                .copied()
                .ok_or(SceneIoError::MissingParent(id))?;
        }
    }
    Ok(())
}

fn decode_scene(bytes: &[u8]) -> Result<SceneDocument, SceneIoError> {
    let mut document = if let Some(compiled) =
        bytes.strip_prefix(COMPILED_MAGIC)
    {
        match bincode::deserialize(compiled) {
            Ok(document) => document,
            Err(current_error) => {
                if let Ok(legacy) =
                    bincode::deserialize::<LegacySceneDocumentV2>(compiled)
                {
                    legacy.into()
                } else {
                    let legacy =
                        bincode::deserialize::<LegacySceneDocumentV1>(compiled)
                            .map_err(|_| current_error)?;
                    legacy.into()
                }
            }
        }
    } else {
        serde_json::from_slice(bytes)?
    };
    migrate_scene_document(&mut document)?;
    validate_version(&document)?;
    Ok(document)
}

/// Upgrades old text-scene shapes before normal validation and loading.
fn migrate_scene_document(
    document: &mut SceneDocument,
) -> Result<(), SceneIoError> {
    match document.format_version {
        0..=2 => {
            // Versions before programmable GPU watches use safe defaults for
            // the fields that were added later.
            document.format_version = SCENE_FORMAT_VERSION;
            Ok(())
        }
        SCENE_FORMAT_VERSION => Ok(()),
        version => Err(SceneIoError::UnsupportedVersion(version)),
    }
}

fn validate_version(document: &SceneDocument) -> Result<(), SceneIoError> {
    if document.format_version == SCENE_FORMAT_VERSION {
        Ok(())
    } else {
        Err(SceneIoError::UnsupportedVersion(document.format_version))
    }
}

fn scene_renderer(
    renderer: MeshRenderer,
    assets: &AssetServer,
) -> Result<SceneMeshRenderer, SceneIoError> {
    let mesh = if renderer.mesh == assets.fallback_mesh {
        SceneMesh::BuiltinCube
    } else if renderer.mesh == assets.builtin_sphere {
        SceneMesh::BuiltinSphere
    } else if let Some(path) = assets.meshes.path(renderer.mesh) {
        SceneMesh::AssetPath(path.to_path_buf())
    } else {
        return Err(SceneIoError::UnsavedMesh(renderer.mesh.key()));
    };
    let material = if renderer.material == assets.fallback_material {
        SceneMaterial::BuiltinError
    } else {
        let material = assets
            .materials
            .get(renderer.material)
            .ok_or(SceneIoError::MissingAssetServer)?;
        SceneMaterial::Inline(scene_material(material, assets)?)
    };
    Ok(SceneMeshRenderer {
        mesh,
        material,
        cast_shadows: renderer.cast_shadows,
        receive_shadows: renderer.receive_shadows,
    })
}

fn scene_material(
    material: &MaterialAsset,
    assets: &AssetServer,
) -> Result<SceneMaterialData, SceneIoError> {
    let texture_path = |texture: Option<Handle<TextureAsset>>| {
        texture
            .map(|handle| {
                assets
                    .textures
                    .path(handle)
                    .map(Path::to_path_buf)
                    .ok_or(SceneIoError::UnsavedTexture(handle.key()))
            })
            .transpose()
    };
    Ok(SceneMaterialData {
        model: match material.model {
            MaterialModel::Pbr => SceneMaterialModel::Pbr,
            MaterialModel::Unlit => SceneMaterialModel::Unlit,
        },
        base_color: material.base_color,
        emissive: material.emissive,
        metallic: material.metallic,
        roughness: material.roughness,
        base_color_texture: texture_path(material.base_color_texture)?,
        normal_texture: texture_path(material.normal_texture)?,
        metallic_roughness_texture: texture_path(
            material.metallic_roughness_texture,
        )?,
        occlusion_texture: texture_path(material.occlusion_texture)?,
        emissive_texture: texture_path(material.emissive_texture)?,
    })
}

fn prepare_assets(
    world: &mut World,
    document: &SceneDocument,
) -> Result<Vec<Option<MeshRenderer>>, SceneIoError> {
    let mut assets = world
        .get_resource_mut::<AssetServer>()
        .ok_or(SceneIoError::MissingAssetServer)?;
    document
        .entities
        .iter()
        .map(|entity| {
            let Some(renderer) = &entity.mesh_renderer else {
                return Ok(None);
            };
            let mesh = match &renderer.mesh {
                SceneMesh::BuiltinCube => assets.fallback_mesh,
                SceneMesh::BuiltinSphere => assets.builtin_sphere,
                SceneMesh::AssetPath(path) => {
                    if let Some(handle) = assets.meshes.handle_for_path(path) {
                        handle
                    } else {
                        assets.load_mesh(path).map_err(|error| {
                            SceneIoError::AssetLoad {
                                path: path.clone(),
                                message: error.to_string(),
                            }
                        })?
                    }
                }
            };
            let material = match &renderer.material {
                SceneMaterial::BuiltinError => assets.fallback_material,
                SceneMaterial::Inline(material) => {
                    let material = runtime_material(material, &mut assets)?;
                    let existing = assets.materials.iter().find_map(
                        |(handle, existing)| {
                            (*existing == material).then_some(handle)
                        },
                    );
                    existing
                        .unwrap_or_else(|| assets.materials.insert(material))
                }
            };
            Ok(Some(MeshRenderer {
                mesh,
                material,
                cast_shadows: renderer.cast_shadows,
                receive_shadows: renderer.receive_shadows,
            }))
        })
        .collect()
}

fn runtime_material(
    material: &SceneMaterialData,
    assets: &mut AssetServer,
) -> Result<MaterialAsset, SceneIoError> {
    fn texture(
        assets: &mut AssetServer,
        path: &Option<PathBuf>,
    ) -> Result<Option<Handle<TextureAsset>>, SceneIoError> {
        path.as_ref()
            .map(|path| {
                if let Some(handle) = assets.textures.handle_for_path(path) {
                    Ok(handle)
                } else {
                    assets.load_texture(path).map_err(|error| {
                        SceneIoError::AssetLoad {
                            path: path.clone(),
                            message: error.to_string(),
                        }
                    })
                }
            })
            .transpose()
    }
    Ok(MaterialAsset {
        model: match material.model {
            SceneMaterialModel::Pbr => MaterialModel::Pbr,
            SceneMaterialModel::Unlit => MaterialModel::Unlit,
        },
        base_color: material.base_color,
        emissive: material.emissive,
        metallic: material.metallic,
        roughness: material.roughness,
        base_color_texture: texture(assets, &material.base_color_texture)?,
        normal_texture: texture(assets, &material.normal_texture)?,
        metallic_roughness_texture: texture(
            assets,
            &material.metallic_roughness_texture,
        )?,
        occlusion_texture: texture(assets, &material.occlusion_texture)?,
        emissive_texture: texture(assets, &material.emissive_texture)?,
    })
}

fn scene_camera(camera: Camera) -> SceneCamera {
    SceneCamera {
        projection: match camera.projection {
            Projection::Perspective {
                vertical_fov_radians,
                near,
                far,
            } => SceneProjection::Perspective {
                vertical_fov_radians,
                near,
                far,
            },
            Projection::Orthographic {
                vertical_size,
                near,
                far,
            } => SceneProjection::Orthographic {
                vertical_size,
                near,
                far,
            },
        },
        active: camera.active,
        priority: camera.priority,
    }
}

fn runtime_camera(camera: SceneCamera) -> Camera {
    Camera {
        projection: match camera.projection {
            SceneProjection::Perspective {
                vertical_fov_radians,
                near,
                far,
            } => Projection::Perspective {
                vertical_fov_radians,
                near,
                far,
            },
            SceneProjection::Orthographic {
                vertical_size,
                near,
                far,
            } => Projection::Orthographic {
                vertical_size,
                near,
                far,
            },
        },
        active: camera.active,
        priority: camera.priority,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::runtime::{
        App, GpuCondition, GpuEventPayload, GpuPhysicsRule, PhysicsSolver,
        RenderExtractPlugin, SimulationClass,
    };
    use crate::MaterialAsset;

    #[derive(
        Component,
        Clone,
        Copy,
        Debug,
        Default,
        Serialize,
        Deserialize,
        PartialEq,
    )]
    struct GameplayTag {
        speed: f32,
    }

    fn scene_app() -> App {
        let mut app = App::new();
        app.add_plugin(crate::AssetPlugin).unwrap();
        app.add_plugin(RenderExtractPlugin).unwrap();
        app.register_scene_component::<GameplayTag>("gameplay_tag")
            .unwrap();
        app
    }

    #[test]
    fn scene_round_trip_preserves_hierarchy_assets_and_registered_components() {
        let mut app = scene_app();
        let (mesh, material) = {
            let mut assets = app.world_mut().resource_mut::<AssetServer>();
            let material = assets.materials.insert(MaterialAsset {
                base_color: [0.2, 0.4, 0.8, 1.0],
                ..MaterialAsset::default()
            });
            (assets.fallback_mesh, material)
        };
        let parent = app.spawn((Name("Root".into()), Transform::default()));
        let child = app.spawn((
            Name("Cube".into()),
            Transform::new([1.0, 2.0, 3.0]),
            MeshRenderer {
                mesh,
                material,
                cast_shadows: true,
                receive_shadows: false,
            },
            GameplayTag { speed: 2.5 },
            PhysicsBody {
                simulation: SimulationClass::GpuDynamic,
                solver: PhysicsSolver::Simplified,
                custom_shader: None,
            },
            RigidBody {
                mass: 12.0,
                ..RigidBody::default()
            },
            Collider {
                restitution: 0.75,
                ..Collider::default()
            },
            GpuPhysicsWatch {
                rules: vec![GpuPhysicsRule::new(
                    "cube_fell",
                    GpuCondition::position_y().less_than(-100.0),
                )
                .payload(GpuEventPayload::Position)],
            },
            ObjectClasses::new(["falling_cubes", "gravity"]),
        ));
        app.set_parent(child, parent).unwrap();
        let document = scene_document(app.world_mut(), "Test").unwrap();

        load_scene_document(app.world_mut(), &document, SceneLoadMode::Replace)
            .unwrap();
        app.update(Duration::ZERO).unwrap();

        let mut query = app.world_mut().query::<(
            &Name,
            &Transform,
            Option<&GameplayTag>,
            Option<&Parent>,
            Option<&PhysicsBody>,
            Option<&RigidBody>,
            Option<&Collider>,
            Option<&GpuPhysicsWatch>,
            Option<&ObjectClasses>,
        )>();
        let entities = query.iter(app.world()).collect::<Vec<_>>();
        assert_eq!(entities.len(), 2);
        let (
            _,
            transform,
            tag,
            parent,
            physics,
            rigid_body,
            collider,
            watch,
            classes,
        ) = entities.iter().find(|(name, ..)| name.0 == "Cube").unwrap();
        assert_eq!(transform.position, [1.0, 2.0, 3.0]);
        assert_eq!(tag.copied(), Some(GameplayTag { speed: 2.5 }));
        assert!(parent.is_some());
        assert_eq!(
            physics.map(|physics| physics.simulation),
            Some(SimulationClass::GpuDynamic)
        );
        assert_eq!(rigid_body.map(|body| body.mass), Some(12.0));
        assert_eq!(collider.map(|collider| collider.restitution), Some(0.75));
        assert_eq!(
            watch
                .and_then(|watch| watch.rules.first())
                .map(|rule| rule.event.as_str()),
            Some("cube_fell")
        );
        assert_eq!(
            classes.map(|classes| classes.names.as_slice()),
            Some(["falling_cubes".to_owned(), "gravity".to_owned()].as_slice())
        );
    }

    #[test]
    fn source_and_compiled_scene_decode_to_same_document() {
        let mut app = scene_app();
        app.spawn((
            Name("Camera".into()),
            Transform::default(),
            Camera::default(),
        ));
        let document = scene_document(app.world_mut(), "Compile").unwrap();
        let source = serde_json::to_vec(&document).unwrap();
        let mut compiled = COMPILED_MAGIC.to_vec();
        compiled.extend(bincode::serialize(&document).unwrap());
        assert_eq!(decode_scene(&source).unwrap(), document);
        assert_eq!(decode_scene(&compiled).unwrap(), document);
    }

    #[test]
    fn version_one_cooked_scene_migrates_without_gpu_watch_data() {
        let legacy = LegacySceneDocumentV1 {
            format_version: 1,
            name: "Old Cooked Scene".into(),
            entities: Vec::new(),
        };
        let mut bytes = COMPILED_MAGIC.to_vec();
        bytes.extend(bincode::serialize(&legacy).unwrap());

        let migrated = decode_scene(&bytes).unwrap();

        assert_eq!(migrated.format_version, SCENE_FORMAT_VERSION);
        assert_eq!(migrated.name, "Old Cooked Scene");
    }

    #[test]
    fn version_two_cooked_scene_migrates_without_object_classes() {
        let legacy = LegacySceneDocumentV2 {
            format_version: 2,
            name: "Scene Before Classes".into(),
            entities: Vec::new(),
        };
        let mut bytes = COMPILED_MAGIC.to_vec();
        bytes.extend(bincode::serialize(&legacy).unwrap());

        let migrated = decode_scene(&bytes).unwrap();

        assert_eq!(migrated.format_version, SCENE_FORMAT_VERSION);
        assert_eq!(migrated.name, "Scene Before Classes");
    }

    #[test]
    fn unversioned_text_scene_migrates_but_newer_scene_is_rejected() {
        let mut app = scene_app();
        app.spawn((Name("Legacy".into()), Transform::default()));
        let current = scene_document(app.world_mut(), "Legacy").unwrap();
        let mut legacy = serde_json::to_value(&current).unwrap();
        legacy.as_object_mut().unwrap().remove("format_version");

        let migrated =
            decode_scene(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(migrated.format_version, SCENE_FORMAT_VERSION);
        assert_eq!(migrated.entities, current.entities);

        let mut newer = current;
        newer.format_version = SCENE_FORMAT_VERSION + 1;
        assert!(matches!(
            decode_scene(&serde_json::to_vec(&newer).unwrap()),
            Err(SceneIoError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn invalid_hierarchy_is_rejected_before_the_open_scene_is_replaced() {
        let mut app = scene_app();
        let root = app.spawn((Name("Root".into()), Transform::default()));
        let child = app.spawn((Name("Child".into()), Transform::default()));
        app.set_parent(child, root).unwrap();
        let original = scene_document(app.world_mut(), "Original").unwrap();
        let mut invalid = original.clone();
        let root_id = invalid
            .entities
            .iter()
            .find(|entity| entity.name.as_deref() == Some("Root"))
            .unwrap()
            .id;
        let child_id = invalid
            .entities
            .iter()
            .find(|entity| entity.name.as_deref() == Some("Child"))
            .unwrap()
            .id;
        invalid
            .entities
            .iter_mut()
            .find(|entity| entity.id == root_id)
            .unwrap()
            .parent = Some(child_id);

        assert!(matches!(
            load_scene_document(
                app.world_mut(),
                &invalid,
                SceneLoadMode::Replace
            ),
            Err(SceneIoError::HierarchyCycle(_))
        ));
        assert_eq!(
            scene_document(app.world_mut(), "Original").unwrap(),
            original
        );
    }

    #[test]
    fn duplicate_object_names_are_rejected() {
        let mut app = scene_app();
        app.spawn((Name("Cube".into()), Transform::default()));
        app.spawn((Name("Cube".into()), Transform::default()));

        assert!(matches!(
            scene_document(app.world_mut(), "Duplicates"),
            Err(SceneIoError::DuplicateName(name)) if name == "Cube"
        ));
    }

    #[test]
    fn saved_scene_can_be_cooked_and_loaded_by_a_fresh_runtime() {
        let test_directory = std::env::temp_dir()
            .join(format!("rusting-scene-test-{}", Uuid::new_v4()));
        let source = test_directory.join("scene.rscene");
        let compiled = test_directory.join("scene.rscene.bin");

        let mut editor_app = scene_app();
        editor_app.spawn((
            Name("Runtime Cube".into()),
            Transform::new([4.0, 5.0, 6.0]),
            GameplayTag { speed: 3.0 },
        ));
        save_scene(editor_app.world_mut(), &source, "Runtime").unwrap();
        cook_scene(&source, &compiled).unwrap();

        let mut game_app = scene_app();
        load_scene(game_app.world_mut(), &compiled, SceneLoadMode::Replace)
            .unwrap();
        let mut query = game_app
            .world_mut()
            .query::<(&Name, &Transform, &GameplayTag)>();
        let (name, transform, tag) = query.single(game_app.world()).unwrap();
        assert_eq!(name.0, "Runtime Cube");
        assert_eq!(transform.position, [4.0, 5.0, 6.0]);
        assert_eq!(tag.speed, 3.0);

        std::fs::remove_file(compiled).unwrap();
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(test_directory).unwrap();
    }

    #[test]
    fn cooked_scene_asset_paths_remain_project_relative_and_reloadable() {
        let project = std::env::temp_dir()
            .join(format!("rusting-portable-scene-{}", Uuid::new_v4()));
        let assets_folder = project.join("assets");
        let scenes_folder = project.join("scenes");
        let build_folder = project.join("build");
        std::fs::create_dir_all(&assets_folder).unwrap();
        std::fs::create_dir_all(&scenes_folder).unwrap();
        let texture_path = assets_folder.join("pixel.png");
        image::RgbaImage::from_raw(1, 1, vec![255, 128, 0, 255])
            .unwrap()
            .save(&texture_path)
            .unwrap();
        let source = scenes_folder.join("main.rscene");
        let cooked = build_folder.join("main.rscene.bin");
        let mut editor = scene_app();
        let (mesh, material) = {
            let mut assets = editor.world_mut().resource_mut::<AssetServer>();
            let texture = assets.load_texture(&texture_path).unwrap();
            let material = assets.materials.insert(MaterialAsset {
                base_color_texture: Some(texture),
                ..MaterialAsset::default()
            });
            (assets.fallback_mesh, material)
        };
        editor.spawn((
            Name("Textured Cube".into()),
            Transform::default(),
            MeshRenderer {
                mesh,
                material,
                cast_shadows: true,
                receive_shadows: true,
            },
        ));

        save_scene(editor.world_mut(), &source, "Portable").unwrap();
        let saved: SceneDocument =
            serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
        let saved_texture = saved.entities[0]
            .mesh_renderer
            .as_ref()
            .and_then(|renderer| match &renderer.material {
                SceneMaterial::Inline(material) => {
                    material.base_color_texture.as_ref()
                }
                SceneMaterial::BuiltinError => None,
            })
            .unwrap();
        assert!(!saved_texture.is_absolute());
        assert!(saved_texture.ends_with(Path::new("assets").join("pixel.png")));
        cook_scene(&source, &cooked).unwrap();

        let mut runtime = scene_app();
        load_scene(runtime.world_mut(), &cooked, SceneLoadMode::Replace)
            .unwrap();
        assert_eq!(runtime.world().resource::<AssetServer>().textures.len(), 2);
        std::fs::remove_dir_all(project).unwrap();
    }
}
