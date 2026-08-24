pub mod assets;
pub mod core;
// pub mod effects;
#[cfg(feature = "editor")]
pub mod editor;
pub mod engine;
pub mod geometry;
pub mod input;
pub mod rendering;
pub mod runtime;
pub mod scene;
pub mod shaders;
#[cfg(test)]
pub mod tests;

pub use assets::{
    AssetPlugin, AssetServer, Handle, MaterialAsset, MeshAsset, SceneAsset,
    TextureAsset,
};
pub use core::collisions::CollisionType;
pub use core::{Material, MaterialBuilder, Physics, ShaderType, Transform};
#[cfg(feature = "editor")]
pub use editor::EditorPlugin;
pub use engine::{Engine, PerspectiveCamera};
pub use geometry::Mesh;
pub use rendering::compute_registry::ComputeShaderType;
pub use runtime::{App, EngineBuilder, Plugin};
