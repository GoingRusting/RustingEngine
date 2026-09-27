use crate::runtime::{PhysicsBody, PhysicsSolver, SimulationClass};

/// Backend route produced from the semantic physics settings authored in the
/// editor. Static bodies intentionally have no compute shader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhysicsExecution {
    Disabled,
    StaticCollider,
    GameplayCpu,
    GpuBuiltIn(ComputeShaderType),
    GpuCustom(String),
    InvalidCustomShader,
}

#[must_use]
pub fn physics_execution(body: &PhysicsBody) -> PhysicsExecution {
    match body.simulation {
        SimulationClass::None => PhysicsExecution::Disabled,
        SimulationClass::Static => PhysicsExecution::StaticCollider,
        SimulationClass::Cpu => PhysicsExecution::GameplayCpu,
        SimulationClass::Gpu => match body.solver {
            PhysicsSolver::Full => {
                PhysicsExecution::GpuBuiltIn(ComputeShaderType::FullPhysics)
            }
            PhysicsSolver::Simplified => {
                PhysicsExecution::GpuBuiltIn(ComputeShaderType::MidPhysic)
            }
            PhysicsSolver::NoCollision => {
                PhysicsExecution::GpuBuiltIn(ComputeShaderType::NoCollision)
            }
            PhysicsSolver::Space => {
                PhysicsExecution::GpuBuiltIn(ComputeShaderType::Space)
            }
            PhysicsSolver::Custom => body.custom_shader.clone().map_or(
                PhysicsExecution::InvalidCustomShader,
                PhysicsExecution::GpuCustom,
            ),
        },
    }
}

/// Which descriptor bindings a compute shader needs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShaderBindings {
    pub needs_read_buffer: bool,  // binding 0
    pub needs_write_buffer: bool, // binding 1
    pub needs_grid_counts: bool,  // binding 2
    pub needs_grid_objects: bool, // binding 3
    pub needs_big_indices: bool,  // binding 4
}

impl ShaderBindings {
    pub fn basic() -> Self {
        Self {
            needs_read_buffer: true,
            needs_write_buffer: true,
            needs_grid_counts: false,
            needs_grid_objects: false,
            needs_big_indices: false,
        }
    }

    pub fn grid_build() -> Self {
        Self {
            needs_read_buffer: true,
            needs_write_buffer: true,
            needs_grid_counts: true,
            needs_grid_objects: true,
            needs_big_indices: true,
        }
    }
}

/// Compute shader variant that determines how physics and transform logic is applied.
/// `FullPhysics` is the most powerful one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ComputeShaderType {
    /// Full physics and collision calculations (heavy)
    #[default]
    FullPhysics,
    /// Still very good for performance ( Has collisions, mass and push objects when they collide, but no rotation on push )
    MidPhysic,
    /// Fast copy without physics logic (for static or purely kinematic objects)
    Static,
    /// Fast physics logic (applies velocity and gravity) but skips object collisions check
    NoCollision,
    /// Yes
    GridBuild,
    /// Space shader: gravity is a point that pulls bodies toward it, not a
    /// direction, so `[0, 0, 0]` pulls toward the origin instead of meaning
    /// "no gravity".
    Space,
    /// useless shader that has no effect on anything. You can use it to test for fast render without compute shader or I dont know
    Empty,
    /// culling shaders for insane optimization
    Cull,
    /// Full physics with body-to-body collisions found through a spatial
    /// hash grid (`basic.comp`); needs a `GridBuild` pass first. Formerly
    /// `Test`.
    GridCollision,
}

#[allow(non_upper_case_globals)]
impl ComputeShaderType {
    #[deprecated(note = "renamed to `ComputeShaderType::GridCollision`")]
    pub const Test: Self = Self::GridCollision;
}

impl ComputeShaderType {
    pub fn sort_key(&self) -> u32 {
        match self {
            ComputeShaderType::FullPhysics => 0,
            ComputeShaderType::MidPhysic => 1,
            ComputeShaderType::Static => 2,
            ComputeShaderType::NoCollision => 3,
            ComputeShaderType::GridBuild => 4,
            ComputeShaderType::Empty => 5,
            ComputeShaderType::Cull => 6,
            ComputeShaderType::GridCollision => 7,
            ComputeShaderType::Space => 8,
        }
    }

    pub fn needs_bindings(&self) -> ShaderBindings {
        match self {
            ComputeShaderType::GridBuild => ShaderBindings::grid_build(),
            ComputeShaderType::GridCollision => ShaderBindings::grid_build(),
            _ => ShaderBindings::basic(),
        }
    }
}
