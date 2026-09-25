pub use rusting_core::components::{
    AmbientLight, Camera, Children, Collider, ColliderShape, CollisionLayers,
    CullingMode, DirectionalLight, GlobalTransform, GpuEffectBody, Name,
    ObjectClasses, Parent, PhysicsBackendStatus, PhysicsBody, PhysicsSettings,
    PhysicsSolver, PointLight, Projection, QualityProfile, RenderBounds,
    RenderSettings, RigidBody, RigidBodyKind, SceneId, SimulationClass,
    SkyLight, SpotLight, ToneMapper, ToneMapping, Visibility,
};

use bevy_ecs::component::Component;

use crate::assets::{Handle, MaterialAsset, MeshAsset};

/// Semantic rendering component. GPU batches are derived during extraction.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshRenderer {
    pub mesh: Handle<MeshAsset>,
    pub material: Handle<MaterialAsset>,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
}
