use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use uuid::Uuid;

use crate::assets::{Handle, MaterialAsset, MeshAsset};

/// Persistent scene identity. Unlike a Bevy [`Entity`], this survives saving,
/// loading, and a new process.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneId(pub Uuid);

impl SceneId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SceneId {
    fn default() -> Self {
        Self::new()
    }
}

/// World-space transform derived from [`crate::Transform`] and hierarchy.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform {
    pub matrix: [[f32; 4]; 4],
}

impl Default for GlobalTransform {
    fn default() -> Self {
        Self {
            matrix: nalgebra::Matrix4::<f32>::identity().into(),
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent(pub Entity);

#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Children(pub Vec<Entity>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
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

impl Default for Projection {
    fn default() -> Self {
        Self::Perspective {
            vertical_fov_radians: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far: 1_000.0,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    pub projection: Projection,
    pub active: bool,
    pub priority: i32,
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Name(pub String);

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    pub color: [f32; 3],
    pub illuminance: f32,
    pub shadows: bool,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            color: [1.0; 3],
            illuminance: 100_000.0,
            shadows: true,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            color: [1.0; 3],
            intensity: 1_000.0,
            range: 10.0,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct AmbientLight {
    pub color: [f32; 3],
    pub intensity: f32,
}

impl Default for AmbientLight {
    fn default() -> Self {
        Self {
            color: [1.0; 3],
            intensity: 0.1,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visibility {
    pub visible: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// Semantic rendering component. GPU batches are derived during extraction.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshRenderer {
    pub mesh: Handle<MeshAsset>,
    pub material: Handle<MaterialAsset>,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QualityProfile {
    Auto,
    Eco,
    #[default]
    Balanced,
    High,
}

#[derive(bevy_ecs::prelude::Resource, Clone, Debug, PartialEq)]
pub struct RenderSettings {
    pub quality: QualityProfile,
    pub vsync: bool,
    pub limit_fps: bool,
    pub max_fps: u32,
    pub render_scale: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            quality: QualityProfile::Auto,
            vsync: false,
            limit_fps: false,
            max_fps: 120,
            render_scale: 1.0,
        }
    }
}

#[derive(bevy_ecs::prelude::Resource, Clone, Debug, PartialEq)]
pub struct PhysicsSettings {
    pub gravity: [f32; 3],
    pub enabled: bool,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81, 0.0],
            enabled: true,
        }
    }
}

/// Marks simulation that can never synchronously drive gameplay state.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuEffectBody;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RigidBodyKind {
    Fixed,
    #[default]
    Dynamic,
    Kinematic,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct RigidBody {
    pub kind: RigidBodyKind,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub gravity_scale: f32,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            kind: RigidBodyKind::Dynamic,
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            gravity_scale: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderShape {
    Box { half_extents: [f32; 3] },
    Sphere { radius: f32 },
    Capsule { half_height: f32, radius: f32 },
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Collider {
    pub shape: ColliderShape,
    pub friction: f32,
    pub restitution: f32,
    pub sensor: bool,
}

impl Default for Collider {
    fn default() -> Self {
        Self {
            shape: ColliderShape::Box {
                half_extents: [0.5; 3],
            },
            friction: 0.5,
            restitution: 0.0,
            sensor: false,
        }
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollisionLayers {
    pub memberships: u32,
    pub filters: u32,
}

impl Default for CollisionLayers {
    fn default() -> Self {
        Self {
            memberships: u32::MAX,
            filters: u32::MAX,
        }
    }
}
