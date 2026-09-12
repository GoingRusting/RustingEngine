use crate::geometry::Mesh;
use crate::scene::object::Instance;
use crate::scene::object::Texture;
use std::fmt;
use std::sync::Arc;
use vulkano::memory::allocator::StandardMemoryAllocator;

/// Errors that can occur while importing a glTF/GLB file for the legacy
/// `Engine::add_gltf` compatibility path.
#[derive(Debug)]
pub enum GltfLoadError {
    Import(gltf::Error),
    MissingPositions,
}

impl fmt::Display for GltfLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GltfLoadError::Import(err) => {
                write!(f, "failed to import glTF file: {err}")
            }
            GltfLoadError::MissingPositions => {
                write!(f, "glTF primitive has no POSITION attribute")
            }
        }
    }
}

impl std::error::Error for GltfLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GltfLoadError::Import(err) => Some(err),
            GltfLoadError::MissingPositions => None,
        }
    }
}

impl From<gltf::Error> for GltfLoadError {
    fn from(err: gltf::Error) -> Self {
        GltfLoadError::Import(err)
    }
}

/// Meshes paired with their base instance data, plus every texture the
/// glTF file referenced.
pub type GltfScene = (Vec<(Mesh, Instance)>, Vec<Texture>);

pub fn load_gltf_scene(
    allocator: &Arc<StandardMemoryAllocator>,
    path: &str,
) -> Result<GltfScene, GltfLoadError> {
    let mut textures: Vec<Texture> = Vec::new();
    let (document, buffers, images) = gltf::import(path)?;

    let mut result = Vec::new();

    for scene in document.scenes() {
        for node in scene.nodes() {
            process_node(
                allocator,
                &buffers,
                &images,
                &mut textures,
                &mut result,
                &node,
                nalgebra::Matrix4::identity(),
            )?;
        }
    }

    Ok((result, textures))
}

use crate::Transform;

fn process_node(
    allocator: &Arc<StandardMemoryAllocator>,
    buffers: &[gltf::buffer::Data],
    images: &[gltf::image::Data],
    textures: &mut Vec<Texture>,
    result: &mut Vec<(Mesh, Instance)>,
    node: &gltf::Node,
    parent_transform: nalgebra::Matrix4<f32>,
) -> Result<(), GltfLoadError> {
    let local_transform = nalgebra::Matrix4::from(node.transform().matrix());
    let global_transform = parent_transform * local_transform;

    // If node has a mesh → extract it
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            let (vertices, indices) = extract_primitive(&primitive, buffers)?;

            let mesh = if let Some(ref idx) = indices {
                Mesh::new_indexed(allocator, &vertices, idx)
            } else {
                Mesh::new(allocator, &vertices, None)
            };

            let material = primitive.material();
            let pbr = material.pbr_metallic_roughness();

            // Base color (RGBA)
            let base_color = pbr.base_color_factor();

            // Convert to RGB, bc I am bitch and I havent done alpha
            let color = [base_color[0], base_color[1], base_color[2]];

            // Metallic / roughness
            let metalness = pbr.metallic_factor();
            let roughness = pbr.roughness_factor();
            let base_color_texture = pbr.base_color_texture().map(|info| {
                let tex = info.texture();
                let img = &images[tex.source().index()];

                let index = textures.len();

                textures.push(Texture {
                    pixels: img.pixels.clone(),
                    width: img.width,
                    height: img.height,
                });

                index
            });
            let metallic_roughness_texture =
                pbr.metallic_roughness_texture().map(|info| {
                    let tex = info.texture();
                    let img = &images[tex.source().index()];

                    let index = textures.len();

                    textures.push(Texture {
                        pixels: img.pixels.clone(),
                        width: img.width,
                        height: img.height,
                    });

                    index
                });
            let instance = Instance {
                model_matrix: Transform::from_matrix(global_transform)
                    .to_matrix(),
                color,
                roughness,
                metalness,
                base_color_texture,
                metallic_roughness_texture,
                ..Default::default()
            };

            result.push((mesh, instance));
        }
    }

    // Recurse into children
    for child in node.children() {
        process_node(
            allocator,
            buffers,
            images,
            textures,
            result,
            &child,
            global_transform,
        )?;
    }

    Ok(())
}

use crate::geometry::VertexPosColorUv;

fn extract_primitive(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> Result<(Vec<VertexPosColorUv>, Option<Vec<u32>>), GltfLoadError> {
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or(GltfLoadError::MissingPositions)?
        .collect();

    let normals: Vec<[f32; 3]> = reader
        .read_normals()
        .map(|n| n.collect())
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);

    let tex_coords: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|tc| tc.into_f32().collect())
        .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

    let indices = reader
        .read_indices()
        .map(|i| i.into_u32().collect::<Vec<u32>>());

    let mut vertices = Vec::with_capacity(positions.len());

    for i in 0..positions.len() {
        vertices.push(VertexPosColorUv {
            position: positions[i],
            normal: normals[i],
            uv: tex_coords[i],
        });
    }

    Ok((vertices, indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_converts_to_typed_import_error() {
        let err = gltf::import("./does-not-exist.gltf").unwrap_err();
        let load_err = GltfLoadError::from(err);
        assert!(matches!(load_err, GltfLoadError::Import(_)));
        assert!(load_err.to_string().contains("failed to import glTF file"));
    }

    #[test]
    fn missing_positions_error_has_a_readable_message() {
        assert_eq!(
            GltfLoadError::MissingPositions.to_string(),
            "glTF primitive has no POSITION attribute"
        );
    }
}
