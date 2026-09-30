pub mod art_direction;
pub mod asset_import;
pub mod assets;
pub mod core;
pub mod demo;
// pub mod effects;
pub mod cli;
#[cfg(feature = "editor")]
pub mod editor;
pub mod engine;
pub mod geometry;
pub mod input;
pub mod project;
#[cfg(feature = "window")]
pub mod project_runner;
pub mod reflect;
pub mod rendering;
pub mod runtime;
pub mod scenario;
pub mod scene_patch;
pub mod schema;
#[cfg(test)]
pub mod tests;

pub use assets::{
    spawn_gltf_nodes, spawn_gltf_nodes_in_world, AlphaMode, AssetPlugin,
    AssetServer, DataAsset, Handle, ImportedGltfLight, ImportedGltfNode,
    ImportedGltfPrimitive, MaterialAsset, MaterialModel, MeshAsset,
    PrimitiveShape, SceneAsset, TextureAsset, TextureFilter, TextureSampler,
    TextureWrap,
};
pub use core::collisions::CollisionType;
pub use core::{Material, MaterialBuilder, Physics, Transform};
#[cfg(feature = "editor")]
pub use editor::EditorPlugin;
/// The egui version [`runtime::RuntimeUi`] draws with.
#[cfg(feature = "ui")]
pub use egui;
pub use engine::{Engine, PerspectiveCamera};
pub use geometry::Mesh;
pub use rendering::compute_profile::ComputeShaderType;
pub use runtime::{
    App, EngineBuilder, GpuCondition, GpuEventMode, GpuEventPayload,
    GpuPhysicsClassWatches, GpuPhysicsEvent, GpuPhysicsRule, GpuPhysicsWatch,
    HybridPhysicsPlugin, ObjectClasses, PhysicsId, Plugin, RenderSettings,
};

/// Common imports for concise native Rust gameplay code.
#[cfg(feature = "window")]
pub mod prelude {
    pub use crate::project_runner::{
        CubeSpawn, GameObject, GameResult, GameScene, GameSnapshot,
        GpuBodySettings, InitialState, RayHit, SphereSpawn,
    };
    pub use crate::runtime::{
        FrameTime, GpuCondition, GpuEventMode, GpuEventPayload,
        GpuPhysicsEvent, GpuPhysicsRule, GpuPhysicsWatch, Name, ObjectClasses,
        PhysicsSolver, PhysicsWorld, PlayerController, RigidBody,
        RigidBodyKind,
    };
    pub use crate::rusting_game;
    pub use crate::Transform;
    pub use crate::{AlphaMode, MaterialAsset, MaterialModel};
    pub use bevy_ecs::entity::Entity;
    pub use bevy_ecs::world::World;
}
