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

use nalgebra::{
    Matrix4, Orthographic3, Perspective3, Point3, Vector3, Vector4,
};
use vulkano::buffer::allocator::{
    SubbufferAllocator, SubbufferAllocatorCreateInfo,
};
use vulkano::buffer::{
    Buffer, BufferContents, BufferCreateInfo, BufferUsage, Subbuffer,
};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BufferCopy, CommandBufferUsage, CopyBufferInfo,
    CopyBufferToImageInfo, DrawIndexedIndirectCommand,
    PrimaryAutoCommandBuffer, RenderPassBeginInfo, SubpassBeginInfo,
    SubpassContents,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::layout::DescriptorSetLayout;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::{DeviceExtensions, DeviceOwned, Queue};
use vulkano::format::Format;
use vulkano::image::sampler::{
    BorderColor, Filter, Sampler, SamplerAddressMode, SamplerCreateInfo,
    SamplerMipmapMode,
};
use vulkano::image::view::{ImageView, ImageViewCreateInfo};
use vulkano::image::{
    Image, ImageCreateInfo, ImageLayout, ImageSubresourceRange, ImageUsage,
};
use vulkano::instance::debug::DebugUtilsLabel;
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::memory::MemoryHeapFlags;
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::graphics::color_blend::{
    AttachmentBlend, ColorBlendAttachmentState, ColorBlendState,
};
use vulkano::pipeline::graphics::depth_stencil::{
    CompareOp, DepthState, DepthStencilState,
};
use vulkano::pipeline::graphics::input_assembly::{
    InputAssemblyState, PrimitiveTopology,
};
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::{
    CullMode, DepthBiasState, FrontFace, RasterizationState,
};
use vulkano::pipeline::graphics::subpass::PipelineSubpassType;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::pipeline::graphics::viewport::{Scissor, Viewport, ViewportState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::layout::{
    PipelineDescriptorSetLayoutCreateInfo, PipelineLayout,
};
use vulkano::pipeline::{
    ComputePipeline, DynamicState, GraphicsPipeline, Pipeline,
    PipelineBindPoint, PipelineShaderStageCreateInfo,
};
use vulkano::query::{
    QueryPool, QueryPoolCreateInfo, QueryResultFlags, QueryType,
};
use vulkano::render_pass::{
    Framebuffer, FramebufferCreateInfo, RenderPass, Subpass,
};
use vulkano::sync::future::FenceSignalFuture;
use vulkano::sync::{GpuFuture, PipelineStage};
use vulkano::DeviceSize;

use crate::assets::{
    AlphaMode, AssetServer, Handle, LodGroupAsset, LodMetric, MaterialAsset,
    MaterialModel, MeshAsset, TextureAsset, TextureColorSpace, TextureFilter,
    TextureSampler, TextureWrap,
};
use crate::rendering::debug_overlay::{DebugLine, RenderDebugOverlay};
use crate::rendering::frame_passes::{FramePass, FrameResource};
use crate::runtime::{
    CullingMode, GpuConditionInstruction, Projection, QualityProfile,
    RawGpuPhysicsEvent, RenderBounds, RenderWorld, ToneMapping,
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
    #[format(R32G32_SFLOAT)]
    uv: [f32; 2],
    #[format(R32G32B32A32_SFLOAT)]
    tangent: [f32; 4],
}

/// One entry of the per-frame visible list: the instance a main-pass draw
/// reads, fetched per instance so each batch draws its visible instances in
/// one call.
#[repr(C)]
#[derive(BufferContents, Vertex, Clone, Copy, Debug, PartialEq, Eq)]
struct VisibleInstance {
    #[format(R32_UINT)]
    instance_index: u32,
}

/// Per-instance input of the GPU cull pass, indexed like the instance buffer.
#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default, PartialEq)]
struct CullInstance {
    /// Local bounding sphere `(center, radius)`; a negative radius is never
    /// culled.
    sphere: [f32; 4],
    /// x: draw command the instance counts into; y: first visible-list slot
    /// of that command's group; z: 1 for alpha-blended instances, which
    /// only the late occlusion phase draws.
    slot: [u32; 4],
    /// xy: `[start, end)` range of the LOD value this instance draws in,
    /// `[0, inf)` outside LOD groups; z: 1 for the screen-size metric.
    lod: [f32; 4],
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct CullPushConstants {
    /// Clip matrix; the shader takes the frustum planes from its rows.
    clip: [[f32; 4]; 4],
    /// Scene viewport offset and extent in pixels of the depth pyramid.
    viewport: [f32; 4],
    /// xyz: camera position; w: [`lod_scale`].
    lod: [f32; 4],
    /// x: instance count; y: phase (see [`CullPhase`]); z: 1 to skip the
    /// frustum test.
    info: [u32; 4],
}

/// What one cull dispatch does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CullPhase {
    /// Frustum test only.
    Frustum = 0,
    /// Frustum-visible opaque instances that were visible last frame.
    Early = 1,
    /// Frustum and depth-pyramid test of everything; records visibility for
    /// the next frame and emits what the early phase did not draw.
    Late = 2,
}

/// Where a frame's main-pass visibility comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CullingPath {
    /// Every instance is drawn; nothing is tested.
    #[default]
    Direct,
    /// Frustum test on the CPU, uploaded as a compacted list.
    Cpu,
    /// Compute pass writes the visible list and indirect draw commands.
    Gpu,
    /// Two-phase GPU occlusion culling: last frame's visible set is drawn
    /// first, a depth pyramid is built from it, and a second cull pass
    /// draws what the pyramid does not hide.
    GpuOcclusion,
}

impl CullingPath {
    /// Whether the cull pass runs on the GPU.
    pub fn on_gpu(self) -> bool {
        matches!(self, CullingPath::Gpu | CullingPath::GpuOcclusion)
    }
}

/// Culling numbers for the profiler, from the last frame whose results are
/// known. The GPU path reads its counts and time back once that frame's
/// submission completes, so they arrive a frame or two late.
#[derive(
    bevy_ecs::prelude::Resource, Clone, Copy, Debug, Default, PartialEq,
)]
pub struct CullingStats {
    pub path: CullingPath,
    /// Instances the main pass considered. Each level of a LOD group is
    /// its own instance.
    pub submitted: usize,
    /// Instances the main pass drew.
    pub visible: usize,
    /// Instances frustum culling, occlusion culling, and LOD selection
    /// skipped.
    pub culled: usize,
    /// Culling cost: CPU wall time on the `Direct` and `Cpu` paths, the
    /// cull dispatches' GPU time on the GPU paths, including the depth
    /// pyramid on `GpuOcclusion`. `None` when the queue has no timestamp
    /// support.
    pub time: Option<Duration>,
}

/// A GPU-culled frame whose counts and time are read once it completes.
struct PendingCullReadback {
    fence: FrameFence,
    path: CullingPath,
    submitted: usize,
    /// Every draw-command set the frame drew with; their counts add up.
    commands: Vec<Subbuffer<[DrawIndexedIndirectCommand]>>,
    /// Holds two start/end timestamp pairs around the cull work.
    timestamps: Option<Arc<QueryPool>>,
}

/// Instances from which GPU culling beats the CPU loop and its upload.
// ponytail: untimed guess; tune from `CullingStats` times measured on real
// integrated and discrete GPUs.
pub const GPU_CULL_MIN_INSTANCES: usize = 4096;

/// Below this many instances, `Auto` draws everything: the few off-view
/// draws cost less than a cull dispatch or a visible-list upload.
// ponytail: untimed guess like `GPU_CULL_MIN_INSTANCES`; tune from the
// `CullingStats` break-even on real hardware.
pub const AUTO_DIRECT_MAX_INSTANCES: usize = 64;

/// Picks the culling path for a frame. `Auto` skips culling for scenes
/// under [`AUTO_DIRECT_MAX_INSTANCES`], even with GPU bodies, since direct
/// drawing is correct for any owner. LOD levels need a path that picks one
/// per object, so `has_lods` rules out `Direct`; `Disabled` then culls by
/// LOD only. GPU-owned bodies force the GPU path,
/// because only the GPU has their newest transforms. Otherwise large scenes
/// go to the GPU; integrated GPUs and `Eco`, where compute time is scarce,
/// need four times as many instances.
pub fn select_culling_path(
    mode: CullingMode,
    instances: usize,
    gpu_owned: usize,
    has_lods: bool,
    quality: QualityProfile,
    capabilities: &RendererCapabilities,
) -> CullingPath {
    let threshold =
        if capabilities.integrated_gpu || quality == QualityProfile::Eco {
            GPU_CULL_MIN_INSTANCES * 4
        } else {
            GPU_CULL_MIN_INSTANCES
        };
    match mode {
        CullingMode::Disabled if !has_lods => CullingPath::Direct,
        CullingMode::Auto
            if instances < AUTO_DIRECT_MAX_INSTANCES && !has_lods =>
        {
            CullingPath::Direct
        }
        CullingMode::FrustumAndOcclusion => CullingPath::GpuOcclusion,
        _ if gpu_owned > 0 || instances >= threshold => CullingPath::Gpu,
        _ => CullingPath::Cpu,
    }
}

/// Local bounding sphere `[center, radius]` of `bounds`.
fn bounding_sphere(bounds: RenderBounds) -> [f32; 4] {
    match bounds {
        RenderBounds::Sphere { center, radius } => {
            [center[0], center[1], center[2], radius]
        }
        RenderBounds::Aabb { min, max } => {
            let half = Vector3::from(max) - Vector3::from(min);
            let center = (Vector3::from(max) + Vector3::from(min)) / 2.0;
            [center.x, center.y, center.z, half.norm() / 2.0]
        }
    }
}

/// `bounding_sphere` moved by `model` the way the cull shader does it: the
/// radius grows with the largest axis scale. A negative radius stays
/// negative.
fn world_sphere(model: &[[f32; 4]; 4], sphere: [f32; 4]) -> [f32; 4] {
    let model = matrix_from_array(*model);
    let center =
        model.transform_point(&Point3::new(sphere[0], sphere[1], sphere[2]));
    let scale = (0..3)
        .map(|axis| model.fixed_view::<3, 1>(0, axis).norm())
        .fold(0.0_f32, f32::max);
    [center.x, center.y, center.z, sphere[3] * scale]
}

/// Projection term of the LOD value: `tan(fov / 2)` for a perspective
/// camera, minus half the view height for an orthographic one.
fn lod_scale(render_world: &RenderWorld) -> f32 {
    match render_world.active_camera.map(|camera| camera.projection) {
        Some(Projection::Perspective {
            vertical_fov_radians,
            ..
        }) => (vertical_fov_radians / 2.0).tan(),
        Some(Projection::Orthographic { vertical_size, .. }) => {
            -vertical_size / 2.0
        }
        None => (std::f32::consts::FRAC_PI_3 / 2.0).tan(),
    }
}

/// LOD value of a world sphere, growing with distance like
/// [`LodGroupAsset::ranges`]: the distance to `camera.xyz`, or the inverse
/// of the view-height fraction the sphere covers. `camera.w` is
/// [`lod_scale`]. A sphere without a radius counts as close. Mirrors
/// `lod_value` in the cull shader.
fn lod_value(camera: [f32; 4], sphere: [f32; 4], screen_size: bool) -> f32 {
    let distance = (Vector3::new(sphere[0], sphere[1], sphere[2])
        - Vector3::new(camera[0], camera[1], camera[2]))
    .norm();
    if !screen_size {
        distance
    } else if sphere[3] <= 0.0 {
        0.0
    } else if camera[3] > 0.0 {
        distance * camera[3] / sphere[3]
    } else {
        -camera[3] / sphere[3]
    }
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

/// Per-object data read by the graphics shaders: the main pass through the
/// visible list, the shadow pass with `gl_InstanceIndex`.
#[repr(C)]
#[derive(BufferContents, Clone, Copy, Debug, Default, PartialEq)]
struct RenderInstanceUpload {
    model: [[f32; 4]; 4],
    /// Inverse-transpose of the model's 3x3 part, as three padded columns.
    /// GPU bodies ignore it and derive theirs from the live model.
    normal: [[f32; 4]; 3],
    color: [f32; 4],
    /// rgb: emissive factor; w: 1 for `MaterialModel::Unlit`, else 0.
    emissive: [f32; 4],
    /// x: metallic; y: roughness; z: 1 when a normal map is bound.
    surface: [f32; 4],
    /// x: GPU physics index or `u32::MAX`; y: alpha mode (0 opaque, 1 mask,
    /// 2 blend); z: mask cutoff as `f32` bits; w: bit 0 casts shadows, bit 1
    /// receives shadows.
    physics: [u32; 4],
}

/// Light-space transform of the shadowed directional light, per frame.
#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct ShadowUpload {
    light_view_projection: [[f32; 4]; 4],
}

/// Side length in texels of the directional shadow map and the view
/// distance in front of the camera it covers, for a resolved profile.
/// Higher profiles cover more distance at a finer texel size; every Vulkan
/// device supports 4096-texel 2D images.
pub fn shadow_settings(profile: QualityProfile) -> (u32, f32) {
    match profile {
        QualityProfile::Eco => (1024, 30.0),
        QualityProfile::Balanced => (2048, 50.0),
        QualityProfile::High | QualityProfile::Auto => (4096, 80.0),
    }
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct CameraUniform {
    view_projection: [[f32; 4]; 4],
    /// w = 1: xyz is the camera position (perspective). w = 0: xyz is the
    /// direction toward an orthographic camera.
    eye: [f32; 4],
    /// Ambient seen by surfaces facing +Y (uniform ambient plus sky).
    ambient: [f32; 4],
    /// Ambient seen by surfaces facing -Y (uniform ambient plus ground).
    ground_ambient: [f32; 4],
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

/// A base-color texture uploaded for the ECS renderer, with the set-1
/// descriptor set that binds it and its sampler.
struct PreparedTexture {
    handle: Handle<TextureAsset>,
    /// `None` when the texture data was malformed and failed to upload.
    view: Option<Arc<ImageView>>,
    sampler: Arc<Sampler>,
    source_revision: u64,
}

/// Set 1 of one material: base color, normal, metallic-roughness, occlusion,
/// and emissive maps, white where the material has none.
struct PreparedMaterial {
    handle: Handle<MaterialAsset>,
    set: Arc<DescriptorSet>,
    /// Views the set was written with; a re-uploaded texture has a new view.
    views: [usize; MATERIAL_TEXTURES],
}

const MATERIAL_TEXTURES: usize = 5;

/// Pixels waiting to be copied into their image by the next recorded frame.
type PendingTextureUpload = (Subbuffer<[u8]>, Arc<Image>);

struct PreparedMesh {
    handle: Handle<MeshAsset>,
    vertices: Subbuffer<[SceneVertex]>,
    indices: Subbuffer<[u32]>,
    source_revision: u64,
    /// Local box around the uploaded vertices, which may be the fallback
    /// mesh's while the real one loads.
    bounds: Option<RenderBounds>,
}

/// Consecutive instances that share one mesh and material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedRenderBatch {
    mesh_key: u64,
    material: Handle<MaterialAsset>,
    first_instance: u32,
    instance_count: u32,
}

/// One alpha-blended instance, drawn alone so it can be sorted per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BlendedInstance {
    instance: u32,
    mesh_key: u64,
    material: Handle<MaterialAsset>,
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
    /// Uploaded mesh revisions the bounds below were computed from.
    mesh_revisions: Vec<(u64, u64)>,
    /// World bounds by instance index. `None` never culls: GPU-physics
    /// bodies, whose CPU transform is stale, and objects without a mesh box.
    bounds: Vec<Option<RenderBounds>>,
    /// Main-pass visible list for the last camera; reset on every rebuild.
    visibility: Option<PreparedVisibility>,
    /// Cull-pass input by instance index.
    cull_source: Vec<CullInstance>,
    /// `cull_source` uploaded on the first GPU-culled frame.
    cull: Option<Subbuffer<[CullInstance]>>,
    /// Per-instance visibility the late occlusion phase writes and the next
    /// frame's early phase reads. Zeroed with `cull`, so the first frame
    /// after a rebuild draws everything in the late phase.
    occlusion: Option<Subbuffer<[u32]>>,
    /// Instances drawn from GPU physics state.
    gpu_owned: usize,
    /// World sphere by instance index for instances in a LOD group, whose
    /// range sits in their `cull_source` entry.
    lod_spheres: Vec<Option<[f32; 4]>>,
    /// `lod_signature` the instances were expanded with.
    lod_signature: Vec<(u64, u64)>,
}

/// Instances the main pass draws, compacted per batch.
struct PreparedVisibility {
    /// Clip matrix the list was picked for and whether the frustum test
    /// ran, or `None` when the camera did not matter.
    clip: Option<([[f32; 4]; 4], bool)>,
    list: Subbuffer<[VisibleInstance]>,
    /// `(first, count)` in `list` for each batch, in batch order.
    batches: Vec<(u32, u32)>,
    /// Visible blended instances whose `instance` is their slot in `list`.
    blended: Vec<BlendedInstance>,
    culled: usize,
}

struct PreparedGpuPhysics {
    source_revision: u64,
    source: Vec<crate::runtime::ExtractedGpuPhysicsBody>,
    body_indices: HashMap<bevy_ecs::entity::Entity, u32>,
    states: Subbuffer<[GpuBodyState]>,
    instructions: Subbuffer<[GpuConditionUpload]>,
    rules: Subbuffer<[GpuRuleState]>,
    /// Live state of surviving bodies, copied from the previous states buffer
    /// before the next dispatch so an edit does not reset their motion.
    carry: Option<(Subbuffer<[GpuBodyState]>, Vec<BufferCopy>)>,
}

struct PreparedLights {
    revision: u64,
    budget: usize,
    buffer: Subbuffer<[LightUpload]>,
    count: u32,
    ambient: [f32; 4],
    ground_ambient: [f32; 4],
    /// Upload index and direction of the one directional light that casts
    /// shadows: the first uploaded one with `shadows` enabled.
    shadow: Option<(u32, [f32; 3])>,
}

/// Resources reused when one swapchain image comes around again.
struct PreparedFrame {
    graphics_set: Arc<DescriptorSet>,
    framebuffer: Arc<Framebuffer>,
    /// Buffers `graphics_set` was written with. The set is rebuilt when any
    /// prepared buffer is reallocated, whatever caused the reallocation.
    bound: GraphicsBuffers,
    /// Value of `SceneRenderer::frames_rendered` when this target was last
    /// drawn; stale entries belong to a recreated swapchain.
    last_used: u64,
}

type GraphicsBuffers = (
    Subbuffer<[GpuBodyState]>,
    Subbuffer<[RenderInstanceUpload]>,
    Subbuffer<[LightUpload]>,
);

fn same_buffer<T: ?Sized>(a: &Subbuffer<T>, b: &Subbuffer<T>) -> bool {
    Arc::ptr_eq(a.buffer(), b.buffer())
        && a.offset() == b.offset()
        && a.size() == b.size()
}

type FrameFence = Arc<FenceSignalFuture<Box<dyn GpuFuture>>>;

/// Submitted frames allowed in flight before [`SceneRenderer::render`] waits.
pub const FRAMES_IN_FLIGHT: usize = 2;

/// Fixed physics ticks dispatched per rendered frame; older ticks are dropped.
const MAX_PHYSICS_STEPS_PER_FRAME: u64 = 8;

/// Submission state of one in-flight frame, reused round-robin.
struct FrameContext {
    /// Signals when this context's last submission finished on the GPU.
    /// Holding it also keeps that submission's resources alive until then.
    fence: Option<FrameFence>,
    /// Host-visible per-frame buffers (debug vertices, physics readback).
    /// Its arenas are reused once this context's frame no longer holds them.
    transient: SubbufferAllocator,
    /// Two timestamp pairs around the cull work, or `None` when the queue
    /// cannot write timestamps.
    timestamps: Option<Arc<QueryPool>>,
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
    /// What the scene pass shows; games always use [`SceneDebugView::Lit`].
    pub debug_view: SceneDebugView,
}

/// Diagnostic output of the scene pass, like Godot's viewport debug draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SceneDebugView {
    /// Normal lit rendering.
    #[default]
    Lit = 0,
    /// Base color and textures only, without lighting.
    Unshaded = 1,
    /// World-space normals after normal mapping, as `normal * 0.5 + 0.5`.
    Normals = 2,
}

impl SceneDebugView {
    pub const ALL: [Self; 3] = [Self::Lit, Self::Unshaded, Self::Normals];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Lit => "Lit",
            Self::Unshaded => "Unshaded",
            Self::Normals => "Normals",
        }
    }
}

impl<'a> SceneRenderOptions<'a> {
    /// Creates the normal, editor-free game rendering configuration.
    #[must_use]
    pub fn game(extent: [u32; 2]) -> Self {
        Self {
            viewport: SceneViewport::full(extent),
            debug_overlay: None,
            debug_view: SceneDebugView::Lit,
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

/// Capacity and asset fallbacks the renderer took instead of failing the
/// frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderCapacityDiagnostics {
    /// Lights beyond [`MAX_LIGHTS`] left out of the latest light upload.
    pub dropped_lights: usize,
    /// Distinct missing meshes of visible objects; they draw the fallback
    /// cube.
    pub missing_meshes: usize,
    /// Distinct missing materials of visible objects; they draw magenta.
    pub missing_materials: usize,
    /// Referenced maps that are missing or failed to upload. A missing base
    /// color map draws magenta, a missing normal map draws flat.
    pub missing_textures: usize,
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
    /// Occlusion frames: last frame's visible set, keeping HDR and depth.
    early_render_pass: Arc<RenderPass>,
    /// Occlusion frames: the rest of the scene over the early pass, then
    /// tone mapping.
    late_render_pass: Arc<RenderPass>,
    pipeline: Arc<GraphicsPipeline>,
    blend_pipeline: Arc<GraphicsPipeline>,
    debug_pipeline: Arc<GraphicsPipeline>,
    debug_on_top_pipeline: Arc<GraphicsPipeline>,
    physics_pipeline: Arc<ComputePipeline>,
    /// Depth-only pass into `shadow_map`; shares the main pipeline layout.
    shadow_pipeline: Arc<GraphicsPipeline>,
    shadow_framebuffer: Arc<Framebuffer>,
    shadow_map: Arc<ImageView>,
    shadow_sampler: Arc<Sampler>,
    depth: Arc<ImageView>,
    /// Float scene color before tone mapping; same size as `depth`.
    hdr: Arc<ImageView>,
    tonemap_pipeline: Arc<GraphicsPipeline>,
    /// Reads `hdr` as the input attachment of the tone-mapping subpass.
    tonemap_set: Arc<DescriptorSet>,
    depth_extent: [u32; 2],
    /// Per-change instance uploads. Arenas are reused once no in-flight
    /// frame references them and double in size when an upload outgrows them.
    instance_allocator: SubbufferAllocator,
    /// Largest per-frame instance upload accepted; see
    /// [`transient_upload_budget`].
    instance_budget: DeviceSize,
    prepared_meshes: HashMap<u64, PreparedMesh>,
    prepared_meshes_revision: u64,
    /// `lod_signature` `visible_meshes` was collected with.
    prepared_meshes_lods: Vec<(u64, u64)>,
    prepared_textures: HashMap<u64, PreparedTexture>,
    prepared_materials: HashMap<u64, PreparedMaterial>,
    /// 1x1 white texture bound in place of every missing material map.
    white_texture: (Arc<ImageView>, Arc<Sampler>),
    /// Per-slot stand-ins for a map that is referenced but missing or
    /// malformed: magenta base color, flat normal, white for the rest.
    missing_textures: [(Arc<ImageView>, Arc<Sampler>); MATERIAL_TEXTURES],
    /// Set 1 with every map white, for materials not prepared yet.
    white_material: Arc<DescriptorSet>,
    samplers: HashMap<TextureSampler, Arc<Sampler>>,
    pending_texture_uploads: Vec<PendingTextureUpload>,
    visible_meshes: Vec<Handle<MeshAsset>>,
    prepared_instances: Option<PreparedRenderInstances>,
    prepared_physics: Option<PreparedGpuPhysics>,
    prepared_lights: Option<PreparedLights>,
    prepared_frames: HashMap<usize, PreparedFrame>,
    frames_rendered: u64,
    pending_physics: Vec<PendingPhysicsReadback>,
    completed_physics_events: Vec<RawGpuPhysicsEvent>,
    last_physics_tick: u64,
    last_frame_passes: Vec<FramePass>,
    last_frame_culled: Option<usize>,
    last_culling_path: CullingPath,
    culling_stats: CullingStats,
    pending_cull: Vec<PendingCullReadback>,
    cull_pipeline: Arc<ComputePipeline>,
    depth_pyramid_copy_pipeline: Arc<ComputePipeline>,
    depth_pyramid_reduce_pipeline: Arc<ComputePipeline>,
    /// Rebuilt with `depth`.
    depth_pyramid: DepthPyramid,
    /// Last GPU-culled frame's draw commands, early set first on occlusion
    /// frames, for readback in tests.
    #[cfg(test)]
    last_draw_commands: Vec<Subbuffer<[DrawIndexedIndirectCommand]>>,
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
        // Subpass 0 lights the scene into a float HDR target. Subpass 1 tone
        // maps it into the output and draws editor lines, which stay exact.
        // Occlusion frames split this into an early pass that keeps HDR and
        // depth and a late pass that continues them; all three passes are
        // compatible, so they share framebuffers and pipelines.
        macro_rules! main_pass {
            ($color:ident, $hdr_load:ident, $hdr_store:ident, $depth_load:ident, $depth_store:ident) => {
                vulkano::ordered_passes_renderpass!(
                    queue.device().clone(),
                    attachments: {
                        color: {
                            format: output_format,
                            samples: 1,
                            load_op: DontCare,
                            store_op: $color,
                        },
                        hdr: {
                            format: HDR_COLOR_FORMAT,
                            samples: 1,
                            load_op: $hdr_load,
                            store_op: $hdr_store,
                        },
                        depth: {
                            format: Format::D32_SFLOAT,
                            samples: 1,
                            load_op: $depth_load,
                            store_op: $depth_store,
                        }
                    },
                    passes: [
                        {
                            color: [hdr],
                            depth_stencil: {depth},
                            input: []
                        },
                        {
                            color: [color],
                            depth_stencil: {depth},
                            input: [hdr]
                        }
                    ]
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
            };
        }
        let render_pass = main_pass!(Store, Clear, DontCare, Clear, DontCare);
        let early_render_pass =
            main_pass!(DontCare, Clear, Store, Clear, Store);
        let late_render_pass =
            main_pass!(Store, Load, DontCare, Load, DontCare);
        let (pipeline, blend_pipeline) =
            create_pipelines(queue.clone(), render_pass.clone())?;
        let debug_pipeline =
            create_debug_pipeline(queue.clone(), render_pass.clone(), true)?;
        let debug_on_top_pipeline =
            create_debug_pipeline(queue.clone(), render_pass.clone(), false)?;
        let physics_pipeline = create_compute_pipeline(
            &queue,
            physics_shader::load(queue.device().clone()),
        )?;
        let cull_pipeline = create_compute_pipeline(
            &queue,
            cull_shader::load(queue.device().clone()),
        )?;
        let depth_pyramid_copy_pipeline = create_compute_pipeline(
            &queue,
            depth_pyramid_copy_shader::load(queue.device().clone()),
        )?;
        let depth_pyramid_reduce_pipeline = create_compute_pipeline(
            &queue,
            depth_pyramid_reduce_shader::load(queue.device().clone()),
        )?;
        let tonemap_pipeline =
            create_tonemap_pipeline(queue.clone(), render_pass.clone())?;
        let (shadow_pipeline, shadow_framebuffer, shadow_map, shadow_sampler) =
            create_shadow_pass(&queue, &memory_allocator, &pipeline)?;
        let depth = create_depth(&memory_allocator, initial_extent)?;
        let hdr = create_hdr(&memory_allocator, initial_extent)?;
        let instance_allocator = SubbufferAllocator::new(
            memory_allocator.clone(),
            SubbufferAllocatorCreateInfo {
                arena_size: INSTANCE_ARENA_BYTES,
                buffer_usage: BufferUsage::STORAGE_BUFFER
                    | BufferUsage::VERTEX_BUFFER
                    | BufferUsage::INDIRECT_BUFFER,
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
        let timestamp_queue =
            queue.device().physical_device().queue_family_properties()
                [queue.queue_family_index() as usize]
                .timestamp_valid_bits
                .is_some();
        let frame_contexts = std::array::from_fn(|_| FrameContext {
            fence: None,
            timestamps: timestamp_queue
                .then(|| {
                    QueryPool::new(
                        queue.device().clone(),
                        QueryPoolCreateInfo {
                            query_count: 4,
                            ..QueryPoolCreateInfo::query_type(
                                QueryType::Timestamp,
                            )
                        },
                    )
                    .ok()
                })
                .flatten(),
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
        let descriptor_allocator =
            Arc::new(StandardDescriptorSetAllocator::new(
                queue.device().clone(),
                Default::default(),
            ));
        let default_sampler =
            texture_sampler(&queue, TextureSampler::default())?;
        let mut pending_texture_uploads = Vec::new();
        let white_view = create_texture(
            &memory_allocator,
            &TextureAsset {
                size: [1, 1],
                rgba8: vec![255; 4],
                color_space: TextureColorSpace::Linear,
                sampler: TextureSampler::default(),
            },
            &mut pending_texture_uploads,
        )?;
        let white_texture = (white_view, default_sampler.clone());
        let mut solid = |rgba8: [u8; 4]| {
            create_texture(
                &memory_allocator,
                &TextureAsset {
                    size: [1, 1],
                    rgba8: rgba8.to_vec(),
                    color_space: TextureColorSpace::Linear,
                    sampler: TextureSampler::default(),
                },
                &mut pending_texture_uploads,
            )
            .map(|view| (view, default_sampler.clone()))
        };
        let missing_textures = [
            solid([255, 0, 255, 255])?,
            solid([128, 128, 255, 255])?,
            white_texture.clone(),
            white_texture.clone(),
            white_texture.clone(),
        ];
        let tonemap_set =
            create_tonemap_set(&descriptor_allocator, &tonemap_pipeline, &hdr)?;
        let depth_pyramid = create_depth_pyramid(
            &memory_allocator,
            &descriptor_allocator,
            &depth_pyramid_copy_pipeline,
            &depth_pyramid_reduce_pipeline,
            &depth,
        )?;
        let white_material = create_material_set(
            &descriptor_allocator,
            &pipeline.layout().set_layouts()[1],
            std::array::from_fn(|_| white_texture.clone()),
        )?;
        Ok(Self {
            instance_allocator,
            instance_budget,
            command_allocator: Arc::new(StandardCommandBufferAllocator::new(
                queue.device().clone(),
                Default::default(),
            )),
            descriptor_allocator,
            prepared_textures: HashMap::new(),
            prepared_materials: HashMap::new(),
            white_texture,
            missing_textures,
            white_material,
            samplers: HashMap::from([(
                TextureSampler::default(),
                default_sampler,
            )]),
            pending_texture_uploads,
            queue,
            memory_allocator,
            render_pass,
            early_render_pass,
            late_render_pass,
            pipeline,
            blend_pipeline,
            debug_pipeline,
            debug_on_top_pipeline,
            physics_pipeline,
            cull_pipeline,
            depth_pyramid_copy_pipeline,
            depth_pyramid_reduce_pipeline,
            depth_pyramid,
            shadow_pipeline,
            shadow_framebuffer,
            shadow_map,
            shadow_sampler,
            depth,
            hdr,
            tonemap_pipeline,
            tonemap_set,
            depth_extent: initial_extent,
            prepared_meshes: HashMap::new(),
            prepared_meshes_revision: 0,
            prepared_meshes_lods: Vec::new(),
            visible_meshes: Vec::new(),
            prepared_instances: None,
            prepared_physics: None,
            prepared_lights: None,
            prepared_frames: HashMap::new(),
            frames_rendered: 0,
            pending_physics: Vec::new(),
            completed_physics_events: Vec::new(),
            last_physics_tick: 0,
            last_frame_passes: Vec::new(),
            last_frame_culled: Some(0),
            last_culling_path: CullingPath::Direct,
            culling_stats: CullingStats::default(),
            pending_cull: Vec::new(),
            #[cfg(test)]
            last_draw_commands: Vec::new(),
            capacity: RenderCapacityDiagnostics::default(),
            frame_contexts,
            frame_index: 0,
            capabilities,
        })
    }

    /// The passes the last `render` recorded, in order. Each one is also a
    /// debug-utils label when the instance enables `ext_debug_utils`.
    pub fn last_frame_passes(&self) -> &[FramePass] {
        &self.last_frame_passes
    }

    /// Instances the last frame skipped in the main pass by frustum culling,
    /// or `None` when the GPU culled and the count stayed on the GPU.
    /// The shadow pass draws them all so off-view casters still cast.
    pub fn last_frame_culled(&self) -> Option<usize> {
        self.last_frame_culled
    }

    /// Culling counts and cost for the profiler; see [`CullingStats`].
    /// Collects completed GPU readbacks first, without waiting.
    pub fn culling_stats(&mut self) -> CullingStats {
        self.collect_cull_readbacks();
        self.culling_stats
    }

    /// Reads back GPU-culled frames whose submission has completed. The
    /// newest completed frame wins.
    fn collect_cull_readbacks(&mut self) {
        let period = f64::from(
            self.queue
                .device()
                .physical_device()
                .properties()
                .timestamp_period,
        );
        let mut index = 0;
        while index < self.pending_cull.len() {
            if !self.pending_cull[index]
                .fence
                .is_signaled()
                .unwrap_or(false)
            {
                index += 1;
                continue;
            }
            // Pending entries are in submission order, so a later completed
            // one overwrites an earlier one.
            let pending = self.pending_cull.remove(index);
            let mut visible = 0;
            for commands in &pending.commands {
                let Ok(commands) = commands.read() else {
                    continue;
                };
                visible += commands
                    .iter()
                    .map(|command| command.instance_count as usize)
                    .sum::<usize>();
            }
            let time = pending.timestamps.and_then(|pool| {
                let mut ticks = [0_u64; 4];
                pool.get_results(0..4, &mut ticks, QueryResultFlags::empty())
                    .ok()
                    .filter(|&available| available)?;
                // ponytail: ignores `timestamp_valid_bits` wrap-around; a
                // wrapped pair reads as zero for one frame.
                let ticks = ticks[1].saturating_sub(ticks[0])
                    + ticks[3].saturating_sub(ticks[2]);
                Some(Duration::from_nanos((ticks as f64 * period) as u64))
            });
            self.culling_stats = CullingStats {
                path: pending.path,
                submitted: pending.submitted,
                visible,
                culled: pending.submitted.saturating_sub(visible),
                time,
            };
        }
    }

    /// The culling path the last `render` took.
    pub fn last_culling_path(&self) -> CullingPath {
        self.last_culling_path
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
        // A viewport reported past the target (rounding, a resize race) is
        // cropped by the scissor; projecting the clamped extent would squash it.
        let visible = options.viewport.clamped_to(extent);
        let viewport = SceneViewport {
            offset: visible.offset,
            extent: options.viewport.extent,
        };
        if extent[0] == 0
            || extent[1] == 0
            || visible.extent[0] == 0
            || visible.extent[1] == 0
        {
            // Time spent minimized is skipped, not simulated in one step on
            // restore.
            self.last_physics_tick = render_world.physics_tick;
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
        // Before this context's timestamp queries are reset and reused.
        self.collect_cull_readbacks();
        self.ensure_depth(extent)?;
        self.prepare_visible_meshes(render_world, assets)?;
        self.prepare_gpu_physics(render_world)?;
        self.prepare_lights(render_world)?;
        self.prepare_render_instances(render_world, assets)?;
        let clip = view_projection(render_world, viewport.extent);
        let prepared = self.prepared_instances.as_ref().unwrap();
        let quality = resolve_quality(render_world.quality, &self.capabilities);
        let path = select_culling_path(
            render_world.culling,
            prepared.cull_source.len(),
            prepared.gpu_owned,
            prepared.lod_spheres.iter().any(Option::is_some),
            quality,
            &self.capabilities,
        );
        let frustum = render_world.culling != CullingMode::Disabled;
        let (eye, forward) = camera_eye_forward(render_world);
        let lod_camera = [eye[0], eye[1], eye[2], lod_scale(render_world)];
        let cull_start = std::time::Instant::now();
        match path {
            CullingPath::Direct => {
                self.prepare_visibility(clip, false, lod_camera)?
            }
            CullingPath::Cpu => {
                self.prepare_visibility(clip, frustum, lod_camera)?
            }
            CullingPath::Gpu | CullingPath::GpuOcclusion => {
                self.prepare_cull_instances()?
            }
        }
        if !path.on_gpu() {
            let prepared = self.prepared_instances.as_ref().unwrap();
            let culled = prepared.visibility.as_ref().unwrap().culled;
            self.culling_stats = CullingStats {
                path,
                submitted: prepared.bounds.len(),
                visible: prepared.bounds.len() - culled,
                culled,
                time: Some(cull_start.elapsed()),
            };
        }
        self.last_culling_path = path;
        self.prepare_materials(assets)?;

        let carry = self.prepared_physics.as_mut().unwrap().carry.take();
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
        self.frames_rendered += 1;
        // A recreated swapchain (resize, vsync toggle, out-of-date) brings
        // new image views. Drop entries no image has used for a while, so the
        // old framebuffers stop keeping the old swapchain alive. Held entries
        // keep their view alive, so a pointer key is never reused while cached.
        let frames_rendered = self.frames_rendered;
        self.prepared_frames
            .retain(|_, frame| frames_rendered - frame.last_used < 16);
        let bound: GraphicsBuffers = (
            physics.states.clone(),
            render_instances.instances.clone(),
            lights.buffer.clone(),
        );
        let stale = self.prepared_frames.get(&frame_key).is_none_or(|frame| {
            !same_buffer(&frame.bound.0, &bound.0)
                || !same_buffer(&frame.bound.1, &bound.1)
                || !same_buffer(&frame.bound.2, &bound.2)
        });
        if stale {
            let graphics_set = DescriptorSet::new(
                self.descriptor_allocator.clone(),
                self.pipeline.layout().set_layouts()[0].clone(),
                [
                    WriteDescriptorSet::buffer(0, bound.0.clone()),
                    WriteDescriptorSet::buffer(1, bound.1.clone()),
                    WriteDescriptorSet::buffer(2, bound.2.clone()),
                ],
                [],
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
            let framebuffer = match self.prepared_frames.remove(&frame_key) {
                Some(frame) => frame.framebuffer,
                None => Framebuffer::new(
                    self.render_pass.clone(),
                    FramebufferCreateInfo {
                        attachments: vec![
                            target.clone(),
                            self.hdr.clone(),
                            self.depth.clone(),
                        ],
                        ..Default::default()
                    },
                )
                .map_err(|error| SceneRenderError(error.to_string()))?,
            };
            self.prepared_frames.insert(
                frame_key,
                PreparedFrame {
                    graphics_set,
                    framebuffer,
                    bound,
                    last_used: frames_rendered,
                },
            );
        }
        let frame = self.prepared_frames.get_mut(&frame_key).unwrap();
        frame.last_used = frames_rendered;
        let graphics_set = frame.graphics_set.clone();
        let framebuffer = frame.framebuffer.clone();
        let orthographic = render_world.active_camera.is_some_and(|camera| {
            matches!(camera.projection, Projection::Orthographic { .. })
        });
        let camera = CameraUniform {
            view_projection: clip.into(),
            eye: if orthographic {
                [-forward[0], -forward[1], -forward[2], 0.0]
            } else {
                [eye[0], eye[1], eye[2], 1.0]
            },
            ambient: lights.ambient,
            ground_ambient: lights.ground_ambient,
            // y: shadowed light index plus one, or 0 for none.
            light_info: [
                lights.count,
                lights.shadow.map_or(0, |(index, _)| index + 1),
                options.debug_view as u32,
                0,
            ],
        };
        // The GPU paths leave `visibility` as the last CPU-culled frame's.
        let visibility = (!path.on_gpu())
            .then(|| render_instances.visibility.as_ref().unwrap());
        self.last_frame_culled = visibility.map(|visibility| visibility.culled);
        // One output set per cull dispatch that feeds draws: the early one
        // first on occlusion frames. The last set feeds the pass that also
        // draws blended instances.
        let occlusion = path == CullingPath::GpuOcclusion;
        let mut cull_sets: Vec<GpuCullSet> = Vec::new();
        if path.on_gpu() {
            for _ in 0..if occlusion { 2 } else { 1 } {
                let (list, draw_commands) = gpu_cull_buffers(
                    &self.instance_allocator,
                    render_instances,
                    &self.prepared_meshes,
                )?;
                let set = DescriptorSet::new(
                    self.descriptor_allocator.clone(),
                    self.cull_pipeline.layout().set_layouts()[0].clone(),
                    [
                        WriteDescriptorSet::buffer(0, physics.states.clone()),
                        WriteDescriptorSet::buffer(
                            1,
                            render_instances.instances.clone(),
                        ),
                        WriteDescriptorSet::buffer(
                            2,
                            render_instances.cull.clone().unwrap(),
                        ),
                        WriteDescriptorSet::buffer(3, draw_commands.clone()),
                        WriteDescriptorSet::buffer(4, list.clone()),
                        WriteDescriptorSet::buffer(
                            5,
                            render_instances.occlusion.clone().unwrap(),
                        ),
                        WriteDescriptorSet::image_view_sampler(
                            6,
                            self.depth_pyramid.view.clone(),
                            self.depth_pyramid.sampler.clone(),
                        ),
                    ],
                    [],
                )
                .map_err(|error| SceneRenderError(error.to_string()))?;
                cull_sets.push((list, draw_commands, set));
            }
        }
        #[cfg(test)]
        {
            self.last_draw_commands = cull_sets
                .iter()
                .map(|(_, commands, _)| commands.clone())
                .collect();
        }
        let gpu_cull = cull_sets.last();
        let early_cull = occlusion.then(|| &cull_sets[0]);
        let (shadow_size, shadow_distance) = shadow_settings(quality);
        if self.shadow_framebuffer.extent()[0] != shadow_size {
            // In-flight frames keep the old map alive through their command
            // buffers.
            (self.shadow_framebuffer, self.shadow_map) = create_shadow_target(
                self.shadow_framebuffer.render_pass(),
                &self.memory_allocator,
                shadow_size,
            )?;
        }
        let light_view_projection = lights.shadow.map(|(_, direction)| {
            shadow_view_projection(eye, forward, direction, shadow_distance)
        });
        let shadow_upload = self.frame_contexts[self.frame_index]
            .transient
            .allocate_sized::<ShadowUpload>()
            .map_err(|error| SceneRenderError(error.to_string()))?;
        *shadow_upload
            .write()
            .map_err(|error| SceneRenderError(error.to_string()))? =
            ShadowUpload {
                light_view_projection: light_view_projection
                    .unwrap_or_else(Matrix4::identity)
                    .into(),
            };
        let shadow_set = DescriptorSet::new(
            self.descriptor_allocator.clone(),
            self.pipeline.layout().set_layouts()[2].clone(),
            [
                WriteDescriptorSet::image_view_sampler(
                    0,
                    self.shadow_map.clone(),
                    self.shadow_sampler.clone(),
                ),
                WriteDescriptorSet::buffer(1, shadow_upload),
            ],
            [],
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;

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
        let labels = self
            .queue
            .device()
            .instance()
            .enabled_extensions()
            .ext_debug_utils;
        if labels {
            let _ = commands.begin_debug_utils_label(DebugUtilsLabel {
                label_name: "SceneRenderer::render".to_string(),
                ..Default::default()
            });
        }
        let mut passes = Vec::new();
        let uploads =
            !self.pending_texture_uploads.is_empty() || carry.is_some();
        if uploads {
            begin_pass(&mut commands, labels, &mut passes, FramePass::Uploads);
        }
        // ponytail: an error between here and submit loses these copies and
        // leaves their images unwritten; re-queue them if that ever shows up.
        for (staging, image) in self.pending_texture_uploads.drain(..) {
            commands
                .copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(
                    staging, image,
                ))
                .map_err(|error| SceneRenderError(error.to_string()))?;
        }
        if let Some((old_states, regions)) = carry {
            commands
                .copy_buffer(CopyBufferInfo {
                    regions: regions.into(),
                    ..CopyBufferInfo::buffers(
                        old_states,
                        physics.states.clone(),
                    )
                })
                .map_err(|error| SceneRenderError(error.to_string()))?;
        }
        if uploads {
            end_pass(&mut commands, labels);
        }
        if physics_ran {
            begin_pass(&mut commands, labels, &mut passes, FramePass::Physics);
            // One dispatch per fixed tick keeps the integration step at
            // `fixed_delta`; a hitch beyond the cap is dropped rather than
            // simulated as one large, tunnelling step.
            let steps = new_ticks.min(MAX_PHYSICS_STEPS_PER_FRAME);
            commands
                .bind_pipeline_compute(self.physics_pipeline.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .bind_descriptor_sets(
                    PipelineBindPoint::Compute,
                    self.physics_pipeline.layout().clone(),
                    0,
                    physics_resources.as_ref().unwrap().0.clone(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?;
            for step in 0..steps {
                let tick = render_world.physics_tick - (steps - 1 - step);
                commands
                    .push_constants(
                        self.physics_pipeline.layout().clone(),
                        0,
                        PhysicsPushConstants {
                            dt: render_world.fixed_delta_seconds,
                            elapsed: render_world.elapsed_seconds,
                            body_count: physics.source.len() as u32,
                            event_capacity: event_capacity.unwrap() as u32,
                            tick_low: tick as u32,
                            tick_high: (tick >> 32) as u32,
                            gravity_x: render_world.physics_gravity[0],
                            gravity_y: render_world.physics_gravity[1],
                            gravity_z: render_world.physics_gravity[2],
                            _padding: [0; 3],
                        },
                    )
                    .map_err(|error| SceneRenderError(error.to_string()))?;
                unsafe {
                    commands
                        .dispatch([
                            physics.source.len().div_ceil(256) as u32,
                            1,
                            1,
                        ])
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                }
            }
            self.last_physics_tick = render_world.physics_tick;
            end_pass(&mut commands, labels);
        } else if new_ticks > 0 {
            // Disabled time must not be simulated later when physics resumes.
            self.last_physics_tick = render_world.physics_tick;
        }
        let timestamps =
            self.frame_contexts[self.frame_index].timestamps.clone();
        let timestamp = |commands: &mut AutoCommandBufferBuilder<
            PrimaryAutoCommandBuffer,
        >,
                         query: u32|
         -> Result<(), SceneRenderError> {
            if let Some(pool) = &timestamps {
                // Safety: the queries are reset in this command buffer before
                // the first write, and the context's previous frame has
                // completed.
                unsafe {
                    if query == 0 {
                        commands.reset_query_pool(pool.clone(), 0..4).map_err(
                            |error| SceneRenderError(error.to_string()),
                        )?;
                    }
                    commands
                        .write_timestamp(
                            pool.clone(),
                            query,
                            PipelineStage::AllCommands,
                        )
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                }
            }
            Ok(())
        };
        let cull_count = render_instances.cull_source.len() as u32;
        let dispatch_cull = |commands: &mut AutoCommandBufferBuilder<
            PrimaryAutoCommandBuffer,
        >,
                             set: &Arc<DescriptorSet>,
                             phase: CullPhase|
         -> Result<(), SceneRenderError> {
            commands
                .bind_pipeline_compute(self.cull_pipeline.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .bind_descriptor_sets(
                    PipelineBindPoint::Compute,
                    self.cull_pipeline.layout().clone(),
                    0,
                    set.clone(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
                .push_constants(
                    self.cull_pipeline.layout().clone(),
                    0,
                    CullPushConstants {
                        clip: clip.into(),
                        viewport: [
                            viewport.offset[0] as f32,
                            viewport.offset[1] as f32,
                            viewport.extent[0] as f32,
                            viewport.extent[1] as f32,
                        ],
                        lod: lod_camera,
                        info: [
                            cull_count,
                            phase as u32,
                            u32::from(!frustum),
                            0,
                        ],
                    },
                )
                .map_err(|error| SceneRenderError(error.to_string()))?;
            unsafe {
                commands
                    .dispatch([cull_count.div_ceil(256).max(1), 1, 1])
                    .map_err(|error| SceneRenderError(error.to_string()))?;
            }
            Ok(())
        };
        if let Some((_, _, set)) = cull_sets.first() {
            begin_pass(&mut commands, labels, &mut passes, FramePass::Culling);
            timestamp(&mut commands, 0)?;
            let phase = if occlusion {
                CullPhase::Early
            } else {
                CullPhase::Frustum
            };
            dispatch_cull(&mut commands, set, phase)?;
            timestamp(&mut commands, 1)?;
            if !occlusion {
                // The second pair times the occlusion work only.
                timestamp(&mut commands, 2)?;
                timestamp(&mut commands, 3)?;
            }
            end_pass(&mut commands, labels);
        }
        let scene_viewport = Viewport {
            offset: [viewport.offset[0] as f32, viewport.offset[1] as f32],
            extent: [viewport.extent[0] as f32, viewport.extent[1] as f32],
            depth_range: 0.0..=1.0,
        };
        let scene_scissor = Scissor {
            offset: visible.offset,
            extent: visible.extent,
        };
        // The shadow map is cleared even without a shadowed light, so the
        // main pass always samples initialized depth.
        begin_pass(&mut commands, labels, &mut passes, FramePass::Shadow);
        commands
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![Some(1.0_f32.into())],
                    ..RenderPassBeginInfo::framebuffer(
                        self.shadow_framebuffer.clone(),
                    )
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
        if let Some(light_view_projection) = light_view_projection {
            // ponytail: masked casters cast as fully opaque and blended ones
            // not at all; add an alpha-tested shadow shader when scenes need
            // foliage shadows.
            commands
                .bind_pipeline_graphics(self.shadow_pipeline.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .bind_descriptor_sets(
                    PipelineBindPoint::Graphics,
                    self.pipeline.layout().clone(),
                    0,
                    graphics_set.clone(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
                .push_constants(
                    self.pipeline.layout().clone(),
                    0,
                    CameraUniform {
                        view_projection: light_view_projection.into(),
                        ..camera
                    },
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
                .set_viewport(
                    0,
                    [Viewport {
                        offset: [0.0, 0.0],
                        extent: [shadow_size as f32; 2],
                        depth_range: 0.0..=1.0,
                    }]
                    .into_iter()
                    .collect(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
                .set_scissor(
                    0,
                    [Scissor {
                        offset: [0, 0],
                        extent: [shadow_size; 2],
                    }]
                    .into_iter()
                    .collect(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?;
            for batch in &render_instances.batches {
                let Some(mesh) = self.prepared_meshes.get(&batch.mesh_key)
                else {
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
        }
        commands
            .end_render_pass(Default::default())
            .map_err(|error| SceneRenderError(error.to_string()))?;
        end_pass(&mut commands, labels);
        let texture_set = |material: Handle<MaterialAsset>| {
            self.prepared_materials
                .get(&material.key())
                .map_or(&self.white_material, |prepared| &prepared.set)
                .clone()
        };
        let indirect = |cull: Option<&GpuCullSet>, group: usize| {
            cull.map(|(_, draw_commands, _)| {
                draw_commands.clone().slice(group as u64..group as u64 + 1)
            })
        };
        // Binds the scene state and draws the opaque batches from `cull`'s
        // outputs, or from the CPU list without one. Returns the bound
        // material set.
        let draw_opaque = |commands: &mut AutoCommandBufferBuilder<
            PrimaryAutoCommandBuffer,
        >,
                           cull: Option<&GpuCullSet>|
         -> Result<
            Option<Arc<DescriptorSet>>,
            SceneRenderError,
        > {
            commands
                .bind_pipeline_graphics(self.pipeline.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .bind_descriptor_sets(
                    PipelineBindPoint::Graphics,
                    self.pipeline.layout().clone(),
                    0,
                    graphics_set.clone(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
                .bind_descriptor_sets(
                    PipelineBindPoint::Graphics,
                    self.pipeline.layout().clone(),
                    2,
                    shadow_set.clone(),
                )
                .map_err(|error| SceneRenderError(error.to_string()))?
                .push_constants(self.pipeline.layout().clone(), 0, camera)
                .map_err(|error| SceneRenderError(error.to_string()))?
                .set_viewport(0, [scene_viewport.clone()].into_iter().collect())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .set_scissor(0, [scene_scissor].into_iter().collect())
                .map_err(|error| SceneRenderError(error.to_string()))?;
            let mut bound_texture: Option<Arc<DescriptorSet>> = None;
            for (group, batch) in render_instances.batches.iter().enumerate() {
                // The GPU list reserves each batch's full range; the CPU list
                // holds only the visible instances.
                let (list, first, count) = match (cull, visibility) {
                    (Some((list, ..)), _) => {
                        (list, batch.first_instance, batch.instance_count)
                    }
                    (None, Some(visibility)) => {
                        let (first, count) = visibility.batches[group];
                        (&visibility.list, first, count)
                    }
                    (None, None) => unreachable!("every path prepares a list"),
                };
                if count == 0 {
                    continue;
                }
                let Some(mesh) = self.prepared_meshes.get(&batch.mesh_key)
                else {
                    continue;
                };
                let set = texture_set(batch.material);
                if !bound_texture
                    .as_ref()
                    .is_some_and(|bound| Arc::ptr_eq(bound, &set))
                {
                    commands
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            self.pipeline.layout().clone(),
                            1,
                            set.clone(),
                        )
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                    bound_texture = Some(set);
                }
                draw_instances(
                    commands,
                    mesh,
                    list,
                    first,
                    count,
                    indirect(cull, group),
                )?;
            }
            Ok(bound_texture)
        };
        let background = Some(render_world.background_color.into());
        if let Some(early) = early_cull {
            // Last frame's visible opaque set lays down depth for the
            // pyramid; the late pass continues its HDR and depth.
            commands
                .begin_render_pass(
                    RenderPassBeginInfo {
                        render_pass: self.early_render_pass.clone(),
                        clear_values: vec![
                            None,
                            background,
                            Some(1.0_f32.into()),
                        ],
                        ..RenderPassBeginInfo::framebuffer(framebuffer.clone())
                    },
                    SubpassBeginInfo {
                        contents: SubpassContents::Inline,
                        ..Default::default()
                    },
                )
                .map_err(|error| SceneRenderError(error.to_string()))?;
            begin_pass(&mut commands, labels, &mut passes, FramePass::Scene);
            draw_opaque(&mut commands, Some(early))?;
            end_pass(&mut commands, labels);
            commands
                .next_subpass(Default::default(), SubpassBeginInfo::default())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .end_render_pass(Default::default())
                .map_err(|error| SceneRenderError(error.to_string()))?;
            begin_pass(
                &mut commands,
                labels,
                &mut passes,
                FramePass::DepthPyramid,
            );
            timestamp(&mut commands, 2)?;
            for (level, (set, size)) in
                self.depth_pyramid.mips.iter().enumerate()
            {
                let pipeline = if level == 0 {
                    &self.depth_pyramid_copy_pipeline
                } else {
                    &self.depth_pyramid_reduce_pipeline
                };
                commands
                    .bind_pipeline_compute(pipeline.clone())
                    .map_err(|error| SceneRenderError(error.to_string()))?
                    .bind_descriptor_sets(
                        PipelineBindPoint::Compute,
                        pipeline.layout().clone(),
                        0,
                        set.clone(),
                    )
                    .map_err(|error| SceneRenderError(error.to_string()))?;
                unsafe {
                    commands
                        .dispatch([size[0].div_ceil(8), size[1].div_ceil(8), 1])
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                }
            }
            end_pass(&mut commands, labels);
            begin_pass(
                &mut commands,
                labels,
                &mut passes,
                FramePass::OcclusionCulling,
            );
            dispatch_cull(
                &mut commands,
                &gpu_cull.unwrap().2,
                CullPhase::Late,
            )?;
            timestamp(&mut commands, 3)?;
            end_pass(&mut commands, labels);
        }
        let (main_pass, clear_values) = if occlusion {
            (self.late_render_pass.clone(), vec![None, None, None])
        } else {
            (
                self.render_pass.clone(),
                vec![None, background, Some(1.0_f32.into())],
            )
        };
        commands
            .begin_render_pass(
                RenderPassBeginInfo {
                    render_pass: main_pass,
                    clear_values,
                    ..RenderPassBeginInfo::framebuffer(framebuffer)
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
        // Labels stay inside one subpass so none spans a subpass boundary.
        let scene_pass = if occlusion {
            FramePass::LateScene
        } else {
            FramePass::Scene
        };
        begin_pass(&mut commands, labels, &mut passes, scene_pass);
        let mut bound_texture = draw_opaque(&mut commands, gpu_cull)?;
        // On the GPU paths every blended instance keeps a draw whose count
        // the cull pass sets to 0 or 1.
        let (blended_list, mut blended) = match (gpu_cull, visibility) {
            (Some((list, ..)), _) => (list, render_instances.blended.clone()),
            (None, Some(visibility)) => {
                (&visibility.list, visibility.blended.clone())
            }
            (None, None) => unreachable!("every path prepares a list"),
        };
        let blended_base = render_instances
            .blended
            .first()
            .map_or(0, |item| item.instance as usize);
        if !blended.is_empty() {
            sort_back_to_front(&mut blended, eye, forward);
            commands
                .bind_pipeline_graphics(self.blend_pipeline.clone())
                .map_err(|error| SceneRenderError(error.to_string()))?;
            for item in blended {
                let Some(mesh) = self.prepared_meshes.get(&item.mesh_key)
                else {
                    continue;
                };
                let set = texture_set(item.material);
                if !bound_texture
                    .as_ref()
                    .is_some_and(|bound| Arc::ptr_eq(bound, &set))
                {
                    commands
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            self.blend_pipeline.layout().clone(),
                            1,
                            set.clone(),
                        )
                        .map_err(|error| SceneRenderError(error.to_string()))?;
                    bound_texture = Some(set);
                }
                let group = gpu_cull.is_some().then(|| {
                    render_instances.batches.len() + item.instance as usize
                        - blended_base
                });
                draw_instances(
                    &mut commands,
                    mesh,
                    blended_list,
                    item.instance,
                    1,
                    group.and_then(|group| indirect(gpu_cull, group)),
                )?;
            }
        }
        // Debug views show raw shader output, so they skip the curve.
        let tone = match options.debug_view {
            SceneDebugView::Lit => {
                render_world.tone_mapping.unwrap_or_default()
            }
            _ => ToneMapping::default(),
        };
        // The whole target is tone mapped, so area outside the viewport shows
        // the background like the old clear did.
        end_pass(&mut commands, labels);
        commands
            .next_subpass(Default::default(), SubpassBeginInfo::default())
            .map_err(|error| SceneRenderError(error.to_string()))?;
        begin_pass(&mut commands, labels, &mut passes, FramePass::ToneMap);
        commands
            .bind_pipeline_graphics(self.tonemap_pipeline.clone())
            .map_err(|error| SceneRenderError(error.to_string()))?
            .bind_descriptor_sets(
                PipelineBindPoint::Graphics,
                self.tonemap_pipeline.layout().clone(),
                0,
                self.tonemap_set.clone(),
            )
            .map_err(|error| SceneRenderError(error.to_string()))?
            .push_constants(
                self.tonemap_pipeline.layout().clone(),
                0,
                tonemap_fragment_shader::ToneMap {
                    exposure: tone.exposure,
                    mapper: tone.mapper as u32,
                },
            )
            .map_err(|error| SceneRenderError(error.to_string()))?
            .set_viewport(
                0,
                [Viewport {
                    offset: [0.0, 0.0],
                    extent: [extent[0] as f32, extent[1] as f32],
                    depth_range: 0.0..=1.0,
                }]
                .into_iter()
                .collect(),
            )
            .map_err(|error| SceneRenderError(error.to_string()))?
            .set_scissor(
                0,
                [Scissor {
                    offset: [0, 0],
                    extent,
                }]
                .into_iter()
                .collect(),
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
        unsafe {
            commands
                .draw(3, 1, 0, 0)
                .map_err(|error| SceneRenderError(error.to_string()))?;
        }
        end_pass(&mut commands, labels);
        // Debug geometry is drawn after tone mapping in the same render pass,
        // so it keeps its exact colors and uses the scene depth, camera, and
        // viewport. The optional input is never provided by the game runner.
        if let Some(overlay) = options.debug_overlay {
            begin_pass(
                &mut commands,
                labels,
                &mut passes,
                FramePass::DebugOverlay,
            );
            commands
                .set_viewport(0, [scene_viewport].into_iter().collect())
                .map_err(|error| SceneRenderError(error.to_string()))?
                .set_scissor(0, [scene_scissor].into_iter().collect())
                .map_err(|error| SceneRenderError(error.to_string()))?;
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
            end_pass(&mut commands, labels);
        }
        commands
            .end_render_pass(Default::default())
            .map_err(|error| SceneRenderError(error.to_string()))?;
        end_pass(&mut commands, labels);
        self.last_frame_passes = passes;
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
        if path.on_gpu() {
            self.pending_cull.push(PendingCullReadback {
                fence: fence.clone(),
                path,
                submitted: render_instances.cull_source.len(),
                commands: cull_sets
                    .into_iter()
                    .map(|(_, commands, _)| commands)
                    .collect(),
                timestamps: self.frame_contexts[self.frame_index]
                    .timestamps
                    .clone(),
            });
        }
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
        let lods = lod_signature(assets);
        if self.prepared_meshes_revision != render_world.renderables_revision
            || self.prepared_meshes_lods != lods
        {
            let groups = lod_groups(assets);
            self.visible_meshes = render_world
                .renderables
                .iter()
                .flat_map(|renderable| {
                    groups.get(&renderable.mesh.key()).map_or_else(
                        || vec![renderable.mesh],
                        |group| {
                            group
                                .levels
                                .iter()
                                .map(|level| level.mesh)
                                .collect()
                        },
                    )
                })
                .collect();
            self.visible_meshes.sort_unstable_by_key(|mesh| mesh.key());
            self.visible_meshes.dedup_by_key(|mesh| mesh.key());
            self.prepared_meshes_revision = render_world.renderables_revision;
            self.prepared_meshes_lods = lods;
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
                .insert(key, self.prepare_mesh(mesh_handle, mesh, revision)?);
        }
        self.capacity.missing_meshes = self
            .visible_meshes
            .iter()
            .filter(|mesh| !assets.meshes.contains(**mesh))
            .count();
        // Meshes removed from the server free their GPU buffers. In-flight
        // command buffers keep their own references until they complete.
        let visible = &self.visible_meshes;
        self.prepared_meshes.retain(|_, prepared| {
            assets.meshes.contains(prepared.handle)
                || visible.contains(&prepared.handle)
        });
        Ok(())
    }

    /// Uploads the base-color textures of the materials in use. Textures
    /// still loading are skipped and sample white until they publish.
    /// Uploads every map of the materials in use and builds their set 1.
    fn prepare_materials(
        &mut self,
        assets: &AssetServer,
    ) -> Result<(), SceneRenderError> {
        let materials = self
            .prepared_instances
            .as_ref()
            .map(|prepared| {
                prepared
                    .material_revisions
                    .iter()
                    .map(|(material, _)| *material)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let slots = |material: &MaterialAsset| {
            [
                material.base_color_texture,
                material.normal_texture,
                material.metallic_roughness_texture,
                material.occlusion_texture,
                material.emissive_texture,
            ]
        };
        let used = materials
            .iter()
            .filter_map(|material| assets.materials.get(*material))
            .flat_map(|material| slots(material).into_iter().flatten())
            .collect::<Vec<_>>();
        for &handle in &used {
            let revision = assets.textures.revision(handle).unwrap_or(0);
            if self
                .prepared_textures
                .get(&handle.key())
                .is_some_and(|prepared| prepared.source_revision == revision)
            {
                continue;
            }
            let Some(texture) = assets.textures.get(handle) else {
                continue;
            };
            let sampler = match self.samplers.get(&texture.sampler) {
                Some(sampler) => sampler.clone(),
                None => {
                    let sampler =
                        texture_sampler(&self.queue, texture.sampler)?;
                    self.samplers.insert(texture.sampler, sampler.clone());
                    sampler
                }
            };
            // A malformed texture uses the slot's missing-map stand-in
            // instead of failing the frame.
            let view = create_texture(
                &self.memory_allocator,
                texture,
                &mut self.pending_texture_uploads,
            )
            .ok();
            self.prepared_textures.insert(
                handle.key(),
                PreparedTexture {
                    handle,
                    view,
                    sampler,
                    source_revision: revision,
                },
            );
        }
        self.prepared_textures.retain(|_, prepared| {
            assets.textures.contains(prepared.handle)
                || used.contains(&prepared.handle)
        });

        let mut missing = used.clone();
        missing.retain(|texture| {
            self.prepared_textures
                .get(&texture.key())
                .is_none_or(|prepared| prepared.view.is_none())
        });
        missing.sort_unstable_by_key(|texture| texture.key());
        missing.dedup();
        self.capacity.missing_textures = missing.len();

        for &handle in &materials {
            let Some(material) = assets.materials.get(handle) else {
                continue;
            };
            let mut slot_index = 0;
            let textures = slots(material).map(|slot| {
                let fallback = &self.missing_textures[slot_index];
                slot_index += 1;
                let Some(texture) = slot else {
                    return self.white_texture.clone();
                };
                self.prepared_textures
                    .get(&texture.key())
                    .and_then(|prepared| {
                        Some((prepared.view.clone()?, prepared.sampler.clone()))
                    })
                    .unwrap_or_else(|| fallback.clone())
            });
            let views = textures
                .each_ref()
                .map(|(view, _)| Arc::as_ptr(view) as usize);
            if self
                .prepared_materials
                .get(&handle.key())
                .is_some_and(|prepared| prepared.views == views)
            {
                continue;
            }
            let set = create_material_set(
                &self.descriptor_allocator,
                &self.pipeline.layout().set_layouts()[1],
                textures,
            )?;
            self.prepared_materials
                .insert(handle.key(), PreparedMaterial { handle, set, views });
        }
        self.prepared_materials.retain(|_, prepared| {
            assets.materials.contains(prepared.handle)
                || materials.contains(&prepared.handle)
        });
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
        let mut shadow = None;
        for extracted in &render_world.directional_lights {
            if uploads.len() == budget {
                break;
            }
            let direction = light_direction(extracted.transform.matrix);
            if shadow.is_none() && extracted.light.shadows {
                shadow = Some((uploads.len() as u32, direction));
            }
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
        let uniform = match (render_world.ambient_light, render_world.sky_light)
        {
            (None, None) => [0.12; 3],
            (ambient, _) => ambient.map_or([0.0; 3], |light| {
                light.color.map(|channel| channel * light.intensity)
            }),
        };
        let hemisphere = |pick: fn(&crate::runtime::SkyLight) -> [f32; 3]| {
            let sky = render_world.sky_light.as_ref();
            let color = sky.map_or([0.0; 3], |sky| {
                pick(sky).map(|channel| channel * sky.intensity)
            });
            [
                uniform[0] + color[0],
                uniform[1] + color[1],
                uniform[2] + color[2],
                1.0,
            ]
        };
        let ambient = hemisphere(|sky| sky.sky_color);
        let ground_ambient = hemisphere(|sky| sky.ground_color);
        self.prepared_lights = Some(PreparedLights {
            revision: render_world.lights_revision,
            budget,
            buffer,
            count,
            ambient,
            ground_ambient,
            shadow,
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
        let lod_signature = lod_signature(assets);
        if self.prepared_instances.as_ref().is_some_and(|prepared| {
            prepared.renderables_revision == render_world.renderables_revision
                && prepared.lod_signature == lod_signature
                && prepared.physics_revision
                    == render_world.gpu_physics_revision
                && prepared.material_revisions.iter().all(
                    |(material, revision)| {
                        assets.materials.revision(*material).unwrap_or(0)
                            == *revision
                    },
                )
                && prepared.mesh_revisions.iter().all(|(mesh, revision)| {
                    self.prepared_meshes.get(mesh).is_some_and(|prepared| {
                        prepared.source_revision == *revision
                    })
                })
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
        self.capacity.missing_materials = materials
            .iter()
            .filter(|material| !assets.materials.contains(**material))
            .count();
        let material_revisions = materials
            .into_iter()
            .map(|material| {
                (material, assets.materials.revision(material).unwrap_or(0))
            })
            .collect();

        // An object whose mesh starts a LOD group becomes one instance per
        // level, each drawn only inside its level's range. Only the finest
        // level casts shadows.
        // ponytail: the shadow pass draws that level at every range; pick
        // shadow LODs per light when distant casters cost too much.
        let groups = lod_groups(assets);
        let mut renderables =
            Vec::with_capacity(render_world.renderables.len());
        let mut lod_ranges = Vec::with_capacity(render_world.renderables.len());
        for renderable in &render_world.renderables {
            let Some(group) = groups.get(&renderable.mesh.key()) else {
                renderables.push(*renderable);
                lod_ranges.push(None);
                continue;
            };
            let screen_size = group.metric == LodMetric::ScreenSize;
            for (level, (lod, range)) in
                group.levels.iter().zip(group.ranges()).enumerate()
            {
                renderables.push(crate::runtime::ExtractedRenderable {
                    mesh: lod.mesh,
                    cast_shadows: renderable.cast_shadows && level == 0,
                    ..*renderable
                });
                lod_ranges.push(Some([
                    range[0],
                    range[1],
                    f32::from(u8::from(screen_size)),
                    0.0,
                ]));
            }
        }
        let (order, batches, blended_start) =
            render_batch_order(&renderables, |material| {
                assets.materials.get(material).is_some_and(|material| {
                    material.alpha_mode == AlphaMode::Blend
                })
            });
        let mut instances = Vec::with_capacity(order.len().max(1));
        for &index in &order {
            let renderable = renderables[index];
            let material = assets.materials.get(renderable.material);
            let color = material
                .map_or([1.0, 0.0, 1.0, 1.0], |material| material.base_color);
            let (emissive, surface) =
                material.map_or(([0.0, 0.0, 0.0, 1.0], [0.0; 4]), |material| {
                    let unlit = material.model == MaterialModel::Unlit;
                    (
                        [
                            material.emissive[0],
                            material.emissive[1],
                            material.emissive[2],
                            f32::from(u8::from(unlit)),
                        ],
                        [
                            material.metallic,
                            material.roughness,
                            f32::from(u8::from(
                                material.normal_texture.is_some(),
                            )),
                            0.0,
                        ],
                    )
                });
            let (alpha_mode, cutoff) =
                match material.map(|material| material.alpha_mode) {
                    Some(AlphaMode::Mask { cutoff }) => (1, cutoff),
                    Some(AlphaMode::Blend) => (2, 0.0),
                    Some(AlphaMode::Opaque) | None => (0, 0.0),
                };
            instances.push(RenderInstanceUpload {
                model: renderable.transform.matrix,
                normal: normal_columns(Matrix4::from(
                    renderable.transform.matrix,
                )),
                color,
                emissive,
                surface,
                physics: [
                    physics_indices
                        .get(&renderable.entity)
                        .copied()
                        .unwrap_or(u32::MAX),
                    alpha_mode,
                    f32::to_bits(cutoff),
                    u32::from(renderable.cast_shadows)
                        | u32::from(renderable.receive_shadows) << 1,
                ],
            });
        }
        // ponytail: GPU-physics bodies sort by their last CPU transform;
        // read back positions if blended GPU bodies ever need exact order.
        let mut mesh_revisions = Vec::new();
        let mut cull_source = Vec::with_capacity(order.len());
        let mut lod_spheres = Vec::with_capacity(order.len());
        let bounds = order
            .iter()
            .map(|&index| {
                let renderable = &renderables[index];
                let mesh = self.prepared_meshes.get(&renderable.mesh.key());
                if let Some(mesh) = mesh {
                    mesh_revisions
                        .push((renderable.mesh.key(), mesh.source_revision));
                }
                let local = renderable
                    .bounds
                    .or_else(|| mesh.and_then(|mesh| mesh.bounds));
                // ponytail: the GPU tests a sphere around the box, which
                // keeps some instances the CPU box test drops; send boxes
                // if that overdraw ever shows up.
                let sphere =
                    local.map_or([0.0, 0.0, 0.0, -1.0], bounding_sphere);
                cull_source.push(CullInstance {
                    sphere,
                    slot: [0; 4],
                    lod: lod_ranges[index].unwrap_or([
                        0.0,
                        f32::INFINITY,
                        0.0,
                        0.0,
                    ]),
                });
                lod_spheres.push(lod_ranges[index].map(|_| {
                    world_sphere(&renderable.transform.matrix, sphere)
                }));
                if physics_indices.contains_key(&renderable.entity) {
                    return None;
                }
                local.map(|local| {
                    local.transformed(&renderable.transform.matrix)
                })
            })
            .collect();
        mesh_revisions.sort_unstable();
        mesh_revisions.dedup();
        for (group, batch) in batches.iter().enumerate() {
            let first = batch.first_instance as usize;
            for instance in
                &mut cull_source[first..first + batch.instance_count as usize]
            {
                instance.slot = [group as u32, batch.first_instance, 0, 0];
            }
        }
        for (offset, instance) in
            cull_source[blended_start..].iter_mut().enumerate()
        {
            let slot = (blended_start + offset) as u32;
            instance.slot = [(batches.len() + offset) as u32, slot, 1, 0];
        }
        let gpu_owned = instances
            .iter()
            .filter(|instance| instance.physics[0] != u32::MAX)
            .count();
        let blended = order[blended_start..]
            .iter()
            .enumerate()
            .map(|(offset, &index)| {
                let renderable = renderables[index];
                BlendedInstance {
                    instance: (blended_start + offset) as u32,
                    mesh_key: renderable.mesh.key(),
                    material: renderable.material,
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
            mesh_revisions,
            bounds,
            visibility: None,
            cull_source,
            cull: None,
            occlusion: None,
            gpu_owned,
            lod_spheres,
            lod_signature,
        });
        Ok(())
    }

    /// Uploads the cull-pass input once per instance rebuild.
    fn prepare_cull_instances(&mut self) -> Result<(), SceneRenderError> {
        let prepared = self.prepared_instances.as_mut().unwrap();
        if prepared.cull.is_some() {
            return Ok(());
        }
        let upload = self
            .instance_allocator
            .allocate_slice::<CullInstance>(
                prepared.cull_source.len().max(1) as DeviceSize
            )
            .map_err(|error| SceneRenderError(error.to_string()))?;
        {
            let mut write = upload
                .write()
                .map_err(|error| SceneRenderError(error.to_string()))?;
            write[0] = CullInstance::default();
            write[..prepared.cull_source.len()]
                .copy_from_slice(&prepared.cull_source);
        }
        let occlusion = self
            .instance_allocator
            .allocate_slice::<u32>(upload.len())
            .map_err(|error| SceneRenderError(error.to_string()))?;
        occlusion
            .write()
            .map_err(|error| SceneRenderError(error.to_string()))?
            .fill(0);
        prepared.cull = Some(upload);
        prepared.occlusion = Some(occlusion);
        Ok(())
    }

    /// Keeps the prepared instances inside their LOD range for `lod_camera`
    /// and, with `frustum`, inside `clip`, and uploads the compacted
    /// main-pass list. Reused while the camera and the instances stay
    /// unchanged.
    fn prepare_visibility(
        &mut self,
        clip: Matrix4<f32>,
        frustum: bool,
        lod_camera: [f32; 4],
    ) -> Result<(), SceneRenderError> {
        let prepared = self.prepared_instances.as_mut().unwrap();
        let lods = prepared.lod_spheres.iter().any(Option::is_some);
        let clip_key = (frustum || lods).then(|| (clip.into(), frustum));
        if prepared
            .visibility
            .as_ref()
            .is_some_and(|visibility| visibility.clip == clip_key)
        {
            return Ok(());
        }
        let planes = frustum.then(|| frustum_planes(&clip));
        let (list, batches, blended) =
            compact_visible(&prepared.batches, &prepared.blended, |instance| {
                let index = instance as usize;
                let lod = prepared.cull_source[index].lod;
                let in_lod = prepared.lod_spheres[index].is_none_or(|sphere| {
                    let value = lod_value(lod_camera, sphere, lod[2] != 0.0);
                    lod[0] <= value && value < lod[1]
                });
                in_lod
                    && match (&planes, prepared.bounds[index]) {
                        (Some(planes), Some(bounds)) => {
                            bounds_in_frustum(planes, &bounds)
                        }
                        _ => true,
                    }
            });
        let culled = prepared.bounds.len() - list.len();
        // A zero-length buffer is invalid; the padding entry is never drawn.
        let upload = self
            .instance_allocator
            .allocate_slice::<VisibleInstance>(list.len().max(1) as DeviceSize)
            .map_err(|error| SceneRenderError(error.to_string()))?;
        {
            let mut write = upload
                .write()
                .map_err(|error| SceneRenderError(error.to_string()))?;
            write[0] = VisibleInstance { instance_index: 0 };
            write[..list.len()].copy_from_slice(&list);
        }
        prepared.visibility = Some(PreparedVisibility {
            clip: clip_key,
            list: upload,
            batches,
            blended,
            culled,
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
        let previous = self.prepared_physics.take();
        let previous_index: HashMap<_, _> = previous
            .as_ref()
            .map(|previous| {
                previous
                    .source
                    .iter()
                    .enumerate()
                    .map(|(index, body)| (body.entity, (index, body)))
                    .collect()
            })
            .unwrap_or_default();
        // Everything before `metadata` is simulation state; metadata holds
        // rule offsets that shift whenever another body's rules change.
        let stride = size_of::<GpuBodyState>() as u64;
        let live_bytes = std::mem::offset_of!(GpuBodyState, metadata) as u64;
        let mut carry_regions = Vec::new();
        for body in &render_world.gpu_physics {
            let body_index = states.len() as u32;
            let rule_offset = rules.len() as u32;
            body_indices.insert(body.entity, body_index);
            // ponytail: rule cooldowns still reset on any edit; carry the
            // rules buffer too if that becomes visible.
            if let Some((old_index, _)) =
                previous_index.get(&body.entity).filter(|(_, old)| {
                    old.physics_id == body.physics_id
                        && old.transform == body.transform
                        && old.rigid_body == body.rigid_body
                        && old.solver == body.solver
                })
            {
                carry_regions.push(BufferCopy {
                    src_offset: *old_index as u64 * stride,
                    dst_offset: body_index as u64 * stride,
                    size: live_bytes,
                    ..Default::default()
                });
            }
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
            storage(
                BufferUsage::STORAGE_BUFFER
                    | BufferUsage::TRANSFER_SRC
                    | BufferUsage::TRANSFER_DST,
            ),
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
        let carry = match previous {
            Some(previous) if !carry_regions.is_empty() => {
                // In-flight frames may still write the old buffer.
                for context in &mut self.frame_contexts {
                    if let Some(fence) = context.fence.take() {
                        fence.wait(None).map_err(|error| {
                            SceneRenderError(error.to_string())
                        })?;
                    }
                }
                Some((previous.states, carry_regions))
            }
            _ => None,
        };
        self.prepared_physics = Some(PreparedGpuPhysics {
            source_revision: render_world.gpu_physics_revision,
            source: render_world.gpu_physics.clone(),
            body_indices,
            states,
            instructions,
            rules,
            carry,
        });
        self.last_physics_tick = self
            .last_physics_tick
            .min(render_world.physics_tick.saturating_sub(1));
        Ok(())
    }

    fn prepare_mesh(
        &self,
        handle: Handle<MeshAsset>,
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
                uv: vertex.uv,
                tangent: vertex.tangent,
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
            handle,
            vertices,
            indices,
            source_revision,
            bounds: crate::runtime::picking::mesh_bounds(mesh).map(
                |(min, max)| RenderBounds::Aabb {
                    min: min.into(),
                    max: max.into(),
                },
            ),
        })
    }

    fn ensure_depth(
        &mut self,
        extent: [u32; 2],
    ) -> Result<(), SceneRenderError> {
        if extent != self.depth_extent {
            self.depth = create_depth(&self.memory_allocator, extent)?;
            self.hdr = create_hdr(&self.memory_allocator, extent)?;
            self.depth_pyramid = create_depth_pyramid(
                &self.memory_allocator,
                &self.descriptor_allocator,
                &self.depth_pyramid_copy_pipeline,
                &self.depth_pyramid_reduce_pipeline,
                &self.depth,
            )?;
            self.tonemap_set = create_tonemap_set(
                &self.descriptor_allocator,
                &self.tonemap_pipeline,
                &self.hdr,
            )?;
            self.depth_extent = extent;
            // Cached framebuffers still point at the old depth and HDR images.
            self.prepared_frames.clear();
        }
        Ok(())
    }
}

/// Every LOD group and its revision. Instances are expanded again when it
/// changes.
fn lod_signature(assets: &AssetServer) -> Vec<(u64, u64)> {
    assets
        .lod_groups
        .iter()
        .map(|(handle, _)| {
            (
                handle.key(),
                assets.lod_groups.revision(handle).unwrap_or(0),
            )
        })
        .collect()
}

/// LOD groups by the key of their finest mesh; the first loaded wins.
fn lod_groups(assets: &AssetServer) -> HashMap<u64, &LodGroupAsset> {
    let mut groups = HashMap::new();
    for (_, group) in assets.lod_groups.iter() {
        if let Some(level) = group.levels.first() {
            groups.entry(level.mesh.key()).or_insert(group);
        }
    }
    groups
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
                material: renderable.material,
                first_instance: instance as u32,
                instance_count: 0,
            });
            previous_key = Some(key);
        }
        batches.last_mut().unwrap().instance_count += 1;
    }
    (order, batches, blended_start)
}

/// Compacts the instances `in_view` keeps into one list: each batch's
/// visible instances stay contiguous and get a `(first, count)` range, and
/// each kept blended instance gets its own slot.
fn compact_visible(
    batches: &[PreparedRenderBatch],
    blended: &[BlendedInstance],
    in_view: impl Fn(u32) -> bool,
) -> (Vec<VisibleInstance>, Vec<(u32, u32)>, Vec<BlendedInstance>) {
    let mut list = Vec::new();
    let ranges = batches
        .iter()
        .map(|batch| {
            let first = list.len() as u32;
            list.extend(
                (batch.first_instance
                    ..batch.first_instance + batch.instance_count)
                    .filter(|instance| in_view(*instance))
                    .map(|instance_index| VisibleInstance { instance_index }),
            );
            (first, list.len() as u32 - first)
        })
        .collect();
    let blended = blended
        .iter()
        .filter(|item| in_view(item.instance))
        .map(|item| {
            list.push(VisibleInstance {
                instance_index: item.instance,
            });
            BlendedInstance {
                instance: list.len() as u32 - 1,
                ..*item
            }
        })
        .collect();
    (list, ranges, blended)
}

/// Gribb-Hartmann planes `(normal, distance)` of a Vulkan clip matrix: left,
/// right, bottom, top, near (clip z >= 0), far. Inside is `n·p + d >= 0`.
fn frustum_planes(clip: &Matrix4<f32>) -> [[f32; 4]; 6] {
    let row = |index: usize| {
        let row = clip.row(index);
        [row[0], row[1], row[2], row[3]]
    };
    let [x, y, z, w] = [row(0), row(1), row(2), row(3)];
    let add = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] + b[i]);
    let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] - b[i]);
    [add(w, x), sub(w, x), add(w, y), sub(w, y), z, sub(w, z)]
}

/// False only when the bounds lie wholly outside one plane: conservative,
/// so a box near a frustum corner may pass.
fn bounds_in_frustum(planes: &[[f32; 4]; 6], bounds: &RenderBounds) -> bool {
    planes.iter().all(|plane| match *bounds {
        RenderBounds::Sphere { center, radius } => {
            let length = (plane[0] * plane[0]
                + plane[1] * plane[1]
                + plane[2] * plane[2])
                .sqrt();
            (0..3).map(|i| plane[i] * center[i]).sum::<f32>() + plane[3]
                >= -radius * length
        }
        RenderBounds::Aabb { min, max } => {
            // The corner farthest along the plane normal.
            (0..3)
                .map(|i| {
                    plane[i] * if plane[i] >= 0.0 { max[i] } else { min[i] }
                })
                .sum::<f32>()
                + plane[3]
                >= 0.0
        }
    })
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

/// Builds a sampler for one texture's filter and wrap modes. Textures have
/// no mip chain yet, so the mipmap filter is ignored.
// ponytail: no mipmaps, so minified textures alias; generate mips with
// blits when textured scenes show shimmer.
fn texture_sampler(
    queue: &Arc<Queue>,
    sampler: TextureSampler,
) -> Result<Arc<Sampler>, SceneRenderError> {
    let filter = |filter| match filter {
        TextureFilter::Nearest => Filter::Nearest,
        TextureFilter::Linear => Filter::Linear,
    };
    let wrap = |wrap| match wrap {
        TextureWrap::Repeat => SamplerAddressMode::Repeat,
        TextureWrap::MirroredRepeat => SamplerAddressMode::MirroredRepeat,
        TextureWrap::ClampToEdge => SamplerAddressMode::ClampToEdge,
    };
    Sampler::new(
        queue.device().clone(),
        SamplerCreateInfo {
            mag_filter: filter(sampler.mag_filter),
            min_filter: filter(sampler.min_filter),
            mipmap_mode: SamplerMipmapMode::Nearest,
            address_mode: [
                wrap(sampler.wrap[0]),
                wrap(sampler.wrap[1]),
                SamplerAddressMode::Repeat,
            ],
            ..Default::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))
}

/// Creates a sampled image for `texture`, queues its pixel copy for the next
/// frame, and returns the set-1 descriptor set that binds it.
/// Opens the debug label for `pass` and records it in `passes`. Labels are
/// only emitted when the instance has `ext_debug_utils`.
fn begin_pass(
    commands: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    labels: bool,
    passes: &mut Vec<FramePass>,
    pass: FramePass,
) {
    passes.push(pass);
    if labels {
        let _ = commands.begin_debug_utils_label(DebugUtilsLabel {
            label_name: pass.label().to_string(),
            ..Default::default()
        });
    }
}

fn end_pass(
    commands: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    labels: bool,
) {
    if labels {
        // Safety: every call pairs with an earlier `begin_pass` or the
        // frame's outer label in the same command buffer.
        let _ = unsafe { commands.end_debug_utils_label() };
    }
}

/// Draws `count` instances of `mesh` from `list[first..]`. With `indirect`,
/// the GPU-written command sets the real count and `count` only bounds the
/// bound range. The slice start stands in for `firstInstance`, which
/// indirect draws may not set without `drawIndirectFirstInstance`.
fn draw_instances(
    commands: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    mesh: &PreparedMesh,
    list: &Subbuffer<[VisibleInstance]>,
    first: u32,
    count: u32,
    indirect: Option<Subbuffer<[DrawIndexedIndirectCommand]>>,
) -> Result<(), SceneRenderError> {
    let range = u64::from(first)..u64::from(first + count);
    commands
        .bind_vertex_buffers(
            0,
            (mesh.vertices.clone(), list.clone().slice(range)),
        )
        .map_err(|error| SceneRenderError(error.to_string()))?
        .bind_index_buffer(mesh.indices.clone())
        .map_err(|error| SceneRenderError(error.to_string()))?;
    unsafe {
        match indirect {
            Some(indirect) => commands.draw_indexed_indirect(indirect),
            None => {
                commands.draw_indexed(mesh.indices.len() as u32, count, 0, 0, 0)
            }
        }
        .map_err(|error| SceneRenderError(error.to_string()))?;
    }
    Ok(())
}

type GpuCullBuffers = (
    Subbuffer<[VisibleInstance]>,
    Subbuffer<[DrawIndexedIndirectCommand]>,
);

/// A cull dispatch's outputs and the descriptor set it writes them through.
type GpuCullSet = (
    Subbuffer<[VisibleInstance]>,
    Subbuffer<[DrawIndexedIndirectCommand]>,
    Arc<DescriptorSet>,
);

/// Per-frame outputs of the GPU cull pass: the visible list, sized like the
/// instance buffer so each group writes into its own instance range, and
/// one draw command per opaque batch and per blended instance, in that
/// order, with a zero instance count for the pass to count up.
fn gpu_cull_buffers(
    allocator: &SubbufferAllocator,
    instances: &PreparedRenderInstances,
    meshes: &HashMap<u64, PreparedMesh>,
) -> Result<GpuCullBuffers, SceneRenderError> {
    let draws = instances
        .batches
        .iter()
        .map(|batch| batch.mesh_key)
        .chain(instances.blended.iter().map(|item| item.mesh_key))
        .map(|mesh_key| DrawIndexedIndirectCommand {
            index_count: meshes
                .get(&mesh_key)
                .map_or(0, |mesh| mesh.indices.len() as u32),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    // Fresh suballocations, so no in-flight frame reads what the host
    // writes or the pass overwrites.
    let draw_commands = allocator
        .allocate_slice::<DrawIndexedIndirectCommand>(
            draws.len().max(1) as DeviceSize
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
    {
        let mut write = draw_commands
            .write()
            .map_err(|error| SceneRenderError(error.to_string()))?;
        write[0] = DrawIndexedIndirectCommand::default();
        write[..draws.len()].copy_from_slice(&draws);
    }
    let list = allocator
        .allocate_slice::<VisibleInstance>(
            instances.cull_source.len().max(1) as DeviceSize
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
    Ok((list, draw_commands))
}

fn create_texture(
    memory_allocator: &Arc<StandardMemoryAllocator>,
    texture: &TextureAsset,
    pending: &mut Vec<PendingTextureUpload>,
) -> Result<Arc<ImageView>, SceneRenderError> {
    let [width, height] = texture.size;
    if width == 0
        || height == 0
        || texture.rgba8.len() != width as usize * height as usize * 4
    {
        return Err(SceneRenderError(format!(
            "texture of {width}x{height} has {} bytes of RGBA8 data",
            texture.rgba8.len()
        )));
    }
    let staging = Buffer::from_iter(
        memory_allocator.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
            ..Default::default()
        },
        texture.rgba8.iter().copied(),
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let image = Image::new(
        memory_allocator.clone(),
        ImageCreateInfo {
            format: match texture.color_space {
                TextureColorSpace::Srgb => Format::R8G8B8A8_SRGB,
                TextureColorSpace::Linear => Format::R8G8B8A8_UNORM,
            },
            extent: [width, height, 1],
            usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let view = ImageView::new_default(image.clone())
        .map_err(|error| SceneRenderError(error.to_string()))?;
    pending.push((staging, image));
    Ok(view)
}

/// Writes one material's set 1, maps in shader binding order.
fn create_material_set(
    descriptor_allocator: &Arc<StandardDescriptorSetAllocator>,
    layout: &Arc<DescriptorSetLayout>,
    textures: [(Arc<ImageView>, Arc<Sampler>); MATERIAL_TEXTURES],
) -> Result<Arc<DescriptorSet>, SceneRenderError> {
    DescriptorSet::new(
        descriptor_allocator.clone(),
        layout.clone(),
        textures
            .into_iter()
            .enumerate()
            .map(|(binding, (view, sampler))| {
                WriteDescriptorSet::image_view_sampler(
                    binding as u32,
                    view,
                    sampler,
                )
            }),
        [],
    )
    .map_err(|error| SceneRenderError(error.to_string()))
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
            // Sampled by the depth-pyramid copy.
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT | ImageUsage::SAMPLED,
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

/// Farthest-depth mip chain of the early occlusion pass's depth, which the
/// late cull pass tests bounds against.
struct DepthPyramid {
    /// Every mip, sampled by the cull pass.
    view: Arc<ImageView>,
    sampler: Arc<Sampler>,
    /// Per mip, in order: the set of its dispatch (the depth copy for mip 0,
    /// a reduction of the mip below for the rest) and the mip's size.
    mips: Vec<(Arc<DescriptorSet>, [u32; 2])>,
}

// ponytail: mip 0 is full resolution, so the chain has one level more than a
// half-resolution pyramid; start at half size if the build shows up in
// `CullingStats` times.
fn create_depth_pyramid(
    allocator: &Arc<StandardMemoryAllocator>,
    descriptor_allocator: &Arc<StandardDescriptorSetAllocator>,
    copy_pipeline: &ComputePipeline,
    reduce_pipeline: &ComputePipeline,
    depth: &Arc<ImageView>,
) -> Result<DepthPyramid, SceneRenderError> {
    let [width, height, _] = depth.image().extent();
    let mip_levels = 32 - width.max(height).leading_zeros();
    let image = Image::new(
        allocator.clone(),
        ImageCreateInfo {
            format: Format::R32_SFLOAT,
            extent: [width, height, 1],
            mip_levels,
            // Transfer source for readback in tests.
            usage: ImageUsage::STORAGE
                | ImageUsage::SAMPLED
                | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let sampler = Sampler::new(
        allocator.device().clone(),
        SamplerCreateInfo {
            lod: 0.0..=vulkano::image::sampler::LOD_CLAMP_NONE,
            ..SamplerCreateInfo::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let mip_view = |level: u32| {
        ImageView::new(
            image.clone(),
            ImageViewCreateInfo {
                subresource_range: ImageSubresourceRange {
                    mip_levels: level..level + 1,
                    ..image.subresource_range()
                },
                ..ImageViewCreateInfo::from_image(&image)
            },
        )
        .map_err(|error| SceneRenderError(error.to_string()))
    };
    let mut mips = Vec::with_capacity(mip_levels as usize);
    for level in 0..mip_levels {
        let (pipeline, source) = if level == 0 {
            (
                copy_pipeline,
                WriteDescriptorSet::image_view_sampler(
                    0,
                    depth.clone(),
                    sampler.clone(),
                ),
            )
        } else {
            (
                reduce_pipeline,
                WriteDescriptorSet::image_view(0, mip_view(level - 1)?),
            )
        };
        let set = DescriptorSet::new(
            descriptor_allocator.clone(),
            pipeline.layout().set_layouts()[0].clone(),
            [source, WriteDescriptorSet::image_view(1, mip_view(level)?)],
            [],
        )
        .map_err(|error| SceneRenderError(error.to_string()))?;
        mips.push((set, [(width >> level).max(1), (height >> level).max(1)]));
    }
    Ok(DepthPyramid {
        view: ImageView::new_default(image)
            .map_err(|error| SceneRenderError(error.to_string()))?,
        sampler,
        mips,
    })
}

/// Scene color format before tone mapping. Blendable and usable as a color
/// and input attachment on every Vulkan device.
const HDR_COLOR_FORMAT: Format = Format::R16G16B16A16_SFLOAT;

fn create_hdr(
    allocator: &Arc<StandardMemoryAllocator>,
    extent: [u32; 2],
) -> Result<Arc<ImageView>, SceneRenderError> {
    let image = Image::new(
        allocator.clone(),
        ImageCreateInfo {
            format: HDR_COLOR_FORMAT,
            extent: [extent[0].max(1), extent[1].max(1), 1],
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::INPUT_ATTACHMENT,
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

fn create_tonemap_set(
    allocator: &Arc<StandardDescriptorSetAllocator>,
    pipeline: &Arc<GraphicsPipeline>,
    hdr: &Arc<ImageView>,
) -> Result<Arc<DescriptorSet>, SceneRenderError> {
    DescriptorSet::new(
        allocator.clone(),
        pipeline.layout().set_layouts()[0].clone(),
        [WriteDescriptorSet::image_view(0, hdr.clone())],
        [],
    )
    .map_err(|error| SceneRenderError(error.to_string()))
}

/// Fullscreen pass in subpass 1 that maps `hdr` into the output format.
fn create_tonemap_pipeline(
    queue: Arc<Queue>,
    render_pass: Arc<RenderPass>,
) -> Result<Arc<GraphicsPipeline>, SceneRenderError> {
    let vertex = tonemap_vertex_shader::load(queue.device().clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("tonemap vertex entry point is missing".into())
        })?;
    let fragment = tonemap_fragment_shader::load(queue.device().clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("tonemap fragment entry point is missing".into())
        })?;
    let stages = [
        PipelineShaderStageCreateInfo::new(vertex),
        PipelineShaderStageCreateInfo::new(fragment),
    ];
    let layout = PipelineLayout::new(
        queue.device().clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
            .into_pipeline_layout_create_info(queue.device().clone())
            .map_err(|error| SceneRenderError(error.to_string()))?,
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let subpass = Subpass::from(render_pass, 1)
        .ok_or_else(|| SceneRenderError("tonemap subpass is missing".into()))?;
    GraphicsPipeline::new(
        queue.device().clone(),
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(Default::default()),
            input_assembly_state: Some(InputAssemblyState::default()),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(MultisampleState::default()),
            depth_stencil_state: Some(DepthStencilState::default()),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                1,
                ColorBlendAttachmentState::default(),
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
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
                    [
                        SceneVertex::per_vertex(),
                        VisibleInstance::per_instance(),
                    ]
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
                dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                    .into_iter()
                    .collect(),
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

/// Pipeline, framebuffer, shadow map, and comparison sampler.
type ShadowPass = (
    Arc<GraphicsPipeline>,
    Arc<Framebuffer>,
    Arc<ImageView>,
    Arc<Sampler>,
);

/// Creates the depth-only shadow pipeline, its fixed-size shadow map and
/// framebuffer, and the comparison sampler the main pass reads it with.
/// The shadow pass leaves the map in the layout the scene pass samples it
/// in, so the render pass performs the transition instead of a barrier.
fn shadow_map_final_layout() -> ImageLayout {
    FramePass::Shadow
        .next_layout(FrameResource::ShadowMap)
        .expect("the scene pass samples the shadow map")
}

fn create_shadow_pass(
    queue: &Arc<Queue>,
    memory_allocator: &Arc<StandardMemoryAllocator>,
    main_pipeline: &Arc<GraphicsPipeline>,
) -> Result<ShadowPass, SceneRenderError> {
    let device = queue.device().clone();
    let render_pass = vulkano::single_pass_renderpass!(
        device.clone(),
        attachments: {
            depth: {
                format: Format::D32_SFLOAT,
                samples: 1,
                load_op: Clear,
                store_op: Store,
                initial_layout: ImageLayout::Undefined,
                final_layout: shadow_map_final_layout(),
            }
        },
        pass: {
            color: [],
            depth_stencil: {depth}
        }
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let (size, _) = shadow_settings(QualityProfile::Balanced);
    let (framebuffer, shadow_map) =
        create_shadow_target(&render_pass, memory_allocator, size)?;
    // Outside the map the border depth of 1.0 compares as lit.
    let sampler = Sampler::new(
        device.clone(),
        SamplerCreateInfo {
            address_mode: [SamplerAddressMode::ClampToBorder; 3],
            border_color: BorderColor::FloatOpaqueWhite,
            compare: Some(CompareOp::LessOrEqual),
            ..Default::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let vertex = shadow_vertex_shader::load(device.clone())
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("shadow vertex entry point is missing".into())
        })?;
    let subpass = Subpass::from(render_pass, 0)
        .ok_or_else(|| SceneRenderError("shadow subpass is missing".into()))?;
    let pipeline = GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: [PipelineShaderStageCreateInfo::new(vertex.clone())]
                .into_iter()
                .collect(),
            vertex_input_state: Some(
                SceneVertex::per_vertex()
                    .definition(&vertex)
                    .map_err(|error| SceneRenderError(error.to_string()))?,
            ),
            input_assembly_state: Some(InputAssemblyState::default()),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState {
                cull_mode: CullMode::Back,
                front_face: FrontFace::CounterClockwise,
                // Slope-scaled bias keeps lit surfaces from shadowing
                // themselves at grazing light angles.
                depth_bias: Some(DepthBiasState {
                    constant_factor: 1.25,
                    clamp: 0.0,
                    slope_factor: 1.75,
                }),
                ..Default::default()
            }),
            multisample_state: Some(MultisampleState::default()),
            depth_stencil_state: Some(DepthStencilState {
                depth: Some(DepthState::simple()),
                ..Default::default()
            }),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(main_pipeline.layout().clone())
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    Ok((pipeline, framebuffer, shadow_map, sampler))
}

/// Creates a `size`-texel square shadow map and its framebuffer.
fn create_shadow_target(
    render_pass: &Arc<RenderPass>,
    memory_allocator: &Arc<StandardMemoryAllocator>,
    size: u32,
) -> Result<(Arc<Framebuffer>, Arc<ImageView>), SceneRenderError> {
    let image = Image::new(
        memory_allocator.clone(),
        ImageCreateInfo {
            format: Format::D32_SFLOAT,
            extent: [size, size, 1],

            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT | ImageUsage::SAMPLED,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    let shadow_map = ImageView::new_default(image)
        .map_err(|error| SceneRenderError(error.to_string()))?;
    let framebuffer = Framebuffer::new(
        render_pass.clone(),
        FramebufferCreateInfo {
            attachments: vec![shadow_map.clone()],
            ..Default::default()
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))?;
    Ok((framebuffer, shadow_map))
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
    // Drawn after tone mapping, so helper colors are exact.
    let subpass = Subpass::from(render_pass, 1)
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
                ColorBlendAttachmentState {
                    // Line colors carry alpha, for example fading grid lines.
                    blend: Some(AttachmentBlend::alpha()),
                    ..Default::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .map_err(|error| SceneRenderError(error.to_string()))
}

fn create_compute_pipeline(
    queue: &Arc<Queue>,
    module: Result<
        Arc<vulkano::shader::ShaderModule>,
        vulkano::Validated<vulkano::VulkanError>,
    >,
) -> Result<Arc<ComputePipeline>, SceneRenderError> {
    let shader = module
        .map_err(|error| SceneRenderError(error.to_string()))?
        .entry_point("main")
        .ok_or_else(|| {
            SceneRenderError("compute entry point is missing".into())
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

/// Active camera position and view direction; the default camera looks down
/// -Z from the origin.
fn camera_eye_forward(render_world: &RenderWorld) -> ([f32; 3], [f32; 3]) {
    render_world
        .active_camera
        .map_or(([0.0; 3], [0.0, 0.0, -1.0]), |camera| {
            (
                light_position(camera.transform.matrix),
                light_direction(camera.transform.matrix),
            )
        })
}

/// Orthographic light-space transform whose box covers the first
/// `distance` in front of the camera. Casters up to another `distance`
/// toward the light still land in the map.
// ponytail: one cascade and no texel snapping, so far shadows are coarse and
// edges shimmer as the camera moves; add cascades and snapping when scenes
// show it.
fn shadow_view_projection(
    eye: [f32; 3],
    forward: [f32; 3],
    direction: [f32; 3],
    distance: f32,
) -> Matrix4<f32> {
    let radius = distance * 0.5;
    let direction = Vector3::from(direction);
    let center = Point3::from(eye) + Vector3::from(forward) * radius;
    let light_eye = center - direction * (radius + distance);
    let up = if direction.y.abs() > 0.99 {
        Vector3::z()
    } else {
        Vector3::y()
    };
    let view = Matrix4::look_at_rh(&light_eye, &center, &up);
    let projection = Orthographic3::new(
        -radius,
        radius,
        -radius,
        radius,
        0.0,
        2.0 * radius + distance,
    )
    .to_homogeneous();
    vulkan_clip_correction() * projection * view
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
layout(location = 2) in vec2 uv;
layout(location = 3) in vec4 tangent;
layout(location = 0) out vec3 v_normal;
layout(push_constant) uniform Camera {
    mat4 view_projection;
    vec4 eye;
    vec4 ambient;
    vec4 ground_ambient;
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
    mat3x4 normal;
    vec4 color;
    vec4 emissive;
    vec4 surface;
    uvec4 physics;
};
layout(set = 0, binding = 1) readonly buffer RenderInstances {
    RenderInstance data[];
} render_instances;
layout(location = 1) out vec4 v_color;
layout(location = 2) out vec3 v_world_position;
layout(location = 3) flat out uvec3 v_alpha;
layout(location = 4) out vec2 v_uv;
layout(location = 5) out vec4 v_tangent;
layout(location = 6) flat out vec4 v_emissive;
layout(location = 7) flat out vec4 v_surface;
// Index into render_instances from the per-frame visible list.
layout(location = 4) in uint instance_index;
void main() {
    RenderInstance instance = render_instances.data[instance_index];
    mat4 model = instance.physics.x == 0xffffffffu
        ? instance.model
        : physics_states.data[instance.physics.x].model;
    vec4 world_position = model * vec4(position, 1.0);
    gl_Position = camera.view_projection * world_position;
    mat3 normal_matrix;
    if (instance.physics.x == 0xffffffffu) {
        normal_matrix = mat3(instance.normal);
    } else {
        // GPU bodies are rotation times scale with no shear, so dividing
        // each column by its squared length gives the inverse-transpose.
        mat3 linear = mat3(model);
        normal_matrix = mat3(
            linear[0] / dot(linear[0], linear[0]),
            linear[1] / dot(linear[1], linear[1]),
            linear[2] / dot(linear[2], linear[2]));
    }
    v_normal = normal_matrix * normal;
    v_color = instance.color;
    v_world_position = world_position.xyz;
    v_alpha = instance.physics.yzw;
    v_uv = uv;
    // Tangents follow the surface, so they take the model's linear part.
    v_tangent = vec4(mat3(model) * tangent.xyz, tangent.w);
    v_emissive = instance.emissive;
    v_surface = instance.surface;
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
layout(location = 3) flat in uvec3 v_alpha;
layout(location = 4) in vec2 v_uv;
layout(location = 5) in vec4 v_tangent;
layout(location = 6) flat in vec4 v_emissive;
layout(location = 7) flat in vec4 v_surface;
layout(location = 0) out vec4 f_color;
layout(set = 1, binding = 0) uniform sampler2D base_color_texture;
layout(set = 1, binding = 1) uniform sampler2D normal_texture;
layout(set = 1, binding = 2) uniform sampler2D metallic_roughness_texture;
layout(set = 1, binding = 3) uniform sampler2D occlusion_texture;
layout(set = 1, binding = 4) uniform sampler2D emissive_texture;
const float PI = 3.14159265;
layout(push_constant) uniform Camera {
    mat4 view_projection;
    vec4 eye;
    vec4 ambient;
    vec4 ground_ambient;
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
layout(set = 2, binding = 0) uniform sampler2DShadow shadow_map;
layout(set = 2, binding = 1) readonly buffer Shadow {
    mat4 light_view_projection;
} shadow;
// Fraction of the shadowed light reaching this fragment, 3x3 PCF.
vec3 hemisphere(vec3 direction) {
    return mix(
        camera.ground_ambient.rgb,
        camera.ambient.rgb,
        direction.y * 0.5 + 0.5
    );
}
float shadow_factor() {
    vec4 clip = shadow.light_view_projection * vec4(v_world_position, 1.0);
    vec3 coords = clip.xyz / clip.w;
    if (coords.z > 1.0) {
        return 1.0;
    }
    vec2 uv = coords.xy * 0.5 + 0.5;
    vec2 texel = 1.0 / vec2(textureSize(shadow_map, 0));
    float lit = 0.0;
    for (int x = -1; x <= 1; ++x) {
        for (int y = -1; y <= 1; ++y) {
            lit += texture(
                shadow_map,
                vec3(uv + vec2(x, y) * texel, coords.z)
            );
        }
    }
    return lit / 9.0;
}
void main() {
    vec4 base_color = v_color * texture(base_color_texture, v_uv);
    if (v_alpha.x == 1u && base_color.a < uintBitsToFloat(v_alpha.y)) {
        discard;
    }
    float alpha = v_alpha.x == 2u ? base_color.a : 1.0;
    // light_info.z is the SceneDebugView: 1 unshaded, 2 normals.
    bool unlit = v_emissive.w > 0.5 && camera.light_info.z != 2u;
    if (unlit || camera.light_info.z == 1u) {
        f_color = vec4(base_color.rgb, alpha);
        return;
    }
    vec3 normal = normalize(v_normal);
    if (v_surface.z > 0.5) {
        vec3 tangent = normalize(
            v_tangent.xyz - normal * dot(normal, v_tangent.xyz)
        );
        vec3 bitangent = cross(normal, tangent) * v_tangent.w;
        vec3 sampled = texture(normal_texture, v_uv).xyz * 2.0 - 1.0;
        normal = normalize(mat3(tangent, bitangent, normal) * sampled);
    }
    if (camera.light_info.z == 2u) {
        f_color = vec4(normal * 0.5 + 0.5, alpha);
        return;
    }
    // glTF packs roughness in green and metallic in blue.
    vec4 packed = texture(metallic_roughness_texture, v_uv);
    float metallic = clamp(v_surface.x * packed.b, 0.0, 1.0);
    float roughness = clamp(v_surface.y * packed.g, 0.04, 1.0);
    float occlusion = texture(occlusion_texture, v_uv).r;
    vec3 view_dir = camera.eye.w > 0.5
        ? normalize(camera.eye.xyz - v_world_position)
        : normalize(camera.eye.xyz);
    float n_dot_v = max(dot(normal, view_dir), 0.0001);
    vec3 f0 = mix(vec3(0.04), base_color.rgb, metallic);
    vec3 diffuse_color = base_color.rgb * (1.0 - metallic);
    float a2 = roughness * roughness * roughness * roughness;
    float k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    // Hemisphere environment: diffuse from the normal, specular from the
    // reflection direction with roughness-aware Fresnel.
    // ponytail: two-color hemisphere, no cubemap or HDRI; the sky system
    // milestone brings image-based lighting.
    vec3 reflected = reflect(-view_dir, normal);
    vec3 env_fresnel = f0 + (max(vec3(1.0 - roughness), f0) - f0)
        * pow(1.0 - n_dot_v, 5.0);
    vec3 result = (hemisphere(normal) * diffuse_color * (1.0 - env_fresnel)
        + hemisphere(reflected) * env_fresnel) * occlusion;
    // ponytail: every fragment loops over every uploaded light, bounded by
    // the quality profile's light budget; add clustered or tiled culling if
    // scenes need more local lights than the budget.
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
        if (index + 1u == camera.light_info.y && (v_alpha.z & 2u) != 0u) {
            attenuation *= shadow_factor();
        }
        float n_dot_l = max(dot(normal, to_light), 0.0);
        if (n_dot_l <= 0.0) {
            continue;
        }
        // Cook-Torrance: GGX distribution, Smith-Schlick geometry,
        // Schlick Fresnel.
        vec3 half_dir = normalize(to_light + view_dir);
        float n_dot_h = max(dot(normal, half_dir), 0.0);
        float v_dot_h = max(dot(view_dir, half_dir), 0.0);
        float d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
        float distribution = a2 / (PI * d * d);
        float geometry = n_dot_v / (n_dot_v * (1.0 - k) + k)
            * n_dot_l / (n_dot_l * (1.0 - k) + k);
        vec3 fresnel = f0 + (1.0 - f0) * pow(1.0 - v_dot_h, 5.0);
        vec3 specular = distribution * geometry * fresnel
            / (4.0 * n_dot_v * n_dot_l + 0.0001);
        vec3 radiance = light.color_intensity.rgb
            * light.color_intensity.w * attenuation;
        // Light intensity is scaled so a white Lambert surface facing a
        // unit light reflects 1, hence the PI on the specular lobe.
        result += ((1.0 - fresnel) * diffuse_color + specular * PI)
            * radiance * n_dot_l;
    }
    result += v_emissive.rgb * texture(emissive_texture, v_uv).rgb;
    f_color = vec4(result, alpha);
}
"
                        }
}

#[rustfmt::skip]
mod shadow_vertex_shader {
    vulkano_shaders::shader! {
                            ty: "vertex",
                            src: r"
#version 450
layout(location = 0) in vec3 position;
layout(push_constant) uniform Camera {
    mat4 view_projection;
    vec4 eye;
    vec4 ambient;
    vec4 ground_ambient;
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
    mat3x4 normal;
    vec4 color;
    vec4 emissive;
    vec4 surface;
    uvec4 physics;
};
layout(set = 0, binding = 1) readonly buffer RenderInstances {
    RenderInstance data[];
} render_instances;
void main() {
    RenderInstance instance = render_instances.data[gl_InstanceIndex];
    if ((instance.physics.w & 1u) == 0u) {
        // Every vertex at one point: a zero-area triangle draws nothing.
        gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
        return;
    }
    mat4 model = instance.physics.x == 0xffffffffu
        ? instance.model
        : physics_states.data[instance.physics.x].model;
    gl_Position = camera.view_projection * model * vec4(position, 1.0);
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
    // Clip to the near plane (z = 0 in Vulkan clip space) before the divide,
    // so an endpoint behind the camera does not flip the screen direction.
    if (start_clip.z < 0.0 && end_clip.z < 0.0) {
        gl_Position = vec4(0.0, 0.0, -1.0, 1.0);
        v_color = vec4(0.0);
        return;
    }
    if (start_clip.z < 0.0) {
        start_clip = mix(start_clip, end_clip, start_clip.z / (start_clip.z - end_clip.z));
    } else if (end_clip.z < 0.0) {
        end_clip = mix(end_clip, start_clip, end_clip.z / (end_clip.z - start_clip.z));
    }
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
mod tonemap_vertex_shader {
    vulkano_shaders::shader! {
        ty: "vertex",
        src: r"
#version 450
void main() {
    // One triangle that covers the whole viewport.
    vec2 uv = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position = vec4(uv * 2.0 - 1.0, 0.0, 1.0);
}
"
    }
}

#[rustfmt::skip]
mod tonemap_fragment_shader {
    vulkano_shaders::shader! {
        ty: "fragment",
        src: r"
#version 450
layout(input_attachment_index = 0, set = 0, binding = 0) uniform subpassInput scene;
// mapper follows ToneMapper: 0 Linear, 1 Reinhard, 2 ACES.
layout(push_constant) uniform ToneMap {
    float exposure;
    uint mapper;
} tone;
layout(location = 0) out vec4 f_color;
void main() {
    vec4 hdr = subpassLoad(scene);
    vec3 c = max(hdr.rgb * tone.exposure, 0.0);
    if (tone.mapper == 1u) {
        c = c / (1.0 + c);
    } else if (tone.mapper == 2u) {
        // Narkowicz 2015 fit of the ACES filmic curve.
        c = (c * (2.51 * c + 0.03)) / (c * (2.43 * c + 0.59) + 0.14);
    }
    f_color = vec4(clamp(c, 0.0, 1.0), hdr.a);
}
"
    }
}

/// Frustum-culls every instance against its newest transform (GPU physics
/// state for GPU bodies) and appends the visible ones to their draw's range
/// of the visible list, counting them in the draw's `instanceCount`.
#[rustfmt::skip]
mod cull_shader {
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
layout(set = 0, binding = 0) readonly buffer PhysicsStates {
    PhysicsState data[];
} physics_states;
struct RenderInstance {
    mat4 model;
    mat3x4 normal;
    vec4 color;
    vec4 emissive;
    vec4 surface;
    uvec4 physics;
};
layout(set = 0, binding = 1) readonly buffer RenderInstances {
    RenderInstance data[];
} render_instances;
struct CullInstance {
    vec4 sphere;
    uvec4 slot;
    vec4 lod;
};
layout(set = 0, binding = 2) readonly buffer CullInstances {
    CullInstance data[];
} cull_instances;
struct DrawCommand {
    uint index_count;
    uint instance_count;
    uint first_index;
    int vertex_offset;
    uint first_instance;
};
layout(set = 0, binding = 3) buffer DrawCommands {
    DrawCommand data[];
} draw_commands;
layout(set = 0, binding = 4) writeonly buffer VisibleList {
    uint data[];
} visible;
layout(set = 0, binding = 5) buffer Occlusion {
    uint data[];
} occlusion;
layout(set = 0, binding = 6) uniform sampler2D depth_pyramid;
layout(push_constant) uniform Cull {
    mat4 clip;
    // Scene viewport offset and extent in depth-pyramid pixels.
    vec4 viewport;
    // xyz: camera position; w: tan(fov / 2), or minus half the view height
    // for an orthographic camera.
    vec4 lod;
    // x: instance count; y: phase (0 frustum, 1 early, 2 late); z: 1 to
    // skip the frustum test.
    uvec4 info;
} cull;

vec4 clip_row(int row) {
    return vec4(cull.clip[0][row], cull.clip[1][row], cull.clip[2][row], cull.clip[3][row]);
}

bool in_frustum(vec3 center, float radius) {
    vec4 x = clip_row(0);
    vec4 y = clip_row(1);
    vec4 z = clip_row(2);
    vec4 w = clip_row(3);
    vec4 planes[6] = vec4[6](w + x, w - x, w + y, w - y, z, w - z);
    for (int plane = 0; plane < 6; ++plane) {
        vec4 p = planes[plane];
        if (dot(p.xyz, center) + p.w < -radius * length(p.xyz)) {
            return false;
        }
    }
    return true;
}

// Distance to the camera, or the inverse of the view-height fraction the
// sphere covers; grows with distance. Mirrors `lod_value` in Rust.
float lod_value(vec3 center, float radius, bool screen_size) {
    float d = distance(center, cull.lod.xyz);
    if (!screen_size) {
        return d;
    }
    if (radius <= 0.0) {
        return 0.0;
    }
    return (cull.lod.w > 0.0 ? d * cull.lod.w : -cull.lod.w) / radius;
}

// True when the depth pyramid proves the sphere lies behind nearer depth
// everywhere it can cover on screen.
bool occluded(vec3 center, float radius) {
    vec2 low = vec2(1.0);
    vec2 high = vec2(-1.0);
    float nearest = 1.0;
    for (int corner = 0; corner < 8; ++corner) {
        vec3 offset = vec3(
            (corner & 1) != 0 ? radius : -radius,
            (corner & 2) != 0 ? radius : -radius,
            (corner & 4) != 0 ? radius : -radius);
        vec4 p = cull.clip * vec4(center + offset, 1.0);
        if (p.w <= 1e-5) {
            // Crosses the camera plane: the screen rectangle is unbounded.
            return false;
        }
        vec3 ndc = p.xyz / p.w;
        low = min(low, ndc.xy);
        high = max(high, ndc.xy);
        nearest = min(nearest, ndc.z);
    }
    ivec2 size = textureSize(depth_pyramid, 0);
    ivec2 first = clamp(ivec2(floor(cull.viewport.xy + (low * 0.5 + 0.5) * cull.viewport.zw)),
        ivec2(0), size - 1);
    ivec2 last = clamp(ivec2(floor(cull.viewport.xy + (high * 0.5 + 0.5) * cull.viewport.zw)),
        ivec2(0), size - 1);
    // The mip where the rectangle spans at most two texels per axis.
    int span = max(last.x - first.x, last.y - first.y) + 1;
    int mip = min(int(ceil(log2(float(span)))), textureQueryLevels(depth_pyramid) - 1);
    ivec2 mip_last = textureSize(depth_pyramid, mip) - 1;
    ivec2 low_texel = min(first >> mip, mip_last);
    ivec2 high_texel = min(last >> mip, mip_last);
    float farthest = 0.0;
    for (int y = low_texel.y; y <= high_texel.y; ++y) {
        for (int x = low_texel.x; x <= high_texel.x; ++x) {
            farthest = max(farthest, texelFetch(depth_pyramid, ivec2(x, y), mip).r);
        }
    }
    return nearest > farthest;
}

void main() {
    uint index = gl_GlobalInvocationID.x;
    if (index >= cull.info.x) {
        return;
    }
    uint phase = cull.info.y;
    CullInstance instance = cull_instances.data[index];
    RenderInstance render = render_instances.data[index];
    mat4 model = render.physics.x == 0xffffffffu
        ? render.model
        : physics_states.data[render.physics.x].model;
    vec3 center = (model * vec4(instance.sphere.xyz, 1.0)).xyz;
    float scale = sqrt(max(
        max(dot(model[0].xyz, model[0].xyz), dot(model[1].xyz, model[1].xyz)),
        dot(model[2].xyz, model[2].xyz)));
    float radius = instance.sphere.w * scale;
    float lod = lod_value(center, radius, instance.lod.z != 0.0);
    // `frustum` means in view before occlusion: in its LOD range and, with
    // bounds and the test enabled, in the frustum.
    bool bounded = instance.sphere.w >= 0.0;
    bool frustum = instance.lod.x <= lod && lod < instance.lod.y
        && (!bounded || cull.info.z != 0u || in_frustum(center, radius));
    bool shown = frustum && (phase != 2u || !bounded || !occluded(center, radius));
    // The early phase draws last frame's visible opaque instances; the late
    // phase recomputes this before overwriting the history.
    bool early = phase != 0u && frustum && occlusion.data[index] != 0u
        && instance.slot.z == 0u;
    if (phase == 2u) {
        occlusion.data[index] = shown ? 1u : 0u;
    }
    bool emit = phase == 0u ? shown : (phase == 1u ? early : shown && !early);
    if (!emit) {
        return;
    }
    uint slot = atomicAdd(draw_commands.data[instance.slot.x].instance_count, 1u);
    visible.data[instance.slot.y + slot] = index;
}
"
            }
}

/// Copies the scene depth into mip 0 of the depth pyramid.
#[rustfmt::skip]
mod depth_pyramid_copy_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0) uniform sampler2D source;
layout(set = 0, binding = 1, r32f) uniform writeonly image2D target;

void main() {
    ivec2 texel = ivec2(gl_GlobalInvocationID.xy);
    if (any(greaterThanEqual(texel, imageSize(target)))) {
        return;
    }
    imageStore(target, texel, vec4(texelFetch(source, texel, 0).r));
}
"
    }
}

/// Writes one depth-pyramid mip as the farthest depth of the texels it
/// covers in the mip below.
#[rustfmt::skip]
mod depth_pyramid_reduce_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0, r32f) uniform readonly image2D source;
layout(set = 0, binding = 1, r32f) uniform writeonly image2D target;

void main() {
    ivec2 texel = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = imageSize(target);
    if (any(greaterThanEqual(texel, size))) {
        return;
    }
    ivec2 first = texel * 2;
    // Mips halve rounding down, so the last row and column also cover the
    // odd source texel left over.
    ivec2 last = mix(first + 1, imageSize(source) - 1, equal(texel, size - 1));
    float farthest = 0.0;
    for (int y = first.y; y <= last.y; ++y) {
        for (int x = first.x; x <= last.x; ++x) {
            farthest = max(farthest, imageLoad(source, ivec2(x, y)).r);
        }
    }
    imageStore(target, texel, vec4(farthest));
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
            offset_of!(RenderInstanceUpload, emissive),
            offset_of!(Reflected, emissive)
        );
        assert_eq!(
            offset_of!(RenderInstanceUpload, surface),
            offset_of!(Reflected, surface)
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
            offset_of!(CameraUniform, ground_ambient),
            offset_of!(ReflectedCamera, ground_ambient)
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
    fn frustum_test_keeps_bounds_that_touch_the_view_and_drops_the_rest() {
        // Default camera: at the origin looking down -Z, 60 degree vertical
        // field of view, near 0.1, far 1000, square aspect.
        let planes = frustum_planes(&view_projection(
            &RenderWorld::default(),
            [100, 100],
        ));
        let sphere = |center, radius| RenderBounds::Sphere { center, radius };
        let aabb = |min, max| RenderBounds::Aabb { min, max };
        // The left plane passes x = -10 tan 30 at z = -10; a center 0.5
        // beyond it sits 0.5 cos 30 = 0.433 from the plane.
        let edge = -10.0 * (30.0_f32).to_radians().tan() - 0.5;
        for (bounds, expected) in [
            (sphere([0.0, 0.0, -10.0], 1.0), true),
            (sphere([0.0, 0.0, 10.0], 1.0), false),
            (sphere([0.0, 0.0, -1100.0], 1.0), false),
            (sphere([edge, 0.0, -10.0], 0.5), true),
            (sphere([edge, 0.0, -10.0], 0.3), false),
            (aabb([5.0, -1.0, -11.0], [6.0, 1.0, -9.0]), true),
            (aabb([7.0, -1.0, -11.0], [8.0, 1.0, -9.0]), false),
            (aabb([-1.0, -1.0, -0.5], [1.0, 1.0, 1.0]), true),
            (aabb([-1.0, -1.0, -0.05], [1.0, 1.0, 1.0]), false),
            (aabb([-1.0, 9.0, -11.0], [1.0, 10.0, -9.0]), false),
            // In front of the camera but short of the near plane.
            (aabb([-0.01, -0.01, -0.07], [0.01, 0.01, -0.05]), false),
        ] {
            assert_eq!(
                bounds_in_frustum(&planes, &bounds),
                expected,
                "{bounds:?}"
            );
        }
    }

    #[test]
    fn compaction_keeps_batches_contiguous_and_slots_blended_instances() {
        let assets = AssetServer::default();
        let batch = |first_instance, instance_count| PreparedRenderBatch {
            mesh_key: 0,
            material: assets.fallback_material,
            first_instance,
            instance_count,
        };
        let blended = |instance| BlendedInstance {
            instance,
            mesh_key: 0,
            material: assets.fallback_material,
            position: [0.0; 3],
        };
        let culled = [2, 4, 5, 8];
        let (list, ranges, kept) = compact_visible(
            &[batch(0, 3), batch(3, 3), batch(6, 1)],
            &[blended(7), blended(8), blended(9)],
            |instance| !culled.contains(&instance),
        );
        let list = list
            .iter()
            .map(|entry| entry.instance_index)
            .collect::<Vec<_>>();
        assert_eq!(list, [0, 1, 3, 6, 7, 9]);
        assert_eq!(ranges, [(0, 2), (2, 1), (3, 1)]);
        // Kept blended instances name their slot, which holds them.
        let slots = kept.iter().map(|item| item.instance).collect::<Vec<_>>();
        assert_eq!(slots, [4, 5]);
        assert_eq!([list[4], list[5]], [7, 9]);
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
                bounds: None,
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
                bounds: None,
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
                material: glass,
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
        debug_view: SceneDebugView,
    }

    impl SlabScene {
        fn new(slabs: &[(f32, MaterialAsset)]) -> Self {
            Self::with_extent(slabs, [8, 8])
        }

        fn with_extent(
            slabs: &[(f32, MaterialAsset)],
            extent: [u32; 2],
        ) -> Self {
            use crate::rendering::swapchain::OFFSCREEN_COLOR_FORMAT;
            let base = crate::rendering::test_support::headless_device();
            let memory_allocator = Arc::new(
                StandardMemoryAllocator::new_default(base.device.clone()),
            );
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
            // Test scenes are far below `AUTO_DIRECT_MAX_INSTANCES`, so
            // `Auto` would never cull them.
            render_world.culling = CullingMode::Frustum;
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
                        bounds: None,
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
                debug_view: SceneDebugView::Lit,
            }
        }

        fn render(&mut self, before: Box<dyn GpuFuture>) -> Box<dyn GpuFuture> {
            self.renderer
                .render(
                    before,
                    ImageView::new_default(self.image.clone()).unwrap(),
                    self.extent,
                    SceneRenderOptions {
                        debug_view: self.debug_view,
                        ..SceneRenderOptions::game(self.extent)
                    },
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
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn base_color_texture_tints_the_surface_and_reloads_next_frame() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let texture = |rgba: [u8; 4]| TextureAsset {
            size: [2, 2],
            rgba8: rgba.repeat(4),
            color_space: TextureColorSpace::Srgb,
            sampler: TextureSampler::default(),
        };
        // Unlit, so the environment's specular does not tint the texel.
        let unlit = MaterialAsset {
            model: MaterialModel::Unlit,
            ..Default::default()
        };
        let mut scene = SlabScene::new(&[(0.0, unlit)]);
        let green = scene.assets.textures.insert(texture([0, 255, 0, 255]));
        let material = scene.render_world.renderables[0].material;
        scene
            .assets
            .materials
            .get_mut(material)
            .unwrap()
            .base_color_texture = Some(green);
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        let [b, g, r, _] = scene.center_pixel();
        assert!(
            g > 200 && r < 30 && b < 30,
            "green texture, got {b} {g} {r}"
        );

        // A replaced texture value gets a new revision and is re-uploaded.
        *scene.assets.textures.get_mut(green).unwrap() =
            texture([255, 0, 0, 255]);
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        let [b, g, r, _] = scene.center_pixel();
        assert!(r > 200 && g < 30 && b < 30, "red texture, got {b} {g} {r}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn material_without_texture_samples_white() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let [b, g, r, _] =
            render_center_pixel(&[(0.0, MaterialAsset::default())]);
        assert!(b > 200 && g > 200 && r > 200, "got {b} {g} {r}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn directional_light_shadow_darkens_receivers_only_when_enabled() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        // Floor slab at z = 0 fills the view; the occluder slab at x = 5 is
        // outside the view, and the light leans so its shadow lands on the
        // view center.
        let mut scene = SlabScene::new(&[
            (0.0, MaterialAsset::default()),
            (1.0, MaterialAsset::default()),
        ]);
        scene.render_world.ambient_light = Some(crate::runtime::AmbientLight {
            color: [0.0; 3],
            intensity: 0.0,
        });
        scene.render_world.renderables[0].receive_shadows = true;
        scene.render_world.renderables[1].cast_shadows = true;
        scene.render_world.renderables[1].transform.matrix =
            (Matrix4::new_translation(&Vector3::new(5.0, 0.0, 1.0))
                * Matrix4::new_nonuniform_scaling(&Vector3::new(
                    4.0, 4.0, 0.1,
                )))
            .into();
        let direction = Vector3::new(-5.0, 0.0, -1.0).normalize();
        scene.render_world.directional_lights.push(
            crate::runtime::ExtractedDirectionalLight {
                entity: bevy_ecs::entity::Entity::from_raw_u32(2000).unwrap(),
                transform: crate::runtime::GlobalTransform {
                    matrix: nalgebra::Rotation3::rotation_between(
                        &-Vector3::z(),
                        &direction,
                    )
                    .unwrap()
                    .to_homogeneous()
                    .into(),
                },
                light: crate::runtime::DirectionalLight {
                    color: [1.0; 3],
                    illuminance: 500_000.0,
                    shadows: true,
                },
            },
        );
        let frame = |scene: &mut SlabScene| {
            scene.render_world.renderables_revision += 1;
            scene.render_world.lights_revision += 1;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            scene.center_pixel()[2]
        };
        let shadowed = frame(&mut scene);
        assert!(
            shadowed < 30,
            "occluder shadows the floor, got r={shadowed}"
        );

        // The occluder's mesh box lies outside the view: the main pass
        // culls it, the shadow pass still draws it.
        assert_eq!(scene.renderer.last_frame_culled(), Some(1));
        let stats = scene.renderer.culling_stats();
        assert_eq!(
            (stats.path, stats.submitted, stats.visible, stats.culled),
            (CullingPath::Cpu, 2, 1, 1)
        );
        assert!(stats.time.is_some());
        scene.render_world.culling = CullingMode::Disabled;
        let shadowed = frame(&mut scene);
        assert_eq!(scene.renderer.last_frame_culled(), Some(0));
        assert!(shadowed < 30, "unculled caster shadows, got r={shadowed}");
        scene.render_world.culling = CullingMode::Frustum;

        // A local override that misses the view culls a drawn object.
        scene.render_world.renderables[1].cast_shadows = false;
        let lit = frame(&mut scene);
        scene.render_world.renderables[0].bounds = Some(RenderBounds::Sphere {
            center: [0.0, 0.0, 500.0],
            radius: 1.0,
        });
        let culled = frame(&mut scene);
        assert_eq!(scene.renderer.last_frame_culled(), Some(2));
        assert!(
            culled + 100 < lit,
            "culled floor is not drawn, {culled} vs {lit}"
        );
        scene.render_world.renderables[1].cast_shadows = true;
        scene.render_world.renderables[0].bounds = None;

        scene.render_world.renderables[1].cast_shadows = false;
        let lit = frame(&mut scene);
        assert!(lit > 150, "non-caster leaves the floor lit, got r={lit}");

        scene.render_world.renderables[1].cast_shadows = true;
        scene.render_world.renderables[0].receive_shadows = false;
        let lit = frame(&mut scene);
        assert!(lit > 150, "non-receiver ignores shadows, got r={lit}");

        scene.render_world.renderables[0].receive_shadows = true;
        scene.render_world.directional_lights[0].light.shadows = false;
        let lit = frame(&mut scene);
        assert!(lit > 150, "light without shadows lights it, got r={lit}");

        // From 40 units away the floor lies past the `Eco` shadow distance
        // but inside the `Balanced` and `High` ones, and each profile
        // renders into its own map size.
        scene.render_world.directional_lights[0].light.shadows = true;
        scene.render_world.active_camera.as_mut().unwrap().transform =
            crate::runtime::GlobalTransform {
                matrix: Matrix4::new_translation(&Vector3::new(0.0, 0.0, 40.0))
                    .into(),
            };
        for (quality, size, shadowed) in [
            (QualityProfile::Eco, 1024, false),
            (QualityProfile::Balanced, 2048, true),
            (QualityProfile::High, 4096, true),
            (QualityProfile::Eco, 1024, false),
        ] {
            scene.render_world.quality = quality;
            let red = frame(&mut scene);
            assert_eq!(red < 30, shadowed, "{quality:?}, got r={red}");
            assert_eq!(scene.renderer.shadow_framebuffer.extent(), [size; 2]);
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn culling_follows_the_camera_and_mesh_edits_without_scene_changes() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        // Two unlit cubes in one batch: the first left of the ±1
        // orthographic view, the second small and centered.
        let unlit = MaterialAsset {
            model: MaterialModel::Unlit,
            ..MaterialAsset::default()
        };
        let mut scene = SlabScene::new(&[(0.0, unlit.clone()), (0.0, unlit)]);
        let material = scene.render_world.renderables[0].material;
        scene.render_world.renderables[1].material = material;
        scene.render_world.renderables[0].transform.matrix =
            Matrix4::new_translation(&Vector3::new(-3.0, 0.0, 0.0)).into();
        scene.render_world.renderables[1].transform.matrix =
            Matrix4::new_scaling(0.5).into();
        let frame = |scene: &mut SlabScene| {
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            scene.center_pixel()
        };
        let center = frame(&mut scene);
        assert_eq!(scene.renderer.last_frame_culled(), Some(1));
        assert!(
            center[0] > 200,
            "the kept instance, not the culled one, draws: {center:?}"
        );

        // Growing the mesh is an asset edit, not a scene change: the
        // renderer re-derives the bounds from the re-uploaded mesh.
        for vertex in &mut scene
            .assets
            .meshes
            .get_mut(scene.assets.fallback_mesh)
            .unwrap()
            .vertices
        {
            vertex.position[0] *= 10.0;
            vertex.position[1] *= 10.0;
        }
        frame(&mut scene);
        assert_eq!(scene.renderer.last_frame_culled(), Some(0));

        // Moving only the camera re-culls the unchanged instances.
        scene
            .render_world
            .active_camera
            .as_mut()
            .unwrap()
            .transform
            .matrix =
            Matrix4::new_translation(&Vector3::new(50.0, 0.0, 5.0)).into();
        frame(&mut scene);
        assert_eq!(scene.renderer.last_frame_culled(), Some(2));
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn pbr_maps_and_unlit_model_shape_the_lit_color() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        use crate::assets::Assets;
        // Renders one camera-facing slab and returns the center [r, g, b].
        let render = |ambient: f32,
                      light: Option<Vector3<f32>>,
                      material: &dyn Fn(
            &mut Assets<TextureAsset>,
        ) -> MaterialAsset| {
            let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
            let handle = scene.render_world.renderables[0].material;
            let material = material(&mut scene.assets.textures);
            *scene.assets.materials.get_mut(handle).unwrap() = material;
            scene.render_world.ambient_light =
                Some(crate::runtime::AmbientLight {
                    color: [ambient; 3],
                    intensity: 1.0,
                });
            if let Some(direction) = light {
                scene.render_world.directional_lights.push(
                    crate::runtime::ExtractedDirectionalLight {
                        entity: bevy_ecs::entity::Entity::from_raw_u32(2000)
                            .unwrap(),
                        transform: crate::runtime::GlobalTransform {
                            matrix: nalgebra::Rotation3::rotation_between(
                                &-Vector3::z(),
                                &direction.normalize(),
                            )
                            .unwrap()
                            .to_homogeneous()
                            .into(),
                        },
                        light: crate::runtime::DirectionalLight {
                            color: [1.0; 3],
                            illuminance: 50_000.0,
                            shadows: false,
                        },
                    },
                );
            }
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let [b, g, r, _] = scene.center_pixel();
            [r, g, b]
        };
        let texture = |rgba: [u8; 4], color_space| TextureAsset {
            size: [1, 1],
            rgba8: rgba.to_vec(),
            color_space,
            sampler: TextureSampler::default(),
        };
        let facing = Some(-Vector3::z());
        let red = [1.0, 0.0, 0.0, 1.0];

        let unlit = render(0.0, facing, &|_| MaterialAsset {
            model: MaterialModel::Unlit,
            base_color: red,
            ..MaterialAsset::default()
        });
        assert_eq!(unlit, [255, 0, 0], "unlit shows base color as is");
        let dark = render(0.0, None, &|_| MaterialAsset {
            base_color: red,
            ..MaterialAsset::default()
        });
        assert_eq!(dark, [0, 0, 0], "PBR without light is black");

        let emissive =
            |textures: &mut Assets<TextureAsset>, masked: bool| MaterialAsset {
                emissive: [0.0, 1.0, 0.0],
                emissive_texture: masked.then(|| {
                    textures.insert(texture(
                        [255, 0, 0, 255],
                        TextureColorSpace::Srgb,
                    ))
                }),
                ..MaterialAsset::default()
            };
        assert_eq!(render(0.0, None, &|t| emissive(t, false))[1], 255);
        assert_eq!(
            render(0.0, None, &|t| emissive(t, true)),
            [0, 0, 0],
            "emissive map multiplies the factor"
        );

        let occluded = |textures: &mut Assets<TextureAsset>| MaterialAsset {
            occlusion_texture: Some(
                textures
                    .insert(texture([0, 0, 0, 255], TextureColorSpace::Linear)),
            ),
            ..MaterialAsset::default()
        };
        assert_eq!(render(1.0, None, &|_| MaterialAsset::default()), [255; 3]);
        assert_eq!(
            render(1.0, None, &occluded),
            [0, 0, 0],
            "occlusion map darkens ambient"
        );

        // A rough white metal has no diffuse lobe, so it is darker than a
        // dielectric under the same light. A metallic-roughness map with
        // blue 0 turns the metal back into the dielectric.
        let surface = |metallic: f32, map: Option<[u8; 4]>| {
            move |textures: &mut Assets<TextureAsset>| MaterialAsset {
                metallic,
                roughness: 1.0,
                metallic_roughness_texture: map.map(|rgba| {
                    textures.insert(texture(rgba, TextureColorSpace::Linear))
                }),
                ..MaterialAsset::default()
            }
        };
        let dielectric = render(0.0, facing, &surface(0.0, None))[0];
        let metal = render(0.0, facing, &surface(1.0, None))[0];
        let unmasked =
            render(0.0, facing, &surface(1.0, Some([0, 255, 0, 255])))[0];
        assert!(
            metal + 40 < dielectric,
            "metal {metal} vs dielectric {dielectric}"
        );
        assert!(
            unmasked.abs_diff(dielectric) <= 2,
            "map blue 0 removes metalness: {unmasked} vs {dielectric}"
        );

        // A normal map tilting the surface toward a grazing light brightens
        // it. The slab's tangent is +X, so tangent-space (1, 0, 1) faces
        // the light.
        let grazing = Some(Vector3::new(-1.0, 0.0, -1.0));
        let tilted =
            |textures: &mut Assets<TextureAsset>, mapped: bool| MaterialAsset {
                roughness: 1.0,
                normal_texture: mapped.then(|| {
                    textures.insert(texture(
                        [218, 128, 218, 255],
                        TextureColorSpace::Linear,
                    ))
                }),
                ..MaterialAsset::default()
            };
        let flat = render(0.0, grazing, &|t| tilted(t, false))[0];
        let mapped = render(0.0, grazing, &|t| tilted(t, true))[0];
        assert!(mapped > flat + 15, "normal map {mapped} vs flat {flat}");
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
    fn culling_path_follows_mode_ownership_count_device_and_quality() {
        let capabilities = |integrated_gpu| RendererCapabilities {
            device_name: String::new(),
            integrated_gpu,
            device_local_bytes: 8 << 30,
            multi_draw_indirect: Capability::default(),
            draw_indirect_count: Capability::default(),
            bindless_textures: Capability::default(),
            memory_budget: Capability::default(),
            timestamp_queries: false,
        };
        let path = |mode, instances, gpu_owned, quality, integrated| {
            select_culling_path(
                mode,
                instances,
                gpu_owned,
                false,
                quality,
                &capabilities(integrated),
            )
        };
        let (auto, high) = (CullingMode::Auto, QualityProfile::High);
        let big = GPU_CULL_MIN_INSTANCES;
        let small = AUTO_DIRECT_MAX_INSTANCES;
        // `Auto` draws small scenes directly, GPU bodies included; an
        // explicit `Frustum` still culls them.
        assert_eq!(path(auto, small - 1, 0, high, false), CullingPath::Direct);
        assert_eq!(path(auto, small - 1, 1, high, false), CullingPath::Direct);
        assert_eq!(path(auto, small, 0, high, false), CullingPath::Cpu);
        let frustum = CullingMode::Frustum;
        assert_eq!(path(frustum, 10, 0, high, false), CullingPath::Cpu);
        assert_eq!(path(frustum, 10, 1, high, false), CullingPath::Gpu);
        // Occlusion needs the GPU path at any size and on any device.
        let occlusion = CullingMode::FrustumAndOcclusion;
        assert_eq!(
            path(occlusion, 1, 0, high, true),
            CullingPath::GpuOcclusion
        );
        assert_eq!(
            path(occlusion, 1, 1, QualityProfile::Eco, false),
            CullingPath::GpuOcclusion
        );
        assert!(
            CullingPath::GpuOcclusion.on_gpu() && CullingPath::Gpu.on_gpu()
        );
        assert!(!CullingPath::Cpu.on_gpu() && !CullingPath::Direct.on_gpu());
        assert_eq!(path(auto, big - 1, 0, high, false), CullingPath::Cpu);
        assert_eq!(path(auto, big, 0, high, false), CullingPath::Gpu);
        assert_eq!(
            path(CullingMode::Frustum, big, 0, high, false),
            CullingPath::Gpu
        );
        // GPU-owned bodies need the GPU whatever the count.
        assert_eq!(path(auto, small, 1, high, false), CullingPath::Gpu);
        assert_eq!(path(auto, small, 1, high, true), CullingPath::Gpu);
        // Integrated GPUs and Eco raise the bar four times.
        assert_eq!(path(auto, big, 0, high, true), CullingPath::Cpu);
        assert_eq!(
            path(auto, big, 0, QualityProfile::Eco, false),
            CullingPath::Cpu
        );
        assert_eq!(path(auto, big * 4, 0, high, true), CullingPath::Gpu);
        // Disabled draws everything, even GPU bodies.
        assert_eq!(
            path(CullingMode::Disabled, big * 4, 1, high, false),
            CullingPath::Direct
        );
        // LOD levels need a path that picks one: never `Direct`.
        let lod_path = |mode, gpu_owned| {
            select_culling_path(
                mode,
                small - 1,
                gpu_owned,
                true,
                high,
                &capabilities(false),
            )
        };
        assert_eq!(lod_path(auto, 0), CullingPath::Cpu);
        assert_eq!(lod_path(CullingMode::Disabled, 0), CullingPath::Cpu);
        assert_eq!(lod_path(CullingMode::Disabled, 1), CullingPath::Gpu);
        assert_eq!(lod_path(occlusion, 0), CullingPath::GpuOcclusion);
    }

    #[test]
    fn lod_value_grows_with_distance_like_group_ranges() {
        // A perspective camera at the origin with a 90 degree view.
        let camera = [0.0, 0.0, 0.0, 1.0];
        let sphere = [0.0, 0.0, -10.0, 1.0];
        assert_eq!(lod_value(camera, sphere, false), 10.0);
        // Radius 1 at distance 10 spans a tenth of the view height.
        assert_eq!(lod_value(camera, sphere, true), 10.0);
        let group = LodGroupAsset {
            metric: LodMetric::ScreenSize,
            levels: [0.5, 0.2, 0.05]
                .map(|until| crate::assets::LodLevel {
                    mesh: AssetServer::default().fallback_mesh,
                    until,
                })
                .to_vec(),
        };
        let ranges = group.ranges();
        let level = |value| {
            ranges
                .iter()
                .position(|[start, end]| start <= &value && &value < end)
        };
        assert_eq!(level(lod_value(camera, sphere, true)), Some(2));
        assert_eq!(
            level(lod_value(camera, [0.0, 0.0, -3.0, 1.0], true)),
            Some(1)
        );
        assert_eq!(
            level(lod_value(camera, [0.0, 0.0, -1.0, 1.0], true)),
            Some(0)
        );
        assert_eq!(
            level(lod_value(camera, [0.0, 0.0, -50.0, 1.0], true)),
            None
        );
        // Orthographic size ignores distance: half height 2 over radius 1.
        assert_eq!(lod_value([0.0, 0.0, 0.0, -2.0], sphere, true), 2.0);
        // Without bounds an object counts as close.
        assert_eq!(lod_value(camera, [0.0, 0.0, -10.0, -1.0], true), 0.0);
        // The CPU sphere matches the shader's: the largest scale sizes it.
        let model = (Matrix4::new_translation(&Vector3::new(1.0, 0.0, 0.0))
            * Matrix4::new_nonuniform_scaling(&Vector3::new(2.0, 3.0, 1.0)))
        .into();
        assert_eq!(
            world_sphere(&model, [1.0, 0.0, 0.0, 1.0]),
            [3.0, 0.0, 0.0, 3.0]
        );
    }

    #[test]
    fn cull_gpu_layouts_match_shader_structs() {
        assert_eq!(std::mem::size_of::<CullInstance>(), 48);
        assert_eq!(std::mem::size_of::<DrawIndexedIndirectCommand>(), 20);
        // Clip matrix, viewport, LOD camera, and info: under the 128-byte
        // guaranteed push-constant range.
        assert_eq!(std::mem::size_of::<CullPushConstants>(), 112);
        // The shader's phase numbers.
        assert_eq!(
            [CullPhase::Frustum, CullPhase::Early, CullPhase::Late]
                .map(|phase| phase as u32),
            [0, 1, 2]
        );
        assert_eq!(
            bounding_sphere(RenderBounds::Aabb {
                min: [-1.0, -2.0, -2.0],
                max: [1.0, 2.0, 2.0],
            }),
            [0.0, 0.0, 0.0, 3.0]
        );
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
    fn debug_lines_blend_with_their_alpha() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[]);
        scene.render_world.background_color = [1.0, 1.0, 1.0, 1.0];
        let mut overlay = RenderDebugOverlay::default();
        overlay.lines.push(DebugLine {
            start: [-1.0, 0.0, 0.0],
            end: [1.0, 0.0, 0.0],
            color: [1.0, 0.0, 0.0, 0.5],
            thickness: 4.0,
            on_top: true,
        });
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
        assert_eq!(r, 255);
        // Blending happens in linear space on the sRGB target, so half
        // alpha stores about 188 rather than 128. Opaque red would store 0.
        assert!((150..=220).contains(&b) && b == g, "{b} {g}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn frames_record_the_declared_passes_with_their_layouts() {
        use crate::rendering::frame_passes::{FramePass, FrameResource};
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
        // The second frame has no uploads or physics ticks left.
        for _ in 0..2 {
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
        }
        assert_eq!(
            scene.renderer.last_frame_passes(),
            [
                FramePass::Shadow,
                FramePass::Scene,
                FramePass::ToneMap,
                FramePass::DebugOverlay
            ]
        );
        let shadow = scene.renderer.shadow_framebuffer.render_pass();
        assert_eq!(
            shadow.attachments()[0].final_layout,
            FramePass::Shadow
                .next_layout(FrameResource::ShadowMap)
                .unwrap()
        );
        let main = &scene.renderer.render_pass;
        let hdr_read = FramePass::ToneMap.accesses()[0];
        assert_eq!(hdr_read.resource, FrameResource::HdrColor);
        assert_eq!(
            main.subpasses()[1].input_attachments[0]
                .as_ref()
                .unwrap()
                .layout,
            hdr_read.layout.unwrap()
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn oversized_viewport_is_cropped_not_squashed() {
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
        // Twice the target height: the line at the view's center now falls
        // on the target's bottom edge, not through its center.
        let before = scene.now();
        scene
            .renderer
            .render(
                before,
                ImageView::new_default(scene.image.clone()).unwrap(),
                scene.extent,
                SceneRenderOptions {
                    viewport: SceneViewport {
                        offset: [0, 0],
                        extent: [scene.extent[0], scene.extent[1] * 2],
                    },
                    debug_overlay: Some(&overlay),
                    debug_view: SceneDebugView::Lit,
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
        assert_ne!([b, g, r], [0, 0, 255]);
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn removed_mesh_assets_leave_the_gpu_cache() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        let cube = scene.assets.meshes.get(scene.assets.fallback_mesh).cloned();
        let mesh = scene.assets.meshes.insert(cube.unwrap());
        scene.render_world.renderables[0].mesh = mesh;
        scene.render_world.renderables_revision += 1;
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        assert!(scene.renderer.prepared_meshes.contains_key(&mesh.key()));

        scene.render_world.renderables[0].mesh = scene.assets.fallback_mesh;
        scene.render_world.renderables_revision += 1;
        scene.assets.meshes.remove(mesh).unwrap();
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        assert!(!scene.renderer.prepared_meshes.contains_key(&mesh.key()));
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
    fn culled_gpu_bodies_keep_simulating() {
        use crate::runtime::{
            ExtractedGpuPhysicsBody, ExtractedGpuPhysicsRule, GpuCondition,
            GpuEventId, GpuEventMode, GpuEventPayload,
        };
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(
            0.0,
            MaterialAsset {
                model: MaterialModel::Unlit,
                ..MaterialAsset::default()
            },
        )]);
        let world = &mut scene.render_world;
        // Far above the ±1 view, so every frame culls it.
        world.renderables[0].transform.matrix =
            Matrix4::new_translation(&Vector3::new(0.0, 5.0, 0.0)).into();
        world.gpu_physics = vec![ExtractedGpuPhysicsBody {
            entity: world.renderables[0].entity,
            physics_id: Default::default(),
            transform: crate::Transform::new([0.0, 5.0, 0.0]),
            rigid_body: Default::default(),
            solver: Default::default(),
            // Fires only once gravity has moved the body.
            rules: vec![ExtractedGpuPhysicsRule {
                event_id: GpuEventId(9),
                instructions: GpuCondition::position_y()
                    .less_than(5.0)
                    .compile()
                    .unwrap(),
                mode: GpuEventMode::WhileTrue,
                payload: GpuEventPayload::None,
                cooldown_seconds: 0.0,
            }],
        }];
        world.gpu_physics_revision = 1;
        world.physics_enabled = true;
        world.physics_gravity = [0.0, -9.81, 0.0];
        world.fixed_delta_seconds = 1.0 / 60.0;
        for tick in 1..=4 {
            scene.render_world.physics_tick = tick;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let commands = scene.renderer.last_draw_commands[0].clone();
            assert_eq!(commands.read().unwrap()[0].instance_count, 0);
            let events = scene.renderer.take_completed_physics_events();
            assert_eq!(events.len(), 1, "tick {tick}: {events:?}");
            assert_eq!(events[0].event_id, 9);
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn gpu_bodies_are_not_culled_by_their_stale_cpu_transform() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(
            0.0,
            MaterialAsset {
                model: MaterialModel::Unlit,
                ..MaterialAsset::default()
            },
        )]);
        // The CPU transform is off-view; the GPU state, which the vertex
        // shader draws, sits in the view center.
        let renderable = &mut scene.render_world.renderables[0];
        renderable.transform.matrix =
            Matrix4::new_translation(&Vector3::new(-3.0, 0.0, 0.0)).into();
        scene.render_world.gpu_physics =
            vec![crate::runtime::ExtractedGpuPhysicsBody {
                entity: renderable.entity,
                physics_id: Default::default(),
                transform: crate::Transform::new([0.0, 0.0, 0.0]),
                rigid_body: Default::default(),
                solver: Default::default(),
                rules: Vec::new(),
            }];
        scene.render_world.gpu_physics_revision = 1;
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        // A GPU body sends the frame down the GPU path, which culls by the
        // GPU state and keeps the body.
        assert_eq!(scene.renderer.last_culling_path(), CullingPath::Gpu);
        assert_eq!(scene.renderer.last_frame_culled(), None);
        let center = scene.center_pixel();
        assert!(center[0] > 200, "GPU body draws: {center:?}");

        // `Auto` draws this one-instance scene directly, with no dispatch.
        scene.render_world.culling = CullingMode::Auto;
        let before = scene.now();
        scene
            .render(before)
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        assert_eq!(scene.renderer.last_culling_path(), CullingPath::Direct);
        assert!(!scene
            .renderer
            .last_frame_passes()
            .contains(&FramePass::Culling));
        assert_eq!(scene.renderer.last_frame_culled(), Some(0));
        let center = scene.center_pixel();
        assert!(center[0] > 200, "GPU body draws directly: {center:?}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn gpu_culling_counts_visible_instances_from_gpu_transforms_into_indirect_draws(
    ) {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let unlit = MaterialAsset {
            model: MaterialModel::Unlit,
            ..MaterialAsset::default()
        };
        let blend = MaterialAsset {
            alpha_mode: AlphaMode::Blend,
            ..unlit.clone()
        };
        let mut scene = SlabScene::new(&[
            (0.0, unlit.clone()),
            (0.0, unlit),
            (0.0, blend.clone()),
            (0.0, blend),
        ]);
        let renderables = &mut scene.render_world.renderables;
        renderables[1].material = renderables[0].material;
        let at = |x: f32, scale: f32| crate::runtime::GlobalTransform {
            matrix: (Matrix4::new_translation(&Vector3::new(x, 0.0, 0.0))
                * Matrix4::new_scaling(scale))
            .into(),
        };
        // Opaque batch: a GPU body whose stale CPU transform is centered but
        // whose GPU state sits left of the view, and a small centered cube.
        renderables[0].transform = at(0.0, 0.5);
        renderables[1].transform = at(0.0, 0.5);
        // Two small centered blended instances, one draw each.
        renderables[2].transform = at(0.0, 0.25);
        renderables[3].transform = at(0.0, 0.25);
        scene.render_world.gpu_physics =
            vec![crate::runtime::ExtractedGpuPhysicsBody {
                entity: renderables[0].entity,
                physics_id: Default::default(),
                transform: crate::Transform::new([-3.0, 0.0, 0.0]),
                rigid_body: Default::default(),
                solver: Default::default(),
                rules: Vec::new(),
            }];
        scene.render_world.gpu_physics_revision = 1;
        let frame = |scene: &mut SlabScene| {
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let commands = scene.renderer.last_draw_commands[0].clone();
            let counts = commands
                .read()
                .unwrap()
                .iter()
                .map(|command| command.instance_count)
                .collect::<Vec<_>>();
            (counts, scene.center_pixel())
        };

        let (counts, center) = frame(&mut scene);
        assert_eq!(scene.renderer.last_culling_path(), CullingPath::Gpu);
        assert!(scene
            .renderer
            .last_frame_passes()
            .contains(&FramePass::Culling));
        // One opaque batch, then one draw per blended instance.
        assert_eq!(
            counts,
            [1, 1, 1],
            "the GPU body is the one instance out of view"
        );
        assert!(center[0] > 200, "the static cube draws: {center:?}");
        let stats = scene.renderer.culling_stats();
        assert_eq!(
            (stats.path, stats.submitted, stats.visible, stats.culled),
            (CullingPath::Gpu, 4, 3, 1),
            "counts read back from the indirect commands"
        );
        assert!(stats.time.is_some(), "cull dispatch is timed");

        // Centered on the GPU body, the same batch keeps only the body, and
        // it draws from its visible-list slot at the GPU position. The
        // blended instances are now off-view.
        scene.render_world.active_camera.as_mut().unwrap().transform =
            crate::runtime::GlobalTransform {
                matrix: Matrix4::new_translation(&Vector3::new(-3.0, 0.0, 5.0))
                    .into(),
            };
        let (counts, center) = frame(&mut scene);
        assert_eq!(counts, [1, 0, 0]);
        assert!(center[0] > 200, "the GPU body draws: {center:?}");
        let stats = scene.renderer.culling_stats();
        assert_eq!((stats.visible, stats.culled), (1, 3));
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn occlusion_culls_hidden_objects_and_draws_revealed_ones_the_same_frame() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let unlit = |base_color| MaterialAsset {
            model: MaterialModel::Unlit,
            base_color,
            ..MaterialAsset::default()
        };
        let mut scene = SlabScene::new(&[
            (0.0, unlit([0.0, 1.0, 0.0, 1.0])),
            (-1.0, unlit([1.0, 0.0, 0.0, 1.0])),
        ]);
        scene.render_world.culling = CullingMode::FrustumAndOcclusion;
        // A small red cube right behind the full-screen green wall.
        scene.render_world.renderables[1].transform.matrix =
            (Matrix4::new_translation(&Vector3::new(0.0, 0.0, -1.0))
                * Matrix4::new_scaling(0.5))
            .into();
        let frame = |scene: &mut SlabScene| {
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let drawn = scene
                .renderer
                .last_draw_commands
                .iter()
                .map(|commands| {
                    commands
                        .read()
                        .unwrap()
                        .iter()
                        .map(|command| command.instance_count)
                        .sum::<u32>()
                })
                .collect::<Vec<_>>();
            (drawn, scene.center_pixel())
        };

        // No history yet: the late phase draws both. The next frame draws
        // both early, and the late phase finds the cube behind the wall.
        let (drawn, _) = frame(&mut scene);
        assert_eq!(
            scene.renderer.last_culling_path(),
            CullingPath::GpuOcclusion
        );
        assert_eq!(drawn, [0, 2], "[early, late] draws");
        assert_eq!(
            scene.renderer.last_frame_passes(),
            [
                FramePass::Uploads,
                FramePass::Culling,
                FramePass::Shadow,
                FramePass::Scene,
                FramePass::DepthPyramid,
                FramePass::OcclusionCulling,
                FramePass::LateScene,
                FramePass::ToneMap,
            ]
        );
        let (drawn, _) = frame(&mut scene);
        assert_eq!(drawn, [2, 0]);
        let (drawn, [_, g, r, _]) = frame(&mut scene);
        assert_eq!(drawn, [1, 0], "the hidden cube is not drawn");
        assert_eq!((r, g), (0, 255));
        let stats = scene.renderer.culling_stats();
        assert_eq!(
            (stats.path, stats.submitted, stats.visible, stats.culled),
            (CullingPath::GpuOcclusion, 2, 1, 1)
        );
        assert!(stats.time.is_some(), "cull and pyramid are timed");

        // The camera moves past the wall, which keeps the visibility
        // history: the cube shows up in this very frame, drawn by the late
        // phase against a pyramid without the wall.
        scene.render_world.active_camera.as_mut().unwrap().transform =
            crate::runtime::GlobalTransform {
                matrix: Matrix4::new_translation(&Vector3::new(0.0, 0.0, -0.5))
                    .into(),
            };
        let (drawn, [_, g, r, _]) = frame(&mut scene);
        // The wall's loose bounding sphere still reaches the frustum, so it
        // draws early and is clipped away; the cube draws late.
        assert_eq!(drawn, [1, 1]);
        assert_eq!((r, g), (255, 0), "no one-frame pop");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn lod_groups_draw_the_level_for_the_camera_distance_on_every_path() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(
            0.0,
            MaterialAsset {
                model: MaterialModel::Unlit,
                base_color: [0.0, 1.0, 0.0, 1.0],
                ..MaterialAsset::default()
            },
        )]);
        scene.render_world.renderables[0].cast_shadows = true;
        // The coarse level is the slab moved off the view center, so the
        // center pixel tells the levels apart.
        let base = scene.assets.fallback_mesh;
        let mut shifted = scene.assets.meshes.get(base).unwrap().clone();
        for vertex in &mut shifted.vertices {
            vertex.position[0] += 0.6;
        }
        let coarse = scene.assets.meshes.insert(shifted);
        scene.assets.lod_groups.insert(LodGroupAsset {
            metric: LodMetric::Distance,
            levels: vec![
                crate::assets::LodLevel {
                    mesh: base,
                    until: 7.0,
                },
                crate::assets::LodLevel {
                    mesh: coarse,
                    until: 11.0,
                },
            ],
        });
        // Returns the meshes drawn and whether the center pixel is green.
        let frame = |scene: &mut SlabScene, camera_z: f32| {
            scene.render_world.active_camera.as_mut().unwrap().transform =
                crate::runtime::GlobalTransform {
                    matrix: Matrix4::new_translation(&Vector3::new(
                        0.0, 0.0, camera_z,
                    ))
                    .into(),
                };
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let prepared = scene.renderer.prepared_instances.as_ref().unwrap();
            let counts = if scene.renderer.last_culling_path().on_gpu() {
                scene
                    .renderer
                    .last_draw_commands
                    .iter()
                    .map(|commands| {
                        commands
                            .read()
                            .unwrap()
                            .iter()
                            .map(|command| command.instance_count)
                            .collect::<Vec<_>>()
                    })
                    .reduce(|a, b| {
                        a.iter().zip(b).map(|(a, b)| a + b).collect()
                    })
                    .unwrap()
            } else {
                let visibility = prepared.visibility.as_ref().unwrap();
                visibility.batches.iter().map(|(_, count)| *count).collect()
            };
            let drawn = prepared
                .batches
                .iter()
                .zip(counts)
                .filter(|(_, count)| *count > 0)
                .map(|(batch, count)| {
                    assert_eq!(count, 1);
                    batch.mesh_key
                })
                .collect::<Vec<_>>();
            (drawn, scene.center_pixel()[1] == 255)
        };
        for (mode, path) in [
            (CullingMode::Frustum, CullingPath::Cpu),
            // `Disabled` still picks a level, so it cannot draw directly.
            (CullingMode::Disabled, CullingPath::Cpu),
            (CullingMode::FrustumAndOcclusion, CullingPath::GpuOcclusion),
        ] {
            scene.render_world.culling = mode;
            assert_eq!(
                frame(&mut scene, 5.0),
                (vec![base.key()], true),
                "{mode:?}"
            );
            assert_eq!(scene.renderer.last_culling_path(), path);
            assert_eq!(
                frame(&mut scene, 9.0),
                (vec![coarse.key()], false),
                "{mode:?}"
            );
            assert_eq!(frame(&mut scene, 13.0), (vec![], false), "{mode:?}");
            assert_eq!(
                frame(&mut scene, 5.0),
                (vec![base.key()], true),
                "{mode:?}"
            );
        }
        let stats = scene.renderer.culling_stats();
        assert_eq!(
            (stats.submitted, stats.visible, stats.culled),
            (2, 1, 1),
            "one instance per level"
        );
        // Only the finest level casts shadows.
        let prepared = scene.renderer.prepared_instances.as_ref().unwrap();
        let instances = prepared.instances.read().unwrap();
        let casters = prepared
            .batches
            .iter()
            .map(|batch| {
                (
                    batch.mesh_key,
                    instances[batch.first_instance as usize].physics[3] & 1,
                )
            })
            .collect::<HashMap<_, _>>();
        assert_eq!((casters[&base.key()], casters[&coarse.key()]), (1, 0));
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn depth_pyramid_mips_hold_the_farthest_depth_below_them() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        // Odd sizes, so the reduction must fold the leftover row and column.
        let extent = [7, 5];
        let mut scene = SlabScene::with_extent(
            &[(
                0.0,
                MaterialAsset {
                    model: MaterialModel::Unlit,
                    ..MaterialAsset::default()
                },
            )],
            extent,
        );
        scene.render_world.culling = CullingMode::FrustumAndOcclusion;
        // Pixels are 0.4 wide over the ±1.4 by ±1 view. The wall leaves a
        // one-pixel border of cleared depth, so a reduction that drops the
        // leftover column and row, or keeps anything but the farthest texel,
        // loses it.
        scene.render_world.renderables[0].transform.matrix =
            Matrix4::new_nonuniform_scaling(&Vector3::new(2.0, 1.2, 0.1))
                .into();
        // The second frame draws the wall in the early pass.
        for _ in 0..2 {
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
        }
        let image = scene.renderer.depth_pyramid.view.image().clone();
        let sizes = scene.renderer.depth_pyramid.mips.iter().map(|mip| mip.1);
        assert_eq!(sizes.clone().collect::<Vec<_>>(), [[7, 5], [3, 2], [1, 1]]);
        let mut builder = AutoCommandBufferBuilder::primary(
            Arc::new(StandardCommandBufferAllocator::new(
                scene.base.device.clone(),
                Default::default(),
            )),
            scene.base.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();
        let mut buffers = Vec::new();
        for (level, [width, height]) in sizes.enumerate() {
            let buffer = Buffer::new_slice::<f32>(
                scene.memory_allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_DST,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                    ..Default::default()
                },
                u64::from(width * height),
            )
            .unwrap();
            builder
                .copy_image_to_buffer(vulkano::command_buffer::CopyImageToBufferInfo {
                    regions: [vulkano::command_buffer::BufferImageCopy {
                        image_subresource:
                            vulkano::image::ImageSubresourceLayers {
                                mip_level: level as u32,
                                ..image.subresource_layers()
                            },
                        image_extent: [width, height, 1],
                        ..Default::default()
                    }]
                    .into(),
                    ..vulkano::command_buffer::CopyImageToBufferInfo::image_buffer(
                        image.clone(),
                        buffer.clone(),
                    )
                })
                .unwrap();
            buffers.push(((width, height), buffer));
        }
        scene
            .now()
            .then_execute(scene.base.queue.clone(), builder.build().unwrap())
            .unwrap()
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        let mips = buffers
            .iter()
            .map(|(size, buffer)| (*size, buffer.read().unwrap().to_vec()))
            .collect::<Vec<_>>();
        let ((width, height), base) = &mips[0];
        for y in 0..*height {
            for x in 0..*width {
                let depth = base[(y * width + x) as usize];
                if (1..width - 1).contains(&x) && (1..height - 1).contains(&y) {
                    assert!(depth < 0.5, "wall at {x},{y}: {base:?}");
                } else {
                    assert_eq!(depth, 1.0, "cleared at {x},{y}: {base:?}");
                }
            }
        }
        for pair in mips.windows(2) {
            let (
                ((source_width, source_height), source),
                ((width, height), mip),
            ) = (&pair[0], &pair[1]);
            for y in 0..*height {
                for x in 0..*width {
                    let last = |texel: u32, size: u32, source_size: u32| {
                        if texel == size - 1 {
                            source_size - 1
                        } else {
                            texel * 2 + 1
                        }
                    };
                    let mut farthest = 0.0_f32;
                    for sy in y * 2..=last(y, *height, *source_height) {
                        for sx in x * 2..=last(x, *width, *source_width) {
                            farthest = farthest
                                .max(source[(sy * source_width + sx) as usize]);
                        }
                    }
                    assert_eq!(mip[(y * width + x) as usize], farthest);
                }
            }
        }
        assert_eq!(mips[2].1, [1.0], "the top mip is the farthest depth");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn adding_a_gpu_body_keeps_live_state_of_the_others() {
        use crate::runtime::ExtractedGpuPhysicsBody;
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let body = |raw, y| ExtractedGpuPhysicsBody {
            entity: bevy_ecs::entity::Entity::from_raw_u32(raw).unwrap(),
            physics_id: Default::default(),
            transform: crate::Transform::new([0.0, y, 0.0]),
            rigid_body: Default::default(),
            solver: Default::default(),
            rules: Vec::new(),
        };
        let mut scene = SlabScene::new(&[]);
        scene.render_world.gpu_physics = vec![body(3000, 5.0)];
        scene.render_world.gpu_physics_revision = 1;
        scene.render_world.physics_enabled = true;
        scene.render_world.fixed_delta_seconds = 1.0 / 60.0;
        scene.render_world.physics_gravity = [0.0, -9.81, 0.0];
        let step = |scene: &mut SlabScene, tick| {
            scene.render_world.physics_tick = tick;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
        };
        for tick in 1..=30 {
            step(&mut scene, tick);
        }
        let states = |scene: &SlabScene| {
            scene
                .renderer
                .prepared_physics
                .as_ref()
                .unwrap()
                .states
                .clone()
        };
        let fallen = states(&scene).read().unwrap()[0].model[3][1];
        assert!(fallen < 4.9, "body fell before the edit: {fallen}");

        // A new body in front shifts the survivor to a new slot.
        scene.render_world.gpu_physics.insert(0, body(3001, 20.0));
        scene.render_world.gpu_physics_revision = 2;
        step(&mut scene, 31);
        let after = states(&scene).read().unwrap()[1].model[3][1];
        assert!(after < fallen, "survivor restarted: {after} vs {fallen}");
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
    fn missing_assets_draw_visible_fallbacks_and_are_counted() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        // Unlit, so the center pixel is the sampled base color.
        let unlit = MaterialAsset {
            model: MaterialModel::Unlit,
            ..Default::default()
        };
        let mut scene = SlabScene::new(&[(0.0, unlit)]);
        // Renders one frame and returns the center pixel as [r, g, b].
        let frame = |scene: &mut SlabScene| {
            scene.render_world.renderables_revision += 1;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let [b, g, r, _] = scene.center_pixel();
            [r, g, b]
        };
        let magenta = [255, 0, 255];
        let material = scene.render_world.renderables[0].material;
        let removed = scene.assets.textures.insert(TextureAsset {
            size: [1, 1],
            rgba8: vec![0, 255, 0, 255],
            color_space: TextureColorSpace::Srgb,
            sampler: TextureSampler::default(),
        });
        scene.assets.textures.remove(removed).unwrap();
        scene
            .assets
            .materials
            .get_mut(material)
            .unwrap()
            .base_color_texture = Some(removed);
        assert_eq!(frame(&mut scene), magenta, "removed base color map");
        assert_eq!(scene.renderer.capacity_diagnostics().missing_textures, 1);

        let malformed = scene.assets.textures.insert(TextureAsset {
            size: [2, 2],
            rgba8: vec![0, 255, 0, 255],
            color_space: TextureColorSpace::Srgb,
            sampler: TextureSampler::default(),
        });
        scene
            .assets
            .materials
            .get_mut(material)
            .unwrap()
            .base_color_texture = Some(malformed);
        assert_eq!(frame(&mut scene), magenta, "malformed base color map");
        assert_eq!(scene.renderer.capacity_diagnostics().missing_textures, 1);

        // A missing normal map shades like no normal map at all. A grazing
        // light and no ambient make the result depend on the normal.
        let lit = scene.assets.materials.get_mut(material).unwrap();
        lit.base_color_texture = None;
        lit.model = MaterialModel::Pbr;
        scene.render_world.ambient_light = Some(crate::runtime::AmbientLight {
            color: [0.0; 3],
            intensity: 0.0,
        });
        let direction = Vector3::new(-1.0, 0.0, -1.0).normalize();
        scene.render_world.directional_lights.push(
            crate::runtime::ExtractedDirectionalLight {
                entity: bevy_ecs::entity::Entity::from_raw_u32(2000).unwrap(),
                transform: crate::runtime::GlobalTransform {
                    matrix: nalgebra::Rotation3::rotation_between(
                        &-Vector3::z(),
                        &direction,
                    )
                    .unwrap()
                    .to_homogeneous()
                    .into(),
                },
                light: crate::runtime::DirectionalLight {
                    color: [1.0; 3],
                    illuminance: 50_000.0,
                    shadows: false,
                },
            },
        );
        scene.render_world.lights_revision += 1;
        let flat = frame(&mut scene);
        scene
            .assets
            .materials
            .get_mut(material)
            .unwrap()
            .normal_texture = Some(removed);
        assert_eq!(frame(&mut scene), flat, "missing normal map is flat");

        let removed_material = scene.render_world.renderables[0].material;
        scene.assets.materials.remove(removed_material).unwrap();
        scene.render_world.renderables[0].mesh = {
            let mesh = scene.assets.meshes.insert(MeshAsset::default());
            scene.assets.meshes.remove(mesh).unwrap();
            mesh
        };
        let [r, g, b] = frame(&mut scene);
        assert!(r > 200 && g < 60 && b > 200, "magenta cube: {r} {g} {b}");
        let diagnostics = scene.renderer.capacity_diagnostics();
        assert_eq!(
            (diagnostics.missing_materials, diagnostics.missing_meshes),
            (1, 1)
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn debug_views_show_unshaded_base_color_and_normals() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(
            0.0,
            MaterialAsset {
                base_color: [1.0, 0.0, 0.0, 1.0],
                ..Default::default()
            },
        )]);
        scene.render_world.ambient_light = Some(crate::runtime::AmbientLight {
            color: [0.0; 3],
            intensity: 0.0,
        });
        // Renders one frame in `view` and returns the center [r, g, b].
        let frame = |scene: &mut SlabScene, view| {
            scene.debug_view = view;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let [b, g, r, _] = scene.center_pixel();
            [r, g, b]
        };
        assert_eq!(frame(&mut scene, SceneDebugView::Lit), [0, 0, 0]);
        assert_eq!(frame(&mut scene, SceneDebugView::Unshaded), [255, 0, 0]);
        // The slab faces +Z: 0.5, 0.5, 1.0 in linear, sRGB-encoded.
        let [r, g, b] = frame(&mut scene, SceneDebugView::Normals);
        assert!(
            r == g && r.abs_diff(188) <= 1 && b == 255,
            "+Z normal: {r} {g} {b}"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn hdr_values_above_one_survive_until_tone_mapping() {
        use crate::runtime::{ToneMapper, ToneMapping};
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(
            0.0,
            MaterialAsset {
                base_color: [0.0, 0.0, 0.0, 1.0],
                emissive: [4.0, 0.5, 0.0],
                ..Default::default()
            },
        )]);
        scene.render_world.ambient_light = Some(crate::runtime::AmbientLight {
            color: [0.0; 3],
            intensity: 0.0,
        });
        // Renders one frame and returns the center [r, g], sRGB-encoded.
        let frame = |scene: &mut SlabScene, mapper, exposure| {
            scene.render_world.tone_mapping =
                Some(ToneMapping { mapper, exposure });
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let [_, g, r, _] = scene.center_pixel();
            [r, g]
        };
        let near = |[r, g]: [u8; 2], [er, eg]: [u8; 2]| {
            r.abs_diff(er) <= 2 && g.abs_diff(eg) <= 2
        };
        // Linear clips at 1 and leaves 0.5 unchanged, like before HDR.
        let linear = frame(&mut scene, ToneMapper::Linear, 1.0);
        assert!(near(linear, [255, 188]), "linear: {linear:?}");
        // Red 4 stored above 1: exposure 0.25 brings it to exactly 1.
        let exposed = frame(&mut scene, ToneMapper::Linear, 0.25);
        assert!(near(exposed, [255, 99]), "exposure 0.25: {exposed:?}");
        // Reinhard: 4 / 5 = 0.8 and 0.5 / 1.5 = 0.33, no clipping.
        let reinhard = frame(&mut scene, ToneMapper::Reinhard, 1.0);
        assert!(near(reinhard, [231, 156]), "reinhard: {reinhard:?}");
        // ACES fit: 4 gives 0.973 and 0.5 gives 0.616.
        let aces = frame(&mut scene, ToneMapper::Aces, 1.0);
        assert!(near(aces, [252, 206]), "aces: {aces:?}");
        // Debug views skip the curve.
        scene.debug_view = SceneDebugView::Normals;
        let [r, g] = frame(&mut scene, ToneMapper::Reinhard, 1.0);
        assert!(r == g && r.abs_diff(188) <= 1, "normals: {r} {g}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn sky_light_lights_up_faces_with_sky_and_down_faces_with_ground() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        scene.render_world.renderables[0].transform.matrix =
            Matrix4::new_nonuniform_scaling(&Vector3::new(4.0, 0.1, 4.0))
                .into();
        scene.render_world.ambient_light = None;
        scene.render_world.sky_light = Some(crate::runtime::SkyLight {
            sky_color: [1.0, 0.0, 0.0],
            ground_color: [0.0, 0.0, 1.0],
            intensity: 1.0,
        });
        // Places the camera 5 m along `side` looking back at the slab and
        // returns the center pixel as [r, g, b].
        let look_from = |scene: &mut SlabScene, side: Vector3<f32>| {
            let camera = scene.render_world.active_camera.as_mut().unwrap();
            camera.transform.matrix = (Matrix4::new_translation(&(side * 5.0))
                * nalgebra::Rotation3::rotation_between(
                    &-Vector3::z(),
                    &-side,
                )
                .unwrap()
                .to_homogeneous())
            .into();
            scene.render_world.lights_revision += 1;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let [b, g, r, _] = scene.center_pixel();
            [r, g, b]
        };

        let [r, g, b] = look_from(&mut scene, Vector3::y());
        assert!(r > 200 && g == 0 && b == 0, "top sees sky: {r} {g} {b}");
        let [r, g, b] = look_from(&mut scene, -Vector3::y());
        assert!(
            r == 0 && g == 0 && b > 200,
            "bottom sees ground: {r} {g} {b}"
        );

        // A smooth metal has no diffuse; it shows the sky by reflection.
        let material = scene.render_world.renderables[0].material;
        let metal = scene.assets.materials.get_mut(material).unwrap();
        metal.metallic = 1.0;
        metal.roughness = 0.1;
        scene.render_world.renderables_revision += 1;
        let [r, g, b] = look_from(&mut scene, Vector3::y());
        assert!(
            r > 200 && g == 0 && b == 0,
            "metal reflects sky: {r} {g} {b}"
        );

        // No sky and no ambient light falls back to a dim gray.
        scene.render_world.sky_light = None;
        let [r, g, b] = look_from(&mut scene, Vector3::y());
        assert!(r == g && g == b && r > 0, "fallback gray: {r} {g} {b}");
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn several_point_lights_add_up_and_fade_out_at_their_range() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut scene = SlabScene::new(&[(0.0, MaterialAsset::default())]);
        scene.render_world.ambient_light = Some(crate::runtime::AmbientLight {
            color: [0.0; 3],
            intensity: 0.0,
        });
        let point = |id: u32, position: [f32; 3], color: [f32; 3], range| {
            crate::runtime::ExtractedPointLight {
                entity: bevy_ecs::entity::Entity::from_raw_u32(id).unwrap(),
                transform: crate::runtime::GlobalTransform {
                    matrix: Matrix4::new_translation(&Vector3::from(position))
                        .into(),
                },
                light: crate::runtime::PointLight {
                    color,
                    intensity: 500.0,
                    range,
                },
            }
        };
        // Returns the center pixel as [r, g, b].
        let frame = |scene: &mut SlabScene| {
            scene.render_world.lights_revision += 1;
            let before = scene.now();
            scene
                .render(before)
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            let [b, g, r, _] = scene.center_pixel();
            [r, g, b]
        };

        let red = point(2000, [0.0, 0.0, 2.0], [1.0, 0.0, 0.0], 4.0);
        scene.render_world.point_lights.push(red);
        let [one_red, g, b] = frame(&mut scene);
        assert!(one_red > 20 && g == 0 && b == 0, "{one_red} {g} {b}");

        scene.render_world.point_lights.push(point(
            2001,
            [1.0, 0.0, 2.0],
            [0.0, 1.0, 0.0],
            4.0,
        ));
        // Out of range: the slab is 2.95 m below a light reaching 1 m.
        scene.render_world.point_lights.push(point(
            2002,
            [0.0, 0.0, 3.0],
            [0.0, 0.0, 1.0],
            1.0,
        ));
        let [r, g, b] = frame(&mut scene);
        assert!(r.abs_diff(one_red) <= 1, "red unchanged, got {r}");
        assert!(g > 20, "second light adds green, got {g}");
        assert_eq!(b, 0, "a light past its range adds nothing");

        scene.render_world.point_lights.push(point(
            2003,
            [0.0, 0.0, 2.0],
            [1.0, 0.0, 0.0],
            4.0,
        ));
        let [two_red, _, _] = frame(&mut scene);
        assert!(two_red > one_red + 20, "{two_red} vs {one_red}");
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
