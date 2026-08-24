use nalgebra::{Matrix4, Rotation3, Vector3};

#[derive(bevy_ecs::component::Component, Copy, Clone, Debug, PartialEq)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

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
    pub fn new(position: [f32; 3]) -> Self {
        Self {
            position,
            ..Default::default()
        }
    }

    pub fn with_position(mut self, x: f32, y: f32, z: f32) -> Self {
        self.position = [x, y, z];
        self
    }

    pub fn with_rotation(mut self, x: f32, y: f32, z: f32) -> Self {
        self.rotation = [x, y, z];
        self
    }

    pub fn with_scale(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = [x, y, z];
        self
    }

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
}
