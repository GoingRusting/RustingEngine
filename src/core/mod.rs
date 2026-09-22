// `collisions` and `transform` moved into the `rusting-core` crate (no
// Vulkan/internal-crate dependencies); re-exported here at their original
// path so no existing `crate::core::...` caller needs to change.
pub use rusting_core::collisions;
pub use rusting_core::transform;

pub mod material;
pub mod physics;

pub use crate::rendering::compute_registry::ComputeShaderType;
pub use crate::rendering::shader_registry::ShaderType;
pub use material::{Material, MaterialBuilder};
pub use physics::Physics;
pub use transform::Transform;
