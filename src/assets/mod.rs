//! Typed, generational CPU asset storage.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf};

use bevy_ecs::prelude::Resource;

use crate::runtime::{App, AppError, Plugin};

/// A compact typed asset identity. Reused slots receive a new generation, so
/// stale handles can never resolve to unrelated assets.
pub struct Handle<T> {
    index: u32,
    generation: u32,
    marker: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }

    #[must_use]
    pub const fn key(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> Debug for Handle<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Handle")
            .field("index", &self.index)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetError {
    EmptyPath,
    Missing(AssetKey),
    StillReferenced { key: AssetKey, references: u32 },
    Load { path: PathBuf, message: String },
}

impl Display for AssetError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPath => {
                formatter.write_str("asset path cannot be empty")
            }
            Self::Missing(key) => {
                write!(formatter, "asset {key:?} is missing or stale")
            }
            Self::StillReferenced { key, references } => {
                write!(
                    formatter,
                    "asset {key:?} still has {references} references"
                )
            }
            Self::Load { path, message } => {
                write!(
                    formatter,
                    "failed to load `{}`: {message}",
                    path.display()
                )
            }
        }
    }
}

impl Error for AssetError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AssetKey {
    pub index: u32,
    pub generation: u32,
}

impl<T> From<Handle<T>> for AssetKey {
    fn from(handle: Handle<T>) -> Self {
        Self {
            index: handle.index,
            generation: handle.generation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    Loaded,
    Failed(String),
}

struct Slot<T> {
    generation: u32,
    revision: u64,
    value: Option<T>,
    path: Option<PathBuf>,
    references: u32,
    state: LoadState,
}

/// Storage for one asset type, including path deduplication and deferred drops.
pub struct Assets<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    paths: HashMap<PathBuf, Handle<T>>,
    deferred: Vec<(u64, T)>,
}

impl<T> Default for Assets<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            paths: HashMap::new(),
            deferred: Vec::new(),
        }
    }
}

impl<T> Assets<T> {
    pub fn insert(&mut self, value: T) -> Handle<T> {
        self.insert_slot(value, None)
    }

    pub fn get_or_insert_with(
        &mut self,
        path: impl AsRef<Path>,
        loader: impl FnOnce(&Path) -> Result<T, AssetError>,
    ) -> Result<Handle<T>, AssetError> {
        let path = normalize_path(path.as_ref())?;
        if let Some(handle) = self.paths.get(&path).copied() {
            if self.contains(handle) {
                return Ok(handle);
            }
            self.paths.remove(&path);
        }
        let value = loader(&path).map_err(|error| AssetError::Load {
            path: path.clone(),
            message: error.to_string(),
        })?;
        Ok(self.insert_slot(value, Some(path)))
    }

    #[must_use]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.slot(handle).and_then(|slot| slot.value.as_ref())
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.slot_mut(handle).and_then(|slot| {
            slot.revision = slot.revision.saturating_add(1);
            slot.value.as_mut()
        })
    }

    #[must_use]
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    #[must_use]
    pub fn load_state(&self, handle: Handle<T>) -> Option<&LoadState> {
        self.slot(handle).map(|slot| &slot.state)
    }

    #[must_use]
    pub fn path(&self, handle: Handle<T>) -> Option<&Path> {
        self.slot(handle).and_then(|slot| slot.path.as_deref())
    }

    #[must_use]
    pub fn revision(&self, handle: Handle<T>) -> Option<u64> {
        self.slot(handle).map(|slot| slot.revision)
    }

    pub fn retain(&mut self, handle: Handle<T>) -> Result<(), AssetError> {
        let slot = self
            .slot_mut(handle)
            .ok_or_else(|| AssetError::Missing(handle.into()))?;
        slot.references = slot.references.saturating_add(1);
        Ok(())
    }

    pub fn release(&mut self, handle: Handle<T>) -> Result<(), AssetError> {
        let slot = self
            .slot_mut(handle)
            .ok_or_else(|| AssetError::Missing(handle.into()))?;
        slot.references = slot.references.saturating_sub(1);
        Ok(())
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Result<T, AssetError> {
        let slot = self
            .slot_mut(handle)
            .ok_or_else(|| AssetError::Missing(handle.into()))?;
        if slot.references > 0 {
            return Err(AssetError::StillReferenced {
                key: handle.into(),
                references: slot.references,
            });
        }
        let path = slot.path.take();
        let value = slot
            .value
            .take()
            .ok_or_else(|| AssetError::Missing(handle.into()))?;
        if let Some(path) = path {
            self.paths.remove(&path);
        }
        self.free.push(handle.index);
        Ok(value)
    }

    pub fn retire(
        &mut self,
        handle: Handle<T>,
        safe_after_frame: u64,
    ) -> Result<(), AssetError> {
        let value = self.remove(handle)?;
        self.deferred.push((safe_after_frame, value));
        Ok(())
    }

    pub fn collect_retired(&mut self, completed_frame: u64) -> usize {
        let before = self.deferred.len();
        self.deferred.retain(|(safe_after_frame, _)| {
            *safe_after_frame > completed_frame
        });
        before - self.deferred.len()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value.as_ref().map(|value| {
                (
                    Handle {
                        index: index as u32,
                        generation: slot.generation,
                        marker: PhantomData,
                    },
                    value,
                )
            })
        })
    }

    pub fn paths(&self) -> impl Iterator<Item = (Handle<T>, &Path)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            Some((
                Handle {
                    index: index as u32,
                    generation: slot.generation,
                    marker: PhantomData,
                },
                slot.path.as_deref()?,
            ))
        })
    }

    fn insert_slot(&mut self, value: T, path: Option<PathBuf>) -> Handle<T> {
        let handle = if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1).max(1);
            slot.revision = slot.revision.saturating_add(1);
            slot.value = Some(value);
            slot.path = path.clone();
            slot.references = 0;
            slot.state = LoadState::Loaded;
            Handle {
                index,
                generation: slot.generation,
                marker: PhantomData,
            }
        } else {
            let index = u32::try_from(self.slots.len())
                .expect("asset slot count exceeds u32");
            self.slots.push(Slot {
                generation: 1,
                revision: 1,
                value: Some(value),
                path: path.clone(),
                references: 0,
                state: LoadState::Loaded,
            });
            Handle {
                index,
                generation: 1,
                marker: PhantomData,
            }
        };
        if let Some(path) = path {
            self.paths.insert(path, handle);
        }
        handle
    }

    fn slot(&self, handle: Handle<T>) -> Option<&Slot<T>> {
        self.slots.get(handle.index as usize).filter(|slot| {
            slot.generation == handle.generation && slot.value.is_some()
        })
    }

    fn slot_mut(&mut self, handle: Handle<T>) -> Option<&mut Slot<T>> {
        self.slots.get_mut(handle.index as usize).filter(|slot| {
            slot.generation == handle.generation && slot.value.is_some()
        })
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf, AssetError> {
    if path.as_os_str().is_empty() {
        return Err(AssetError::EmptyPath);
    }
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(canonical);
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| AssetError::Load {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshAsset {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureColorSpace {
    Srgb,
    Linear,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureAsset {
    pub size: [u32; 2],
    pub rgba8: Vec<u8>,
    pub color_space: TextureColorSpace,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaterialModel {
    #[default]
    Pbr,
    Unlit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaterialAsset {
    pub model: MaterialModel,
    pub base_color: [f32; 4],
    pub emissive: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
    pub base_color_texture: Option<Handle<TextureAsset>>,
    pub normal_texture: Option<Handle<TextureAsset>>,
    pub metallic_roughness_texture: Option<Handle<TextureAsset>>,
    pub occlusion_texture: Option<Handle<TextureAsset>>,
    pub emissive_texture: Option<Handle<TextureAsset>>,
}

impl Default for MaterialAsset {
    fn default() -> Self {
        Self {
            model: MaterialModel::Pbr,
            base_color: [1.0; 4],
            emissive: [0.0; 3],
            metallic: 0.0,
            roughness: 0.5,
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneAsset {
    pub source: Option<PathBuf>,
}

/// Canonical CPU asset collections and built-in fallbacks.
#[derive(Resource)]
pub struct AssetServer {
    pub meshes: Assets<MeshAsset>,
    pub textures: Assets<TextureAsset>,
    pub materials: Assets<MaterialAsset>,
    pub scenes: Assets<SceneAsset>,
    pub fallback_mesh: Handle<MeshAsset>,
    pub fallback_texture: Handle<TextureAsset>,
    pub fallback_material: Handle<MaterialAsset>,
}

impl Default for AssetServer {
    fn default() -> Self {
        let mut meshes = Assets::default();
        let fallback_mesh = meshes.insert(fallback_cube());
        let mut textures = Assets::default();
        let fallback_texture = textures.insert(TextureAsset {
            size: [1, 1],
            rgba8: vec![255; 4],
            color_space: TextureColorSpace::Srgb,
        });
        let mut materials = Assets::default();
        let fallback_material = materials.insert(MaterialAsset {
            base_color: [1.0, 0.0, 1.0, 1.0],
            base_color_texture: Some(fallback_texture),
            ..MaterialAsset::default()
        });
        Self {
            meshes,
            textures,
            materials,
            scenes: Assets::default(),
            fallback_mesh,
            fallback_texture,
            fallback_material,
        }
    }
}

fn fallback_cube() -> MeshAsset {
    let faces = [
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, 0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
    ];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, positions) in faces {
        let base = vertices.len() as u32;
        for (position, uv) in positions.into_iter().zip([
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ]) {
            vertices.push(MeshVertex {
                position,
                normal,
                uv,
                tangent: [1.0, 0.0, 0.0, 1.0],
            });
        }
        indices.extend_from_slice(&[
            base,
            base + 1,
            base + 2,
            base,
            base + 2,
            base + 3,
        ]);
    }
    MeshAsset { vertices, indices }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AssetPlugin;

impl Plugin for AssetPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.insert_resource(AssetServer::default());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_handle_does_not_resolve_reused_slot() {
        let mut assets = Assets::default();
        let stale = assets.insert(String::from("old"));
        assert_eq!(assets.remove(stale).unwrap(), "old");
        let current = assets.insert(String::from("new"));
        assert_eq!(stale.index(), current.index());
        assert_ne!(stale.generation(), current.generation());
        assert!(assets.get(stale).is_none());
        assert_eq!(assets.get(current).map(String::as_str), Some("new"));
    }

    #[test]
    fn equivalent_paths_are_deduplicated() {
        let mut assets = Assets::default();
        let first = assets
            .get_or_insert_with("assets/../assets/cube.mesh", |_| Ok(7_u32))
            .unwrap();
        let second = assets
            .get_or_insert_with("./assets/cube.mesh", |_| Ok(9_u32))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(assets.get(first), Some(&7));
    }

    #[test]
    fn referenced_assets_cannot_be_removed() {
        let mut assets = Assets::default();
        let handle = assets.insert(42);
        assets.retain(handle).unwrap();
        assert!(matches!(
            assets.remove(handle),
            Err(AssetError::StillReferenced { .. })
        ));
        assets.release(handle).unwrap();
        assert_eq!(assets.remove(handle), Ok(42));
    }

    #[test]
    fn retired_assets_wait_for_safe_frame() {
        let mut assets = Assets::default();
        let handle = assets.insert(42);
        assets.retire(handle, 5).unwrap();
        assert_eq!(assets.collect_retired(4), 0);
        assert_eq!(assets.collect_retired(5), 1);
    }

    #[test]
    fn mutable_access_advances_asset_revision() {
        let mut assets = Assets::default();
        let handle = assets.insert(1_u32);
        let initial = assets.revision(handle).unwrap();
        *assets.get_mut(handle).unwrap() = 2;
        assert!(assets.revision(handle).unwrap() > initial);
        assert_eq!(assets.get(handle), Some(&2));
    }
}
