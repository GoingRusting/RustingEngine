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
            .expect("Failed to load GridCollision compute shader");
        let cp_test = create_compute_pipeline(device, cs_test, "GridCollision");
        pipelines.insert(ComputeShaderType::GridCollision, cp_test);

        let cs_space = cs_space::load(device.clone())
            .expect("Failed to load Space compute shader");
        let cp_space = create_compute_pipeline(device, cs_space, "Space");
        pipelines.insert(ComputeShaderType::Space, cp_space);

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
    #[allow(deprecated)]
    fn old_test_name_still_selects_the_grid_solver() {
        assert_eq!(ComputeShaderType::Test, ComputeShaderType::GridCollision);
        assert!(matches!(
            ComputeShaderType::GridCollision,
            ComputeShaderType::Test
        ));
    }

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
            simulation: SimulationClass::Gpu,
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
            simulation: SimulationClass::Gpu,
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

    /// Runs one `basic.comp` step with the grid cell listing `order`.
    fn grid_step(
        base: &crate::rendering::HeadlessVulkanBase,
        bodies: &[crate::scene::object::InstanceData],
        order: &[u32],
    ) -> Vec<crate::scene::object::InstanceData> {
        use crate::rendering::readback::read_back_buffer;
        use crate::scene::object::PhysicsPushConstants;
        use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
        use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
        use vulkano::command_buffer::{
            AutoCommandBufferBuilder, CommandBufferUsage,
        };
        use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
        use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
        use vulkano::memory::allocator::{
            AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
        };
        use vulkano::pipeline::{Pipeline, PipelineBindPoint};
        use vulkano::sync::GpuFuture;

        const HASH_SIZE: u32 = 65521;
        const MAX_PER_CELL: u32 = 128;
        const CELL_SIZE: f32 = 10.0;

        let device = &base.device;
        let memory =
            Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let commands = Arc::new(StandardCommandBufferAllocator::new(
            device.clone(),
            Default::default(),
        ));
        let sets = Arc::new(StandardDescriptorSetAllocator::new(
            device.clone(),
            Default::default(),
        ));
        let storage = |data: Vec<u32>| {
            Buffer::from_iter(
                memory.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::STORAGE_BUFFER,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                data,
            )
            .unwrap()
        };
        let instances = |data: Vec<crate::scene::object::InstanceData>| {
            Buffer::from_iter(
                memory.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::STORAGE_BUFFER
                        | BufferUsage::TRANSFER_SRC,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                data,
            )
            .unwrap()
        };

        // Every body sits in cell (0, 0, 0); mirror `hashCell` from the shader.
        let cell = 0_u32.wrapping_mul(2_654_435_761)
            ^ 0_u32.wrapping_mul(2_246_822_519)
            ^ 0_u32.wrapping_mul(3_266_489_917);
        let cell = (cell % HASH_SIZE) as usize;
        let mut counts = vec![0; HASH_SIZE as usize];
        counts[cell] = order.len() as u32;
        let mut objects = vec![0; (HASH_SIZE * MAX_PER_CELL) as usize];
        objects[cell * MAX_PER_CELL as usize..][..order.len()]
            .copy_from_slice(order);

        let read = instances(bodies.to_vec());
        let write = instances(bodies.to_vec());
        let pipeline = create_compute_pipeline(
            device,
            cs_test::load(device.clone()).unwrap(),
            "GridCollision",
        );
        let set = DescriptorSet::new(
            sets,
            pipeline.layout().set_layouts()[0].clone(),
            [
                WriteDescriptorSet::buffer(0, read),
                WriteDescriptorSet::buffer(1, write.clone()),
                WriteDescriptorSet::buffer(2, storage(counts)),
                WriteDescriptorSet::buffer(3, storage(objects)),
                WriteDescriptorSet::buffer(4, storage(vec![0])),
            ],
            [],
        )
        .unwrap();
        let mut builder = AutoCommandBufferBuilder::primary(
            commands.clone(),
            base.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();
        builder
            .bind_pipeline_compute(pipeline.clone())
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                pipeline.layout().clone(),
                0,
                set,
            )
            .unwrap()
            .push_constants(
                pipeline.layout().clone(),
                0,
                PhysicsPushConstants {
                    dt: 1.0 / 60.0,
                    total_objects: bodies.len() as u32,
                    offset: 0,
                    count: bodies.len() as u32,
                    num_big_objects: 0,
                    _pad: [0; 3],
                    global_gravity: [0.0, -9.81, 0.0, CELL_SIZE],
                },
            )
            .unwrap();
        unsafe { builder.dispatch([1, 1, 1]) }.unwrap();
        vulkano::sync::now(device.clone())
            .then_execute(base.queue.clone(), builder.build().unwrap())
            .unwrap()
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        read_back_buffer(device, &base.queue, &memory, &commands, write)
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn grid_contacts_do_not_depend_on_cell_insertion_order() {
        use crate::rendering::test_support::headless_device;
        use crate::scene::object::InstanceData;

        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let base = headless_device();
        let body = |position: [f32; 3], velocity: [f32; 3]| InstanceData {
            model: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [position[0], position[1], position[2], 1.0],
            ],
            color: [1.0; 4],
            mat_props: [0.5, 0.0, 0.0, 0.0],
            velocity: [velocity[0], velocity[1], velocity[2], 0.3],
            angular_velocity: [0.0, 0.0, 0.0, 0.5],
            physic_props: [0.0, 1.0, 1.0, 0.0],
        };
        // Four overlapping boxes: each one touches every other one, so its
        // contacts are solved one after another in cell-list order.
        let bodies = [
            body([2.0, 2.0, 2.0], [1.0, 0.0, 0.0]),
            body([2.6, 2.3, 2.1], [-1.0, 0.5, 0.0]),
            body([2.3, 2.7, 2.4], [0.0, -2.0, 0.3]),
            body([2.5, 2.4, 2.8], [0.2, 0.0, -1.5]),
        ];

        let forward = grid_step(base, &bodies, &[0, 1, 2, 3]);
        let reversed = grid_step(base, &bodies, &[3, 2, 1, 0]);

        let bits = |bodies: &[InstanceData]| {
            bytemuck::cast_slice::<_, u32>(bodies).to_vec()
        };
        assert_ne!(bits(&forward), bits(&bodies), "contacts must move bodies");
        assert_eq!(bits(&forward), bits(&reversed));
    }
}
