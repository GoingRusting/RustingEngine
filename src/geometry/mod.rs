#[allow(dead_code, unused)]
// ! SHAPE AND MESH SYSTEM - Defines geometry and scene organization
// More I do this, more I understand and respect all developers that created a blender, unity, unreal engine and etc. just F for every man that did something like that and biggest F for that people, who did it opensource. F
pub mod gltf_loader;
use std::sync::Arc;
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::pipeline::graphics::vertex_input::Vertex;
// * Trait combining all requirements for vertex types
pub trait VertexType:
    Vertex + bytemuck::Pod + bytemuck::Zeroable + Send + Sync
{
}

// ! VERTEX WITH POSITION AND COLOR - Basic vertex format
#[repr(C)]
#[derive(Copy, Clone, Debug, Vertex, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VertexPosColor {
    #[format(R32G32B32_SFLOAT)]
    pub position: [f32; 3], // XYZ position
    #[format(R32G32B32_SFLOAT)]
    pub color: [f32; 3], // RGB color
    #[format(R32G32B32_SFLOAT)]
    pub barycentric: [f32; 3],
}

impl VertexType for VertexPosColor {}

// ! VERTEX WITH UV COORDINATES - For textured objects (future use)
#[repr(C)]
#[derive(Copy, Clone, Debug, Vertex, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VertexPosColorUv {
    #[format(R32G32B32_SFLOAT)]
    pub position: [f32; 3], // XYZ position
    #[format(R32G32B32_SFLOAT)]
    pub normal: [f32; 3], // Normals
    #[format(R32G32_SFLOAT)]
    pub uv: [f32; 2], // Texture coordinates
}

impl VertexType for VertexPosColorUv {}

// ! VERTEX WITH NORMALS - For lighting calculations
#[repr(C)]
#[derive(Copy, Clone, Debug, Vertex, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VertexPosColorNormal {
    #[format(R32G32B32_SFLOAT)]
    pub position: [f32; 3], // XYZ position
    #[format(R32G32B32_SFLOAT)]
    pub color: [f32; 3], // RGB color
    #[format(R32G32B32_SFLOAT)]
    pub normal: [f32; 3], // Normals(for light)
    #[format(R32G32B32_SFLOAT)]
    pub barycentric: [f32; 3], // yes
}

impl VertexType for VertexPosColorNormal {}

// ! MESH - Container for vertex and index data on the GPU
#[derive(Clone)]
pub struct Mesh {
    pub vertices: Subbuffer<[VertexPosColorUv]>, // GPU buffer of vertices
    pub indices: Option<Subbuffer<[u32]>>, // Optional index buffer (for reuse)
    pub vertex_count: u32,                 // Number of vertices
    pub index_count: u32,                  // Number of indices (if using)
}

impl Mesh {
    // * Create a new mesh from CPU-side data
    pub fn new(
        memory_allocator: &Arc<StandardMemoryAllocator>,
        vertices: &[VertexPosColorUv],
        indices: Option<&[u32]>,
    ) -> Self {
        // Upload vertices to GPU
        let vertex_buffer = Buffer::from_iter(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            vertices.iter().copied(),
        )
        .unwrap();

        let vertex_count = vertices.len() as u32;

        // Upload indices if provided
        let (index_buffer, index_count) = if let Some(indices) = indices {
            let buffer = Buffer::from_iter(
                memory_allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::INDEX_BUFFER,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                indices.iter().copied(),
            )
            .unwrap();
            (Some(buffer), indices.len() as u32)
        } else {
            (None, 0)
        };

        Self {
            vertices: vertex_buffer,
            indices: index_buffer,
            vertex_count,
            index_count,
        }
    }

    // * Create a mesh with indexed geometry (more efficient for complex shapes)
    pub fn new_indexed(
        memory_allocator: &Arc<StandardMemoryAllocator>,
        vertices: &[VertexPosColorUv],
        indices: &[u32],
    ) -> Self {
        Self::new(memory_allocator, vertices, Some(indices))
    }
}

// ? Why am I even writing a comments if I am the only one who read it, am I schizophrenic?

// ! PRIMITIVE SHAPES - Factory functions for creating common meshes
pub mod shapes {
    use super::*;
    use std::f32::consts::PI;

    // Helper function to normalize a vector
    fn normalize(v: [f32; 3]) -> [f32; 3] {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 0.0 {
            [v[0] / len, v[1] / len, v[2] / len]
        } else {
            v
        }
    }

    fn calculate_normal(p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]) -> [f32; 3] {
        let u = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
        let v = [p3[0] - p1[0], p3[1] - p1[1], p3[2] - p1[2]];

        // Cross product
        let normal = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];

        normalize(normal)
    }

    // Helper function to add a triangle with barycentric coordinates
    fn add_triangle(
        vertices: &mut Vec<VertexPosColorUv>,
        p1: [f32; 3],
        p2: [f32; 3],
        p3: [f32; 3],
    ) {
        // Calculate face normal
        let normal = calculate_normal(p1, p2, p3);

        vertices.push(VertexPosColorUv {
            position: p1,
            normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p2,
            normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p3,
            normal,
            uv: [1.0, 1.0],
        });
    }

    fn add_triangle_idx(
        vertices: &mut Vec<VertexPosColorUv>,
        indices: &mut Vec<u32>,
        p1: [f32; 3],
        p2: [f32; 3],
        p3: [f32; 3],
    ) {
        let normal = calculate_normal(p1, p2, p3);
        let base_idx = vertices.len() as u32;

        vertices.push(VertexPosColorUv {
            position: p1,
            normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p2,
            normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p3,
            normal,
            uv: [1.0, 1.0],
        });

        indices.push(base_idx);
        indices.push(base_idx + 1);
        indices.push(base_idx + 2);
    }

    // * Wrong Normal triangle
    fn add_wrong_triangle(
        vertices: &mut Vec<VertexPosColorUv>,
        p1: [f32; 3],
        p2: [f32; 3],
        p3: [f32; 3],
    ) {
        // Calculate face normal
        let mut normal = calculate_normal(p1, p2, p3);
        normal[0] *= -1.0;
        normal[1] *= -1.0;
        normal[2] *= -1.0;
        vertices.push(VertexPosColorUv {
            position: p1,
            normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p2,
            normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p3,
            normal,
            uv: [1.0, 1.0],
        });
    }

    // Helper function to add a quad as two triangles
    // I dont know who will use it, maybe some chill guy                      or lady....
    #[allow(dead_code)]
    fn add_quad(
        vertices: &mut Vec<VertexPosColorUv>,
        p1: [f32; 3],
        p2: [f32; 3],
        p3: [f32; 3],
        p4: [f32; 3],
    ) {
        // Calculate normal for first triangle
        let normal1 = calculate_normal(p1, p2, p3);
        // Calculate normal for second triangle
        let normal2 = calculate_normal(p3, p4, p1);

        // First triangle
        vertices.push(VertexPosColorUv {
            position: p1,
            normal: normal1,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p2,
            normal: normal1,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p3,
            normal: normal1,
            uv: [1.0, 1.0],
        });

        // Second triangle
        vertices.push(VertexPosColorUv {
            position: p3,
            normal: normal2,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p4,
            normal: normal2,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: p1,
            normal: normal2,
            uv: [1.0, 1.0],
        });
    }

    pub fn create_triangle(
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) -> Mesh {
        let mut vertices: Vec<VertexPosColorUv> = Vec::new();

        let v = [[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]];

        // Normal for a flat triangle in XY plane
        add_triangle(&mut vertices, v[0], v[1], v[2]);

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a unit cube centered at origin
    pub fn create_cube(
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) -> Mesh {
        let mut vertices: Vec<VertexPosColorUv> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        let v = [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        ];

        // Front face (+Z)
        add_triangle_idx(&mut vertices, &mut indices, v[0], v[1], v[2]);
        add_triangle_idx(&mut vertices, &mut indices, v[2], v[3], v[0]);

        // Right face (+X)
        add_triangle_idx(&mut vertices, &mut indices, v[1], v[5], v[6]);
        add_triangle_idx(&mut vertices, &mut indices, v[6], v[2], v[1]);

        // Back face (-Z)
        add_triangle_idx(&mut vertices, &mut indices, v[5], v[4], v[7]);
        add_triangle_idx(&mut vertices, &mut indices, v[7], v[6], v[5]);

        // Left face (-X)
        add_triangle_idx(&mut vertices, &mut indices, v[4], v[0], v[3]);
        add_triangle_idx(&mut vertices, &mut indices, v[3], v[7], v[4]);

        // Top face (+Y)
        add_triangle_idx(&mut vertices, &mut indices, v[3], v[2], v[6]);
        add_triangle_idx(&mut vertices, &mut indices, v[6], v[7], v[3]);

        // Bottom face (-Y)
        add_triangle_idx(&mut vertices, &mut indices, v[4], v[5], v[1]);
        add_triangle_idx(&mut vertices, &mut indices, v[1], v[0], v[4]);

        Mesh::new(memory_allocator, &vertices, Some(&indices))
    }

    // * Wrong Cube, to test normals
    pub fn create_wrong_cube(
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) -> Mesh {
        let mut vertices: Vec<VertexPosColorUv> = Vec::new();

        let v = [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        ];

        // Front face (+Z)
        add_wrong_triangle(&mut vertices, v[0], v[1], v[2]);
        add_wrong_triangle(&mut vertices, v[2], v[3], v[0]);

        // Right face (+X)
        add_wrong_triangle(&mut vertices, v[1], v[5], v[6]);
        add_wrong_triangle(&mut vertices, v[6], v[2], v[1]);

        // Back face (-Z)
        add_wrong_triangle(&mut vertices, v[5], v[4], v[7]);
        add_wrong_triangle(&mut vertices, v[7], v[6], v[5]);

        // Left face (-X)
        add_wrong_triangle(&mut vertices, v[4], v[0], v[3]);
        add_wrong_triangle(&mut vertices, v[3], v[7], v[4]);

        // Top face (+Y)
        add_wrong_triangle(&mut vertices, v[3], v[2], v[6]);
        add_wrong_triangle(&mut vertices, v[6], v[7], v[3]);

        // Bottom face (-Y)
        add_wrong_triangle(&mut vertices, v[4], v[5], v[1]);
        add_wrong_triangle(&mut vertices, v[1], v[0], v[4]);

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a sphere by subdividing into sectors and stacks,
    // ! DO NOT WRITE 16 OR 32, IT IS NOT TOTAL SUBDIVIDES!!!, write like 2-4 max
    pub fn create_sphere(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        sectors: u32,
        stacks: u32,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let radius = 0.5;

        for i in 0..=stacks {
            let stack_angle = PI / 2.0 - (i as f32) * PI / stacks as f32;
            let xy = radius * stack_angle.cos();
            let z = radius * stack_angle.sin();

            for j in 0..=sectors {
                let sector_angle = (j as f32) * 2.0 * PI / sectors as f32;
                let x = xy * sector_angle.cos();
                let y = xy * sector_angle.sin();

                // For smooth shading, normal is the normalized position vector
                let normal = normalize([x, y, z]);

                vertices.push(VertexPosColorUv {
                    position: [x, y, z],

                    normal,
                    uv: [1.0, 1.0],
                });
            }
        }

        for i in 0..stacks {
            for j in 0..sectors {
                let p1 = i * (sectors + 1) + j;
                let p2 = p1 + (sectors + 1);
                let p3 = p1 + 1;
                let p4 = p2 + 1;

                indices.push(p1);
                indices.push(p2);
                indices.push(p3);

                indices.push(p2);
                indices.push(p4);
                indices.push(p3);
            }
        }

        // Reorder vertices based on indices to avoid indexed rendering
        let mut triangle_vertices = Vec::new();
        for i in (0..indices.len()).step_by(3) {
            let v1 = vertices[indices[i] as usize];
            let v2 = vertices[indices[i + 1] as usize];
            let v3 = vertices[indices[i + 2] as usize];
            triangle_vertices.push(v1);
            triangle_vertices.push(v2);
            triangle_vertices.push(v3);
        }

        Mesh::new(memory_allocator, &triangle_vertices, None)
    }

    // * Create a sphere using icosahedron subdivision (better distribution)
    pub fn create_sphere_subdivided(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        subdivisions: u32,
    ) -> Mesh {
        let t = (1.0 + (5.0_f32).sqrt()) / 2.0;

        let initial_vertices = [
            [-1.0, t, 0.0],
            [1.0, t, 0.0],
            [-1.0, -t, 0.0],
            [1.0, -t, 0.0],
            [0.0, -1.0, t],
            [0.0, 1.0, t],
            [0.0, -1.0, -t],
            [0.0, 1.0, -t],
            [t, 0.0, -1.0],
            [t, 0.0, 1.0],
            [-t, 0.0, -1.0],
            [-t, 0.0, 1.0],
        ];

        let initial_indices = [
            0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11, 1, 5, 9, 5, 11, 4,
            11, 10, 2, 10, 7, 6, 7, 1, 8, 3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8,
            3, 8, 9, 4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9, 8, 1,
        ];

        // Normalize all vertices to lie on sphere
        let mut normalized_vertices: Vec<[f32; 3]> =
            initial_vertices.iter().map(|v| normalize(*v)).collect();

        // Subdivide
        let mut indices = initial_indices.to_vec();

        for _ in 0..subdivisions {
            let mut new_indices = Vec::new();
            let mut mid_point_cache = std::collections::HashMap::new();

            for i in (0..indices.len()).step_by(3) {
                let v1 = normalized_vertices[indices[i] as usize];
                let v2 = normalized_vertices[indices[i + 1] as usize];
                let v3 = normalized_vertices[indices[i + 2] as usize];

                // Get or create midpoints
                let a = *mid_point_cache
                    .entry((indices[i], indices[i + 1]))
                    .or_insert_with(|| {
                        normalized_vertices.push(normalize([
                            (v1[0] + v2[0]) * 0.5,
                            (v1[1] + v2[1]) * 0.5,
                            (v1[2] + v2[2]) * 0.5,
                        ]));
                        (normalized_vertices.len() - 1) as u32
                    });

                let b = *mid_point_cache
                    .entry((indices[i + 1], indices[i + 2]))
                    .or_insert_with(|| {
                        normalized_vertices.push(normalize([
                            (v2[0] + v3[0]) * 0.5,
                            (v2[1] + v3[1]) * 0.5,
                            (v2[2] + v3[2]) * 0.5,
                        ]));
                        (normalized_vertices.len() - 1) as u32
                    });

                let c = *mid_point_cache
                    .entry((indices[i + 2], indices[i]))
                    .or_insert_with(|| {
                        normalized_vertices.push(normalize([
                            (v3[0] + v1[0]) * 0.5,
                            (v3[1] + v1[1]) * 0.5,
                            (v3[2] + v1[2]) * 0.5,
                        ]));
                        (normalized_vertices.len() - 1) as u32
                    });

                // Create 4 triangles
                new_indices.extend_from_slice(&[indices[i], a, c]);
                new_indices.extend_from_slice(&[indices[i + 1], b, a]);
                new_indices.extend_from_slice(&[indices[i + 2], c, b]);
                new_indices.extend_from_slice(&[a, b, c]);
            }
            indices = new_indices;
        }

        // Create final vertex list with normals (normalized position for smooth shading)
        let mut final_vertices = Vec::new();
        for i in (0..indices.len()).step_by(3) {
            let v1_pos = normalized_vertices[indices[i] as usize];
            let v2_pos = normalized_vertices[indices[i + 1] as usize];
            let v3_pos = normalized_vertices[indices[i + 2] as usize];

            // For smooth shading, normal is the normalized position
            let v1_normal = v1_pos;
            let v2_normal = v2_pos;
            let v3_normal = v3_pos;

            final_vertices.push(VertexPosColorUv {
                position: v1_pos,
                normal: v1_normal,
                uv: [1.0, 1.0],
            });
            final_vertices.push(VertexPosColorUv {
                position: v2_pos,
                normal: v2_normal,
                uv: [1.0, 1.0],
            });
            final_vertices.push(VertexPosColorUv {
                position: v3_pos,
                normal: v3_normal,
                uv: [1.0, 1.0],
            });
        }

        let final_indices: Vec<u32> =
            (0..final_vertices.len() as u32).collect();
        Mesh::new(memory_allocator, &final_vertices, Some(&final_indices))
    }

    // * Create a tetrahedron
    pub fn create_tetrahedron(
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let a = 0.5;

        // Four vertices of a regular tetrahedron
        let v = [[a, a, a], [a, -a, -a], [-a, a, -a], [-a, -a, a]];

        // Four faces (each triangle)
        add_triangle(&mut vertices, v[0], v[1], v[2]);
        add_triangle(&mut vertices, v[0], v[2], v[3]);
        add_triangle(&mut vertices, v[0], v[3], v[1]);
        add_triangle(&mut vertices, v[1], v[3], v[2]);

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create an octahedron
    pub fn create_octahedron(
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let a = 0.5;

        // Six vertices
        let v = [
            [a, 0.0, 0.0],
            [-a, 0.0, 0.0],
            [0.0, a, 0.0],
            [0.0, -a, 0.0],
            [0.0, 0.0, a],
            [0.0, 0.0, -a],
        ];

        // Top half (4 triangles)
        add_triangle(&mut vertices, v[4], v[0], v[2]);
        add_triangle(&mut vertices, v[4], v[2], v[1]);
        add_triangle(&mut vertices, v[4], v[1], v[3]);
        add_triangle(&mut vertices, v[4], v[3], v[0]);

        // Bottom half (4 triangles)
        add_triangle(&mut vertices, v[5], v[2], v[0]);
        add_triangle(&mut vertices, v[5], v[1], v[2]);
        add_triangle(&mut vertices, v[5], v[3], v[1]);
        add_triangle(&mut vertices, v[5], v[0], v[3]);

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a dodecahedron
    pub fn create_dodecahedron(
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let phi = (1.0 + (5.0_f32).sqrt()) / 2.0; // Golden ratio
        let a = 0.3;
        let b = a / phi;
        let c = a * phi;

        // 20 vertices
        let v = [
            [a, a, a],
            [a, a, -a],
            [a, -a, a],
            [a, -a, -a],
            [-a, a, a],
            [-a, a, -a],
            [-a, -a, a],
            [-a, -a, -a],
            [0.0, b, -c],
            [0.0, -b, -c],
            [0.0, b, c],
            [0.0, -b, c],
            [b, c, 0.0],
            [b, -c, 0.0],
            [-b, c, 0.0],
            [-b, -c, 0.0],
            [c, 0.0, b],
            [c, 0.0, -b],
            [-c, 0.0, b],
            [-c, 0.0, -b],
        ];

        // 12 pentagonal faces (each split into 3 triangles)
        let faces = [
            [0, 10, 11, 2, 16],
            [0, 16, 17, 1, 8],
            [0, 8, 4, 14, 10],
            [1, 17, 3, 9, 5],
            [1, 5, 13, 12, 8],
            [2, 11, 6, 18, 15],
            [2, 15, 13, 3, 16],
            [3, 13, 5, 9, 17],
            [4, 8, 12, 14, 18],
            [4, 18, 6, 11, 10],
            [5, 9, 7, 19, 13],
            [6, 15, 19, 7, 18],
        ];

        for face in faces.iter() {
            // Triangulate pentagon (fan from first vertex)
            for i in 1..(face.len() - 1) {
                add_triangle(
                    &mut vertices,
                    v[face[0]],
                    v[face[i]],
                    v[face[i + 1]],
                );
            }
        }

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create an icosahedron
    pub fn create_icosahedron(
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let phi = (1.0 + (5.0_f32).sqrt()) / 2.0;
        let a = 0.5;

        // 12 vertices (normalize to sphere for better shape)
        let v_raw = [
            [-a, phi, 0.0],
            [a, phi, 0.0],
            [-a, -phi, 0.0],
            [a, -phi, 0.0],
            [0.0, -a, phi],
            [0.0, a, phi],
            [0.0, -a, -phi],
            [0.0, a, -phi],
            [phi, 0.0, -a],
            [phi, 0.0, a],
            [-phi, 0.0, -a],
            [-phi, 0.0, a],
        ];

        // Normalize vertices to lie on sphere
        let v: Vec<[f32; 3]> = v_raw.iter().map(|&p| normalize(p)).collect();

        // 20 triangular faces
        let faces = [
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
        ];

        for face in faces.iter() {
            add_triangle(&mut vertices, v[face[0]], v[face[1]], v[face[2]]);
        }

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a torus, this shit has sick formula
    pub fn create_torus(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        major_radius: f32,
        minor_radius: f32,
        major_segments: u32,
        minor_segments: u32,
    ) -> Mesh {
        let mut vertices = Vec::new();

        for i in 0..major_segments {
            let major_angle = (i as f32) * 2.0 * PI / major_segments as f32;
            let next_major_angle =
                ((i + 1) as f32) * 2.0 * PI / major_segments as f32;

            let cos_major = major_angle.cos();
            let sin_major = major_angle.sin();
            let cos_next_major = next_major_angle.cos();
            let sin_next_major = next_major_angle.sin();

            for j in 0..minor_segments {
                let minor_angle = (j as f32) * 2.0 * PI / minor_segments as f32;
                let next_minor_angle =
                    ((j + 1) as f32) * 2.0 * PI / minor_segments as f32;

                let cos_minor = minor_angle.cos();
                let sin_minor = minor_angle.sin();
                let cos_next_minor = next_minor_angle.cos();
                let sin_next_minor = next_minor_angle.sin();

                // Calculate the four corners of the quad
                let p1 = [
                    (major_radius + minor_radius * cos_minor) * cos_major,
                    (major_radius + minor_radius * cos_minor) * sin_major,
                    minor_radius * sin_minor,
                ];

                let p2 = [
                    (major_radius + minor_radius * cos_next_minor) * cos_major,
                    (major_radius + minor_radius * cos_next_minor) * sin_major,
                    minor_radius * sin_next_minor,
                ];

                let p3 = [
                    (major_radius + minor_radius * cos_next_minor)
                        * cos_next_major,
                    (major_radius + minor_radius * cos_next_minor)
                        * sin_next_major,
                    minor_radius * sin_next_minor,
                ];

                let p4 = [
                    (major_radius + minor_radius * cos_minor) * cos_next_major,
                    (major_radius + minor_radius * cos_minor) * sin_next_major,
                    minor_radius * sin_minor,
                ];

                // Create two triangles for the quad
                add_triangle(&mut vertices, p1, p2, p3);
                add_triangle(&mut vertices, p3, p4, p1); // ! Change to add_quad, but now right now, bc I dont want to
            }
        }

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a cylinder
    pub fn create_cylinder(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        radius: f32,
        height: f32,
        sectors: u32,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let half_height = height / 2.0;

        // Create vertices around the circle
        let mut circle_points = Vec::new();
        for i in 0..sectors {
            let angle = (i as f32) * 2.0 * PI / sectors as f32;
            circle_points.push([radius * angle.cos(), radius * angle.sin()]);
        }

        // Side faces (quads) - normals point radially outward
        for i in 0..sectors {
            let current_idx = i as usize;
            let next_idx = ((i + 1) % sectors) as usize;

            let p1 = [
                circle_points[current_idx][0],
                circle_points[current_idx][1],
                -half_height,
            ];
            let p2 = [
                circle_points[next_idx][0],
                circle_points[next_idx][1],
                -half_height,
            ];
            let p3 = [
                circle_points[next_idx][0],
                circle_points[next_idx][1],
                half_height,
            ];
            let p4 = [
                circle_points[current_idx][0],
                circle_points[current_idx][1],
                half_height,
            ];

            // For sides, we need radial normals
            let normal = normalize([
                circle_points[current_idx][0],
                circle_points[current_idx][1],
                0.0,
            ]);

            // Add triangles with proper normals
            vertices.push(VertexPosColorUv {
                position: p1,
                normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p2,
                normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p3,
                normal,
                uv: [1.0, 1.0],
            });

            vertices.push(VertexPosColorUv {
                position: p3,
                normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p4,
                normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p1,
                normal,
                uv: [1.0, 1.0],
            });
        }

        // Top cap (triangles from center) - normal points up (+Y)
        let top_normal = [0.0, 0.0, 1.0];
        for i in 0..sectors {
            let current_idx = i as usize;
            let next_i = ((i + 1) % sectors) as usize;
            let p1 = [0.0, 0.0, half_height];
            let p2 = [
                circle_points[current_idx][0],
                circle_points[current_idx][1],
                half_height,
            ];
            let p3 = [
                circle_points[next_i][0],
                circle_points[next_i][1],
                half_height,
            ];

            vertices.push(VertexPosColorUv {
                position: p1,
                normal: top_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p2,
                normal: top_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p3,
                normal: top_normal,
                uv: [1.0, 1.0],
            });
        }

        // Bottom cap (triangles from center) - normal points down (-Y)
        let bottom_normal = [0.0, 0.0, -1.0];
        for i in 0..sectors {
            let current_idx = i as usize;
            let next_i = ((i + 1) % sectors) as usize;

            let p1 = [0.0, 0.0, -half_height];
            let p2 = [
                circle_points[current_idx][0],
                circle_points[current_idx][1],
                -half_height,
            ];
            let p3 = [
                circle_points[next_i][0],
                circle_points[next_i][1],
                -half_height,
            ];

            vertices.push(VertexPosColorUv {
                position: p1,
                normal: bottom_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p2,
                normal: bottom_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p3,
                normal: bottom_normal,
                uv: [1.0, 1.0],
            });
        }

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a cone
    pub fn create_cone(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        radius: f32,
        height: f32,
        sectors: u32,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let half_height = height / 2.0;

        // Create vertices around the circle
        let mut circle_points = Vec::new();
        for i in 0..sectors {
            let angle = (i as f32) * 2.0 * PI / sectors as f32;
            circle_points.push([radius * angle.cos(), radius * angle.sin()]);
        }

        // Tip of cone
        let tip = [0.0, 0.0, half_height];

        // Side faces (triangles) - normals are perpendicular to the cone surface
        for i in 0..sectors {
            let current_idx = i as usize;
            let next_i = ((i + 1) % sectors) as usize;
            let p1 = tip;
            let p2 = [
                circle_points[current_idx][0],
                circle_points[current_idx][1],
                -half_height,
            ];
            let p3 = [
                circle_points[next_i][0],
                circle_points[next_i][1],
                -half_height,
            ];

            // Calculate normal for side face
            let side_normal = calculate_normal(p1, p2, p3);

            vertices.push(VertexPosColorUv {
                position: p1,
                normal: side_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p2,
                normal: side_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p3,
                normal: side_normal,
                uv: [1.0, 1.0],
            });
        }

        // Bottom cap (triangles from center) - normal points down (-Y)
        let bottom_normal = [0.0, 0.0, -1.0];
        for i in 0..sectors {
            let current_idx = i as usize;
            let next_i = ((i + 1) % sectors) as usize;
            let p1 = [0.0, 0.0, -half_height];
            let p2 = [
                circle_points[current_idx][0],
                circle_points[current_idx][1],
                -half_height,
            ];
            let p3 = [
                circle_points[next_i][0],
                circle_points[next_i][1],
                -half_height,
            ];

            vertices.push(VertexPosColorUv {
                position: p1,
                normal: bottom_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p2,
                normal: bottom_normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: p3,
                normal: bottom_normal,
                uv: [1.0, 1.0],
            });
        }

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a flat plane
    pub fn create_plane(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        width: f32,
        height: f32,
    ) -> Mesh {
        let w = width / 2.0;
        let h = height / 2.0;

        let v0 = [-w, -h, 0.0];
        let v1 = [w, -h, 0.0];
        let v2 = [w, h, 0.0];
        let v3 = [-w, h, 0.0];

        // Plane normal points up (+Z)
        let normal = [0.0, 0.0, 1.0];

        let vertices = vec![
            VertexPosColorUv {
                position: v0,
                normal,
                uv: [1.0, 1.0],
            },
            VertexPosColorUv {
                position: v1,
                normal,
                uv: [1.0, 1.0],
            },
            VertexPosColorUv {
                position: v2,
                normal,
                uv: [1.0, 1.0],
            },
            VertexPosColorUv {
                position: v2,
                normal,
                uv: [1.0, 1.0],
            },
            VertexPosColorUv {
                position: v3,
                normal,
                uv: [1.0, 1.0],
            },
            VertexPosColorUv {
                position: v0,
                normal,
                uv: [1.0, 1.0],
            },
        ];

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a grid of lines (useful for debugging/visualization)
    pub fn create_grid(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        size: f32,
        divisions: u32,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let step = size / divisions as f32;
        let half = size / 2.0;

        // For lines, normals don't matter much, but we'll set them to zero
        let normal = [0.0, 0.0, 0.0];

        // Lines along X axis
        for i in 0..=divisions {
            let x = -half + i as f32 * step;
            vertices.push(VertexPosColorUv {
                position: [x, -half, 0.0],
                normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: [x, half, 0.0],
                normal,
                uv: [1.0, 1.0],
            });
        }

        // Lines along Y axis
        for i in 0..=divisions {
            let y = -half + i as f32 * step;
            vertices.push(VertexPosColorUv {
                position: [-half, y, 0.0],
                normal,
                uv: [1.0, 1.0],
            });
            vertices.push(VertexPosColorUv {
                position: [half, y, 0.0],
                normal,
                uv: [1.0, 1.0],
            });
        }

        Mesh::new(memory_allocator, &vertices, None)
    }

    // * Create a pyramid (square base)
    pub fn create_pyramid(
        memory_allocator: &Arc<StandardMemoryAllocator>,

        size: f32,
        height: f32,
    ) -> Mesh {
        let mut vertices = Vec::new();
        let half = size / 2.0;
        let half_height = height / 2.0;

        // Base(after base) vertices
        let b1 = [-half, -half, -half_height];
        let b2 = [half, -half, -half_height];
        let b3 = [half, half, -half_height];
        let b4 = [-half, half, -half_height];

        // Apex, like a game, haha, ha?
        let apex = [0.0, 0.0, half_height];

        // Base (two triangles) - normal points down (-Z)
        let base_normal = [0.0, 0.0, -1.0];

        vertices.push(VertexPosColorUv {
            position: b1,
            normal: base_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b2,
            normal: base_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b3,
            normal: base_normal,
            uv: [1.0, 1.0],
        });

        vertices.push(VertexPosColorUv {
            position: b3,
            normal: base_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b4,
            normal: base_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b1,
            normal: base_normal,
            uv: [1.0, 1.0],
        });

        // Four sides - calculate normals for each face
        let side1_normal = calculate_normal(apex, b1, b2);
        let side2_normal = calculate_normal(apex, b2, b3);
        let side3_normal = calculate_normal(apex, b3, b4);
        let side4_normal = calculate_normal(apex, b4, b1);

        // Side 1
        vertices.push(VertexPosColorUv {
            position: apex,
            normal: side1_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b1,
            normal: side1_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b2,
            normal: side1_normal,
            uv: [1.0, 1.0],
        });

        // Side 2
        vertices.push(VertexPosColorUv {
            position: apex,
            normal: side2_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b2,
            normal: side2_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b3,
            normal: side2_normal,
            uv: [1.0, 1.0],
        });

        // Side 3
        vertices.push(VertexPosColorUv {
            position: apex,
            normal: side3_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b3,
            normal: side3_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b4,
            normal: side3_normal,
            uv: [1.0, 1.0],
        });

        // Side 4
        vertices.push(VertexPosColorUv {
            position: apex,
            normal: side4_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b4,
            normal: side4_normal,
            uv: [1.0, 1.0],
        });
        vertices.push(VertexPosColorUv {
            position: b1,
            normal: side4_normal,
            uv: [1.0, 1.0],
        });

        Mesh::new(memory_allocator, &vertices, None)
    }
}
