use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use serde::{Deserialize, Serialize};
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

/// Reusable classes assigned to one scene object.
///
/// Names identify one object. Classes select any number of objects, and one
/// object can belong to several classes at the same time.
#[derive(
    Component, Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize,
)]
pub struct ObjectClasses {
    /// Class names such as `gravity`, `enemy`, or `falling_cubes`.
    pub names: Vec<String>,
}

impl ObjectClasses {
    /// Creates a clean class list without empty or repeated names.
    #[must_use]
    pub fn new(classes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let mut result = Self::default();
        for class in classes {
            result.add(class);
        }
        result
    }

    /// Adds a class if this object does not already have it.
    pub fn add(&mut self, class: impl Into<String>) -> bool {
        let class = class.into();
        let class = class.trim();
        if class.is_empty() || self.contains(class) {
            return false;
        }
        self.names.push(class.to_owned());
        true
    }

    /// Removes a class from this object.
    pub fn remove(&mut self, class: &str) -> bool {
        let old_len = self.names.len();
        self.names.retain(|current| current != class);
        self.names.len() != old_len
    }

    /// Returns true when this object belongs to the requested class.
    #[must_use]
    pub fn contains(&self, class: &str) -> bool {
        self.names.iter().any(|current| current == class)
    }
}

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
    /// RGBA color used to clear the game render target before drawing.
    pub background_color: [f32; 4],
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            quality: QualityProfile::Auto,
            vsync: false,
            limit_fps: false,
            max_fps: 120,
            render_scale: 1.0,
            background_color: [0.025, 0.04, 0.07, 1.0],
        }
    }
}

#[derive(bevy_ecs::prelude::Resource, Clone, Debug, PartialEq)]
pub struct PhysicsSettings {
    pub gravity: [f32; 3],
    pub enabled: bool,
}

/// Reports which physics backends are connected to the ECS scene runner.
/// The compatibility `Engine` has its own legacy GPU path and does not use
/// this resource.
#[derive(
    bevy_ecs::prelude::Resource, Clone, Copy, Debug, Default, PartialEq, Eq,
)]
pub struct PhysicsBackendStatus {
    pub gameplay_available: bool,
    pub gpu_dynamic_available: bool,
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

/// Selects which simulation backend owns an entity.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize,
)]
pub enum SimulationClass {
    None,
    Static,
    #[default]
    Gameplay,
    GpuDynamic,
}

/// Built-in GPU compute profile, or a project-provided compute shader.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize,
)]
pub enum PhysicsSolver {
    #[default]
    Full,
    Simplified,
    NoCollision,
    Space,
    Custom,
}

/// Semantic physics configuration authored by the editor.
///
/// Static and disabled bodies are deliberately excluded from dynamic compute
/// dispatches. Custom shader paths are project-relative and validated by the
/// editor before they are saved.
#[derive(Component, Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PhysicsBody {
    pub simulation: SimulationClass,
    pub solver: PhysicsSolver,
    pub custom_shader: Option<String>,
}

impl Default for PhysicsBody {
    fn default() -> Self {
        Self {
            simulation: SimulationClass::Gameplay,
            solver: PhysicsSolver::Full,
            custom_shader: None,
        }
    }
}

impl PhysicsBody {
    #[must_use]
    pub fn participates_in_dynamic_simulation(&self) -> bool {
        matches!(
            self.simulation,
            SimulationClass::Gameplay | SimulationClass::GpuDynamic
        )
    }

    #[must_use]
    pub fn uses_gpu(&self) -> bool {
        self.simulation == SimulationClass::GpuDynamic
    }
}

#[derive(
    Component,
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    PartialEq,
    Eq,
    Serialize,
)]
pub enum RigidBodyKind {
    Fixed,
    #[default]
    Dynamic,
    Kinematic,
}

#[derive(Component, Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct RigidBody {
    pub kind: RigidBodyKind,
    pub mass: f32,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub gravity_scale: f32,
}

impl Default for RigidBody {
    fn default() -> Self {
        Self {
            kind: RigidBodyKind::Dynamic,
            mass: 1.0,
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            gravity_scale: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub enum ColliderShape {
    Box { half_extents: [f32; 3] },
    Sphere { radius: f32 },
    Capsule { half_height: f32, radius: f32 },
}

#[derive(Component, Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
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

#[derive(
    Component, Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize,
)]
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
