pub mod assets;
pub mod core;
pub mod demo;
// pub mod effects;
#[cfg(feature = "editor")]
pub mod editor;
pub mod engine;
pub mod geometry;
pub mod input;
#[cfg(feature = "window")]
pub mod project_runner;
pub mod rendering;
pub mod runtime;
pub mod scene;
pub mod shaders;
#[cfg(test)]
pub mod tests;

pub use assets::{
    AssetPlugin, AssetServer, Handle, ImportedGltfPrimitive, MaterialAsset,
    MaterialModel, MeshAsset, SceneAsset, TextureAsset,
};
pub use core::collisions::CollisionType;
pub use core::{Material, MaterialBuilder, Physics, ShaderType, Transform};
#[cfg(feature = "editor")]
pub use editor::EditorPlugin;
pub use engine::{Engine, PerspectiveCamera};
pub use geometry::Mesh;
pub use rendering::compute_registry::ComputeShaderType;
pub use runtime::{
    App, EngineBuilder, GpuCondition, GpuEventMode, GpuEventPayload,
    GpuPhysicsClassWatches, GpuPhysicsEvent, GpuPhysicsRule, GpuPhysicsWatch,
    HybridPhysicsPlugin, ObjectClasses, PhysicsId, Plugin, RenderSettings,
};

/// Common imports for concise native Rust gameplay code.
#[cfg(feature = "window")]
pub mod prelude {
    pub use crate::project_runner::{
        CubeSpawn, GameObject, GameResult, GameScene, GpuBodySettings,
        SphereSpawn,
    };
    pub use crate::runtime::{
        FrameTime, GpuCondition, GpuEventMode, GpuEventPayload,
        GpuPhysicsEvent, GpuPhysicsRule, GpuPhysicsWatch, ObjectClasses,
        PhysicsSolver,
    };
    pub use crate::rusting_game;
    pub use crate::Transform;
    pub use crate::{MaterialAsset, MaterialModel};
}
