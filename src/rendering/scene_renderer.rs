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

use nalgebra::{Matrix4, Orthographic3, Perspective3, Vector4};
use vulkano::buffer::allocator::{
    SubbufferAllocator, SubbufferAllocatorCreateInfo,
};
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
use vulkano::device::{DeviceExtensions, Queue};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::memory::MemoryHeapFlags;
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::graphics::color_blend::{
    AttachmentBlend, ColorBlendAttachmentState, ColorBlendState,
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
use vulkano::DeviceSize;

use crate::assets::{AlphaMode, AssetServer, Handle, MaterialAsset, MeshAsset};
use crate::rendering::debug_overlay::{DebugLine, RenderDebugOverlay};
use crate::runtime::{
    GpuConditionInstruction, Projection, QualityProfile, RawGpuPhysicsEvent,
    RenderWorld,
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
    /// x: GPU physics index or `u32::MAX`; y: alpha mode (0 opaque, 1 mask,
    /// 2 blend); z: mask cutoff as `f32` bits.
    physics: [u32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct CameraUniform {
    view_projection: [[f32; 4]; 4],
    ambient: [f32; 4],
    light_info: [u32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default, PartialEq)]
struct LightUpload {
    position_kind: [f32; 4],
    direction_range: [f32; 4],
    color_intensity: [f32; 4],
    spot_angles: [f32; 4],
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

/// One alpha-blended instance, drawn alone so it can be sorted per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BlendedInstance {
    instance: u32,
    mesh_key: u64,
    position: [f32; 3],
}

/// Cached instance data rebuilt only when extracted render data changes.
struct PreparedRenderInstances {
    renderables_revision: u64,
    physics_revision: u64,
    material_revisions: Vec<(Handle<MaterialAsset>, u64)>,
    instances: Subbuffer<[RenderInstanceUpload]>,
    /// Opaque and masked batches.
    batches: Vec<PreparedRenderBatch>,
    /// Blended instances, stored after every batched instance.
    blended: Vec<BlendedInstance>,
}

struct PreparedGpuPhysics {
    source_revision: u64,
    source: Vec<crate::runtime::ExtractedGpuPhysicsBody>,
    body_indices: HashMap<bevy_ecs::entity::Entity, u32>,
    states: Subbuffer<[GpuBodyState]>,
    instructions: Subbuffer<[GpuConditionUpload]>,
    rules: Subbuffer<[GpuRuleState]>,
}

struct PreparedLights {
    revision: u64,
    budget: usize,
    buffer: Subbuffer<[LightUpload]>,
    count: u32,
    ambient: [f32; 4],
}

/// Resources reused when one swapchain image comes around again.
struct PreparedFrame {
    graphics_set: Arc<DescriptorSet>,
    framebuffer: Arc<Framebuffer>,
    renderables_revision: u64,
    physics_revision: u64,
    lights_revision: u64,
}

type FrameFence = Arc<FenceSignalFuture<Box<dyn GpuFuture>>>;

/// Submitted frames allowed in flight before [`SceneRenderer::render`] waits.
pub const FRAMES_IN_FLIGHT: usize = 2;

/// Submission state of one in-flight frame, reused round-robin.
struct FrameContext {
    /// Signals when this context's last submission finished on the GPU.
    /// Holding it also keeps that submission's resources alive until then.
    fence: Option<FrameFence>,
    /// Host-visible per-frame buffers (debug vertices, physics readback).
    /// Its arenas are reused once this context's frame no longer holds them.
    transient: SubbufferAllocator,
}

struct PendingPhysicsReadback {
    fence: FrameFence,
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

/// Largest number of lights uploaded per frame; extra lights are dropped and
/// counted in [`RenderCapacityDiagnostics::dropped_lights`].
pub const MAX_LIGHTS: usize = 64;

/// Fixed-capacity fallbacks the renderer took instead of failing the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderCapacityDiagnostics {
    /// Lights beyond [`MAX_LIGHTS`] left out of the latest light upload.
    pub dropped_lights: usize,
    /// GPU physics events lost to event-buffer overflow since creation.
    pub physics_events_dropped: u64,
}

/// Minimal opaque forward pass with depth buffering and prepared mesh caching.
pub struct SceneRenderer {
    queue: Arc<Queue>,
    memory_allocator: Arc<StandardMemoryAllocator>,
    command_allocator: Arc<StandardCommandBufferAllocator>,
    descriptor_allocator: Arc<StandardDescriptorSetAllocator>,
    render_pass: Arc<RenderPass>,
    pipeline: Arc<GraphicsPipeline>,
    blend_pipeline: Arc<GraphicsPipeline>,
    debug_pipeline: Arc<GraphicsPipeline>,
    debug_on_top_pipeline: Arc<GraphicsPipeline>,
    physics_pipeline: Arc<ComputePipeline>,
    depth: Arc<ImageView>,
    depth_extent: [u32; 2],
    /// Per-change instance uploads. Arenas are reused once no in-flight
    /// frame references them and double in size when an upload outgrows them.
    instance_allocator: SubbufferAllocator,
    /// Largest per-frame instance upload accepted; see
    /// [`transient_upload_budget`].
    instance_budget: DeviceSize,
    prepared_meshes: HashMap<u64, PreparedMesh>,
    prepared_meshes_revision: u64,
    visible_meshes: Vec<Handle<MeshAsset>>,
    prepared_instances: Option<PreparedRenderInstances>,
    prepared_physics: Option<PreparedGpuPhysics>,
    prepared_lights: Option<PreparedLights>,
    prepared_frames: HashMap<usize, PreparedFrame>,
    pending_physics: Vec<PendingPhysicsReadback>,
    completed_physics_events: Vec<RawGpuPhysicsEvent>,
    last_physics_tick: u64,
    capacity: RenderCapacityDiagnostics,
    frame_contexts: [FrameContext; FRAMES_IN_FLIGHT],
    frame_index: usize,
    capabilities: RendererCapabilities,
}

impl SceneRenderer {
    pub fn new(
        queue: Arc<Queue>,
        memory_allocator: Arc<StandardMemoryAllocator>,
        output_format: Format,
        initial_extent: [u32; 2],
    ) -> Result<Self, SceneRenderError> {
        let limits = DeviceLimits::of(queue.device().physical_device());
        let capabilities = RendererCapabilities::detect(queue.device());
        let shortfalls = limits.shortfalls(&LOW_END_BASELINE);
        if !shortfalls.is_empty() {
            return Err(SceneRenderError(format!(
                "{} is below the renderer baseline: {}",
                queue.device().physical_device().properties().device_name,
                shortfalls.join("; ")
            )));
        }
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
        let (pipeline, blend_pipeline) =
            create_pipelines(queue.clone(), render_pass.clone())?;
        let debug_pipeline =
            create_debug_pipeline(queue.clone(), render_pass.clone(), true)?;
        let debug_on_top_pipeline =
            create_debug_pipeline(queue.clone(), render_pass.clone(), false)?;
        let physics_pipeline = create_physics_pipeline(queue.clone())?;
        let depth = create_depth(&memory_allocator, initial_extent)?;
        let instance_allocator = SubbufferAllocator::new(
            memory_allocator.clone(),
            SubbufferAllocatorCreateInfo {
                arena_size: INSTANCE_ARENA_BYTES,
                buffer_usage: BufferUsage::STORAGE_BUFFER,
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        );
        let instance_budget = transient_upload_budget(
            queue
                .device()
                .physical_device()
                .memory_properties()
                .memory_heaps
                .iter()
                .map(|heap| (heap.size, heap.flags)),
        )
        // The instance buffer is bound as one storage-buffer range.
        .min(DeviceSize::from(limits.max_storage_buffer_range));
        let frame_contexts = std::array::from_fn(|_| FrameContext {
            fence: None,
            transient: SubbufferAllocator::new(
                memory_allocator.clone(),
                SubbufferAllocatorCreateInfo {
                    arena_size: TRANSIENT_ARENA_BYTES,
                    buffer_usage: BufferUsage::STORAGE_BUFFER
                        | BufferUsage::VERTEX_BUFFER,
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                    ..Default::default()
                },
            ),
        });
        Ok(Self {
            instance_allocator,
            instance_budget,
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
            blend_pipeline,
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
            prepared_lights: None,
            prepared_frames: HashMap::new(),
            pending_physics: Vec::new(),
            completed_physics_events: Vec::new(),
            last_physics_tick: 0,
            capacity: RenderCapacityDiagnostics::default(),
            frame_contexts,
            frame_index: 0,
            capabilities,
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
        for context in &mut self.frame_contexts {
            // Completed frames release their resources without a wait.
            if context
                .fence
                .as_ref()
                .is_some_and(|fence| fence.is_signaled().unwrap_or(false))
            {
                context.fence = None;
            }
        }
        if let Some(fence) = self.frame_contexts[self.frame_index].fence.take()
        {
            // Blocks only when the GPU is FRAMES_IN_FLIGHT frames behind.
            fence
                .wait(None)
                .map_err(|error| SceneRenderError(error.to_string()))?;
        }
        self.ensure_depth(extent)?;
        self.prepare_visible_meshes(render_world, assets)?;
        self.prepare_gpu_physics(render_world)?;
        self.prepare_lights(render_world)?;
        self.prepare_render_instances(render_world, assets)?;

        let physics = self.prepared_physics.as_ref().unwrap();
        let lights = self.prepared_lights.as_ref().unwrap();
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
                    WriteDescriptorSet::buffer(2, lights.buffer.clone()),
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
                    lights_revision: render_world.lights_revision,
                },
            );
        }

        let frame = self.prepared_frames.get_mut(&frame_key).unwrap();
        if frame.renderables_revision != render_world.renderables_revision
            || frame.physics_revision != render_world.gpu_physics_revision
            || frame.lights_revision != render_world.lights_revision
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
                    WriteDescriptorSet::buffer(2, lights.buffer.clone()),
                ],
                [],
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            frame.renderables_revision = render_world.renderables_revision;
            frame.physics_revision = render_world.gpu_physics_revision;
            frame.lights_revision = render_world.lights_revision;
        }
        let graphics_set = frame.graphics_set.clone();
        let framebuffer = frame.framebuffer.clone();
        let camera = CameraUniform {
            view_projection: view_projection(render_world, viewport.extent)
                .into(),
            ambient: lights.ambient,
            light_info: [lights.count, 0, 0, 0],
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
            let transient = &self.frame_contexts[self.frame_index].transient;
            let event_header = transient
                .allocate_sized::<GpuEventHeader>()
                .map_err(|error| SceneRenderError(error.to_string()))?;
            *event_header
                .write()
                .map_err(|error| SceneRenderError(error.to_string()))? =
                GpuEventHeader::default();
            let event_buffer = transient
                .allocate_slice::<GpuEventUpload>(event_capacity as u64)
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
        let debug_labels_enabled = self
            .queue
            .device()
            .instance()
            .enabled_extensions()
            .ext_debug_utils;
        if debug_labels_enabled {
            let _ = commands.begin_debug_utils_label(
                vulkano::instance::debug::DebugUtilsLabel {
                    label_name: "SceneRenderer::render".to_string(),
                    ..Default::default()
                },
            );
        }
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
        if !render_instances.blended.is_empty() {
            let (eye, forward) = render_world.active_camera.map_or(
                ([0.0; 3], [0.0, 0.0, -1.0]),
                |camera| {
                    (
                        light_position(camera.transform.matrix),
                        light_direction(camera.transform.matrix),
                    )
                },
            );
            let mut blended = render_instances.blended.clone();
            sort_back_to_front(&mut blended, eye, forward);
            commands
                .bind_pipeline_graphics(self.blend_pipeline.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?;
            let mut bound_mesh = None;
            for item in blended {
                let Some(mesh) = self.prepared_meshes.get(&item.mesh_key)
                else {
                    continue;
                };
                if bound_mesh != Some(item.mesh_key) {
                    commands
                        .bind_vertex_buffers(0, mesh.vertices.clone())
                        .map_err(|error| SceneRenderError(error.to_string()))?
                        .bind_index_buffer(mesh.indices.clone())
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                    bound_mesh = Some(item.mesh_key);
                }
                unsafe {
                    commands
                        .draw_indexed(
                            mesh.indices.len() as u32,
                            1,
                            0,
                            0,
                            item.instance,
                        )
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                }
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
                let upload = self.frame_contexts[self.frame_index]
                    .transient
                    .allocate_slice::<DebugVertex>(vertices.len() as u64)
                    .map_err(|error| SceneRenderError(error.to_string()))?;
                upload
                    .write()
                    .map_err(|error| SceneRenderError(error.to_string()))?
                    .copy_from_slice(&vertices);
                let vertices = upload;
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
        if debug_labels_enabled {
            let _ = unsafe { commands.end_debug_utils_label() };
        }
        let command_buffer = commands
            .build()
            .map_err(|error| SceneRenderError(error.to_string()))?;
        let future = before
            .then_execute(self.queue.clone(), command_buffer)
            .map_err(|error| SceneRenderError(error.to_string()))?;
        // Vulkano implements GpuFuture for Arc<FenceSignalFuture>. This
        // renderer stays on the window thread and never sends it elsewhere.
        #[allow(clippy::arc_with_non_send_sync)]
        let fence = Arc::new(future.boxed().then_signal_fence());
        self.frame_contexts[self.frame_index].fence = Some(fence.clone());
        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        if physics_ran {
            let (_, event_header, event_buffer) = physics_resources.unwrap();
            self.pending_physics.push(PendingPhysicsReadback {
                fence: fence.clone(),
                header: event_header,
                events: event_buffer,
            });
        }
        Ok(fence.boxed())
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

    fn prepare_lights(
        &mut self,
        render_world: &RenderWorld,
    ) -> Result<(), SceneRenderError> {
        let budget = light_budget(resolve_quality(
            render_world.quality,
            &self.capabilities,
        ));
        if self.prepared_lights.as_ref().is_some_and(|prepared| {
            prepared.revision == render_world.lights_revision
                && prepared.budget == budget
        }) {
            return Ok(());
        }

        let mut uploads = Vec::with_capacity(budget);
        for extracted in &render_world.directional_lights {
            if uploads.len() == budget {
                break;
            }
            let direction = light_direction(extracted.transform.matrix);
            uploads.push(LightUpload {
                position_kind: [0.0, 0.0, 0.0, 0.0],
                direction_range: [
                    direction[0],
                    direction[1],
                    direction[2],
                    0.0,
                ],
                color_intensity: [
                    extracted.light.color[0],
                    extracted.light.color[1],
                    extracted.light.color[2],
                    extracted.light.illuminance / 100_000.0,
                ],
                spot_angles: [0.0; 4],
            });
        }
        for extracted in &render_world.point_lights {
            if uploads.len() == budget {
                break;
            }
            let position = light_position(extracted.transform.matrix);
            uploads.push(LightUpload {
                position_kind: [position[0], position[1], position[2], 1.0],
                direction_range: [
                    0.0,
                    0.0,
                    0.0,
                    extracted.light.range.max(0.01),
                ],
                color_intensity: [
                    extracted.light.color[0],
                    extracted.light.color[1],
                    extracted.light.color[2],
                    extracted.light.intensity / 1_000.0,
                ],
                spot_angles: [0.0; 4],
            });
        }
        for extracted in &render_world.spot_lights {
            if uploads.len() == budget {
                break;
            }
            let position = light_position(extracted.transform.matrix);
            let direction = light_direction(extracted.transform.matrix);
            uploads.push(LightUpload {
                position_kind: [position[0], position[1], position[2], 2.0],
                direction_range: [
                    direction[0],
                    direction[1],
                    direction[2],
                    extracted.light.range.max(0.01),
                ],
                color_intensity: [
                    extracted.light.color[0],
                    extracted.light.color[1],
                    extracted.light.color[2],
                    extracted.light.intensity / 1_000.0,
                ],
                spot_angles: [
                    extracted.light.inner_angle.cos(),
                    extracted.light.outer_angle.cos(),
                    0.0,
                    0.0,
                ],
            });
        }
        self.capacity.dropped_lights = render_world.directional_lights.len()
            + render_world.point_lights.len()
            + render_world.spot_lights.len()
            - uploads.len();
        let count = uploads.len() as u32;
        if uploads.is_empty() {
            uploads.push(LightUpload::default());
        }
        let buffer = Buffer::from_iter(
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
            uploads,
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        let ambient = render_world.ambient_light.map_or(
            [0.12, 0.12, 0.12, 1.0],
            |light| {
                [
                    light.color[0] * light.intensity,
                    light.color[1] * light.intensity,
                    light.color[2] * light.intensity,
                    1.0,
                ]
            },
        );
        self.prepared_lights = Some(PreparedLights {
            revision: render_world.lights_revision,
            budget,
            buffer,
            count,
            ambient,
        });
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

        let (order, batches, blended_start) =
            render_batch_order(&render_world.renderables, |material| {
                assets.materials.get(material).is_some_and(|material| {
                    material.alpha_mode == AlphaMode::Blend
                })
            });
        let mut instances = Vec::with_capacity(order.len().max(1));
        for &index in &order {
            let renderable = render_world.renderables[index];
            let material = assets.materials.get(renderable.material);
            let color = material
                .map_or([1.0, 0.0, 1.0, 1.0], |material| material.base_color);
            let (alpha_mode, cutoff) =
                match material.map(|material| material.alpha_mode) {
                    Some(AlphaMode::Mask { cutoff }) => (1, cutoff),
                    Some(AlphaMode::Blend) => (2, 0.0),
                    Some(AlphaMode::Opaque) | None => (0, 0.0),
                };
            instances.push(RenderInstanceUpload {
                model: renderable.transform.matrix,
                color,
                physics: [
                    physics_indices
                        .get(&renderable.entity)
                        .copied()
                        .unwrap_or(u32::MAX),
                    alpha_mode,
                    f32::to_bits(cutoff),
                    0,
                ],
            });
        }
        // ponytail: GPU-physics bodies sort by their last CPU transform;
        // read back positions if blended GPU bodies ever need exact order.
        let blended = order[blended_start..]
            .iter()
            .enumerate()
            .map(|(offset, &index)| {
                let renderable = render_world.renderables[index];
                BlendedInstance {
                    instance: (blended_start + offset) as u32,
                    mesh_key: renderable.mesh.key(),
                    position: light_position(renderable.transform.matrix),
                }
            })
            .collect();
        if instances.is_empty() {
            instances.push(RenderInstanceUpload::default());
        }
        let bytes = std::mem::size_of_val(instances.as_slice()) as DeviceSize;
        if bytes > self.instance_budget {
            return Err(SceneRenderError(format!(
                "{} render instances need {bytes} bytes, over the {} byte \
                 device-local upload budget",
                instances.len(),
                self.instance_budget
            )));
        }
        let upload = self
            .instance_allocator
            .allocate_slice::<RenderInstanceUpload>(
                instances.len() as DeviceSize
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
        upload
            .write()
            .map_err(|error| SceneRenderError(error.to_string()))?
            .copy_from_slice(&instances);
        let instances = upload;
        self.prepared_instances = Some(PreparedRenderInstances {
            renderables_revision: render_world.renderables_revision,
            physics_revision: render_world.gpu_physics_revision,
            material_revisions,
            instances,
            batches,
            blended,
        });
        Ok(())
    }

    /// Optional GPU features detected when the renderer was created.
    pub fn capabilities(&self) -> &RendererCapabilities {
        &self.capabilities
    }

    /// Capacity fallbacks taken so far; see [`RenderCapacityDiagnostics`].
    pub fn capacity_diagnostics(&self) -> RenderCapacityDiagnostics {
        self.capacity
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
                self.capacity.physics_events_dropped +=
                    u64::from(header.overflow);
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
/// Blended objects go last and are not batched; the returned index is where
/// they start in the order.
fn render_batch_order(
    renderables: &[crate::runtime::ExtractedRenderable],
    is_blended: impl Fn(Handle<MaterialAsset>) -> bool,
) -> (Vec<usize>, Vec<PreparedRenderBatch>, usize) {
    let blended = renderables
        .iter()
        .map(|renderable| is_blended(renderable.material))
        .collect::<Vec<_>>();
    let mut order = (0..renderables.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| {
        let renderable = &renderables[*index];
        (
            blended[*index],
            renderable.mesh.key(),
            renderable.material.key(),
        )
    });
    let blended_start = order.partition_point(|index| !blended[*index]);
    let mut batches = Vec::<PreparedRenderBatch>::new();
    let mut previous_key = None;
    for (instance, index) in order[..blended_start].iter().copied().enumerate()
    {
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
    (order, batches, blended_start)
}

/// Orders blended instances farthest-first along the camera view direction.
fn sort_back_to_front(
    instances: &mut [BlendedInstance],
    eye: [f32; 3],
    forward: [f32; 3],
) {
    let depth = |instance: &BlendedInstance| {
        (0..3)
            .map(|axis| (instance.position[axis] - eye[axis]) * forward[axis])
            .sum::<f32>()
    };
    instances.sort_by(|a, b| depth(b).total_cmp(&depth(a)));
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

/// Creates the opaque/mask pipeline and the alpha-blend pipeline. Both share
/// one layout so descriptor sets and push constants stay bound between them.
fn create_pipelines(
    queue: Arc<Queue>,
    render_pass: Arc<RenderPass>,
) -> Result<(Arc<GraphicsPipeline>, Arc<GraphicsPipeline>), SceneRenderError> {
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
    let create = |blend: bool| {
        GraphicsPipeline::new(
            queue.device().clone(),
            None,
            GraphicsPipelineCreateInfo {
                stages: stages.iter().cloned().collect(),
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
                // Blended surfaces test against opaque depth but do not write
                // it, so farther blended surfaces drawn later still show.
                depth_stencil_state: Some(DepthStencilState {
                    depth: Some(DepthState {
                        write_enable: !blend,
                        ..DepthState::simple()
                    }),
                    ..Default::default()
                }),
                color_blend_state: Some(
                    ColorBlendState::with_attachment_states(
                        1,
                        ColorBlendAttachmentState {
                            blend: blend.then(AttachmentBlend::alpha),
                            ..Default::default()
                        },
                    ),
                ),
                dynamic_state: [DynamicState::Viewport].into_iter().collect(),
                subpass: Some(PipelineSubpassType::BeginRenderPass(
                    subpass.clone(),
                )),
                ..GraphicsPipelineCreateInfo::layout(layout.clone())
            },
        )
        .map_err(|error| SceneRenderError(error.to_string()))
    };
    Ok((create(false)?, create(true)?))
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

fn light_position(matrix: [[f32; 4]; 4]) -> [f32; 3] {
    let matrix = matrix_from_array(matrix);
    [matrix[(0, 3)], matrix[(1, 3)], matrix[(2, 3)]]
}

/// Starting arena size for instance uploads (about 2,700 instances).
const INSTANCE_ARENA_BYTES: DeviceSize = 256 * 1024;
/// First arena size of each frame context's transient allocator.
const TRANSIENT_ARENA_BYTES: DeviceSize = 64 * 1024;

/// Device properties the renderer depends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceLimits {
    pub api_version: vulkano::Version,
    pub max_compute_work_group_invocations: u32,
    pub max_compute_work_group_size_x: u32,
    pub max_push_constants_size: u32,
    pub max_storage_buffer_range: u32,
    /// `D32_SFLOAT` usable as an optimal-tiling depth attachment.
    pub depth_attachment: bool,
}

/// Lowest device `SceneRenderer` accepts, sized for Intel UHD 620-class
/// integrated GPUs. Every value is the Vulkan-required minimum except
/// compute invocations, which the physics shader's `local_size_x = 256`
/// needs, and the version (1.1, exposed by every current Intel, AMD, NVIDIA
/// and Mesa driver for that hardware generation).
pub const LOW_END_BASELINE: DeviceLimits = DeviceLimits {
    api_version: vulkano::Version::V1_1,
    max_compute_work_group_invocations: 256,
    max_compute_work_group_size_x: 256,
    max_push_constants_size: 128,
    max_storage_buffer_range: 1 << 27,
    depth_attachment: true,
};

impl DeviceLimits {
    pub fn of(device: &vulkano::device::physical::PhysicalDevice) -> Self {
        let properties = device.properties();
        Self {
            api_version: properties.api_version,
            max_compute_work_group_invocations: properties
                .max_compute_work_group_invocations,
            max_compute_work_group_size_x: properties
                .max_compute_work_group_size[0],
            max_push_constants_size: properties.max_push_constants_size,
            max_storage_buffer_range: properties.max_storage_buffer_range,
            depth_attachment: device
                .format_properties(Format::D32_SFLOAT)
                .is_ok_and(|format| {
                    format.optimal_tiling_features.intersects(
                        vulkano::format::FormatFeatures::DEPTH_STENCIL_ATTACHMENT,
                    )
                }),
        }
    }

    /// Names every property below `baseline`; empty when the device meets it.
    pub fn shortfalls(&self, baseline: &DeviceLimits) -> Vec<String> {
        let mut missing = Vec::new();
        if self.api_version < baseline.api_version {
            missing.push(format!(
                "Vulkan {} < {}",
                self.api_version, baseline.api_version
            ));
        }
        for (name, have, need) in [
            (
                "maxComputeWorkGroupInvocations",
                self.max_compute_work_group_invocations,
                baseline.max_compute_work_group_invocations,
            ),
            (
                "maxComputeWorkGroupSize[0]",
                self.max_compute_work_group_size_x,
                baseline.max_compute_work_group_size_x,
            ),
            (
                "maxPushConstantsSize",
                self.max_push_constants_size,
                baseline.max_push_constants_size,
            ),
            (
                "maxStorageBufferRange",
                self.max_storage_buffer_range,
                baseline.max_storage_buffer_range,
            ),
        ] {
            if have < need {
                missing.push(format!("{name} {have} < {need}"));
            }
        }
        if baseline.depth_attachment && !self.depth_attachment {
            missing.push("D32_SFLOAT depth attachment unsupported".into());
        }
        missing
    }
}

/// One optional device capability. `supported` is what the GPU offers;
/// `enabled` is what the logical device turned on. Passes may only use it
/// when both hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capability {
    pub supported: bool,
    pub enabled: bool,
}

impl Capability {
    pub fn usable(self) -> bool {
        self.supported && self.enabled
    }
}

/// Optional GPU features above [`LOW_END_BASELINE`] that later passes can
/// use, with the baseline path as their fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererCapabilities {
    pub device_name: String,
    pub integrated_gpu: bool,
    /// Size of the largest device-local memory heap.
    pub device_local_bytes: DeviceSize,
    pub multi_draw_indirect: Capability,
    /// `vkCmdDrawIndexedIndirectCount`, core in 1.2 or via the KHR extension.
    pub draw_indirect_count: Capability,
    /// Descriptor indexing features needed for a bindless texture array.
    pub bindless_textures: Capability,
    /// `VK_EXT_memory_budget` for live memory usage.
    pub memory_budget: Capability,
    /// GPU timestamps on graphics and compute queues; needs no enabling.
    pub timestamp_queries: bool,
}

/// Which optional features a feature and extension set provides, in the
/// order of the [`RendererCapabilities`] fields.
fn optional_features(
    features: &vulkano::device::DeviceFeatures,
    extensions: &DeviceExtensions,
) -> [bool; 4] {
    [
        features.multi_draw_indirect,
        features.draw_indirect_count || extensions.khr_draw_indirect_count,
        features.runtime_descriptor_array
            && features.descriptor_binding_partially_bound
            && features.shader_sampled_image_array_non_uniform_indexing
            && features.descriptor_binding_variable_descriptor_count,
        extensions.ext_memory_budget,
    ]
}

impl RendererCapabilities {
    pub fn detect(device: &vulkano::device::Device) -> Self {
        let physical = device.physical_device();
        let supported = optional_features(
            physical.supported_features(),
            physical.supported_extensions(),
        );
        let enabled = optional_features(
            device.enabled_features(),
            device.enabled_extensions(),
        );
        let [multi_draw_indirect, draw_indirect_count, bindless_textures, memory_budget] =
            std::array::from_fn(|index| Capability {
                supported: supported[index],
                enabled: enabled[index],
            });
        let properties = physical.properties();
        Self {
            device_name: properties.device_name.clone(),
            integrated_gpu: properties.device_type
                == vulkano::device::physical::PhysicalDeviceType::IntegratedGpu,
            device_local_bytes: physical
                .memory_properties()
                .memory_heaps
                .iter()
                .filter(|heap| {
                    heap.flags.intersects(MemoryHeapFlags::DEVICE_LOCAL)
                })
                .map(|heap| heap.size)
                .max()
                .unwrap_or(0),
            multi_draw_indirect,
            draw_indirect_count,
            bindless_textures,
            memory_budget,
            timestamp_queries: properties.timestamp_compute_and_graphics,
        }
    }
}

/// Turns a requested profile into a concrete one. `Auto` picks `Eco` on
/// integrated GPUs, `Balanced` below 4 GiB of device-local memory, and
/// `High` otherwise.
// ponytail: static device heuristic; switch to measured frame time once
// frame pacing is profiled on real hardware.
pub fn resolve_quality(
    requested: QualityProfile,
    capabilities: &RendererCapabilities,
) -> QualityProfile {
    match requested {
        QualityProfile::Auto if capabilities.integrated_gpu => {
            QualityProfile::Eco
        }
        QualityProfile::Auto if capabilities.device_local_bytes < 4 << 30 => {
            QualityProfile::Balanced
        }
        QualityProfile::Auto => QualityProfile::High,
        concrete => concrete,
    }
}

/// Lights uploaded per frame for a resolved profile. Lights past the budget
/// are dropped and reported in [`RenderCapacityDiagnostics`].
pub fn light_budget(profile: QualityProfile) -> usize {
    match profile {
        QualityProfile::Eco => MAX_LIGHTS / 4,
        QualityProfile::Balanced => MAX_LIGHTS / 2,
        QualityProfile::High | QualityProfile::Auto => MAX_LIGHTS,
    }
}

/// Caps one frame's transient uploads at half the largest device-local heap,
/// leaving the rest for meshes, images, and other applications.
// ponytail: static heap size, not live usage; query VK_EXT_memory_budget if
// scenes start sharing the GPU with other heavy workloads.
fn transient_upload_budget(
    heaps: impl IntoIterator<Item = (DeviceSize, MemoryHeapFlags)>,
) -> DeviceSize {
    heaps
        .into_iter()
        .filter(|(_, flags)| flags.intersects(MemoryHeapFlags::DEVICE_LOCAL))
        .map(|(size, _)| size / 2)
        .max()
        .unwrap_or(DeviceSize::MAX)
}

fn light_direction(matrix: [[f32; 4]; 4]) -> [f32; 3] {
    let direction =
        matrix_from_array(matrix) * Vector4::new(0.0, 0.0, -1.0, 0.0);
    let length = (direction.x * direction.x
        + direction.y * direction.y
        + direction.z * direction.z)
        .sqrt();
    if length > f32::EPSILON {
        [
            direction.x / length,
            direction.y / length,
            direction.z / length,
        ]
    } else {
        [0.0, 0.0, -1.0]
    }
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
    vec4 ambient;
    uvec4 light_info;
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
layout(location = 2) out vec3 v_world_position;
layout(location = 3) flat out uvec2 v_alpha;
void main() {
    RenderInstance instance = render_instances.data[gl_InstanceIndex];
    mat4 model = instance.physics.x == 0xffffffffu
        ? instance.model
        : physics_states.data[instance.physics.x].model;
    vec4 world_position = model * vec4(position, 1.0);
    gl_Position = camera.view_projection * world_position;
    mat3 normal_matrix = transpose(inverse(mat3(model)));
    v_normal = normal_matrix * normal;
    v_color = instance.color;
    v_world_position = world_position.xyz;
    v_alpha = instance.physics.yz;
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
layout(location = 2) in vec3 v_world_position;
layout(location = 3) flat in uvec2 v_alpha;
layout(location = 0) out vec4 f_color;
layout(push_constant) uniform Camera {
    mat4 view_projection;
    vec4 ambient;
    uvec4 light_info;
} camera;
struct Light {
    vec4 position_kind;
    vec4 direction_range;
    vec4 color_intensity;
    vec4 spot_angles;
};
layout(set = 0, binding = 2) readonly buffer Lights {
    Light data[];
} lights;
void main() {
    if (v_alpha.x == 1u && v_color.a < uintBitsToFloat(v_alpha.y)) {
        discard;
    }
    vec3 normal = normalize(v_normal);
    vec3 result = v_color.rgb * camera.ambient.rgb;
    for (uint index = 0; index < camera.light_info.x; ++index) {
        Light light = lights.data[index];
        float kind = light.position_kind.w;
        vec3 to_light;
        float attenuation = 1.0;
        if (kind < 0.5) {
            to_light = normalize(-light.direction_range.xyz);
        } else {
            vec3 delta = light.position_kind.xyz - v_world_position;
            float distance_to_light = length(delta);
            to_light = distance_to_light > 0.0001
                ? delta / distance_to_light
                : vec3(0.0, 1.0, 0.0);
            float range_fade = clamp(
                1.0 - distance_to_light / light.direction_range.w,
                0.0,
                1.0
            );
            attenuation = range_fade * range_fade;
            if (kind > 1.5) {
                float cone = dot(
                    -to_light,
                    normalize(light.direction_range.xyz)
                );
                attenuation *= smoothstep(
                    light.spot_angles.y,
                    light.spot_angles.x,
                    cone
                );
            }
        }
        float diffuse = max(dot(normal, to_light), 0.0);
        vec3 radiance = light.color_intensity.rgb
            * light.color_intensity.w * attenuation;
        result += v_color.rgb * radiance * diffuse;
    }
    f_color = vec4(result, v_alpha.x == 2u ? v_color.a : 1.0);
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

    /// These compare our hand-written CPU upload structs against the
    /// `vulkano_shaders`-generated types for the same GLSL struct/block,
    /// reflected straight from the compiled SPIR-V, instead of hardcoded
    /// magic-number offsets that can drift silently from the shader source.
    #[test]
    fn render_instance_layout_matches_shader_struct() {
        use std::mem::{offset_of, size_of};

        type Reflected = super::vertex_shader::RenderInstance;
        assert_eq!(size_of::<RenderInstanceUpload>(), size_of::<Reflected>());
        assert_eq!(
            offset_of!(RenderInstanceUpload, color),
            offset_of!(Reflected, color)
        );
        assert_eq!(
            offset_of!(RenderInstanceUpload, physics),
            offset_of!(Reflected, physics)
        );
    }

    #[test]
    fn light_gpu_layouts_match_shader_structs() {
        use std::mem::{offset_of, size_of};

        type ReflectedCamera = super::vertex_shader::Camera;
        assert_eq!(size_of::<CameraUniform>(), size_of::<ReflectedCamera>());
        assert_eq!(
            offset_of!(CameraUniform, ambient),
            offset_of!(ReflectedCamera, ambient)
        );
        assert_eq!(
            offset_of!(CameraUniform, light_info),
            offset_of!(ReflectedCamera, light_info)
        );

        type ReflectedLight = super::fragment_shader::Light;
        assert_eq!(size_of::<LightUpload>(), size_of::<ReflectedLight>());
        assert_eq!(
            offset_of!(LightUpload, direction_range),
            offset_of!(ReflectedLight, direction_range)
        );
        assert_eq!(
            offset_of!(LightUpload, color_intensity),
            offset_of!(ReflectedLight, color_intensity)
        );
        assert_eq!(
            offset_of!(LightUpload, spot_angles),
            offset_of!(ReflectedLight, spot_angles)
        );
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

        let (order, batches, blended_start) =
            render_batch_order(&renderables, |_| false);

        assert_eq!(order.len(), 10_000);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].first_instance, 0);
        assert_eq!(batches[0].instance_count, 10_000);
        assert_eq!(blended_start, 10_000);
    }

    #[test]
    fn blended_objects_render_last_unbatched_and_back_to_front() {
        let mut assets = AssetServer::default();
        let glass = assets.materials.insert(MaterialAsset {
            alpha_mode: AlphaMode::Blend,
            ..MaterialAsset::default()
        });
        let renderable = |index: u32, material, z: f32| {
            crate::runtime::ExtractedRenderable {
                entity: bevy_ecs::entity::Entity::from_raw_u32(index).unwrap(),
                transform: crate::runtime::GlobalTransform {
                    matrix: Matrix4::new_translation(&nalgebra::Vector3::new(
                        0.0, 0.0, z,
                    ))
                    .into(),
                },
                mesh: assets.fallback_mesh,
                material,
                cast_shadows: true,
                receive_shadows: true,
            }
        };
        let renderables = [
            renderable(1, glass, -1.0),
            renderable(2, assets.fallback_material, 0.0),
            renderable(3, glass, -5.0),
            renderable(4, assets.fallback_material, 0.0),
        ];

        let (order, batches, blended_start) =
            render_batch_order(&renderables, |material| material == glass);

        assert_eq!(blended_start, 2);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].instance_count, 2);
        assert!(order[..2].iter().all(|index| [1, 3].contains(index)));
        let mut blended = order[2..]
            .iter()
            .enumerate()
            .map(|(offset, &index)| BlendedInstance {
                instance: 2 + offset as u32,
                mesh_key: 0,
                position: light_position(renderables[index].transform.matrix),
            })
            .collect::<Vec<_>>();
        sort_back_to_front(&mut blended, [0.0; 3], [0.0, 0.0, -1.0]);
        assert_eq!(blended[0].position[2], -5.0, "farthest drawn first");
        assert_eq!(blended[1].position[2], -1.0);
    }

    /// Full-screen slabs at the given depths seen by an orthographic camera
    /// at z = 5, rendered into an 8x8 offscreen image.
    struct SlabScene {
        base: &'static crate::rendering::HeadlessVulkanBase,
        memory_allocator: Arc<StandardMemoryAllocator>,
        renderer: SceneRenderer,
        assets: AssetServer,
        render_world: RenderWorld,
        image: Arc<Image>,
        extent: [u32; 2],
    }

    impl SlabScene {
        fn new(slabs: &[(f32, MaterialAsset)]) -> Self {
            use crate::rendering::swapchain::OFFSCREEN_COLOR_FORMAT;
            let base = crate::rendering::test_support::headless_device();
            let memory_allocator = Arc::new(
                StandardMemoryAllocator::new_default(base.device.clone()),
            );
            let extent = [8, 8];
            let renderer = SceneRenderer::new(
                base.queue.clone(),
                memory_allocator.clone(),
                OFFSCREEN_COLOR_FORMAT,
                extent,
            )
            .unwrap();
            let mut assets = AssetServer::default();
            let mut render_world = RenderWorld::default();
            render_world.ambient_light = Some(crate::runtime::AmbientLight {
                color: [1.0; 3],
                intensity: 1.0,
            });
            render_world.background_color = [0.0, 0.0, 0.0, 1.0];
            // Revision 0 means "nothing extracted yet" to the renderer caches.
            render_world.renderables_revision = 1;
            render_world.lights_revision = 1;
            render_world.active_camera =
                Some(crate::runtime::ExtractedCamera {
                    entity: bevy_ecs::entity::Entity::from_raw_u32(1000)
                        .unwrap(),
                    transform: crate::runtime::GlobalTransform {
                        matrix: Matrix4::new_translation(
                            &nalgebra::Vector3::new(0.0, 0.0, 5.0),
                        )
                        .into(),
                    },
                    projection: Projection::Orthographic {
                        vertical_size: 2.0,
                        near: 0.1,
                        far: 100.0,
                    },
                    priority: 0,
                });
            for (index, (z, material)) in slabs.iter().enumerate() {
                let material = assets.materials.insert(material.clone());
                render_world.renderables.push(
                    crate::runtime::ExtractedRenderable {
                        entity: bevy_ecs::entity::Entity::from_raw_u32(
                            index as u32 + 1,
                        )
                        .unwrap(),
                        transform: crate::runtime::GlobalTransform {
                            matrix: (Matrix4::new_translation(
                                &nalgebra::Vector3::new(0.0, 0.0, *z),
                            ) * Matrix4::new_nonuniform_scaling(
                                &nalgebra::Vector3::new(4.0, 4.0, 0.1),
                            ))
                            .into(),
                        },
                        mesh: assets.fallback_mesh,
                        material,
                        cast_shadows: false,
                        receive_shadows: false,
                    },
                );
            }
            let image = Image::new(
                memory_allocator.clone(),
                ImageCreateInfo {
                    format: OFFSCREEN_COLOR_FORMAT,
                    extent: [extent[0], extent[1], 1],
                    usage: ImageUsage::COLOR_ATTACHMENT
                        | ImageUsage::TRANSFER_SRC,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                    ..Default::default()
                },
            )
            .unwrap();
            Self {
                base,
                memory_allocator,
                renderer,
                assets,
                render_world,
                image,
                extent,
            }
        }

        fn render(&mut self, before: Box<dyn GpuFuture>) -> Box<dyn GpuFuture> {
            self.renderer
                .render(
                    before,
                    ImageView::new_default(self.image.clone()).unwrap(),
                    self.extent,
                    SceneRenderOptions::game(self.extent),
                    &self.render_world,
                    &self.assets,
                )
                .unwrap()
        }

        fn now(&self) -> Box<dyn GpuFuture> {
            vulkano::sync::now(self.base.device.clone()).boxed()
        }

        /// Returns the center pixel as `[b, g, r, a]`.
        fn center_pixel(&self) -> [u8; 4] {
            let extent = self.extent;
            let pixels = crate::rendering::readback::read_back_image(
                &self.base.device,
                &self.base.queue,
                &self.memory_allocator,
                &Arc::new(StandardCommandBufferAllocator::new(
                    self.base.device.clone(),
                    Default::default(),
                )),
                &self.image,
            );
            let center =
                ((extent[1] / 2 * extent[0] + extent[0] / 2) * 4) as usize;
            pixels[center..center + 4].try_into().unwrap()
        }
    }

    fn render_center_pixel(slabs: &[(f32, MaterialAsset)]) -> [u8; 4] {
        let mut scene = SlabScene::new(slabs);
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        scene.center_pixel()
    }

    #[test]
    fn optional_features_need_every_bindless_bit_and_accept_either_indirect_count_source(
    ) {
        use vulkano::device::DeviceFeatures;
        let none = DeviceExtensions::empty();
        assert_eq!(
            optional_features(&DeviceFeatures::empty(), &none),
            [false; 4]
        );
        let partial_bindless = DeviceFeatures {
            runtime_descriptor_array: true,
            descriptor_binding_partially_bound: true,
            shader_sampled_image_array_non_uniform_indexing: true,
            ..DeviceFeatures::empty()
        };
        assert!(!optional_features(&partial_bindless, &none)[2]);
        let bindless = DeviceFeatures {
            descriptor_binding_variable_descriptor_count: true,
            ..partial_bindless
        };
        assert!(optional_features(&bindless, &none)[2]);
        let khr_count = DeviceExtensions {
            khr_draw_indirect_count: true,
            ext_memory_budget: true,
            ..DeviceExtensions::empty()
        };
        assert_eq!(
            optional_features(&DeviceFeatures::empty(), &khr_count),
            [false, true, false, true]
        );
        let core_count = DeviceFeatures {
            draw_indirect_count: true,
            multi_draw_indirect: true,
            ..DeviceFeatures::empty()
        };
        assert_eq!(
            optional_features(&core_count, &none),
            [true, true, false, false]
        );
        assert!(!Capability {
            supported: true,
            enabled: false
        }
        .usable());
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn renderer_reports_detected_capabilities_and_enables_none_it_does_not_use()
    {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let scene = SlabScene::new(&[]);
        let capabilities = scene.renderer.capabilities();
        let physical = scene.base.device.physical_device();
        assert_eq!(capabilities.device_name, physical.properties().device_name);
        assert!(capabilities.device_local_bytes > 0);
        assert_eq!(
            capabilities.multi_draw_indirect.supported,
            physical.supported_features().multi_draw_indirect
        );
        // Nothing uses these yet, so the device must not pay for them.
        for capability in [
            capabilities.multi_draw_indirect,
            capabilities.draw_indirect_count,
            capabilities.bindless_textures,
        ] {
            assert!(!capability.enabled, "{capabilities:?}");
        }
        eprintln!("{capabilities:#?}");
    }

    #[test]
    fn auto_quality_resolves_from_device_class_and_concrete_profiles_pass_through(
    ) {
        let capabilities =
            |integrated_gpu, gib: DeviceSize| RendererCapabilities {
                device_name: String::new(),
                integrated_gpu,
                device_local_bytes: gib << 30,
                multi_draw_indirect: Capability::default(),
                draw_indirect_count: Capability::default(),
                bindless_textures: Capability::default(),
                memory_budget: Capability::default(),
                timestamp_queries: false,
            };
        let auto = |caps| resolve_quality(QualityProfile::Auto, &caps);
        assert_eq!(auto(capabilities(true, 16)), QualityProfile::Eco);
        assert_eq!(auto(capabilities(false, 2)), QualityProfile::Balanced);
        assert_eq!(auto(capabilities(false, 12)), QualityProfile::High);
        assert_eq!(
            resolve_quality(QualityProfile::High, &capabilities(true, 1)),
            QualityProfile::High
        );
        let budgets = [
            QualityProfile::Eco,
            QualityProfile::Balanced,
            QualityProfile::High,
        ]
        .map(light_budget);
        assert!(budgets.is_sorted() && budgets[2] == MAX_LIGHTS);
    }

    #[test]
    fn baseline_shortfalls_name_every_missing_property() {
        assert!(LOW_END_BASELINE.shortfalls(&LOW_END_BASELINE).is_empty());
        let weak = DeviceLimits {
            api_version: vulkano::Version::V1_0,
            max_compute_work_group_invocations: 128,
            max_compute_work_group_size_x: 128,
            depth_attachment: false,
            ..LOW_END_BASELINE
        };
        let shortfalls = weak.shortfalls(&LOW_END_BASELINE);
        assert_eq!(shortfalls.len(), 4, "{shortfalls:?}");
        assert!(shortfalls[0].starts_with("Vulkan 1.0"));
        assert_eq!(shortfalls[1], "maxComputeWorkGroupInvocations 128 < 256");
        assert!(shortfalls[3].contains("D32_SFLOAT"));
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn test_device_meets_the_low_end_baseline() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let base = crate::rendering::test_support::headless_device();
        let limits = DeviceLimits::of(base.device.physical_device());
        assert_eq!(limits.shortfalls(&LOW_END_BASELINE), Vec::<String>::new());
    }

    #[test]
    fn transient_budget_is_half_the_largest_device_local_heap() {
        assert_eq!(
            transient_upload_budget([
                (64 << 30, MemoryHeapFlags::empty()),
                (12 << 30, MemoryHeapFlags::DEVICE_LOCAL),
                (256 << 20, MemoryHeapFlags::DEVICE_LOCAL),
            ]),
            6 << 30
        );
        assert_eq!(transient_upload_budget([]), DeviceSize::MAX);
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn instance_uploads_reuse_arenas_grow_on_demand_and_respect_budget() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        let frame = |scene: &mut SlabScene| {
            scene.render_world.renderables_revision += 1;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let instances =
                &scene.renderer.prepared_instances.as_ref().unwrap();
            let buffer = instances.instances.buffer().clone();
            buffer
        };
        let first = frame(&mut scene);
        let second = frame(&mut scene);
        assert!(
            Arc::ptr_eq(&first, &second),
            "small re-uploads suballocate the same arena, not a new buffer"
        );

        let template = scene.render_world.renderables[0];
        scene.render_world.renderables = (0..5_000u32)
            .map(|index| crate::runtime::ExtractedRenderable {
                entity: bevy_ecs::entity::Entity::from_raw_u32(index + 1)
                    .unwrap(),
                ..template
            })
            .collect();
        let grown = frame(&mut scene);
        let needed =
            5_000 * std::mem::size_of::<RenderInstanceUpload>() as DeviceSize;
        assert!(!Arc::ptr_eq(&first, &grown));
        assert!(grown.size() >= needed, "arena grew to fit the upload");
        let [b, g, r, _] = scene.center_pixel();
        assert_eq!([b, g, r], [255, 255, 255]);

        scene.renderer.instance_budget = needed - 1;
        scene.render_world.renderables_revision += 1;
        let before = scene.now();
        let error = scene
            .renderer
            .render(
                before,
                ImageView::new_default(scene.image.clone()).unwrap(),
                scene.extent,
                SceneRenderOptions::game(scene.extent),
                &scene.render_world,
                &scene.assets,
            )
            .err()
            .expect("over-budget upload is an explicit error");
        assert!(error.0.contains("budget"), "{error:?}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn frame_contexts_bound_frames_in_flight_and_wait_only_on_reuse() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        // Frames are handed back unflushed, as a caller may hold them before
        // presenting, so their fences stay unsignaled until someone flushes.
        let mut held = Vec::new();
        let mut fences = Vec::new();
        for slot in 0..FRAMES_IN_FLIGHT {
            let before = scene.now();
            held.push(scene.render(before));
            fences.push(
                scene.renderer.frame_contexts[slot].fence.clone().unwrap(),
            );
        }
        assert_eq!(scene.renderer.frame_index, 0, "ring wrapped");
        assert!(
            fences.iter().all(|f| !f.is_signaled().unwrap()),
            "unfinished frames keep their fences"
        );

        let before = scene.now();
        let next = scene.render(before);
        assert!(
            fences[0].is_signaled().unwrap(),
            "reusing context 0 first submitted and waited for its frame"
        );
        assert!(
            !fences[1].is_signaled().unwrap(),
            "contexts that are not reused are never waited on"
        );
        assert!(!Arc::ptr_eq(
            scene.renderer.frame_contexts[0].fence.as_ref().unwrap(),
            &fences[0]
        ));
        next.then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        drop(held);
        let [b, g, r, _] = scene.center_pixel();
        assert_eq!([b, g, r], [255, 255, 255]);
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn transient_uploads_come_from_per_context_arenas_across_reuse() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[]);
        let mut overlay = RenderDebugOverlay::default();
        overlay.lines.push(DebugLine {
            start: [-1.0, 0.0, 0.0],
            end: [1.0, 0.0, 0.0],
            color: [1.0, 0.0, 0.0, 1.0],
            thickness: 4.0,
            on_top: true,
        });
        // Two full laps, so every context's arena is reused after its wait.
        for frame in 0..FRAMES_IN_FLIGHT * 2 {
            let before = scene.now();
            scene
                .renderer
                .render(
                    before,
                    ImageView::new_default(scene.image.clone()).unwrap(),
                    scene.extent,
                    SceneRenderOptions {
                        debug_overlay: Some(&overlay),
                        ..SceneRenderOptions::game(scene.extent)
                    },
                    &scene.render_world,
                    &scene.assets,
                )
                .unwrap()
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let [b, g, r, _] = scene.center_pixel();
            assert_eq!([b, g, r], [0, 0, 255], "debug line in frame {frame}");
        }
        let arena = |context: &FrameContext| {
            context
                .transient
                .allocate_sized::<u32>()
                .unwrap()
                .buffer()
                .clone()
        };
        assert!(
            !Arc::ptr_eq(
                &arena(&scene.renderer.frame_contexts[0]),
                &arena(&scene.renderer.frame_contexts[1])
            ),
            "contexts never share an arena"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn physics_events_read_back_from_transient_arenas_every_tick() {
        use crate::runtime::{
            ExtractedGpuPhysicsBody, ExtractedGpuPhysicsRule, GpuCondition,
            GpuEventId, GpuEventMode, GpuEventPayload,
        };
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[]);
        let world = &mut scene.render_world;
        world.gpu_physics = vec![ExtractedGpuPhysicsBody {
            entity: bevy_ecs::entity::Entity::from_raw_u32(3000).unwrap(),
            physics_id: Default::default(),
            transform: crate::Transform::new([0.0, 5.0, 0.0]),
            rigid_body: Default::default(),
            solver: Default::default(),
            rules: vec![ExtractedGpuPhysicsRule {
                event_id: GpuEventId(7),
                instructions: GpuCondition::position_y()
                    .greater_than(0.0)
                    .compile()
                    .unwrap(),
                mode: GpuEventMode::WhileTrue,
                payload: GpuEventPayload::None,
                cooldown_seconds: 0.0,
            }],
        }];
        world.gpu_physics_revision = 1;
        world.physics_enabled = true;
        world.fixed_delta_seconds = 1.0 / 60.0;
        // More ticks than contexts, so readback arenas are reused.
        for tick in 1..=(FRAMES_IN_FLIGHT as u64 * 2) {
            scene.render_world.physics_tick = tick;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let events = scene.renderer.take_completed_physics_events();
            assert_eq!(events.len(), 1, "tick {tick}: {events:?}");
            assert_eq!(events[0].event_id, 7);
            assert_eq!(events[0].tick_low, tick as u32);
        }
        assert_eq!(
            scene.renderer.capacity_diagnostics().physics_events_dropped,
            0
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn resize_defers_old_depth_destruction_until_its_frame_completes() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        let before = scene.now();
        let frame_one = scene.render(before);
        let old_depth = Arc::downgrade(&scene.renderer.depth);
        // A resize replaces the depth target while frame 1 is not submitted.
        scene.renderer.ensure_depth([16, 16]).unwrap();
        assert!(
            old_depth.upgrade().is_some(),
            "frame 1 still owns the replaced depth target"
        );
        frame_one
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        assert!(
            old_depth.upgrade().is_none(),
            "released once frame 1 completed and its fence was dropped"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn lights_over_capacity_render_first_max_lights_and_report_the_rest() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        let light = crate::runtime::ExtractedPointLight {
            entity: bevy_ecs::entity::Entity::from_raw_u32(2000).unwrap(),
            transform: crate::runtime::GlobalTransform::default(),
            light: crate::runtime::PointLight::default(),
        };
        scene.render_world.quality = QualityProfile::High;
        scene.render_world.point_lights = vec![light; MAX_LIGHTS + 3];
        scene.render_world.lights_revision += 1;
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        assert_eq!(scene.renderer.capacity_diagnostics().dropped_lights, 3);
        assert_eq!(
            scene.renderer.prepared_lights.as_ref().unwrap().count,
            MAX_LIGHTS as u32
        );

        // A profile change alone must rebuild the light list.
        scene.render_world.quality = QualityProfile::Eco;
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        let eco = light_budget(QualityProfile::Eco);
        assert_eq!(
            scene.renderer.capacity_diagnostics().dropped_lights,
            MAX_LIGHTS + 3 - eco
        );
        assert_eq!(
            scene.renderer.prepared_lights.as_ref().unwrap().count,
            eco as u32
        );

        scene.render_world.point_lights.truncate(1);
        scene.render_world.lights_revision += 1;
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        assert_eq!(scene.renderer.capacity_diagnostics().dropped_lights, 0);
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn hot_reloaded_mesh_swaps_next_frame_and_old_buffers_outlive_in_flight_frame(
    ) {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        let cube = scene.assets.meshes.get(scene.assets.fallback_mesh).cloned();
        let mesh = scene.assets.meshes.insert(cube.unwrap());
        scene.render_world.renderables[0].mesh = mesh;

        // Frame 1 is submitted but deliberately not waited on.
        let before = scene.now();
        #[allow(clippy::arc_with_non_send_sync)]
        let frame_one = Arc::new(
            scene.render(before).then_signal_fence_and_flush().unwrap(),
        );
        let old_vertices = Arc::downgrade(
            scene.renderer.prepared_meshes[&mesh.key()]
                .vertices
                .buffer(),
        );

        // Hot reload between frames: shrink the slab out of the center pixel.
        for vertex in &mut scene.assets.meshes.get_mut(mesh).unwrap().vertices {
            vertex.position = vertex.position.map(|value| value * 0.01);
        }
        let frame_two = scene.render(frame_one.clone().boxed());
        assert!(
            old_vertices.upgrade().is_some(),
            "replaced buffers stay alive while frame 1 may still read them"
        );
        frame_two
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        let [b, g, r, _] = scene.center_pixel();
        assert_eq!([b, g, r], [0, 0, 0], "frame 2 draws the reloaded mesh");
        drop(frame_one);
        // The renderer drops completed frame fences on its next frame.
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        assert!(
            old_vertices.upgrade().is_none(),
            "old buffers are freed once every referencing frame completed"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn alpha_modes_render_opaque_mask_and_sorted_blend() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let material = |base_color, alpha_mode| MaterialAsset {
            model: crate::assets::MaterialModel::Unlit,
            base_color,
            alpha_mode,
            ..MaterialAsset::default()
        };
        let green = material([0.0, 1.0, 0.0, 1.0], AlphaMode::Opaque);
        let mask = AlphaMode::Mask { cutoff: 0.5 };

        let [_, g, r, _] = render_center_pixel(&[
            (0.0, green.clone()),
            (1.0, material([1.0, 0.0, 0.0, 0.2], mask)),
        ]);
        assert_eq!((r, g), (0, 255), "masked-out slab is discarded");

        let [_, g, r, _] = render_center_pixel(&[
            (0.0, green.clone()),
            (1.0, material([1.0, 0.0, 0.0, 0.8], mask)),
        ]);
        assert_eq!((r, g), (255, 0), "kept masked slab is fully opaque");

        let [_, g, r, _] = render_center_pixel(&[
            (0.0, green),
            (1.0, material([1.0, 0.0, 0.0, 0.3], AlphaMode::Opaque)),
        ]);
        assert_eq!((r, g), (255, 0), "opaque ignores base-color alpha");

        // The near slab is inserted first, so draw order is only correct if
        // blended objects are sorted back to front.
        let [b, _, r, _] = render_center_pixel(&[
            (1.0, material([1.0, 0.0, 0.0, 0.5], AlphaMode::Blend)),
            (0.0, material([0.0, 0.0, 1.0, 0.5], AlphaMode::Blend)),
        ]);
        assert!(r > b + 30, "near red must blend over far blue: r={r} b={b}");
        assert!(b > 80, "far blue must still show through: b={b}");
    }

    #[test]
    fn hybrid_physics_gpu_layouts_match_shader_structs() {
        use std::mem::{offset_of, size_of};

        type ReflectedBody = super::physics_shader::PhysicsState;
        assert_eq!(size_of::<GpuBodyState>(), size_of::<ReflectedBody>());
        assert_eq!(
            offset_of!(GpuBodyState, metadata),
            offset_of!(ReflectedBody, metadata)
        );

        assert_eq!(
            size_of::<GpuConditionUpload>(),
            size_of::<super::physics_shader::ConditionInstruction>()
        );
        assert_eq!(
            size_of::<GpuRuleState>(),
            size_of::<super::physics_shader::RuleState>()
        );
        assert_eq!(
            size_of::<GpuEventUpload>(),
            size_of::<super::physics_shader::PhysicsEvent>()
        );
        assert_eq!(
            size_of::<PhysicsPushConstants>(),
            size_of::<super::physics_shader::PhysicsPush>()
        );

        // `EventHeader` is a bare buffer block (no named GLSL struct), so
        // there is no generated reflected type to compare against — its
        // hand-computed layout is checked directly instead.
        assert_eq!(size_of::<GpuEventHeader>(), 16);
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
