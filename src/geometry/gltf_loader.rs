use crate::geometry::Mesh;
use crate::scene::object::Instance;
use crate::scene::object::Texture;
use std::sync::Arc;
use vulkano::memory::allocator::StandardMemoryAllocator;

pub fn load_gltf_scene(
    allocator: &Arc<StandardMemoryAllocator>,
    path: &str,
) -> (Vec<(Mesh, Instance)>, Vec<Texture>) {
    let mut textures: Vec<Texture> = Vec::new();
    let (document, buffers, images) = gltf::import(path).unwrap();

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
            );
        }
    }

    (result, textures)
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
) {
    let local_transform = nalgebra::Matrix4::from(node.transform().matrix());
    let global_transform = parent_transform * local_transform;

    // If node has a mesh → extract it
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            let (vertices, indices) = extract_primitive(&primitive, buffers);

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
            let metallic_roughness_texture = pbr.metallic_roughness_texture().map(|info| {
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
                model_matrix: Transform::from_matrix(global_transform).to_matrix(),
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
        );
    }
}

use crate::geometry::VertexPosColorUv;

fn extract_primitive(
    primitive: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
) -> (Vec<VertexPosColorUv>, Option<Vec<u32>>) {
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

    let positions: Vec<[f32; 3]> = reader.read_positions().unwrap().collect();

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

    (vertices, indices)
}
