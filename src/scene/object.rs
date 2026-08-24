use crate::rendering::compute_registry::ComputeShaderType;
use crate::rendering::shader_registry::ShaderType;
use crate::scene::animation::AnimationType;
use crate::{geometry::Mesh, Material, Physics};
use nalgebra::{Matrix4, Rotation3, Vector3};
use vulkano::buffer::Subbuffer;
use vulkano::command_buffer::DrawIndexedIndirectCommand;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceData {
    pub model: [[f32; 4]; 4], // 64b. [3].xyz = position, [0..2] = rotate/scale
    pub color: [f32; 4],      // 16b. rgb + emissive
    pub mat_props: [f32; 4],  // 16b. x=roughness, y=metalness, z,w=custom
    pub velocity: [f32; 4], // 16b. xyz = linear speed, w = (Bounciness/Restitution)
    pub angular_velocity: [f32; 4], // 16b. xyz = angle speed, w = Friction
    pub physic_props: [f32; 4], // 16b. x = shape(0=Box,1=Sphere), y = mass, z = gravity_scale, w = grid_hack
} // Total: 144 bytes, if you want you want to copy my library and rewrite it, remember always to keep `total mod 16 = 0` or GPU will work bad

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PhysicsPushConstants {
    pub dt: f32,
    pub total_objects: u32,
    pub offset: u32,
    pub count: u32,
    pub num_big_objects: u32,
    pub _pad: [u32; 3], // padding to reach offset 32 (vec4 alignment)
    pub global_gravity: [f32; 4], // xyz = global gravity vector, w = (CELL_SIZE) [-9.81, 0.0, 0.0, ]
}

#[derive(Clone)]
pub struct Instance {
    pub color: [f32; 3],
    pub emissive: f32,
    pub roughness: f32,
    pub metalness: f32,
    pub base_color_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub animation: AnimationType,
    pub model_matrix: [[f32; 4]; 4],
    pub physics: Physics,
    pub shader: ShaderType,
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            animation: AnimationType::Static,
            model_matrix: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            color: [1.0, 1.0, 1.0],
            emissive: 0.0,
            base_color_texture: None,
            metallic_roughness_texture: None,
            roughness: 0.5,
            metalness: 0.5,
            physics: Physics::default(),
            shader: ShaderType::Pbr,
        }
    }
}

impl Instance {
    /// Copies every render-facing property from a material.
    pub fn apply_material(&mut self, material: &Material) {
        self.color = material.color;
        self.emissive = material.emissive;
        self.roughness = material.roughness;
        self.metalness = material.metalness;
        self.shader = material.shader;
        self.base_color_texture = material.base_color_texture;
        self.metallic_roughness_texture = material.metallic_roughness_texture;
    }
}

const _: () = {
    use std::mem::{align_of, offset_of, size_of};

    assert!(size_of::<InstanceData>() == 144);
    assert!(align_of::<InstanceData>() == 4);
    assert!(offset_of!(InstanceData, model) == 0);
    assert!(offset_of!(InstanceData, color) == 64);
    assert!(offset_of!(InstanceData, mat_props) == 80);
    assert!(offset_of!(InstanceData, velocity) == 96);
    assert!(offset_of!(InstanceData, angular_velocity) == 112);
    assert!(offset_of!(InstanceData, physic_props) == 128);

    assert!(size_of::<PhysicsPushConstants>() == 48);
    assert!(offset_of!(PhysicsPushConstants, dt) == 0);
    assert!(offset_of!(PhysicsPushConstants, total_objects) == 4);
    assert!(offset_of!(PhysicsPushConstants, offset) == 8);
    assert!(offset_of!(PhysicsPushConstants, count) == 12);
    assert!(offset_of!(PhysicsPushConstants, num_big_objects) == 16);
    assert!(offset_of!(PhysicsPushConstants, global_gravity) == 32);
};

pub struct Texture {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

// ! TRANSFORM - Position, rotation, and scale of an object
#[derive(Copy, Clone, Debug)]
pub struct Transform {
    pub position: [f32; 3], // Translation in world space
    pub rotation: [f32; 3], // Euler angles, but I like Quaternions, but its complex as hell to implement it everywhere, but I sure that I will do this soon(maybe later then soon)
    pub scale: [f32; 3],    // Scale factors
}

// * It can be used to create a new objects or set default settings, like in blender 'reset transform' or something like that
impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
        }
    }
}

impl Transform {
    pub fn to_matrix(&self) -> [[f32; 4]; 4] {
        let translation =
            Matrix4::new_translation(&Vector3::from(self.position));

        let rotation = Rotation3::from_euler_angles(
            self.rotation[0],
            self.rotation[1],
            self.rotation[2],
        )
        .to_homogeneous();

        let scale = Matrix4::new_nonuniform_scaling(&Vector3::from(self.scale));

        let model_matrix = translation * rotation * scale;

        model_matrix.into()
    }
    pub fn from_matrix(m: Matrix4<f32>) -> Self {
        let position = [m[(0, 3)], m[(1, 3)], m[(2, 3)]];

        let scale = [
            Vector3::new(m[(0, 0)], m[(1, 0)], m[(2, 0)]).norm(),
            Vector3::new(m[(0, 1)], m[(1, 1)], m[(2, 1)]).norm(),
            Vector3::new(m[(0, 2)], m[(1, 2)], m[(2, 2)]).norm(),
        ];

        let rotation_matrix = nalgebra::Matrix3::new(
            m[(0, 0)] / scale[0],
            m[(0, 1)] / scale[1],
            m[(0, 2)] / scale[2],
            m[(1, 0)] / scale[0],
            m[(1, 1)] / scale[1],
            m[(1, 2)] / scale[2],
            m[(2, 0)] / scale[0],
            m[(2, 1)] / scale[1],
            m[(2, 2)] / scale[2],
        );

        let rotation =
            Rotation3::from_matrix_unchecked(rotation_matrix).euler_angles();

        Self {
            position,
            rotation: [rotation.0, rotation.1, rotation.2],
            scale,
        }
    }
    // * Helper methods for common transformations, but I use dirrect change, bc I did like that all time in THREE.js, but this functions can be pretty useful for someone
    // translate
    pub fn translate(mut self, x: f32, y: f32, z: f32) -> Self {
        self.position = [x, y, z];
        self
    }

    // rotate
    pub fn rotate(mut self, x: f32, y: f32, z: f32) -> Self {
        self.rotation = [x, y, z];
        self
    }

    // scale
    pub fn scale(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = [x, y, z];
        self
    }

    // uniform_scale
    pub fn uniform_scale(mut self, s: f32) -> Self {
        self.scale = [s, s, s];
        self
    }

    // I am going crazy
}

// Multiply two transforms (apply rhs then lhs)
impl std::ops::Mul for Transform {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        // Convert both to matrices, multiply, then convert back
        let lhs_matrix = nalgebra::Matrix4::from(self.to_matrix());
        let rhs_matrix = nalgebra::Matrix4::from(rhs.to_matrix());
        let result_matrix = lhs_matrix * rhs_matrix;
        Transform::from_matrix(result_matrix)
    }
}

pub struct RenderBatch {
    pub mesh: Mesh,
    /// Stable IDs corresponding one-to-one with `instances`.
    pub instance_ids: Vec<u64>,
    pub base_color_texture: Option<usize>,
    pub metallic_roughness_texture: Option<usize>,
    pub instances: Vec<Instance>,
    pub shader: ShaderType,
    pub compute_shader: ComputeShaderType,
    pub indirect_buffer: Subbuffer<[DrawIndexedIndirectCommand]>,
    pub base_instance_offset: u32,
}
