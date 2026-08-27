//! Forward scene renderer fed exclusively by the extracted render world.
//!
//! The renderer does not read gameplay ECS state. This boundary lets the same
//! prepared assets and draw path target a swapchain today and an offscreen
//! editor viewport in a later pass without changing scene ownership.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::time::Duration;

use nalgebra::{Matrix4, Orthographic3, Perspective3};
use vulkano::buffer::{
    Buffer, BufferContents, BufferCreateInfo, BufferUsage, Subbuffer,
};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, RenderPassBeginInfo,
    SubpassBeginInfo, SubpassContents,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::Queue;
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::graphics::color_blend::{
    ColorBlendAttachmentState, ColorBlendState,
};
use vulkano::pipeline::graphics::depth_stencil::{
    DepthState, DepthStencilState,
};
use vulkano::pipeline::graphics::input_assembly::{
    InputAssemblyState, PrimitiveTopology,
};
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::{
    CullMode, FrontFace, RasterizationState,
};
use vulkano::pipeline::graphics::subpass::PipelineSubpassType;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::pipeline::graphics::viewport::{Viewport, ViewportState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::layout::{
    PipelineDescriptorSetLayoutCreateInfo, PipelineLayout,
};
use vulkano::pipeline::{
    ComputePipeline, DynamicState, GraphicsPipeline, Pipeline,
    PipelineBindPoint, PipelineShaderStageCreateInfo,
};
use vulkano::render_pass::{
    Framebuffer, FramebufferCreateInfo, RenderPass, Subpass,
};
use vulkano::sync::future::FenceSignalFuture;
use vulkano::sync::GpuFuture;

use crate::assets::{AssetServer, Handle, MaterialAsset, MeshAsset};
use crate::rendering::debug_overlay::{DebugLine, RenderDebugOverlay};
use crate::runtime::{
    GpuConditionInstruction, Projection, RawGpuPhysicsEvent, RenderWorld,
};

#[derive(Debug)]
pub struct SceneRenderError(String);

impl Display for SceneRenderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for SceneRenderError {}

#[repr(C)]
#[derive(BufferContents, Vertex, Clone, Copy)]
struct SceneVertex {
    #[format(R32G32B32_SFLOAT)]
    position: [f32; 3],
    #[format(R32G32B32_SFLOAT)]
    normal: [f32; 3],
}

/// Vertex used by the editor-only debug pass.
#[repr(C)]
#[derive(BufferContents, Vertex, Clone, Copy)]
struct DebugVertex {
    #[format(R32G32B32_SFLOAT)]
    start: [f32; 3],
    #[format(R32G32B32_SFLOAT)]
    end: [f32; 3],
    #[format(R32G32B32A32_SFLOAT)]
    color: [f32; 4],
    #[format(R32G32_SFLOAT)]
    corner: [f32; 2],
    #[format(R32_SFLOAT)]
    thickness: f32,
}

/// Per-object data read with `gl_InstanceIndex` by the graphics shader.
#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default, PartialEq)]
struct RenderInstanceUpload {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    physics: [u32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct CameraUniform {
    view_projection: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct DebugPushConstants {
    view_projection: [[f32; 4]; 4],
    viewport_size: [f32; 2],
    _padding: [f32; 2],
}

fn debug_line_vertices(line: &DebugLine) -> [DebugVertex; 6] {
    let vertex = |corner| DebugVertex {
        start: line.start,
        end: line.end,
        color: line.color,
        corner,
        thickness: line.thickness,
    };
    [
        vertex([0.0, -1.0]),
        vertex([1.0, -1.0]),
        vertex([1.0, 1.0]),
        vertex([0.0, -1.0]),
        vertex([1.0, 1.0]),
        vertex([0.0, 1.0]),
    ]
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, PartialEq)]
struct GpuBodyState {
    model: [[f32; 4]; 4],
    velocity: [f32; 4],
    angular_velocity: [f32; 4],
    properties: [f32; 4],
    custom_values: [f32; 4],
    metadata: [u32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default, PartialEq)]
struct GpuConditionUpload {
    words: [u32; 4],
    values: [f32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default, PartialEq)]
struct GpuRuleState {
    config: [u32; 4],
    timing: [f32; 4],
    state: [u32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default)]
struct GpuEventHeader {
    count: u32,
    overflow: u32,
    reserved: [u32; 2],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default)]
struct GpuEventUpload {
    header: [u32; 4],
    timing: [u32; 4],
    payload: [f32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct PhysicsPushConstants {
    dt: f32,
    elapsed: f32,
    body_count: u32,
    event_capacity: u32,
    tick_low: u32,
    tick_high: u32,
    gravity_x: f32,
    gravity_y: f32,
    gravity_z: f32,
    _padding: [u32; 3],
}

struct PreparedMesh {
    vertices: Subbuffer<[SceneVertex]>,
    indices: Subbuffer<[u32]>,
    source_revision: u64,
}

/// Consecutive instances that share one mesh and material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedRenderBatch {
    mesh_key: u64,
    first_instance: u32,
    instance_count: u32,
}

/// Cached instance data rebuilt only when extracted render data changes.
struct PreparedRenderInstances {
    renderables_revision: u64,
    physics_revision: u64,
    material_revisions: Vec<(Handle<MaterialAsset>, u64)>,
    instances: Subbuffer<[RenderInstanceUpload]>,
    batches: Vec<PreparedRenderBatch>,
}

struct PreparedGpuPhysics {
    source_revision: u64,
    source: Vec<crate::runtime::ExtractedGpuPhysicsBody>,
    body_indices: HashMap<bevy_ecs::entity::Entity, u32>,
    states: Subbuffer<[GpuBodyState]>,
    instructions: Subbuffer<[GpuConditionUpload]>,
    rules: Subbuffer<[GpuRuleState]>,
}

/// Resources reused when one swapchain image comes around again.
struct PreparedFrame {
    graphics_set: Arc<DescriptorSet>,
    framebuffer: Arc<Framebuffer>,
    renderables_revision: u64,
    physics_revision: u64,
}

type PhysicsFence = Arc<FenceSignalFuture<Box<dyn GpuFuture>>>;

struct PendingPhysicsReadback {
    fence: PhysicsFence,
    header: Subbuffer<GpuEventHeader>,
    events: Subbuffer<[GpuEventUpload]>,
}

/// Pixel-space area of a render target occupied by the 3D scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneViewport {
    pub offset: [u32; 2],
    pub extent: [u32; 2],
}

/// Per-frame options that differ between a normal game render and Scene View.
///
/// The generic overlay type keeps editor code out of the renderer API. Games
/// use [`Self::game`] and therefore cannot accidentally draw editor helpers.
#[derive(Clone, Copy, Debug)]
pub struct SceneRenderOptions<'a> {
    /// Portion of the target image used by this 3D view.
    pub viewport: SceneViewport,
    /// Optional temporary lines drawn after scene meshes.
    pub debug_overlay: Option<&'a RenderDebugOverlay>,
}

impl<'a> SceneRenderOptions<'a> {
    /// Creates the normal, editor-free game rendering configuration.
    #[must_use]
    pub fn game(extent: [u32; 2]) -> Self {
        Self {
            viewport: SceneViewport::full(extent),
            debug_overlay: None,
        }
    }
}

impl SceneViewport {
    #[must_use]
    pub const fn full(extent: [u32; 2]) -> Self {
        Self {
            offset: [0, 0],
            extent,
        }
    }

    fn clamped_to(self, target_extent: [u32; 2]) -> Self {
        let offset = [
            self.offset[0].min(target_extent[0]),
            self.offset[1].min(target_extent[1]),
        ];
        Self {
            offset,
            extent: [
                self.extent[0].min(target_extent[0].saturating_sub(offset[0])),
                self.extent[1].min(target_extent[1].saturating_sub(offset[1])),
            ],
        }
    }
}

/// Minimal opaque forward pass with depth buffering and prepared mesh caching.
pub struct SceneRenderer {
    queue: Arc<Queue>,
    memory_allocator: Arc<StandardMemoryAllocator>,
    command_allocator: Arc<StandardCommandBufferAllocator>,
    descriptor_allocator: Arc<StandardDescriptorSetAllocator>,
    render_pass: Arc<RenderPass>,
    pipeline: Arc<GraphicsPipeline>,
    debug_pipeline: Arc<GraphicsPipeline>,
    debug_on_top_pipeline: Arc<GraphicsPipeline>,
    physics_pipeline: Arc<ComputePipeline>,
    depth: Arc<ImageView>,
    depth_extent: [u32; 2],
    prepared_meshes: HashMap<u64, PreparedMesh>,
    prepared_meshes_revision: u64,
    visible_meshes: Vec<Handle<MeshAsset>>,
    prepared_instances: Option<PreparedRenderInstances>,
    prepared_physics: Option<PreparedGpuPhysics>,
    prepared_frames: HashMap<usize, PreparedFrame>,
    pending_physics: Vec<PendingPhysicsReadback>,
    completed_physics_events: Vec<RawGpuPhysicsEvent>,
    last_physics_tick: u64,
}

impl SceneRenderer {
    pub fn new(
        queue: Arc<Queue>,
        memory_allocator: Arc<StandardMemoryAllocator>,
        output_format: Format,
        initial_extent: [u32; 2],
    ) -> Result<Self, SceneRenderError> {
        let render_pass = vulkano::single_pass_renderpass!(
            queue.device().clone(),
            attachments: {
                color: {
                    format: output_format,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
                depth: {
                    format: Format::D32_SFLOAT,
                    samples: 1,
                    load_op: Clear,
                    store_op: DontCare,
                }
            },
            pass: {
                color: [color],
                depth_stencil: {depth}
            }
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        let pipeline = create_pipeline(queue.clone(), render_pass.clone())?;
        let debug_pipeline =
            create_debug_pipeline(queue.clone(), render_pass.clone(), true)?;
        let debug_on_top_pipeline =
            create_debug_pipeline(queue.clone(), render_pass.clone(), false)?;
        let physics_pipeline = create_physics_pipeline(queue.clone())?;
        let depth = create_depth(&memory_allocator, initial_extent)?;
        Ok(Self {
            command_allocator: Arc::new(StandardCommandBufferAllocator::new(
                queue.device().clone(),
                Default::default(),
            )),
            descriptor_allocator: Arc::new(
                StandardDescriptorSetAllocator::new(
                    queue.device().clone(),
                    Default::default(),
                ),
            ),
            queue,
            memory_allocator,
            render_pass,
            pipeline,
            debug_pipeline,
            debug_on_top_pipeline,
            physics_pipeline,
            depth,
            depth_extent: initial_extent,
            prepared_meshes: HashMap::new(),
            prepared_meshes_revision: 0,
            visible_meshes: Vec::new(),
            prepared_instances: None,
            prepared_physics: None,
            prepared_frames: HashMap::new(),
            pending_physics: Vec::new(),
            completed_physics_events: Vec::new(),
            last_physics_tick: 0,
        })
    }

    pub fn render(
        &mut self,
        before: Box<dyn GpuFuture>,
        target: Arc<ImageView>,
        extent: [u32; 2],
        options: SceneRenderOptions<'_>,
        render_world: &RenderWorld,
        assets: &AssetServer,
    ) -> Result<Box<dyn GpuFuture>, SceneRenderError> {
        let viewport = options.viewport.clamped_to(extent);
        if extent[0] == 0
            || extent[1] == 0
            || viewport.extent[0] == 0
            || viewport.extent[1] == 0
        {
            return Ok(before);
        }
        self.ensure_depth(extent)?;
        self.prepare_visible_meshes(render_world, assets)?;
        self.prepare_gpu_physics(render_world)?;
        self.prepare_render_instances(render_world, assets)?;

        let physics = self.prepared_physics.as_ref().unwrap();
        let new_ticks = render_world
            .physics_tick
            .saturating_sub(self.last_physics_tick);
        let physics_ran = render_world.physics_enabled
            && new_ticks > 0
            && !physics.source.is_empty();

        let render_instances = self.prepared_instances.as_ref().unwrap();
        let frame_key = Arc::as_ptr(&target) as usize;
        if !self.prepared_frames.contains_key(&frame_key) {
            let graphics_set = DescriptorSet::new(
                self.descriptor_allocator.clone(),
                self.pipeline.layout().set_layouts()[0].clone(),
                [
                    WriteDescriptorSet::buffer(0, physics.states.clone()),
                    WriteDescriptorSet::buffer(
                        1,
                        render_instances.instances.clone(),
                    ),
                ],
                [],
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            let framebuffer = Framebuffer::new(
                self.render_pass.clone(),
                FramebufferCreateInfo {
                    attachments: vec![target.clone(), self.depth.clone()],
                    ..Default::default()
                },
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            self.prepared_frames.insert(
                frame_key,
                PreparedFrame {
                    graphics_set,
                    framebuffer,
                    renderables_revision: render_world.renderables_revision,
                    physics_revision: render_world.gpu_physics_revision,
                },
            );
        }

        let frame = self.prepared_frames.get_mut(&frame_key).unwrap();
        if frame.renderables_revision != render_world.renderables_revision
            || frame.physics_revision != render_world.gpu_physics_revision
        {
            frame.graphics_set = DescriptorSet::new(
                self.descriptor_allocator.clone(),
                self.pipeline.layout().set_layouts()[0].clone(),
                [
                    WriteDescriptorSet::buffer(0, physics.states.clone()),
                    WriteDescriptorSet::buffer(
                        1,
                        render_instances.instances.clone(),
                    ),
                ],
                [],
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            frame.renderables_revision = render_world.renderables_revision;
            frame.physics_revision = render_world.gpu_physics_revision;
        }
        let graphics_set = frame.graphics_set.clone();
        let framebuffer = frame.framebuffer.clone();
        let camera = CameraUniform {
            view_projection: view_projection(render_world, viewport.extent)
                .into(),
        };

        // Physics runs at the fixed rate, which is commonly much lower than
        // render FPS. Do not allocate readback buffers on interpolation-only
        // frames where no compute dispatch will use them.
        let event_capacity = physics_ran.then(|| {
            physics
                .source
                .iter()
                .map(|body| body.rules.len())
                .sum::<usize>()
                .clamp(64, 65_536)
        });
        let physics_resources = if let Some(event_capacity) = event_capacity {
            let event_header = Buffer::from_data(
                self.memory_allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::STORAGE_BUFFER,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                    ..Default::default()
                },
                GpuEventHeader::default(),
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            let event_buffer = Buffer::new_slice::<GpuEventUpload>(
                self.memory_allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::STORAGE_BUFFER,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                    ..Default::default()
                },
                event_capacity as u64,
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            let physics_set = DescriptorSet::new(
                self.descriptor_allocator.clone(),
                self.physics_pipeline.layout().set_layouts()[0].clone(),
                [
                    WriteDescriptorSet::buffer(0, physics.states.clone()),
                    WriteDescriptorSet::buffer(1, physics.instructions.clone()),
                    WriteDescriptorSet::buffer(2, physics.rules.clone()),
                    WriteDescriptorSet::buffer(3, event_header.clone()),
                    WriteDescriptorSet::buffer(4, event_buffer.clone()),
                ],
                [],
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            Some((physics_set, event_header, event_buffer))
        } else {
            None
        };

        let mut commands = AutoCommandBufferBuilder::primary(
            self.command_allocator.clone(),
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        if physics_ran {
            let dt = render_world.fixed_delta_seconds * new_ticks as f32;
            commands
                .bind_pipeline_compute(self.physics_pipeline.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .bind_descriptor_sets(
                    PipelineBindPoint::Compute,
                    self.physics_pipeline.layout().clone(),
                    0,
                    physics_resources.as_ref().unwrap().0.clone(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
                .push_constants(
                    self.physics_pipeline.layout().clone(),
                    0,
                    PhysicsPushConstants {
                        dt,
                        elapsed: render_world.elapsed_seconds,
                        body_count: physics.source.len() as u32,
                        event_capacity: event_capacity.unwrap() as u32,
                        tick_low: render_world.physics_tick as u32,
                        tick_high: (render_world.physics_tick >> 32) as u32,
                        gravity_x: render_world.physics_gravity[0],
                        gravity_y: render_world.physics_gravity[1],
                        gravity_z: render_world.physics_gravity[2],
                        _padding: [0; 3],
                    },
                )
                .map_err(|error| SceneRenderError(error.to_string()))?;
            unsafe {
                commands
                    .dispatch([physics.source.len().div_ceil(256) as u32, 1, 1])
                    .map_err(|error| SceneRenderError(error.to_string()))?;
            }
            self.last_physics_tick = render_world.physics_tick;
        } else if new_ticks > 0 {
            // Disabled time must not be simulated later when physics resumes.
            self.last_physics_tick = render_world.physics_tick;
        }
        commands
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![
                        Some(render_world.background_color.into()),
                        Some(1.0_f32.into()),
                    ],
                    ..RenderPassBeginInfo::framebuffer(framebuffer)
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .map_err(|error| SceneRenderError(error.to_string()))?
            .bind_pipeline_graphics(self.pipeline.clone())
            .map_err(|error| SceneRenderError(error.to_string()))?
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.pipeline.layout().clone(),
                0,
                graphics_set,
            )
            .map_err(|error| SceneRenderError(error.to_string()))?
            .push_constants(self.pipeline.layout().clone(), 0, camera)
            .map_err(|error| SceneRenderError(error.to_string()))?
            .set_viewport(
                0,
                [Viewport {
                    offset: [
                        viewport.offset[0] as f32,
                        viewport.offset[1] as f32,
                    ],
                    extent: [
                        viewport.extent[0] as f32,
                        viewport.extent[1] as f32,
                    ],
                    depth_range: 0.0..=1.0,
                }]
                .into_iter()
                .collect(),
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;

        for batch in &render_instances.batches {
            let Some(mesh) = self.prepared_meshes.get(&batch.mesh_key) else {
                continue;
            };
            commands
                .bind_vertex_buffers(0, mesh.vertices.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .bind_index_buffer(mesh.indices.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?;
            unsafe {
                commands
                    .draw_indexed(
                        mesh.indices.len() as u32,
                        batch.instance_count,
                        0,
                        0,
                        batch.first_instance,
                    )
                    .map_err(|error| SceneRenderError(error.to_string()))?;
            }
        }
        // Debug geometry is submitted in the same render pass, so it uses the
        // exact editor camera and viewport as the scene below it. The optional
        // input is never provided by the game runner.
        if let Some(overlay) = options.debug_overlay {
            for (on_top, pipeline) in [
                (false, self.debug_pipeline.clone()),
                (true, self.debug_on_top_pipeline.clone()),
            ] {
                let vertices = overlay
                    .lines
                    .iter()
                    .filter(|line| line.on_top == on_top)
                    .flat_map(debug_line_vertices)
                    .collect::<Vec<_>>();
                if vertices.is_empty() {
                    continue;
                }
                let vertices = Buffer::from_iter(
                    self.memory_allocator.clone(),
                    BufferCreateInfo {
                        usage: BufferUsage::VERTEX_BUFFER,
                        ..Default::default()
                    },
                    AllocationCreateInfo {
                        memory_type_filter: MemoryTypeFilter::PREFER_HOST
                            | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                        ..Default::default()
                    },
                    vertices,
                )
                .map_err(|error| SceneRenderError(error.to_string()))?;
                commands
                    .bind_pipeline_graphics(pipeline.clone())
                    .map_err(|error| SceneRenderError(error.to_string()))?
                    .push_constants(
                        pipeline.layout().clone(),
                        0,
                        DebugPushConstants {
                            view_projection: camera.view_projection,
                            viewport_size: [
                                viewport.extent[0] as f32,
                                viewport.extent[1] as f32,
                            ],
                            _padding: [0.0; 2],
                        },
                    )
                    .map_err(|error| SceneRenderError(error.to_string()))?
                    .bind_vertex_buffers(0, vertices.clone())
                    .map_err(|error| SceneRenderError(error.to_string()))?;
                unsafe {
                    commands
                        .draw(vertices.len() as u32, 1, 0, 0)
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                }
            }
        }
        commands
            .end_render_pass(Default::default())
            .map_err(|error| SceneRenderError(error.to_string()))?;
        let command_buffer = commands
            .build()
            .map_err(|error| SceneRenderError(error.to_string()))?;
        let future = before
            .then_execute(self.queue.clone(), command_buffer)
            .map_err(|error| SceneRenderError(error.to_string()))?;
        if physics_ran {
            let (_, event_header, event_buffer) = physics_resources.unwrap();
            // Vulkano implements GpuFuture for Arc<FenceSignalFuture>. This
            // renderer stays on the window thread and never sends it elsewhere.
            #[allow(clippy::arc_with_non_send_sync)]
            let fence = Arc::new(future.boxed().then_signal_fence());
            self.pending_physics.push(PendingPhysicsReadback {
                fence: fence.clone(),
                header: event_header,
                events: event_buffer,
            });
            Ok(fence.boxed())
        } else {
            Ok(future.boxed())
        }
    }

    fn prepare_visible_meshes(
        &mut self,
        render_world: &RenderWorld,
        assets: &AssetServer,
    ) -> Result<(), SceneRenderError> {
        if self.prepared_meshes_revision != render_world.renderables_revision {
            self.visible_meshes = render_world
                .renderables
                .iter()
                .map(|renderable| renderable.mesh)
                .collect();
            self.visible_meshes.sort_unstable_by_key(|mesh| mesh.key());
            self.visible_meshes.dedup_by_key(|mesh| mesh.key());
            self.prepared_meshes_revision = render_world.renderables_revision;
        }
        for mesh_handle in self.visible_meshes.iter().copied() {
            let key = mesh_handle.key();
            let revision = assets.meshes.revision(mesh_handle).unwrap_or(0);
            if self
                .prepared_meshes
                .get(&key)
                .is_some_and(|mesh| mesh.source_revision == revision)
            {
                continue;
            }
            let mesh = assets
                .meshes
                .get(mesh_handle)
                .or_else(|| assets.meshes.get(assets.fallback_mesh))
                .ok_or_else(|| {
                    SceneRenderError("fallback mesh is missing".into())
                })?;
            self.prepared_meshes
                .insert(key, self.prepare_mesh(mesh, revision)?);
        }
        Ok(())
    }

    /// Packs render objects into large GPU instance batches.
    ///
    /// Ten thousand cubes with the same mesh and material become one Vulkan
    /// draw call. GPU-owned objects store only their physics-buffer index here,
    /// so their changing transforms never need a CPU instance upload.
    fn prepare_render_instances(
        &mut self,
        render_world: &RenderWorld,
        assets: &AssetServer,
    ) -> Result<(), SceneRenderError> {
        let physics_indices =
            &self.prepared_physics.as_ref().unwrap().body_indices;
        if self.prepared_instances.as_ref().is_some_and(|prepared| {
            prepared.renderables_revision == render_world.renderables_revision
                && prepared.physics_revision
                    == render_world.gpu_physics_revision
                && prepared.material_revisions.iter().all(
                    |(material, revision)| {
                        assets.materials.revision(*material).unwrap_or(0)
                            == *revision
                    },
                )
        }) {
            return Ok(());
        }

        let mut materials = render_world
            .renderables
            .iter()
            .map(|renderable| renderable.material)
            .collect::<Vec<_>>();
        materials.sort_unstable_by_key(|material| material.key());
        materials.dedup_by_key(|material| material.key());
        let material_revisions = materials
            .into_iter()
            .map(|material| {
                (material, assets.materials.revision(material).unwrap_or(0))
            })
            .collect();

        let (order, batches) = render_batch_order(&render_world.renderables);
        let mut instances = Vec::with_capacity(order.len().max(1));
        for index in order {
            let renderable = render_world.renderables[index];
            let color = assets
                .materials
                .get(renderable.material)
                .map_or([1.0, 0.0, 1.0, 1.0], |material| material.base_color);
            instances.push(RenderInstanceUpload {
                model: renderable.transform.matrix,
                color,
                physics: [
                    physics_indices
                        .get(&renderable.entity)
                        .copied()
                        .unwrap_or(u32::MAX),
                    0,
                    0,
                    0,
                ],
            });
        }
        if instances.is_empty() {
            instances.push(RenderInstanceUpload::default());
        }
        let instances = Buffer::from_iter(
            self.memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            instances,
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        self.prepared_instances = Some(PreparedRenderInstances {
            renderables_revision: render_world.renderables_revision,
            physics_revision: render_world.gpu_physics_revision,
            material_revisions,
            instances,
            batches,
        });
        Ok(())
    }

    /// Returns physics events only after their GPU submission has completed.
    ///
    /// The signal is checked first, so this never waits for unfinished work.
    pub fn take_completed_physics_events(&mut self) -> Vec<RawGpuPhysicsEvent> {
        let mut index = 0;
        while index < self.pending_physics.len() {
            let signaled = self.pending_physics[index]
                .fence
                .is_signaled()
                .unwrap_or(false);
            if !signaled {
                index += 1;
                continue;
            }
            let pending = self.pending_physics.swap_remove(index);
            // The zero timeout only cleans an already-signaled submission.
            if pending.fence.wait(Some(Duration::ZERO)).is_err() {
                continue;
            }
            let Ok(header) = pending.header.read() else {
                continue;
            };
            let count =
                (header.count as usize).min(pending.events.len() as usize);
            if header.overflow > 0 {
                eprintln!(
                    "GPU physics event buffer overflowed by at least {} events",
                    header.overflow
                );
            }
            drop(header);
            let Ok(events) = pending.events.read() else {
                continue;
            };
            self.completed_physics_events.extend(
                events.iter().take(count).map(|event| RawGpuPhysicsEvent {
                    body_slot: event.header[0],
                    body_generation: event.header[1],
                    event_id: event.header[2],
                    flags: event.header[3],
                    tick_low: event.timing[0],
                    tick_high: event.timing[1],
                    payload_kind: event.timing[2],
                    reserved: event.timing[3],
                    payload: event.payload,
                }),
            );
        }
        std::mem::take(&mut self.completed_physics_events)
    }

    fn prepare_gpu_physics(
        &mut self,
        render_world: &RenderWorld,
    ) -> Result<(), SceneRenderError> {
        if self.prepared_physics.as_ref().is_some_and(|prepared| {
            prepared.source_revision == render_world.gpu_physics_revision
        }) {
            return Ok(());
        }

        let mut states =
            Vec::with_capacity(render_world.gpu_physics.len().max(1));
        let mut instructions = Vec::new();
        let mut rules = Vec::new();
        let mut body_indices = HashMap::new();
        for body in &render_world.gpu_physics {
            let body_index = states.len() as u32;
            let rule_offset = rules.len() as u32;
            body_indices.insert(body.entity, body_index);
            for rule in &body.rules {
                let instruction_offset = instructions.len() as u32;
                instructions.extend(rule.instructions.iter().map(
                    |instruction: &GpuConditionInstruction| {
                        GpuConditionUpload {
                            words: [
                                instruction.opcode,
                                instruction.operand,
                                instruction.flags,
                                instruction.reserved,
                            ],
                            values: instruction.values,
                        }
                    },
                ));
                rules.push(GpuRuleState {
                    config: [
                        instruction_offset,
                        rule.instructions.len() as u32,
                        rule.event_id.0,
                        rule.mode as u32,
                    ],
                    timing: [rule.cooldown_seconds, -1.0e20, 0.0, 0.0],
                    state: [rule.payload as u32, 0, 0, 0],
                });
            }
            states.push(GpuBodyState {
                model: body.transform.to_matrix(),
                velocity: [
                    body.rigid_body.linear_velocity[0],
                    body.rigid_body.linear_velocity[1],
                    body.rigid_body.linear_velocity[2],
                    0.0,
                ],
                angular_velocity: [
                    body.rigid_body.angular_velocity[0],
                    body.rigid_body.angular_velocity[1],
                    body.rigid_body.angular_velocity[2],
                    0.0,
                ],
                properties: [
                    body.rigid_body.mass,
                    body.rigid_body.gravity_scale,
                    match body.rigid_body.kind {
                        crate::runtime::RigidBodyKind::Fixed => 0.0,
                        crate::runtime::RigidBodyKind::Dynamic => 1.0,
                        crate::runtime::RigidBodyKind::Kinematic => 2.0,
                    },
                    0.0,
                ],
                // The first custom value carries the selected solver to the
                // shared ECS physics shader. Space is solver value 4.
                custom_values: [
                    match body.solver {
                        crate::runtime::PhysicsSolver::Full => 0.0,
                        crate::runtime::PhysicsSolver::Simplified => 1.0,
                        crate::runtime::PhysicsSolver::NoCollision => 2.0,
                        crate::runtime::PhysicsSolver::Custom => 3.0,
                        crate::runtime::PhysicsSolver::Space => 4.0,
                    },
                    0.0,
                    0.0,
                    0.0,
                ],
                metadata: [
                    body.physics_id.slot,
                    body.physics_id.generation,
                    rule_offset,
                    body.rules.len() as u32,
                ],
            });
        }
        if states.is_empty() {
            states.push(GpuBodyState {
                model: Matrix4::<f32>::identity().into(),
                velocity: [0.0; 4],
                angular_velocity: [0.0; 4],
                properties: [0.0; 4],
                custom_values: [0.0; 4],
                metadata: [0; 4],
            });
        }
        if instructions.is_empty() {
            instructions.push(GpuConditionUpload::default());
        }
        if rules.is_empty() {
            rules.push(GpuRuleState::default());
        }

        let storage = |usage| BufferCreateInfo {
            usage,
            ..Default::default()
        };
        let upload = AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
            ..Default::default()
        };
        let states = Buffer::from_iter(
            self.memory_allocator.clone(),
            storage(BufferUsage::STORAGE_BUFFER),
            upload.clone(),
            states,
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        let instructions = Buffer::from_iter(
            self.memory_allocator.clone(),
            storage(BufferUsage::STORAGE_BUFFER),
            upload.clone(),
            instructions,
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        let rules = Buffer::from_iter(
            self.memory_allocator.clone(),
            storage(BufferUsage::STORAGE_BUFFER),
            upload,
            rules,
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        self.prepared_physics = Some(PreparedGpuPhysics {
            source_revision: render_world.gpu_physics_revision,
            source: render_world.gpu_physics.clone(),
            body_indices,
            states,
            instructions,
            rules,
        });
        self.last_physics_tick = render_world.physics_tick.saturating_sub(1);
        Ok(())
    }

    fn prepare_mesh(
        &self,
        mesh: &MeshAsset,
        source_revision: u64,
    ) -> Result<PreparedMesh, SceneRenderError> {
        let vertices = Buffer::from_iter(
            self.memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            mesh.vertices.iter().map(|vertex| SceneVertex {
                position: vertex.position,
                normal: vertex.normal,
            }),
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        let indices = Buffer::from_iter(
            self.memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::INDEX_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            mesh.indices.iter().copied(),
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        Ok(PreparedMesh {
            vertices,
            indices,
            source_revision,
        })
    }

    fn ensure_depth(
        &mut self,
        extent: [u32; 2],
    ) -> Result<(), SceneRenderError> {
        if extent != self.depth_extent {
            self.depth = create_depth(&self.memory_allocator, extent)?;
            self.depth_extent = extent;
            // Cached framebuffers still point at the old depth image.
            self.prepared_frames.clear();
        }
        Ok(())
    }
}

/// Sorts objects so equal meshes and materials can use one instanced draw.
fn render_batch_order(
    renderables: &[crate::runtime::ExtractedRenderable],
) -> (Vec<usize>, Vec<PreparedRenderBatch>) {
    let mut order = (0..renderables.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| {
        let renderable = &renderables[*index];
        (renderable.mesh.key(), renderable.material.key())
    });
    let mut batches = Vec::<PreparedRenderBatch>::new();
    let mut previous_key = None;
    for (instance, index) in order.iter().copied().enumerate() {
        let renderable = renderables[index];
        let key = (renderable.mesh.key(), renderable.material.key());
        if previous_key != Some(key) {
            batches.push(PreparedRenderBatch {
                mesh_key: key.0,
                first_instance: instance as u32,
                instance_count: 0,
            });
            previous_key = Some(key);
        }
        batches.last_mut().unwrap().instance_count += 1;
    }
    (order, batches)
}

fn create_depth(
    allocator: &Arc<StandardMemoryAllocator>,
    extent: [u32; 2],
) -> Result<Arc<ImageView>, SceneRenderError> {
    let image = Image::new(
        allocator.clone(),
        ImageCreateInfo {
            format: Format::D32_SFLOAT,
            extent: [extent[0].max(1), extent[1].max(1), 1],
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    ImageView::new_default(image)
        .map_err(|error| SceneRenderError(error.to_string()))
}

fn create_pipeline(
    queue: Arc<Queue>,
    render_pass: Arc<RenderPass>,
) -> Result<Arc<GraphicsPipeline>, SceneRenderError> {
    let vertex = vertex_shader::load(queue.device().clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("scene vertex entry point is missing".into())
        })?;
    let fragment = fragment_shader::load(queue.device().clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("scene fragment entry point is missing".into())
        })?;
    let stages = [
        PipelineShaderStageCreateInfo::new(vertex.clone()),
        PipelineShaderStageCreateInfo::new(fragment),
    ];
    let layout = PipelineLayout::new(
        queue.device().clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
            .into_pipeline_layout_create_info(queue.device().clone())
            .map_err(|error| SceneRenderError(error.to_string()))?,
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let subpass = Subpass::from(render_pass, 0)
        .ok_or_else(|| SceneRenderError("scene subpass is missing".into()))?;
    GraphicsPipeline::new(
        queue.device().clone(),
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(
                SceneVertex::per_vertex()
                    .definition(&vertex)
                    .map_err(|error| SceneRenderError(error.to_string()))?,
            ),
            input_assembly_state: Some(InputAssemblyState::default()),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState {
                cull_mode: CullMode::Back,
                // Source meshes are CCW when viewed from outside. The clip
                // correction makes that convention match Vulkan framebuffer
                // space; changing this to clockwise renders the cube inside-out.
                front_face: FrontFace::CounterClockwise,
                ..Default::default()
            }),
            multisample_state: Some(MultisampleState::default()),
            depth_stencil_state: Some(DepthStencilState {
                depth: Some(DepthState::simple()),
                ..Default::default()
            }),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                1,
                ColorBlendAttachmentState::default(),
            )),
            dynamic_state: [DynamicState::Viewport].into_iter().collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))
}

/// Makes a minimal unlit line pipeline for Scene View helpers.
///
/// We use standard one-pixel Vulkan lines here. Interactive handles can later
/// use triangle geometry when they need thick, portable shapes.
fn create_debug_pipeline(
    queue: Arc<Queue>,
    render_pass: Arc<RenderPass>,
    depth_test: bool,
) -> Result<Arc<GraphicsPipeline>, SceneRenderError> {
    let vertex = debug_vertex_shader::load(queue.device().clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("debug vertex entry point is missing".into())
        })?;
    let fragment = debug_fragment_shader::load(queue.device().clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("debug fragment entry point is missing".into())
        })?;
    let stages = [
        PipelineShaderStageCreateInfo::new(vertex.clone()),
        PipelineShaderStageCreateInfo::new(fragment),
    ];
    let layout = PipelineLayout::new(
        queue.device().clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
            .into_pipeline_layout_create_info(queue.device().clone())
            .map_err(|error| SceneRenderError(error.to_string()))?,
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let subpass = Subpass::from(render_pass, 0)
        .ok_or_else(|| SceneRenderError("debug subpass is missing".into()))?;
    GraphicsPipeline::new(
        queue.device().clone(),
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(
                DebugVertex::per_vertex()
                    .definition(&vertex)
                    .map_err(|error| SceneRenderError(error.to_string()))?,
            ),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState {
                cull_mode: CullMode::None,
                ..Default::default()
            }),
            multisample_state: Some(MultisampleState::default()),
            depth_stencil_state: Some(if depth_test {
                DepthStencilState {
                    // Spatial helpers hide behind meshes without changing the
                    // scene depth buffer.
                    depth: Some(DepthState {
                        write_enable: false,
                        ..DepthState::simple()
                    }),
                    ..Default::default()
                }
            } else {
                DepthStencilState::default()
            }),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                1,
                ColorBlendAttachmentState::default(),
            )),
            dynamic_state: [DynamicState::Viewport].into_iter().collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))
}

fn create_physics_pipeline(
    queue: Arc<Queue>,
) -> Result<Arc<ComputePipeline>, SceneRenderError> {
    let shader = physics_shader::load(queue.device().clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("physics entry point is missing".into())
        })?;
    let stage = PipelineShaderStageCreateInfo::new(shader);
    let layout = PipelineLayout::new(
        queue.device().clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages([&stage])
            .into_pipeline_layout_create_info(queue.device().clone())
            .map_err(|error| SceneRenderError(error.to_string()))?,
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    ComputePipeline::new(
        queue.device().clone(),
        None,
        ComputePipelineCreateInfo::stage_layout(stage, layout),
    )
    .map_err(|error| SceneRenderError(error.to_string()))
}

fn view_projection(
    render_world: &RenderWorld,
    extent: [u32; 2],
) -> Matrix4<f32> {
    let aspect = extent[0] as f32 / extent[1].max(1) as f32;
    if let Some(camera) = render_world.active_camera {
        let world_from_camera = matrix_from_array(camera.transform.matrix);
        let view = world_from_camera
            .try_inverse()
            .unwrap_or_else(Matrix4::identity);
        let projection = match camera.projection {
            Projection::Perspective {
                vertical_fov_radians,
                near,
                far,
            } => Perspective3::new(aspect, vertical_fov_radians, near, far)
                .to_homogeneous(),
            Projection::Orthographic {
                vertical_size,
                near,
                far,
            } => Orthographic3::new(
                -vertical_size * aspect * 0.5,
                vertical_size * aspect * 0.5,
                -vertical_size * 0.5,
                vertical_size * 0.5,
                near,
                far,
            )
            .to_homogeneous(),
        };
        vulkan_clip_correction() * projection * view
    } else {
        vulkan_clip_correction()
            * Perspective3::new(
                aspect,
                std::f32::consts::FRAC_PI_3,
                0.1,
                1_000.0,
            )
            .to_homogeneous()
    }
}

/// Converts nalgebra's OpenGL clip convention to Vulkan's inverted Y axis and
/// zero-to-one depth range.
fn vulkan_clip_correction() -> Matrix4<f32> {
    Matrix4::new(
        1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0,
        0.0, 1.0,
    )
}

fn matrix_from_array(matrix: [[f32; 4]; 4]) -> Matrix4<f32> {
    Matrix4::from_column_slice(&matrix.concat())
}

#[cfg(test)]
fn normal_columns(model: Matrix4<f32>) -> [[f32; 4]; 3] {
    let linear = model.fixed_view::<3, 3>(0, 0).into_owned();
    let normal = linear
        .try_inverse()
        .map_or_else(nalgebra::Matrix3::identity, |inverse| {
            inverse.transpose()
        });
    [
        [normal[(0, 0)], normal[(1, 0)], normal[(2, 0)], 0.0],
        [normal[(0, 1)], normal[(1, 1)], normal[(2, 1)], 0.0],
        [normal[(0, 2)], normal[(1, 2)], normal[(2, 2)], 0.0],
    ]
}

#[rustfmt::skip]
mod vertex_shader {
    vulkano_shaders::shader! {
                            ty: "vertex",
                            src: r"
#version 450
layout(location = 0) in vec3 position;
layout(location = 1) in vec3 normal;
layout(location = 0) out vec3 v_normal;
layout(push_constant) uniform Camera {
    mat4 view_projection;
} camera;
struct PhysicsState {
    mat4 model;
    vec4 velocity;
    vec4 angular_velocity;
    vec4 properties;
    vec4 custom_values;
    uvec4 metadata;
};
layout(set = 0, binding = 0) readonly buffer PhysicsStates {
    PhysicsState data[];
} physics_states;
struct RenderInstance {
    mat4 model;
    vec4 color;
    uvec4 physics;
};
layout(set = 0, binding = 1) readonly buffer RenderInstances {
    RenderInstance data[];
} render_instances;
layout(location = 1) out vec4 v_color;
void main() {
    RenderInstance instance = render_instances.data[gl_InstanceIndex];
    mat4 model = instance.physics.x == 0xffffffffu
        ? instance.model
        : physics_states.data[instance.physics.x].model;
    gl_Position = camera.view_projection * model * vec4(position, 1.0);
    mat3 normal_matrix = transpose(inverse(mat3(model)));
    v_normal = normal_matrix * normal;
    v_color = instance.color;
}
"
                        }
}

#[rustfmt::skip]
mod fragment_shader {
    vulkano_shaders::shader! {
                            ty: "fragment",
                            src: r"
#version 450
layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec4 v_color;
layout(location = 0) out vec4 f_color;
void main() {
    vec3 n = normalize(v_normal);
    float diffuse = max(dot(n, normalize(vec3(0.4, 0.8, 0.5))), 0.0);
    f_color = vec4(v_color.rgb * (0.22 + diffuse * 0.78), v_color.a);
}
"
                        }
}

#[rustfmt::skip]
mod debug_vertex_shader {
    vulkano_shaders::shader! {
        ty: "vertex",
        src: r"
#version 450
layout(location = 0) in vec3 start;
layout(location = 1) in vec3 end;
layout(location = 2) in vec4 color;
layout(location = 3) in vec2 corner;
layout(location = 4) in float thickness;
layout(location = 0) out vec4 v_color;
layout(push_constant) uniform Camera {
    mat4 view_projection;
    vec2 viewport_size;
    vec2 padding;
} camera;
void main() {
    vec4 start_clip = camera.view_projection * vec4(start, 1.0);
    vec4 end_clip = camera.view_projection * vec4(end, 1.0);
    vec2 start_ndc = start_clip.xy / start_clip.w;
    vec2 end_ndc = end_clip.xy / end_clip.w;
    vec2 screen_direction = (end_ndc - start_ndc) * camera.viewport_size;
    float direction_length = length(screen_direction);
    vec2 normal = direction_length > 0.0001
        ? vec2(-screen_direction.y, screen_direction.x) / direction_length
        : vec2(0.0, 1.0);
    vec4 clip = mix(start_clip, end_clip, corner.x);
    clip.xy += normal * corner.y * thickness / camera.viewport_size * clip.w;
    gl_Position = clip;
    v_color = color;
}
"
    }
}

#[rustfmt::skip]
mod debug_fragment_shader {
    vulkano_shaders::shader! {
        ty: "fragment",
        src: r"
#version 450
layout(location = 0) in vec4 v_color;
layout(location = 0) out vec4 f_color;
void main() {
    f_color = v_color;
}
"
    }
}

#[rustfmt::skip]
mod physics_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 256, local_size_y = 1, local_size_z = 1) in;

struct PhysicsState {
    mat4 model;
    vec4 velocity;
    vec4 angular_velocity;
    vec4 properties;
    vec4 custom_values;
    uvec4 metadata;
};
struct ConditionInstruction {
    uvec4 words;
    vec4 values;
};
struct RuleState {
    uvec4 config;
    vec4 timing;
    uvec4 state;
};
struct PhysicsEvent {
    uvec4 header;
    uvec4 timing;
    vec4 payload;
};

layout(set = 0, binding = 0) buffer PhysicsStates { PhysicsState data[]; } bodies;
layout(set = 0, binding = 1) readonly buffer Conditions { ConditionInstruction data[]; } conditions;
layout(set = 0, binding = 2) buffer Rules { RuleState data[]; } rules;
layout(set = 0, binding = 3) buffer EventHeader { uint count; uint overflow; uvec2 reserved; } event_header;
layout(set = 0, binding = 4) buffer Events { PhysicsEvent data[]; } events;

layout(push_constant) uniform PhysicsPush {
    float dt;
    float elapsed;
    uint body_count;
    uint event_capacity;
    uint tick_low;
    uint tick_high;
    float gravity_x;
    float gravity_y;
    float gravity_z;
    uint padding_0;
    uint padding_1;
    uint padding_2;
} pc;

float read_field(PhysicsState body, uint field) {
    vec3 position = body.model[3].xyz;
    vec3 scale = vec3(length(body.model[0].xyz), length(body.model[1].xyz), length(body.model[2].xyz));
    if (field == 0u) return position.x;
    if (field == 1u) return position.y;
    if (field == 2u) return position.z;
    if (field == 3u) return body.velocity.x;
    if (field == 4u) return body.velocity.y;
    if (field == 5u) return body.velocity.z;
    if (field == 6u) return body.angular_velocity.x;
    if (field == 7u) return body.angular_velocity.y;
    if (field == 8u) return body.angular_velocity.z;
    if (field == 9u) return scale.x;
    if (field == 10u) return scale.y;
    if (field == 11u) return scale.z;
    if (field == 12u) return body.properties.x;
    if (field == 13u) return body.properties.y;
    if (field == 14u) return length(body.velocity.xyz);
    if (field >= 0x100u && field < 0x104u) return body.custom_values[field - 0x100u];
    return 0.0;
}

bool compare_value(float left, float right, uint comparison) {
    if (comparison == 0u) return left < right;
    if (comparison == 1u) return left <= right;
    if (comparison == 2u) return left > right;
    if (comparison == 3u) return left >= right;
    if (comparison == 4u) return abs(left - right) <= 0.00001;
    return abs(left - right) > 0.00001;
}

bool evaluate_condition(PhysicsState body, uint offset, uint count) {
    bool stack[64];
    uint stack_size = 0u;
    for (uint index = 0u; index < count && index < 64u; index++) {
        ConditionInstruction instruction = conditions.data[offset + index];
        uint operation = instruction.words.x;
        if (operation == 1u) {
            stack[stack_size++] = compare_value(
                read_field(body, instruction.words.y),
                instruction.values.x,
                instruction.words.z
            );
        } else if (operation == 2u) {
            float value = read_field(body, instruction.words.y);
            stack[stack_size++] = value >= instruction.values.x && value <= instruction.values.y;
        } else if (operation == 3u) {
            // Collision state will be supplied by the spatial solver stage.
            stack[stack_size++] = false;
        } else if (operation == 4u) {
            stack[stack_size++] = length(body.velocity.xyz) < 0.02 && length(body.angular_velocity.xyz) < 0.02;
        } else if (operation == 5u) {
            stack[stack_size++] = pc.elapsed >= instruction.values.x;
        } else if (operation == 16u && stack_size >= 2u) {
            bool right = stack[--stack_size];
            stack[stack_size - 1u] = stack[stack_size - 1u] && right;
        } else if (operation == 17u && stack_size >= 2u) {
            bool right = stack[--stack_size];
            stack[stack_size - 1u] = stack[stack_size - 1u] || right;
        } else if (operation == 18u && stack_size >= 1u) {
            stack[stack_size - 1u] = !stack[stack_size - 1u];
        }
    }
    return stack_size == 1u && stack[0];
}

vec4 event_payload(PhysicsState body, uint payload_kind) {
    if (payload_kind == 1u) return vec4(body.model[3].xyz, 1.0);
    if (payload_kind == 2u) return body.velocity;
    if (payload_kind == 3u) return body.angular_velocity;
    if (payload_kind == 5u) return body.custom_values;
    return vec4(0.0);
}

void emit_event(PhysicsState body, RuleState rule) {
    uint event_index = atomicAdd(event_header.count, 1u);
    if (event_index >= pc.event_capacity) {
        atomicAdd(event_header.overflow, 1u);
        return;
    }
    events.data[event_index].header = uvec4(
        body.metadata.x,
        body.metadata.y,
        rule.config.z,
        0u
    );
    events.data[event_index].timing = uvec4(
        pc.tick_low,
        pc.tick_high,
        rule.state.x,
        0u
    );
    events.data[event_index].payload = event_payload(body, rule.state.x);
}

void main() {
    uint body_index = gl_GlobalInvocationID.x;
    if (body_index >= pc.body_count) return;

    PhysicsState body = bodies.data[body_index];
    // properties.z: 0 = fixed, 1 = dynamic, 2 = kinematic.
    if (body.properties.z > 0.5 && body.properties.z < 1.5 && body.properties.x > 0.0) {
        if (abs(body.custom_values.x - 4.0) < 0.5) {
            // Space mode attracts bodies toward the origin. The force is
            // softened near the target so bodies do not explode numerically.
            vec3 to_target = -body.model[3].xyz;
            float distance_squared = dot(to_target, to_target);
            if (distance_squared > 0.000001) {
                vec3 direction = normalize(to_target);
                float safe_distance_squared = max(distance_squared, 4.0);
                body.velocity.xyz += direction
                    * (500.0 / safe_distance_squared)
                    * body.properties.y * pc.dt;
            }
        } else {
            body.velocity.xyz += vec3(pc.gravity_x, pc.gravity_y, pc.gravity_z)
                * body.properties.y * pc.dt;
        }
        body.model[3].xyz += body.velocity.xyz * pc.dt;
    }
    bodies.data[body_index] = body;

    uint rule_offset = body.metadata.z;
    uint rule_count = body.metadata.w;
    for (uint local_rule = 0u; local_rule < rule_count; local_rule++) {
        uint rule_index = rule_offset + local_rule;
        RuleState rule = rules.data[rule_index];
        bool current = evaluate_condition(body, rule.config.x, rule.config.y);
        bool previous = rule.state.z != 0u;
        bool already_emitted = rule.state.y != 0u;
        bool should_emit = false;
        if (rule.config.w == 0u) should_emit = current && !previous;
        else if (rule.config.w == 1u) should_emit = !current && previous;
        else if (rule.config.w == 2u) should_emit = current;
        else if (rule.config.w == 3u) should_emit = current && !already_emitted;

        bool cooldown_ready = pc.elapsed - rule.timing.y >= rule.timing.x;
        if (should_emit && cooldown_ready) {
            emit_event(body, rule);
            rule.timing.y = pc.elapsed;
            rule.state.y = 1u;
        }
        rule.state.z = current ? 1u : 0u;
        rules.data[rule_index] = rule;
    }
}
"
    }
}

#[cfg(test)]
mod tests {
    use nalgebra::Vector4;

    use super::*;

    fn perspective(aspect: f32, near: f32, far: f32) -> Matrix4<f32> {
        vulkan_clip_correction()
            * Perspective3::new(aspect, std::f32::consts::FRAC_PI_3, near, far)
                .to_homogeneous()
    }

    fn ndc(matrix: &Matrix4<f32>, point: Vector4<f32>) -> Vector4<f32> {
        let clip = matrix * point;
        clip / clip.w
    }

    #[test]
    fn perspective_makes_near_geometry_larger_than_far_geometry() {
        let projection = perspective(16.0 / 9.0, 0.1, 100.0);
        let near = ndc(&projection, Vector4::new(1.0, 0.0, -2.0, 1.0));
        let far = ndc(&projection, Vector4::new(1.0, 0.0, -4.0, 1.0));
        assert!(near.x.abs() > far.x.abs());
    }

    #[test]
    fn perspective_maps_depth_to_vulkan_zero_to_one_range() {
        let near_plane = 0.1;
        let far_plane = 100.0;
        let projection = perspective(1.0, near_plane, far_plane);
        let near = ndc(&projection, Vector4::new(0.0, 0.0, -near_plane, 1.0));
        let far = ndc(&projection, Vector4::new(0.0, 0.0, -far_plane, 1.0));
        assert!(near.z.abs() < 0.000_01, "near depth was {}", near.z);
        assert!((far.z - 1.0).abs() < 0.000_01, "far depth was {}", far.z);
    }

    #[test]
    fn projection_preserves_square_pixel_aspect() {
        let extent = [1000.0, 500.0];
        let projection = perspective(extent[0] / extent[1], 0.1, 100.0);
        let x = ndc(&projection, Vector4::new(1.0, 0.0, -4.0, 1.0));
        let y = ndc(&projection, Vector4::new(0.0, 1.0, -4.0, 1.0));
        let horizontal_pixels = x.x.abs() * extent[0];
        let vertical_pixels = y.y.abs() * extent[1];
        assert!((horizontal_pixels - vertical_pixels).abs() < 0.001);
    }

    #[test]
    fn matrix_upload_round_trip_preserves_columns() {
        let matrix =
            Matrix4::new_translation(&nalgebra::Vector3::new(2.0, 3.0, 4.0));
        let uploaded: [[f32; 4]; 4] = matrix.into();
        assert_eq!(matrix_from_array(uploaded), matrix);
    }

    #[test]
    fn render_instance_layout_matches_shader_struct() {
        use std::mem::{offset_of, size_of};

        assert_eq!(size_of::<RenderInstanceUpload>(), 96);
        assert_eq!(offset_of!(RenderInstanceUpload, color), 64);
        assert_eq!(offset_of!(RenderInstanceUpload, physics), 80);
    }

    #[test]
    fn ten_thousand_equal_cubes_become_one_render_batch() {
        let assets = AssetServer::default();
        let renderables = (0..10_000)
            .map(|index| crate::runtime::ExtractedRenderable {
                entity: bevy_ecs::entity::Entity::from_raw_u32(index).unwrap(),
                transform: crate::runtime::GlobalTransform::default(),
                mesh: assets.fallback_mesh,
                material: assets.fallback_material,
                cast_shadows: true,
                receive_shadows: true,
            })
            .collect::<Vec<_>>();

        let (order, batches) = render_batch_order(&renderables);

        assert_eq!(order.len(), 10_000);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].first_instance, 0);
        assert_eq!(batches[0].instance_count, 10_000);
    }

    #[test]
    fn hybrid_physics_gpu_layouts_match_shader_structs() {
        use std::mem::{offset_of, size_of};

        assert_eq!(size_of::<GpuBodyState>(), 144);
        assert_eq!(offset_of!(GpuBodyState, metadata), 128);
        assert_eq!(size_of::<GpuConditionUpload>(), 32);
        assert_eq!(size_of::<GpuRuleState>(), 48);
        assert_eq!(size_of::<GpuEventHeader>(), 16);
        assert_eq!(size_of::<GpuEventUpload>(), 48);
        assert_eq!(size_of::<PhysicsPushConstants>(), 48);
    }

    #[test]
    fn normal_matrix_ignores_translation_and_handles_scale() {
        let model =
            Matrix4::new_translation(&nalgebra::Vector3::new(2.0, 3.0, 4.0))
                * Matrix4::new_nonuniform_scaling(&nalgebra::Vector3::new(
                    2.0, 4.0, 5.0,
                ));
        assert_eq!(
            normal_columns(model),
            [
                [0.5, 0.0, 0.0, 0.0],
                [0.0, 0.25, 0.0, 0.0],
                [0.0, 0.0, 0.2, 0.0],
            ]
        );
    }
}
