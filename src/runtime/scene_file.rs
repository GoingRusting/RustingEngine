//! Versioned, editor-authored scene files and compiled runtime scene data.

use std::collections::{BTreeMap, HashMap};
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
    compile_script, Camera, Collider, CollisionLayers, CompiledScript,
    MeshRenderer, Name, Parent, PhysicsBody, Projection, RigidBody, SceneId,
    ScriptComponent, Visibility,
};

pub const SCENE_FORMAT_VERSION: u32 = 1;
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
    Script { path: PathBuf, message: String },
    MissingAssetServer,
    MissingAssetPath(PathBuf),
    UnsavedMesh(u64),
    UnsavedTexture(u64),
    MissingParent(Uuid),
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
            Self::Script { path, message } => {
                write!(
                    formatter,
                    "failed to compile script `{}`: {message}",
                    path.display()
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
            Self::MissingParent(id) => {
                write!(formatter, "scene parent {id} does not exist")
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
    pub format_version: u32,
    pub name: String,
    pub entities: Vec<SceneEntity>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneEntity {
    pub id: Uuid,
    pub parent: Option<Uuid>,
    pub name: Option<String>,
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
    pub script: Option<SceneScript>,
    #[serde(default)]
    pub components: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SceneScript {
    pub source_path: PathBuf,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compiled: Option<CompiledScript>,
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
        Option<&Transform>,
        Option<&MeshRenderer>,
        Option<&Camera>,
        Option<&Visibility>,
        Option<&PhysicsBody>,
        Option<&RigidBody>,
        Option<&Collider>,
        Option<&CollisionLayers>,
        Option<&ScriptComponent>,
    )>();
    let raw = query
        .iter(world)
        .map(
            |(
                entity,
                id,
                parent,
                name,
                transform,
                renderer,
                camera,
                visibility,
                physics_body,
                rigid_body,
                collider,
                collision_layers,
                script,
            )| {
                (
                    entity,
                    *id,
                    parent.copied(),
                    name.cloned(),
                    transform.copied(),
                    renderer.copied(),
                    camera.copied(),
                    visibility.copied(),
                    physics_body.cloned(),
                    rigid_body.copied(),
                    collider.copied(),
                    collision_layers.copied(),
                    script.cloned(),
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
        transform,
        renderer,
        camera,
        visibility,
        physics_body,
        rigid_body,
        collider,
        collision_layers,
        script,
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
            transform: transform.map(Into::into),
            mesh_renderer,
            camera: camera.map(scene_camera),
            visible: visibility.map(|visibility| visibility.visible),
            physics_body,
            rigid_body,
            collider,
            collision_layers,
            script: script.map(|script| SceneScript {
                source_path: PathBuf::from(script.source_path),
                enabled: script.enabled,
                compiled: None,
            }),
            components,
        });
    }
    entities.sort_by_key(|entity| entity.id);
    Ok(SceneDocument {
        format_version: SCENE_FORMAT_VERSION,
        name: name.into(),
        entities,
    })
}

pub fn save_scene(
    world: &mut World,
    path: impl AsRef<Path>,
    name: impl Into<String>,
) -> Result<(), SceneIoError> {
    let document = scene_document(world, name)?;
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&document)?)?;
    Ok(())
}

pub fn load_scene(
    world: &mut World,
    path: impl AsRef<Path>,
    mode: SceneLoadMode,
) -> Result<usize, SceneIoError> {
    let bytes = std::fs::read(path)?;
    let document = decode_scene(&bytes)?;
    load_scene_document(world, &document, mode)
}

pub fn cook_scene(
    source: impl AsRef<Path>,
    destination: impl AsRef<Path>,
) -> Result<(), SceneIoError> {
    let source = source.as_ref();
    let mut document: SceneDocument =
        serde_json::from_slice(&std::fs::read(source)?)?;
    validate_version(&document)?;
    let project_root = source
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    for entity in &mut document.entities {
        let Some(script) = &mut entity.script else {
            continue;
        };
        let path = if script.source_path.is_absolute()
            || script.source_path.exists()
        {
            script.source_path.clone()
        } else {
            project_root.join(&script.source_path)
        };
        let source = std::fs::read_to_string(&path).map_err(|error| {
            SceneIoError::Script {
                path: path.clone(),
                message: error.to_string(),
            }
        })?;
        script.compiled = Some(compile_script(&source).map_err(|error| {
            SceneIoError::Script {
                path: path.clone(),
                message: error.to_string(),
            }
        })?);
    }
    let mut bytes = COMPILED_MAGIC.to_vec();
    bytes.extend(bincode::serialize(&document)?);
    let destination = destination.as_ref();
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(destination, bytes)?;
    Ok(())
}

pub fn load_scene_document(
    world: &mut World,
    document: &SceneDocument,
    mode: SceneLoadMode,
) -> Result<usize, SceneIoError> {
    validate_version(document)?;
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
        if let Some(script) = &scene_entity.script {
            entity.insert(ScriptComponent {
                source_path: script.source_path.display().to_string(),
                enabled: script.enabled,
                compiled: script.compiled.clone(),
            });
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

fn decode_scene(bytes: &[u8]) -> Result<SceneDocument, SceneIoError> {
    let document = if let Some(compiled) = bytes.strip_prefix(COMPILED_MAGIC) {
        bincode::deserialize(compiled)?
    } else {
        serde_json::from_slice(bytes)?
    };
    validate_version(&document)?;
    Ok(document)
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
                SceneMesh::AssetPath(path) => {
                    assets.meshes.handle_for_path(path).ok_or_else(|| {
                        SceneIoError::MissingAssetPath(path.clone())
                    })?
                }
            };
            let material = match &renderer.material {
                SceneMaterial::BuiltinError => assets.fallback_material,
                SceneMaterial::Inline(material) => {
                    let material = runtime_material(material, &assets)?;
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
    assets: &AssetServer,
) -> Result<MaterialAsset, SceneIoError> {
    let texture = |path: &Option<PathBuf>| {
        path.as_ref()
            .map(|path| {
                assets
                    .textures
                    .handle_for_path(path)
                    .ok_or_else(|| SceneIoError::MissingAssetPath(path.clone()))
            })
            .transpose()
    };
    Ok(MaterialAsset {
        model: match material.model {
            SceneMaterialModel::Pbr => MaterialModel::Pbr,
            SceneMaterialModel::Unlit => MaterialModel::Unlit,
        },
        base_color: material.base_color,
        emissive: material.emissive,
        metallic: material.metallic,
        roughness: material.roughness,
        base_color_texture: texture(&material.base_color_texture)?,
        normal_texture: texture(&material.normal_texture)?,
        metallic_roughness_texture: texture(
            &material.metallic_roughness_texture,
        )?,
        occlusion_texture: texture(&material.occlusion_texture)?,
        emissive_texture: texture(&material.emissive_texture)?,
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
        App, PhysicsSolver, RenderExtractPlugin, ScriptPlugin, SimulationClass,
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
        )>();
        let entities = query.iter(app.world()).collect::<Vec<_>>();
        assert_eq!(entities.len(), 2);
        let (_, transform, tag, parent, physics, rigid_body, collider) =
            entities.iter().find(|(name, ..)| name.0 == "Cube").unwrap();
        assert_eq!(transform.position, [1.0, 2.0, 3.0]);
        assert_eq!(tag.copied(), Some(GameplayTag { speed: 2.5 }));
        assert!(parent.is_some());
        assert_eq!(
            physics.map(|physics| physics.simulation),
            Some(SimulationClass::GpuDynamic)
        );
        assert_eq!(rigid_body.map(|body| body.mass), Some(12.0));
        assert_eq!(collider.map(|collider| collider.restitution), Some(0.75));
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
    fn cooked_scene_executes_embedded_script_without_source_file() {
        let test_directory = std::env::temp_dir()
            .join(format!("rusting-script-cook-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&test_directory).unwrap();
        let script_path = test_directory.join("main.rscript");
        let source_scene = test_directory.join("scene.rscene");
        let compiled_scene = test_directory.join("scene.rscene.bin");
        std::fs::write(
            &script_path,
            "let cube = scene.get_object(\"Scripted Cube\");\n\
             onSceneStart() { cube.x = 9; }",
        )
        .unwrap();

        let mut editor_app = scene_app();
        editor_app.spawn((Name("Scripted Cube".into()), Transform::default()));
        editor_app.spawn(ScriptComponent {
            source_path: script_path.display().to_string(),
            enabled: true,
            compiled: None,
        });
        save_scene(editor_app.world_mut(), &source_scene, "Scripted").unwrap();
        cook_scene(&source_scene, &compiled_scene).unwrap();
        std::fs::remove_file(script_path).unwrap();

        let mut game_app = scene_app();
        game_app.add_plugin(ScriptPlugin).unwrap();
        load_scene(
            game_app.world_mut(),
            &compiled_scene,
            SceneLoadMode::Replace,
        )
        .unwrap();
        game_app.update(Duration::from_millis(16)).unwrap();
        let mut query = game_app.world_mut().query::<(&Name, &Transform)>();
        let (_, transform) = query
            .iter(game_app.world())
            .find(|(name, _)| name.0 == "Scripted Cube")
            .unwrap();
        assert_eq!(transform.position[0], 9.0);

        std::fs::remove_file(source_scene).unwrap();
        std::fs::remove_file(compiled_scene).unwrap();
        std::fs::remove_dir(test_directory).unwrap();
    }
}
