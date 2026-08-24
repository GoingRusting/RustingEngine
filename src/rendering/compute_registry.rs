use std::collections::HashMap;
use std::sync::Arc;

use vulkano::device::Device;
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::layout::{
    PipelineDescriptorSetLayoutCreateInfo, PipelineLayout,
};
use vulkano::pipeline::{ComputePipeline, PipelineShaderStageCreateInfo};
use vulkano::shader::ShaderModule;

use crate::runtime::{PhysicsBody, PhysicsSolver, SimulationClass};
use crate::shaders::compute::*;

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
        SimulationClass::Gameplay => PhysicsExecution::GameplayCpu,
        SimulationClass::GpuDynamic => match body.solver {
            PhysicsSolver::Full => {
                PhysicsExecution::GpuBuiltIn(ComputeShaderType::FullPhysics)
            }
            PhysicsSolver::Simplified => {
                PhysicsExecution::GpuBuiltIn(ComputeShaderType::MidPhysic)
            }
            PhysicsSolver::NoCollision => {
                PhysicsExecution::GpuBuiltIn(ComputeShaderType::NoCollision)
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
    /// useless shader that has no effect on anything. You can use it to test for fast render without compute shader or I dont know
    Empty,
    /// culling shaders for insane optimization
    Cull,
    /// This is testing not stable shaders
    Test,
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
            ComputeShaderType::Test => 7,
        }
    }

    pub fn needs_bindings(&self) -> ShaderBindings {
        match self {
            ComputeShaderType::GridBuild => ShaderBindings::grid_build(),
            ComputeShaderType::Test => ShaderBindings::grid_build(),
            _ => ShaderBindings::basic(),
        }
    }
}

pub struct ComputeShaderRegistry {
    pipelines: HashMap<ComputeShaderType, Arc<ComputePipeline>>,
    scene_shader: Option<ComputeShaderType>,
}

impl ComputeShaderRegistry {
    pub fn new(device: &Arc<Device>) -> Self {
        let mut pipelines = HashMap::new();

        let cs_full = cs_full::load(device.clone())
            .expect("Failed to load FullPhysics compute shader");
        let cp_full = create_compute_pipeline(device, cs_full, "FullPhysics");
        pipelines.insert(ComputeShaderType::FullPhysics, cp_full);

        let cs_mid = cs_no_rot::load(device.clone())
            .expect("Failed to load MidPhysics compute shader");
        let cp_mid = create_compute_pipeline(device, cs_mid, "MidPhysics");
        pipelines.insert(ComputeShaderType::MidPhysic, cp_mid);

        let cs_static = cs_empty::load(device.clone())
            .expect("Failed to load Static compute shader");
        let cp_static = create_compute_pipeline(device, cs_static, "Static");
        pipelines.insert(ComputeShaderType::Static, cp_static);

        let cs_no_col = cs_no_coll::load(device.clone())
            .expect("Failed to load NoCollision compute shader");
        let cp_no_col =
            create_compute_pipeline(device, cs_no_col, "NoCollision");
        pipelines.insert(ComputeShaderType::NoCollision, cp_no_col);

        let cs_grid = cs_grid_build::load(device.clone())
            .expect("Failed to load GridBuild compute shader");
        let cp_grid = create_compute_pipeline(device, cs_grid, "GridBuild");
        pipelines.insert(ComputeShaderType::GridBuild, cp_grid);

        let cs_empty = cs_empty::load(device.clone())
            .expect("Failed to load Empty compute shader");
        let cp_empty = create_compute_pipeline(device, cs_empty, "Empty");
        pipelines.insert(ComputeShaderType::Empty, cp_empty);

        let cs_cull = cs_cull::load(device.clone())
            .expect("Failed to load Cull compute shader");
        let cp_cull = create_compute_pipeline(device, cs_cull, "Cull");
        pipelines.insert(ComputeShaderType::Cull, cp_cull);

        let cs_test = cs_test::load(device.clone())
            .expect("Failed to load Test compute shader");
        let cp_test = create_compute_pipeline(device, cs_test, "Test");
        pipelines.insert(ComputeShaderType::Test, cp_test);

        Self {
            pipelines,
            scene_shader: None,
        }
    }

    pub fn get_pipeline(
        &self,
        shader_type: ComputeShaderType,
    ) -> &Arc<ComputePipeline> {
        self.pipelines
            .get(&shader_type)
            .expect("Compute pipeline not found")
    }

    /// Set a scene-wide shader override. All objects will use this shader,
    /// ignoring their per-object shader setting.
    pub fn set_scene_shader(&mut self, shader: ComputeShaderType) {
        self.scene_shader = Some(shader);
    }

    pub fn get_default_shader(&self) -> ComputeShaderType {
        ComputeShaderType::default()
    }

    /// Clear the scene-wide shader override. Objects will use their per-object shader.
    pub fn clear_scene_shader(&mut self) {
        self.scene_shader = None;
    }

    /// Return the scene physic shader or default
    pub fn scene_shader(&self) -> ComputeShaderType {
        self.scene_shader
            .unwrap_or_else(|| self.get_default_shader())
    }

    /// Return the scene physic shader or None
    pub fn scene_shader_optional(&self) -> Option<ComputeShaderType> {
        self.scene_shader
    }
}

fn create_compute_pipeline(
    device: &Arc<Device>,
    shader: Arc<ShaderModule>,
    name: &str,
) -> Arc<ComputePipeline> {
    let stage =
        PipelineShaderStageCreateInfo::new(shader.entry_point("main").unwrap());
    let layout = PipelineLayout::new(
        device.clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages([&stage])
            .into_pipeline_layout_create_info(device.clone())
            .unwrap(),
    )
    .unwrap();

    ComputePipeline::new(
        device.clone(),
        None,
        ComputePipelineCreateInfo::stage_layout(stage, layout),
    )
    .unwrap_or_else(|error| {
        panic!("Failed to create {name} compute pipeline: {error}")
    })
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CullPushConstants {
    pub view_proj: [[f32; 4]; 4],
    pub batch_offset: u32, // Index of first instance in the physics buffer
    pub batch_count: u32,  // How many instances in this batch
    pub visible_list_offset: u32, // Where to start writing in the VisibleIndices buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_bodies_skip_compute_dispatch() {
        let body = PhysicsBody {
            simulation: SimulationClass::Static,
            ..PhysicsBody::default()
        };
        assert_eq!(physics_execution(&body), PhysicsExecution::StaticCollider);
    }

    #[test]
    fn gpu_profiles_route_to_the_selected_shader() {
        let body = PhysicsBody {
            simulation: SimulationClass::GpuDynamic,
            solver: PhysicsSolver::Simplified,
            custom_shader: None,
        };
        assert_eq!(
            physics_execution(&body),
            PhysicsExecution::GpuBuiltIn(ComputeShaderType::MidPhysic)
        );
    }

    #[test]
    fn custom_gpu_profile_keeps_its_project_shader_path() {
        let body = PhysicsBody {
            simulation: SimulationClass::GpuDynamic,
            solver: PhysicsSolver::Custom,
            custom_shader: Some("src/shaders/compute/crowd.comp".into()),
        };
        assert_eq!(
            physics_execution(&body),
            PhysicsExecution::GpuCustom(
                "src/shaders/compute/crowd.comp".into()
            )
        );
    }
}
