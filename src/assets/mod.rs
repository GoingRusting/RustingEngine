//! Typed, generational CPU asset storage.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::path::{Component, Path, PathBuf};

use bevy_ecs::prelude::Resource;
use serde::{Deserialize, Serialize};

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

    /// Inserts a prepared asset with a source path, or returns the asset that
    /// was already imported from the same path.
    pub fn insert_with_path(
        &mut self,
        path: impl AsRef<Path>,
        value: T,
    ) -> Result<Handle<T>, AssetError> {
        let path = normalize_path(path.as_ref())?;
        if let Some(handle) = self.paths.get(&path).copied() {
            if self.contains(handle) {
                return Ok(handle);
            }
            self.paths.remove(&path);
        }
        Ok(self.insert_slot(value, Some(path)))
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

    /// Resolves an already-loaded asset by its normalized source path.
    #[must_use]
    pub fn handle_for_path(&self, path: impl AsRef<Path>) -> Option<Handle<T>> {
        let path = normalize_path(path.as_ref()).ok()?;
        self.paths
            .get(&path)
            .copied()
            .filter(|handle| self.contains(*handle))
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

/// Converts a decoded glTF image into tightly packed RGBA8, expanding
/// whichever channel layout the source used. 16-bit and float glTF image
/// formats are rare (most exporters emit 8-bit PNG/JPEG); they are
/// downsampled to 8 bits rather than rejected.
fn gltf_image_to_rgba8(image: &gltf::image::Data) -> Vec<u8> {
    use gltf::image::Format;
    let pixel_count = (image.width as usize) * (image.height as usize);
    let mut rgba8 = Vec::with_capacity(pixel_count * 4);
    match image.format {
        Format::R8 => {
            for &r in &image.pixels {
                rgba8.extend_from_slice(&[r, r, r, 255]);
            }
        }
        Format::R8G8 => {
            for chunk in image.pixels.chunks_exact(2) {
                rgba8.extend_from_slice(&[chunk[0], chunk[1], 0, 255]);
            }
        }
        Format::R8G8B8 => {
            for chunk in image.pixels.chunks_exact(3) {
                rgba8.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
        }
        Format::R8G8B8A8 => rgba8.extend_from_slice(&image.pixels),
        Format::R16 => {
            for chunk in image.pixels.chunks_exact(2) {
                let v = chunk[1];
                rgba8.extend_from_slice(&[v, v, v, 255]);
            }
        }
        Format::R16G16 => {
            for chunk in image.pixels.chunks_exact(4) {
                rgba8.extend_from_slice(&[chunk[1], chunk[3], 0, 255]);
            }
        }
        Format::R16G16B16 => {
            for chunk in image.pixels.chunks_exact(6) {
                rgba8.extend_from_slice(&[chunk[1], chunk[3], chunk[5], 255]);
            }
        }
        Format::R16G16B16A16 => {
            for chunk in image.pixels.chunks_exact(8) {
                rgba8.extend_from_slice(&[
                    chunk[1], chunk[3], chunk[5], chunk[7],
                ]);
            }
        }
        Format::R32G32B32FLOAT => {
            for chunk in image.pixels.chunks_exact(12) {
                let channel = |bytes: &[u8]| {
                    (f32::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ])
                    .clamp(0.0, 1.0)
                        * 255.0) as u8
                };
                rgba8.extend_from_slice(&[
                    channel(&chunk[0..4]),
                    channel(&chunk[4..8]),
                    channel(&chunk[8..12]),
                    255,
                ]);
            }
        }
        Format::R32G32B32A32FLOAT => {
            for chunk in image.pixels.chunks_exact(16) {
                let channel = |bytes: &[u8]| {
                    (f32::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ])
                    .clamp(0.0, 1.0)
                        * 255.0) as u8
                };
                rgba8.extend_from_slice(&[
                    channel(&chunk[0..4]),
                    channel(&chunk[4..8]),
                    channel(&chunk[8..12]),
                    channel(&chunk[12..16]),
                ]);
            }
        }
    }
    rgba8
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeshAsset {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

/// Procedural meshes shipped with the engine and available in Add Object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveShape {
    Cube,
    Sphere,
    Triangle,
    Plane,
    Tetrahedron,
    Octahedron,
    Dodecahedron,
    Icosahedron,
    Pyramid,
    Cylinder,
    Cone,
    Torus,
}

impl PrimitiveShape {
    pub const ALL: [Self; 12] = [
        Self::Cube,
        Self::Sphere,
        Self::Triangle,
        Self::Plane,
        Self::Tetrahedron,
        Self::Octahedron,
        Self::Dodecahedron,
        Self::Icosahedron,
        Self::Pyramid,
        Self::Cylinder,
        Self::Cone,
        Self::Torus,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cube => "Cube",
            Self::Sphere => "Sphere",
            Self::Triangle => "Triangle",
            Self::Plane => "Plane",
            Self::Tetrahedron => "Tetrahedron",
            Self::Octahedron => "Octahedron",
            Self::Dodecahedron => "Dodecahedron",
            Self::Icosahedron => "Icosahedron",
            Self::Pyramid => "Pyramid",
            Self::Cylinder => "Cylinder",
            Self::Cone => "Cone",
            Self::Torus => "Torus",
        }
    }
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
    /// Shared smooth sphere used by the editor's Add Sphere action.
    pub builtin_sphere: Handle<MeshAsset>,
    /// Every procedural primitive, keyed by its stable serialized identity.
    pub builtin_primitives: HashMap<PrimitiveShape, Handle<MeshAsset>>,
    pub fallback_texture: Handle<TextureAsset>,
    pub fallback_material: Handle<MaterialAsset>,
}

/// One glTF primitive prepared for assignment in the editor.
#[derive(Clone, Debug)]
pub struct ImportedGltfPrimitive {
    /// Readable mesh and primitive name shown in Assets.
    pub name: String,
    /// Typed CPU mesh created from the primitive.
    pub mesh: Handle<MeshAsset>,
    /// PBR material factors created from the glTF material.
    pub material: Handle<MaterialAsset>,
}

impl AssetServer {
    #[must_use]
    pub fn builtin_primitive(
        &self,
        shape: PrimitiveShape,
    ) -> Handle<MeshAsset> {
        self.builtin_primitives[&shape]
    }

    #[must_use]
    pub fn primitive_for_handle(
        &self,
        handle: Handle<MeshAsset>,
    ) -> Option<PrimitiveShape> {
        self.builtin_primitives
            .iter()
            .find_map(|(shape, candidate)| {
                (*candidate == handle).then_some(*shape)
            })
    }

    /// Loads an engine-native mesh created by the glTF importer.
    pub fn load_mesh(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<Handle<MeshAsset>, AssetError> {
        self.meshes.get_or_insert_with(path, |path| {
            let bytes =
                std::fs::read(path).map_err(|error| AssetError::Load {
                    path: path.to_owned(),
                    message: error.to_string(),
                })?;
            bincode::deserialize(&bytes).map_err(|error| AssetError::Load {
                path: path.to_owned(),
                message: error.to_string(),
            })
        })
    }

    /// Loads an image as an sRGB texture and deduplicates its source path.
    pub fn load_texture(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<Handle<TextureAsset>, AssetError> {
        self.textures.get_or_insert_with(path, |path| {
            let image =
                image::open(path).map_err(|error| AssetError::Load {
                    path: path.to_owned(),
                    message: error.to_string(),
                })?;
            let rgba = image.to_rgba8();
            Ok(TextureAsset {
                size: [rgba.width(), rgba.height()],
                rgba8: rgba.into_raw(),
                color_space: TextureColorSpace::Srgb,
            })
        })
    }

    /// Imports one glTF texture slot as an sRGB or linear [`TextureAsset`],
    /// deduplicating repeated references to the same image/color-space pair
    /// by their synthesized asset path.
    fn import_gltf_texture(
        &mut self,
        source: &Path,
        images: &[gltf::image::Data],
        texture: &gltf::texture::Texture,
        color_space: TextureColorSpace,
    ) -> Result<Handle<TextureAsset>, AssetError> {
        let image = &images[texture.source().index()];
        let suffix = match color_space {
            TextureColorSpace::Srgb => "srgb",
            TextureColorSpace::Linear => "linear",
        };
        let texture_key = source.with_extension(format!(
            "gltf-image-{}-{suffix}.rtexture",
            texture.source().index()
        ));
        self.textures.get_or_insert_with(texture_key, |_| {
            Ok(TextureAsset {
                size: [image.width, image.height],
                rgba8: gltf_image_to_rgba8(image),
                color_space,
            })
        })
    }

    /// Imports every triangle primitive from a glTF or GLB file as typed CPU
    /// mesh and material assets. Rendering uploads them later as usual.
    pub fn import_gltf(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<Vec<ImportedGltfPrimitive>, AssetError> {
        let source = normalize_path(path.as_ref())?;
        let (document, buffers, images) =
            gltf::import(&source).map_err(|error| AssetError::Load {
                path: source.clone(),
                message: error.to_string(),
            })?;
        let mut imported = Vec::new();
        for mesh in document.meshes() {
            for primitive in mesh.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    return Err(AssetError::Load {
                        path: source.clone(),
                        message: "only triangle glTF primitives are supported"
                            .into(),
                    });
                }
                let reader =
                    primitive.reader(|buffer| Some(&buffers[buffer.index()]));
                let positions = reader
                    .read_positions()
                    .ok_or_else(|| AssetError::Load {
                        path: source.clone(),
                        message: "glTF primitive has no positions".into(),
                    })?
                    .collect::<Vec<_>>();
                let normals = reader
                    .read_normals()
                    .map(Iterator::collect)
                    .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
                let uvs = reader
                    .read_tex_coords(0)
                    .map(|values| values.into_f32().collect())
                    .unwrap_or_else(|| vec![[0.0; 2]; positions.len()]);
                let tangents = reader
                    .read_tangents()
                    .map(Iterator::collect)
                    .unwrap_or_else(|| {
                        vec![[1.0, 0.0, 0.0, 1.0]; positions.len()]
                    });
                if normals.len() != positions.len()
                    || uvs.len() != positions.len()
                    || tangents.len() != positions.len()
                {
                    return Err(AssetError::Load {
                        path: source.clone(),
                        message:
                            "glTF vertex attributes have different lengths"
                                .into(),
                    });
                }
                let vertices = positions
                    .into_iter()
                    .enumerate()
                    .map(|(index, position)| MeshVertex {
                        position,
                        normal: normals[index],
                        uv: uvs[index],
                        tangent: tangents[index],
                    })
                    .collect::<Vec<_>>();
                let indices =
                    if let Some(values) = reader.read_indices() {
                        values.into_u32().collect()
                    } else {
                        let vertex_count = u32::try_from(vertices.len())
                            .map_err(|_| AssetError::Load {
                                path: source.clone(),
                                message: "glTF primitive has too many vertices"
                                    .into(),
                            })?;
                        (0..vertex_count).collect()
                    };
                let mesh_key = source.with_extension(format!(
                    "mesh-{}-{}.rmesh",
                    mesh.index(),
                    primitive.index()
                ));
                let mesh_asset = MeshAsset { vertices, indices };
                std::fs::write(
                    &mesh_key,
                    bincode::serialize(&mesh_asset).map_err(|error| {
                        AssetError::Load {
                            path: mesh_key.clone(),
                            message: error.to_string(),
                        }
                    })?,
                )
                .map_err(|error| AssetError::Load {
                    path: mesh_key.clone(),
                    message: error.to_string(),
                })?;
                let mesh_handle =
                    self.meshes.insert_with_path(mesh_key, mesh_asset)?;
                let gltf_material = primitive.material();
                let pbr = gltf_material.pbr_metallic_roughness();
                let base_color_texture = pbr
                    .base_color_texture()
                    .map(|info| {
                        self.import_gltf_texture(
                            &source,
                            &images,
                            &info.texture(),
                            TextureColorSpace::Srgb,
                        )
                    })
                    .transpose()?;
                let metallic_roughness_texture = pbr
                    .metallic_roughness_texture()
                    .map(|info| {
                        self.import_gltf_texture(
                            &source,
                            &images,
                            &info.texture(),
                            TextureColorSpace::Linear,
                        )
                    })
                    .transpose()?;
                let normal_texture = gltf_material
                    .normal_texture()
                    .map(|info| {
                        self.import_gltf_texture(
                            &source,
                            &images,
                            &info.texture(),
                            TextureColorSpace::Linear,
                        )
                    })
                    .transpose()?;
                let occlusion_texture = gltf_material
                    .occlusion_texture()
                    .map(|info| {
                        self.import_gltf_texture(
                            &source,
                            &images,
                            &info.texture(),
                            TextureColorSpace::Linear,
                        )
                    })
                    .transpose()?;
                let emissive_texture = gltf_material
                    .emissive_texture()
                    .map(|info| {
                        self.import_gltf_texture(
                            &source,
                            &images,
                            &info.texture(),
                            TextureColorSpace::Srgb,
                        )
                    })
                    .transpose()?;
                let material_key = source.with_extension(format!(
                    "gltf-material-{}",
                    gltf_material.index().unwrap_or(usize::MAX)
                ));
                let material = self.materials.insert_with_path(
                    material_key,
                    MaterialAsset {
                        model: MaterialModel::Pbr,
                        base_color: pbr.base_color_factor(),
                        emissive: gltf_material.emissive_factor(),
                        metallic: pbr.metallic_factor(),
                        roughness: pbr.roughness_factor(),
                        base_color_texture,
                        normal_texture,
                        metallic_roughness_texture,
                        occlusion_texture,
                        emissive_texture,
                    },
                )?;
                imported.push(ImportedGltfPrimitive {
                    name: format!(
                        "{} / Primitive {}",
                        mesh.name().unwrap_or("Mesh"),
                        primitive.index()
                    ),
                    mesh: mesh_handle,
                    material,
                });
            }
        }
        Ok(imported)
    }
}

impl Default for AssetServer {
    fn default() -> Self {
        let mut meshes = Assets::default();
        let mut builtin_primitives = HashMap::new();
        for shape in PrimitiveShape::ALL {
            builtin_primitives
                .insert(shape, meshes.insert(procedural_primitive_mesh(shape)));
        }
        let fallback_mesh = builtin_primitives[&PrimitiveShape::Cube];
        let builtin_sphere = builtin_primitives[&PrimitiveShape::Sphere];
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
            builtin_sphere,
            builtin_primitives,
            fallback_texture,
            fallback_material,
        }
    }
}

/// Builds a half-size UV sphere for built-in and procedural scene objects.
///
/// The returned CPU mesh can be stored in [`Assets`] and uploaded by any
/// renderer. Keeping this generator in the asset module lets the editor,
/// ECS runtime, and future import tools use the same geometry format.
pub fn procedural_sphere_mesh(subdivisions: u32) -> MeshAsset {
    use std::f32::consts::PI;

    let stacks = subdivisions.clamp(2, 128);
    let sectors = stacks * 2;
    let mut vertices =
        Vec::with_capacity(((stacks + 1) * (sectors + 1)) as usize);
    let mut indices = Vec::with_capacity((stacks * sectors * 6) as usize);
    for stack in 0..=stacks {
        let vertical = stack as f32 / stacks as f32;
        let phi = PI * vertical;
        let ring = 0.5 * phi.sin();
        let y = 0.5 * phi.cos();
        for sector in 0..=sectors {
            let horizontal = sector as f32 / sectors as f32;
            let theta = 2.0 * PI * horizontal;
            let x = ring * theta.cos();
            let z = ring * theta.sin();
            vertices.push(MeshVertex {
                position: [x, y, z],
                normal: [x * 2.0, y * 2.0, z * 2.0],
                uv: [horizontal, vertical],
                tangent: [0.0, 1.0, 0.0, 1.0],
            });
        }
    }
    let row = sectors + 1;
    for stack in 0..stacks {
        for sector in 0..sectors {
            let first = stack * row + sector;
            let second = first + row;
            indices.extend_from_slice(&[
                first,
                second,
                first + 1,
                second,
                second + 1,
                first + 1,
            ]);
        }
    }
    MeshAsset { vertices, indices }
}

/// Builds the CPU mesh used for one built-in editor primitive.
#[must_use]
pub fn procedural_primitive_mesh(shape: PrimitiveShape) -> MeshAsset {
    match shape {
        PrimitiveShape::Cube => fallback_cube(),
        PrimitiveShape::Sphere => procedural_sphere_mesh(16),
        PrimitiveShape::Triangle => mesh_from_triangles(&[[
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.0, 0.5, 0.0],
        ]]),
        PrimitiveShape::Plane => mesh_from_triangles(&[
            [[-0.5, 0.0, -0.5], [-0.5, 0.0, 0.5], [0.5, 0.0, 0.5]],
            [[-0.5, 0.0, -0.5], [0.5, 0.0, 0.5], [0.5, 0.0, -0.5]],
        ]),
        PrimitiveShape::Tetrahedron => polyhedron_mesh(
            &[
                [0.5, 0.5, 0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, -0.5],
                [0.5, -0.5, -0.5],
            ],
            &[[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]],
        ),
        PrimitiveShape::Octahedron => polyhedron_mesh(
            &[
                [0.5, 0.0, 0.0],
                [-0.5, 0.0, 0.0],
                [0.0, 0.5, 0.0],
                [0.0, -0.5, 0.0],
                [0.0, 0.0, 0.5],
                [0.0, 0.0, -0.5],
            ],
            &[
                [0, 2, 4],
                [4, 2, 1],
                [1, 2, 5],
                [5, 2, 0],
                [4, 3, 0],
                [1, 3, 4],
                [5, 3, 1],
                [0, 3, 5],
            ],
        ),
        PrimitiveShape::Dodecahedron => dodecahedron_mesh(),
        PrimitiveShape::Icosahedron => icosahedron_mesh(),
        PrimitiveShape::Pyramid => pyramid_mesh(),
        PrimitiveShape::Cylinder => cylinder_mesh(32),
        PrimitiveShape::Cone => cone_mesh(32),
        PrimitiveShape::Torus => torus_mesh(32, 12),
    }
}

fn mesh_from_triangles(triangles: &[[[f32; 3]; 3]]) -> MeshAsset {
    let mut vertices = Vec::with_capacity(triangles.len() * 3);
    let mut indices = Vec::with_capacity(triangles.len() * 3);
    for triangle in triangles {
        let edge_a = subtract(triangle[1], triangle[0]);
        let edge_b = subtract(triangle[2], triangle[0]);
        let normal = normalize3(cross(edge_a, edge_b));
        let base = vertices.len() as u32;
        for (position, uv) in
            triangle
                .iter()
                .copied()
                .zip([[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]])
        {
            vertices.push(MeshVertex {
                position,
                normal,
                uv,
                tangent: [1.0, 0.0, 0.0, 1.0],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    MeshAsset { vertices, indices }
}

fn polyhedron_mesh(points: &[[f32; 3]], faces: &[[usize; 3]]) -> MeshAsset {
    let triangles = faces
        .iter()
        .map(|face| {
            let mut triangle =
                [points[face[0]], points[face[1]], points[face[2]]];
            let normal = cross(
                subtract(triangle[1], triangle[0]),
                subtract(triangle[2], triangle[0]),
            );
            let center = [
                (triangle[0][0] + triangle[1][0] + triangle[2][0]) / 3.0,
                (triangle[0][1] + triangle[1][1] + triangle[2][1]) / 3.0,
                (triangle[0][2] + triangle[1][2] + triangle[2][2]) / 3.0,
            ];
            if dot(normal, center) < 0.0 {
                triangle.swap(1, 2);
            }
            triangle
        })
        .collect::<Vec<_>>();
    mesh_from_triangles(&triangles)
}

fn icosahedron_mesh() -> MeshAsset {
    let golden = (1.0 + 5.0_f32.sqrt()) * 0.5;
    let raw = [
        [-1.0, golden, 0.0],
        [1.0, golden, 0.0],
        [-1.0, -golden, 0.0],
        [1.0, -golden, 0.0],
        [0.0, -1.0, golden],
        [0.0, 1.0, golden],
        [0.0, -1.0, -golden],
        [0.0, 1.0, -golden],
        [golden, 0.0, -1.0],
        [golden, 0.0, 1.0],
        [-golden, 0.0, -1.0],
        [-golden, 0.0, 1.0],
    ];
    let points = raw.map(|point| {
        let normal = normalize3(point);
        [normal[0] * 0.5, normal[1] * 0.5, normal[2] * 0.5]
    });
    polyhedron_mesh(
        &points,
        &[
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ],
    )
}

fn dodecahedron_mesh() -> MeshAsset {
    let golden = (1.0 + 5.0_f32.sqrt()) * 0.5;
    let inverse = 1.0 / golden;
    let raw = [
        [-1.0, -1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, 1.0, 1.0],
        [1.0, -1.0, -1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, -1.0],
        [1.0, 1.0, 1.0],
        [0.0, -inverse, -golden],
        [0.0, -inverse, golden],
        [0.0, inverse, -golden],
        [0.0, inverse, golden],
        [-inverse, -golden, 0.0],
        [-inverse, golden, 0.0],
        [inverse, -golden, 0.0],
        [inverse, golden, 0.0],
        [-golden, 0.0, -inverse],
        [golden, 0.0, -inverse],
        [-golden, 0.0, inverse],
        [golden, 0.0, inverse],
    ];
    let scale = 0.5 / 3.0_f32.sqrt();
    let points =
        raw.map(|point| [point[0] * scale, point[1] * scale, point[2] * scale]);
    let mut face_sets = Vec::<Vec<usize>>::new();
    for first in 0..points.len() - 2 {
        for second in first + 1..points.len() - 1 {
            for third in second + 1..points.len() {
                let mut normal = cross(
                    subtract(points[second], points[first]),
                    subtract(points[third], points[first]),
                );
                if dot(normal, normal) < 1.0e-8 {
                    continue;
                }
                normal = normalize3(normal);
                let plane = dot(normal, points[first]);
                let distances = points.map(|point| dot(normal, point) - plane);
                let supports_hull = distances
                    .iter()
                    .all(|distance| *distance <= 1.0e-4)
                    || distances.iter().all(|distance| *distance >= -1.0e-4);
                if !supports_hull {
                    continue;
                }
                let face = distances
                    .iter()
                    .enumerate()
                    .filter_map(|(index, distance)| {
                        (distance.abs() <= 1.0e-4).then_some(index)
                    })
                    .collect::<Vec<_>>();
                if face.len() == 5 && !face_sets.contains(&face) {
                    face_sets.push(face);
                }
            }
        }
    }

    let mut triangles = Vec::with_capacity(36);
    for mut face in face_sets {
        let center = face.iter().fold([0.0; 3], |mut sum, index| {
            for axis in 0..3 {
                sum[axis] += points[*index][axis] / 5.0;
            }
            sum
        });
        let normal = normalize3(center);
        let basis_x = normalize3(subtract(points[face[0]], center));
        let basis_y = cross(normal, basis_x);
        face.sort_by(|left, right| {
            let left_delta = subtract(points[*left], center);
            let right_delta = subtract(points[*right], center);
            let left_angle =
                dot(left_delta, basis_y).atan2(dot(left_delta, basis_x));
            let right_angle =
                dot(right_delta, basis_y).atan2(dot(right_delta, basis_x));
            left_angle.total_cmp(&right_angle)
        });
        for corner in 1..face.len() - 1 {
            triangles.push([
                points[face[0]],
                points[face[corner]],
                points[face[corner + 1]],
            ]);
        }
    }
    mesh_from_triangles(&triangles)
}

fn pyramid_mesh() -> MeshAsset {
    polyhedron_mesh(
        &[
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
            [0.0, 0.5, 0.0],
        ],
        &[
            [0, 1, 2],
            [0, 2, 3],
            [0, 4, 1],
            [1, 4, 2],
            [2, 4, 3],
            [3, 4, 0],
        ],
    )
}

fn cylinder_mesh(segments: u32) -> MeshAsset {
    let mut triangles = Vec::with_capacity((segments * 4) as usize);
    for step in 0..segments {
        let a = std::f32::consts::TAU * step as f32 / segments as f32;
        let b = std::f32::consts::TAU * (step + 1) as f32 / segments as f32;
        let bottom_a = [0.5 * a.cos(), -0.5, 0.5 * a.sin()];
        let bottom_b = [0.5 * b.cos(), -0.5, 0.5 * b.sin()];
        let top_a = [bottom_a[0], 0.5, bottom_a[2]];
        let top_b = [bottom_b[0], 0.5, bottom_b[2]];
        triangles.extend_from_slice(&[
            [bottom_a, top_b, bottom_b],
            [bottom_a, top_a, top_b],
            [[0.0, 0.5, 0.0], top_b, top_a],
            [[0.0, -0.5, 0.0], bottom_a, bottom_b],
        ]);
    }
    mesh_from_triangles(&triangles)
}

fn cone_mesh(segments: u32) -> MeshAsset {
    let mut triangles = Vec::with_capacity((segments * 2) as usize);
    for step in 0..segments {
        let a = std::f32::consts::TAU * step as f32 / segments as f32;
        let b = std::f32::consts::TAU * (step + 1) as f32 / segments as f32;
        let point_a = [0.5 * a.cos(), -0.5, 0.5 * a.sin()];
        let point_b = [0.5 * b.cos(), -0.5, 0.5 * b.sin()];
        triangles.push([point_a, [0.0, 0.5, 0.0], point_b]);
        triangles.push([[0.0, -0.5, 0.0], point_a, point_b]);
    }
    mesh_from_triangles(&triangles)
}

fn torus_mesh(major_segments: u32, minor_segments: u32) -> MeshAsset {
    let mut vertices =
        Vec::with_capacity((major_segments * minor_segments) as usize);
    for major in 0..major_segments {
        let u = std::f32::consts::TAU * major as f32 / major_segments as f32;
        for minor in 0..minor_segments {
            let v =
                std::f32::consts::TAU * minor as f32 / minor_segments as f32;
            let normal = [u.cos() * v.cos(), v.sin(), u.sin() * v.cos()];
            let ring = 0.36 + 0.14 * v.cos();
            vertices.push(MeshVertex {
                position: [ring * u.cos(), 0.14 * v.sin(), ring * u.sin()],
                normal,
                uv: [
                    major as f32 / major_segments as f32,
                    minor as f32 / minor_segments as f32,
                ],
                tangent: [-u.sin(), 0.0, u.cos(), 1.0],
            });
        }
    }
    let mut indices =
        Vec::with_capacity((major_segments * minor_segments * 6) as usize);
    for major in 0..major_segments {
        for minor in 0..minor_segments {
            let a = major * minor_segments + minor;
            let b = ((major + 1) % major_segments) * minor_segments + minor;
            let c = major * minor_segments + (minor + 1) % minor_segments;
            let d = ((major + 1) % major_segments) * minor_segments
                + (minor + 1) % minor_segments;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    MeshAsset { vertices, indices }
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length = dot(value, value).sqrt();
    if length > f32::EPSILON {
        [value[0] / length, value[1] / length, value[2] / length]
    } else {
        [0.0, 1.0, 0.0]
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

    fn gltf_image(
        format: gltf::image::Format,
        pixels: Vec<u8>,
    ) -> gltf::image::Data {
        gltf::image::Data {
            pixels,
            format,
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn gltf_image_conversion_expands_every_supported_channel_layout() {
        assert_eq!(
            gltf_image_to_rgba8(&gltf_image(
                gltf::image::Format::R8,
                vec![200]
            )),
            vec![200, 200, 200, 255]
        );
        assert_eq!(
            gltf_image_to_rgba8(&gltf_image(
                gltf::image::Format::R8G8B8,
                vec![10, 20, 30]
            )),
            vec![10, 20, 30, 255]
        );
        assert_eq!(
            gltf_image_to_rgba8(&gltf_image(
                gltf::image::Format::R8G8B8A8,
                vec![10, 20, 30, 40]
            )),
            vec![10, 20, 30, 40]
        );
        assert_eq!(
            gltf_image_to_rgba8(&gltf_image(
                gltf::image::Format::R32G32B32FLOAT,
                [1.0f32, 0.5, 0.0]
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect(),
            )),
            vec![255, 127, 0, 255]
        );
    }

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

    #[test]
    fn texture_loader_decodes_and_deduplicates_image_files() {
        let folder = std::env::temp_dir()
            .join(format!("rusting-texture-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("pixel.png");
        image::RgbaImage::from_raw(1, 1, vec![12, 34, 56, 255])
            .unwrap()
            .save(&path)
            .unwrap();
        let mut server = AssetServer::default();

        let first = server.load_texture(&path).unwrap();
        let second = server.load_texture(&path).unwrap();

        assert_eq!(first, second);
        assert_eq!(server.textures.get(first).unwrap().size, [1, 1]);
        assert_eq!(
            server.textures.get(first).unwrap().rgba8,
            [12, 34, 56, 255]
        );
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn native_mesh_loader_restores_imported_mesh_data() {
        let folder = std::env::temp_dir()
            .join(format!("rusting-mesh-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("triangle.rmesh");
        let mesh = MeshAsset {
            vertices: vec![MeshVertex::default(); 3],
            indices: vec![0, 1, 2],
        };
        std::fs::write(&path, bincode::serialize(&mesh).unwrap()).unwrap();
        let mut server = AssetServer::default();

        let handle = server.load_mesh(&path).unwrap();

        assert_eq!(server.meshes.get(handle), Some(&mesh));
        assert_eq!(server.load_mesh(&path).unwrap(), handle);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn every_builtin_primitive_has_valid_triangle_geometry() {
        let server = AssetServer::default();
        for shape in PrimitiveShape::ALL {
            let handle = server.builtin_primitive(shape);
            let mesh = server.meshes.get(handle).unwrap();
            assert!(
                !mesh.vertices.is_empty(),
                "{} has no vertices",
                shape.label()
            );
            assert_eq!(
                mesh.indices.len() % 3,
                0,
                "{} is not triangulated",
                shape.label()
            );
            assert!(
                mesh.indices
                    .iter()
                    .all(|index| (*index as usize) < mesh.vertices.len()),
                "{} contains an invalid index",
                shape.label()
            );
            assert_eq!(server.primitive_for_handle(handle), Some(shape));
        }
    }
}
