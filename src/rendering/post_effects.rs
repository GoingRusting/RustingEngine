//! Screen-space effects recorded as compute passes around the main render
//! pass: bloom over the finished HDR image before tone mapping, and ambient
//! occlusion traced from a depth prepass before the scene is lit, and the
//! brightness measurement auto exposure adapts to.

use std::sync::Arc;

use vulkano::buffer::{
    Buffer, BufferContents, BufferCreateInfo, BufferUsage, Subbuffer,
};
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, PrimaryAutoCommandBuffer,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::Queue;
use vulkano::format::Format;
use vulkano::image::sampler::{
    Filter, Sampler, SamplerAddressMode, SamplerCreateInfo,
};
use vulkano::image::view::{ImageView, ImageViewCreateInfo};
use vulkano::image::{
    Image, ImageCreateInfo, ImageSubresourceRange, ImageUsage,
};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::pipeline::{ComputePipeline, Pipeline, PipelineBindPoint};

use super::scene_renderer::{
    create_compute_pipeline, name_object, SceneRenderError,
};

fn error(error: impl std::fmt::Display) -> SceneRenderError {
    SceneRenderError(error.to_string())
}

/// Most bloom mips below the half-resolution first one. Seven reach about
/// 1/128 of the screen height, wide enough for a soft halo.
const MAX_BLOOM_MIPS: u32 = 7;

/// Compute pipelines of both effects, built once per renderer.
pub(super) struct PostPipelines {
    bloom_prefilter: Arc<ComputePipeline>,
    bloom_down: Arc<ComputePipeline>,
    bloom_up: Arc<ComputePipeline>,
    occlusion_trace: Arc<ComputePipeline>,
    occlusion_blur: Arc<ComputePipeline>,
    exposure: Arc<ComputePipeline>,
    linear: Arc<Sampler>,
    /// Point sampler; the occlusion target has no guaranteed linear
    /// filtering.
    pub(super) nearest: Arc<Sampler>,
}

impl PostPipelines {
    pub(super) fn new(queue: &Arc<Queue>) -> Result<Self, SceneRenderError> {
        let device = queue.device().clone();
        let create = |module| create_compute_pipeline(queue, module);
        let sampler = |filter| {
            Sampler::new(
                device.clone(),
                SamplerCreateInfo {
                    mag_filter: filter,
                    min_filter: filter,
                    address_mode: [SamplerAddressMode::ClampToEdge; 3],
                    ..Default::default()
                },
            )
            .map_err(error)
        };
        let pipelines = Self {
            bloom_prefilter: create(bloom_prefilter_shader::load(
                device.clone(),
            ))?,
            bloom_down: create(bloom_down_shader::load(device.clone()))?,
            bloom_up: create(bloom_up_shader::load(device.clone()))?,
            occlusion_trace: create(occlusion_trace_shader::load(
                device.clone(),
            ))?,
            occlusion_blur: create(occlusion_blur_shader::load(
                device.clone(),
            ))?,
            exposure: create(exposure_shader::load(device.clone()))?,
            linear: sampler(Filter::Linear)?,
            nearest: sampler(Filter::Nearest)?,
        };
        for (pipeline, name) in [
            (&pipelines.bloom_prefilter, "Bloom prefilter"),
            (&pipelines.bloom_down, "Bloom downsample"),
            (&pipelines.bloom_up, "Bloom upsample"),
            (&pipelines.occlusion_trace, "Ambient occlusion trace"),
            (&pipelines.occlusion_blur, "Ambient occlusion blur"),
            (&pipelines.exposure, "Auto exposure"),
        ] {
            name_object(&**pipeline, name);
        }
        Ok(pipelines)
    }
}

/// One mip view of `image`.
fn mip_view(
    image: &Arc<Image>,
    level: u32,
) -> Result<Arc<ImageView>, SceneRenderError> {
    ImageView::new(
        image.clone(),
        ImageViewCreateInfo {
            subresource_range: ImageSubresourceRange {
                mip_levels: level..level + 1,
                ..image.subresource_range()
            },
            ..ImageViewCreateInfo::from_image(image)
        },
    )
    .map_err(error)
}

fn storage_image(
    allocator: &Arc<StandardMemoryAllocator>,
    name: &str,
    format: Format,
    extent: [u32; 2],
    mip_levels: u32,
) -> Result<Arc<Image>, SceneRenderError> {
    let image = Image::new(
        allocator.clone(),
        ImageCreateInfo {
            format,
            extent: [extent[0].max(1), extent[1].max(1), 1],
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
    .map_err(error)?;
    name_object(&*image, name);
    Ok(image)
}

#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
struct BloomParams {
    threshold: f32,
    spread: f32,
}

/// Half-resolution mip chain the bloom is blurred through, rebuilt with the
/// HDR target.
pub(super) struct BloomChain {
    /// Mip 0, which holds the finished glow after the upsample passes.
    pub(super) view: Arc<ImageView>,
    /// Bilinear clamp-to-edge sampler tone mapping reads `view` with.
    pub(super) sampler: Arc<Sampler>,
    /// Prefilter, then downsamples, then upsamples: pipeline index (0, 1
    /// or 2), set and output size.
    steps: Vec<(usize, Arc<DescriptorSet>, [u32; 2])>,
}

impl BloomChain {
    pub(super) fn new(
        allocator: &Arc<StandardMemoryAllocator>,
        descriptor_allocator: &Arc<StandardDescriptorSetAllocator>,
        pipelines: &PostPipelines,
        hdr: &Arc<ImageView>,
    ) -> Result<Self, SceneRenderError> {
        let [width, height, _] = hdr.image().extent();
        let base = [width.div_ceil(2).max(1), height.div_ceil(2).max(1)];
        let levels = base[0].min(base[1]).ilog2().clamp(1, MAX_BLOOM_MIPS);
        let image = storage_image(
            allocator,
            "Bloom",
            Format::R16G16B16A16_SFLOAT,
            base,
            levels,
        )?;
        let size =
            |level: u32| [(base[0] >> level).max(1), (base[1] >> level).max(1)];
        let set = |pipeline: &Arc<ComputePipeline>,
                   source: Arc<ImageView>,
                   target: u32| {
            DescriptorSet::new(
                descriptor_allocator.clone(),
                pipeline.layout().set_layouts()[0].clone(),
                [
                    WriteDescriptorSet::image_view_sampler(
                        0,
                        source,
                        pipelines.linear.clone(),
                    ),
                    WriteDescriptorSet::image_view(
                        1,
                        mip_view(&image, target)?,
                    ),
                ],
                [],
            )
            .map_err(error)
        };
        let mut steps = vec![(
            0,
            set(&pipelines.bloom_prefilter, hdr.clone(), 0)?,
            size(0),
        )];
        for level in 1..levels {
            steps.push((
                1,
                set(
                    &pipelines.bloom_down,
                    mip_view(&image, level - 1)?,
                    level,
                )?,
                size(level),
            ));
        }
        for level in (0..levels - 1).rev() {
            steps.push((
                2,
                set(&pipelines.bloom_up, mip_view(&image, level + 1)?, level)?,
                size(level),
            ));
        }
        Ok(Self {
            view: mip_view(&image, 0)?,
            sampler: pipelines.linear.clone(),
            steps,
        })
    }

    /// Records the prefilter, the downsample chain and the upsample chain,
    /// leaving the glow in mip 0. Returns the dispatch count.
    pub(super) fn record(
        &self,
        commands: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        pipelines: &PostPipelines,
        bloom: &crate::runtime::Bloom,
    ) -> Result<u32, SceneRenderError> {
        let params = BloomParams {
            threshold: bloom.threshold.max(0.0),
            // The ends of the range would drop the finest or every coarser
            // mip entirely.
            spread: 0.05 + 0.9 * bloom.spread.clamp(0.0, 1.0),
        };
        let pipelines = [
            &pipelines.bloom_prefilter,
            &pipelines.bloom_down,
            &pipelines.bloom_up,
        ];
        for (index, set, size) in &self.steps {
            let pipeline = pipelines[*index];
            commands
                .bind_pipeline_compute(pipeline.clone())
                .map_err(error)?
                .bind_descriptor_sets(
                    PipelineBindPoint::Compute,
                    pipeline.layout().clone(),
                    0,
                    set.clone(),
                )
                .map_err(error)?;
            // The downsample takes no parameters.
            if !pipeline.layout().push_constant_ranges().is_empty() {
                commands
                    .push_constants(pipeline.layout().clone(), 0, params)
                    .map_err(error)?;
            }
            unsafe {
                commands
                    .dispatch([size[0].div_ceil(8), size[1].div_ceil(8), 1])
                    .map_err(error)?;
            }
        }
        Ok(self.steps.len() as u32)
    }
}

/// Exposure auto exposure has adapted to, in a buffer tone mapping reads,
/// rebuilt with the HDR target.
pub(super) struct ExposureMeter {
    /// One `f32`: the adapted exposure factor.
    pub(super) buffer: Subbuffer<f32>,
    set: Arc<DescriptorSet>,
    /// When the last measurement was recorded; `None` adapts at once.
    last: Option<std::time::Instant>,
}

impl ExposureMeter {
    pub(super) fn new(
        allocator: &Arc<StandardMemoryAllocator>,
        descriptor_allocator: &Arc<StandardDescriptorSetAllocator>,
        pipelines: &PostPipelines,
        hdr: &Arc<ImageView>,
    ) -> Result<Self, SceneRenderError> {
        let buffer = Buffer::new_sized::<f32>(
            allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .map_err(error)?;
        let set = DescriptorSet::new(
            descriptor_allocator.clone(),
            pipelines.exposure.layout().set_layouts()[0].clone(),
            [
                WriteDescriptorSet::image_view_sampler(
                    0,
                    hdr.clone(),
                    pipelines.linear.clone(),
                ),
                WriteDescriptorSet::buffer(1, buffer.clone()),
            ],
            [],
        )
        .map_err(error)?;
        Ok(Self {
            buffer,
            set,
            last: None,
        })
    }

    /// Frames without auto exposure call this, so the next one adapts at
    /// once instead of from a stale value.
    pub(super) fn reset(&mut self) {
        self.last = None;
    }

    /// Measures the HDR image and moves the exposure in `buffer` toward
    /// its target, by the wall-clock time since the last measurement.
    pub(super) fn record(
        &mut self,
        commands: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        pipelines: &PostPipelines,
        settings: &crate::runtime::AutoExposure,
    ) -> Result<(), SceneRenderError> {
        let now = std::time::Instant::now();
        let blend = self.last.map_or(1.0, |last| {
            let seconds = now.duration_since(last).as_secs_f32();
            1.0 - (-settings.speed.max(0.0) * seconds).exp()
        });
        self.last = Some(now);
        let min_exposure = settings.min_exposure.max(1e-3);
        let pipeline = &pipelines.exposure;
        commands
            .bind_pipeline_compute(pipeline.clone())
            .map_err(error)?
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                pipeline.layout().clone(),
                0,
                self.set.clone(),
            )
            .map_err(error)?
            .push_constants(
                pipeline.layout().clone(),
                0,
                exposure_shader::Params {
                    key: settings.key.max(1e-3),
                    min_exposure,
                    max_exposure: settings.max_exposure.max(min_exposure),
                    blend,
                },
            )
            .map_err(error)?;
        unsafe {
            commands.dispatch([1, 1, 1]).map_err(error)?;
        }
        Ok(())
    }
}

/// Camera and settings the occlusion passes read, in a per-frame buffer.
#[repr(C)]
#[derive(BufferContents, Clone, Copy)]
pub(super) struct OcclusionParams {
    pub(super) view_projection: [[f32; 4]; 4],
    pub(super) inverse_view_projection: [[f32; 4]; 4],
    /// Scene viewport in target pixels: offset in xy, size in zw.
    pub(super) viewport: [f32; 4],
    /// Camera forward in xyz and `dot(eye, forward)` in w, so
    /// `dot(p, xyz) - w` is a point's distance in front of the camera.
    pub(super) forward: [f32; 4],
    /// Radius in metres and intensity exponent.
    pub(super) settings: [f32; 4],
}

/// Raw and blurred occlusion targets, rebuilt with the depth target.
pub(super) struct OcclusionTargets {
    /// Occlusion per target pixel, 1 where nothing occludes.
    pub(super) view: Arc<ImageView>,
    raw: Arc<ImageView>,
    extent: [u32; 2],
}

impl OcclusionTargets {
    pub(super) fn new(
        allocator: &Arc<StandardMemoryAllocator>,
        extent: [u32; 2],
    ) -> Result<Self, SceneRenderError> {
        // The raw pass also keeps each pixel's view distance for the
        // depth-aware blur. Two-channel storage formats are optional in
        // Vulkan, so it takes four.
        let raw = storage_image(
            allocator,
            "Ambient occlusion (raw)",
            Format::R16G16B16A16_SFLOAT,
            extent,
            1,
        )?;
        let blurred = storage_image(
            allocator,
            "Ambient occlusion",
            Format::R32_SFLOAT,
            extent,
            1,
        )?;
        Ok(Self {
            view: ImageView::new_default(blurred).map_err(error)?,
            raw: ImageView::new_default(raw).map_err(error)?,
            extent,
        })
    }

    /// Traces occlusion from `depth` and blurs it into `view`. Returns the
    /// dispatch count.
    pub(super) fn record(
        &self,
        commands: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        descriptor_allocator: &Arc<StandardDescriptorSetAllocator>,
        pipelines: &PostPipelines,
        depth: &Arc<ImageView>,
        params: Subbuffer<OcclusionParams>,
    ) -> Result<u32, SceneRenderError> {
        let passes = [
            (&pipelines.occlusion_trace, depth.clone(), self.raw.clone()),
            (
                &pipelines.occlusion_blur,
                self.raw.clone(),
                self.view.clone(),
            ),
        ];
        for (pipeline, source, target) in passes {
            let set = DescriptorSet::new(
                descriptor_allocator.clone(),
                pipeline.layout().set_layouts()[0].clone(),
                [
                    WriteDescriptorSet::image_view_sampler(
                        0,
                        source,
                        pipelines.nearest.clone(),
                    ),
                    WriteDescriptorSet::image_view(1, target),
                    WriteDescriptorSet::buffer(2, params.clone()),
                ],
                [],
            )
            .map_err(error)?;
            commands
                .bind_pipeline_compute(pipeline.clone())
                .map_err(error)?
                .bind_descriptor_sets(
                    PipelineBindPoint::Compute,
                    pipeline.layout().clone(),
                    0,
                    set,
                )
                .map_err(error)?;
            unsafe {
                commands
                    .dispatch([
                        self.extent[0].div_ceil(8),
                        self.extent[1].div_ceil(8),
                        1,
                    ])
                    .map_err(error)?;
            }
        }
        Ok(2)
    }
}

/// First bloom level: a 13-tap downsample of the HDR image (Jimenez 2014)
/// whose five 2x2 groups are weighted by 1 / (1 + luma) so single bright
/// pixels do not flicker, then a soft-knee threshold.
#[rustfmt::skip]
mod bloom_prefilter_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0) uniform sampler2D source;
layout(set = 0, binding = 1, rgba16f) uniform writeonly image2D target;
layout(push_constant) uniform Params {
    float threshold;
    float spread;
} params;

vec3 tap(vec2 uv, vec2 texel, vec2 offset) {
    // Half-float infinities and NaNs would spread over the whole screen.
    vec3 color = textureLod(source, uv + offset * texel, 0.0).rgb;
    return any(isnan(color)) ? vec3(0.0) : min(color, vec3(65000.0));
}

vec4 group(vec3 a, vec3 b, vec3 c, vec3 d) {
    vec3 average = (a + b + c + d) * 0.25;
    float weight = 1.0 / (1.0 + dot(average, vec3(0.2126, 0.7152, 0.0722)));
    return vec4(average * weight, weight);
}

void main() {
    ivec2 texel_id = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = imageSize(target);
    if (any(greaterThanEqual(texel_id, size))) {
        return;
    }
    vec2 uv = (vec2(texel_id) + 0.5) / vec2(size);
    vec2 texel = 1.0 / vec2(textureSize(source, 0));
    vec3 a = tap(uv, texel, vec2(-2.0, -2.0));
    vec3 b = tap(uv, texel, vec2(0.0, -2.0));
    vec3 c = tap(uv, texel, vec2(2.0, -2.0));
    vec3 d = tap(uv, texel, vec2(-2.0, 0.0));
    vec3 e = tap(uv, texel, vec2(0.0, 0.0));
    vec3 f = tap(uv, texel, vec2(2.0, 0.0));
    vec3 g = tap(uv, texel, vec2(-2.0, 2.0));
    vec3 h = tap(uv, texel, vec2(0.0, 2.0));
    vec3 i = tap(uv, texel, vec2(2.0, 2.0));
    vec3 j = tap(uv, texel, vec2(-1.0, -1.0));
    vec3 k = tap(uv, texel, vec2(1.0, -1.0));
    vec3 l = tap(uv, texel, vec2(-1.0, 1.0));
    vec3 m = tap(uv, texel, vec2(1.0, 1.0));
    vec4 sum = group(j, k, l, m) * 0.5
        + group(a, b, d, e) * 0.125
        + group(b, c, e, f) * 0.125
        + group(d, e, g, h) * 0.125
        + group(e, f, h, i) * 0.125;
    vec3 color = sum.rgb / sum.a;
    // Soft knee half the threshold wide, so the glow fades in smoothly.
    float brightness = max(color.r, max(color.g, color.b));
    float knee = 0.5 * params.threshold;
    float soft = clamp(brightness - params.threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 1e-4);
    float contribution = max(soft, brightness - params.threshold)
        / max(brightness, 1e-4);
    imageStore(target, texel_id, vec4(color * contribution, 1.0));
}
"
    }
}

/// Further bloom levels: the same 13-tap downsample without the weighting.
#[rustfmt::skip]
mod bloom_down_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0) uniform sampler2D source;
layout(set = 0, binding = 1, rgba16f) uniform writeonly image2D target;

vec3 tap(vec2 uv, vec2 texel, vec2 offset) {
    return textureLod(source, uv + offset * texel, 0.0).rgb;
}

void main() {
    ivec2 texel_id = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = imageSize(target);
    if (any(greaterThanEqual(texel_id, size))) {
        return;
    }
    vec2 uv = (vec2(texel_id) + 0.5) / vec2(size);
    vec2 texel = 1.0 / vec2(textureSize(source, 0));
    vec3 color = tap(uv, texel, vec2(0.0)) * 0.125
        + (tap(uv, texel, vec2(-2.0, -2.0)) + tap(uv, texel, vec2(2.0, -2.0))
            + tap(uv, texel, vec2(-2.0, 2.0)) + tap(uv, texel, vec2(2.0, 2.0)))
            * 0.03125
        + (tap(uv, texel, vec2(0.0, -2.0)) + tap(uv, texel, vec2(-2.0, 0.0))
            + tap(uv, texel, vec2(2.0, 0.0)) + tap(uv, texel, vec2(0.0, 2.0)))
            * 0.0625
        + (tap(uv, texel, vec2(-1.0, -1.0)) + tap(uv, texel, vec2(1.0, -1.0))
            + tap(uv, texel, vec2(-1.0, 1.0)) + tap(uv, texel, vec2(1.0, 1.0)))
            * 0.125;
    imageStore(target, texel_id, vec4(color, 1.0));
}
"
    }
}

/// Upsample: blends a 3x3 tent of the coarser level into this level by
/// `spread`. The weights sum to one, so the glow keeps its energy.
#[rustfmt::skip]
mod bloom_up_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0) uniform sampler2D source;
layout(set = 0, binding = 1, rgba16f) uniform image2D target;
layout(push_constant) uniform Params {
    float threshold;
    float spread;
} params;

vec3 tap(vec2 uv, vec2 texel, vec2 offset) {
    return textureLod(source, uv + offset * texel, 0.0).rgb;
}

void main() {
    ivec2 texel_id = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = imageSize(target);
    if (any(greaterThanEqual(texel_id, size))) {
        return;
    }
    vec2 uv = (vec2(texel_id) + 0.5) / vec2(size);
    vec2 texel = 1.0 / vec2(textureSize(source, 0));
    vec3 tent = tap(uv, texel, vec2(0.0)) * 4.0
        + (tap(uv, texel, vec2(-1.0, 0.0)) + tap(uv, texel, vec2(1.0, 0.0))
            + tap(uv, texel, vec2(0.0, -1.0)) + tap(uv, texel, vec2(0.0, 1.0)))
            * 2.0
        + tap(uv, texel, vec2(-1.0, -1.0)) + tap(uv, texel, vec2(1.0, -1.0))
        + tap(uv, texel, vec2(-1.0, 1.0)) + tap(uv, texel, vec2(1.0, 1.0));
    vec3 here = imageLoad(target, texel_id).rgb;
    imageStore(target, texel_id, vec4(mix(here, tent / 16.0, params.spread), 1.0));
}
"
    }
}

/// Screen-space ambient occlusion: 16 samples in the world-space hemisphere
/// around the normal rebuilt from depth, each counted when the depth buffer
/// shows a surface in front of it within the radius. A 4x4 Bayer rotation
/// per pixel spreads the samples, and the blur averages it out.
#[rustfmt::skip]
mod occlusion_trace_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0) uniform sampler2D depth;
layout(set = 0, binding = 1, rgba16f) uniform writeonly image2D target;
layout(set = 0, binding = 2) readonly buffer Params {
    mat4 view_projection;
    mat4 inverse_view_projection;
    vec4 viewport;
    vec4 forward;
    vec4 settings;
} params;
const int SAMPLES = 16;
const float BAYER[16] = float[](
    0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0,
    3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0
);

vec3 world_at(vec2 pixel, float d) {
    vec2 ndc = (pixel - params.viewport.xy) / params.viewport.zw * 2.0 - 1.0;
    vec4 p = params.inverse_view_projection * vec4(ndc, d, 1.0);
    return p.xyz / p.w;
}

float view_distance(vec3 p) {
    return dot(p, params.forward.xyz) - params.forward.w;
}

float depth_at(ivec2 pixel) {
    return texelFetch(depth, clamp(pixel, ivec2(0), textureSize(depth, 0) - 1), 0).r;
}

// The neighbour step on one axis whose depth is closer to the centre's, so
// normals stay flat up to silhouettes.
vec3 flatter_step(ivec2 pixel, float d, vec3 p, ivec2 axis) {
    float before = depth_at(pixel - axis);
    float after = depth_at(pixel + axis);
    return abs(after - d) < abs(before - d)
        ? world_at(vec2(pixel + axis) + 0.5, after) - p
        : p - world_at(vec2(pixel - axis) + 0.5, before);
}

void main() {
    ivec2 pixel = ivec2(gl_GlobalInvocationID.xy);
    if (any(greaterThanEqual(pixel, imageSize(target)))) {
        return;
    }
    float d = depth_at(pixel);
    // Background, and pixels outside the scene viewport, are unoccluded;
    // their distance is near the half-float limit.
    if (d >= 1.0) {
        imageStore(target, pixel, vec4(1.0, 65000.0, 0.0, 0.0));
        return;
    }
    vec3 p = world_at(vec2(pixel) + 0.5, d);
    vec3 normal = normalize(cross(
        flatter_step(pixel, d, p, ivec2(1, 0)),
        flatter_step(pixel, d, p, ivec2(0, 1))
    ));
    // Face the camera: the near-plane point this pixel looks through.
    if (dot(normal, world_at(vec2(pixel) + 0.5, 0.0) - p) < 0.0) {
        normal = -normal;
    }
    // Orthonormal basis around the normal (Duff et al. 2017).
    float sign_z = normal.z >= 0.0 ? 1.0 : -1.0;
    float a = -1.0 / (sign_z + normal.z);
    float b = normal.x * normal.y * a;
    vec3 tangent = vec3(1.0 + sign_z * normal.x * normal.x * a, sign_z * b, -sign_z * normal.x);
    vec3 bitangent = vec3(b, sign_z + normal.y * normal.y * a, -normal.y);
    float radius = params.settings.x;
    float rotation = BAYER[(pixel.y & 3) * 4 + (pixel.x & 3)] / 16.0 * 6.2831853;
    float here = view_distance(p);
    float occlusion = 0.0;
    for (int i = 0; i < SAMPLES; ++i) {
        // Cosine-weighted spiral over the hemisphere; lengths grow with a
        // shuffled index so near samples dominate.
        float t = (float(i) + 0.5) / float(SAMPLES);
        float angle = float(i) * 2.3999632 + rotation;
        float ring = sqrt(t);
        float scale = (float((i * 7) % SAMPLES) + 0.5) / float(SAMPLES);
        scale = mix(0.1, 1.0, scale * scale);
        vec3 offset = tangent * (ring * cos(angle)) + bitangent * (ring * sin(angle))
            + normal * sqrt(1.0 - t);
        vec3 sample_point = p + offset * radius * scale;
        vec4 clip = params.view_projection * vec4(sample_point, 1.0);
        if (clip.w <= 0.0) {
            continue;
        }
        vec2 screen = params.viewport.xy
            + (clip.xy / clip.w * 0.5 + 0.5) * params.viewport.zw;
        if (any(lessThan(screen, params.viewport.xy))
            || any(greaterThanEqual(screen, params.viewport.xy + params.viewport.zw))) {
            continue;
        }
        float scene = depth_at(ivec2(screen));
        if (scene >= 1.0) {
            continue;
        }
        float occluder = view_distance(world_at(floor(screen) + 0.5, scene));
        if (occluder < view_distance(sample_point) - 0.025 * radius) {
            // Surfaces far in front of this one do not occlude it.
            occlusion += smoothstep(0.0, 1.0, radius / max(abs(here - occluder), 1e-4));
        }
    }
    imageStore(target, pixel, vec4(1.0 - occlusion / float(SAMPLES), here, 0.0, 0.0));
}
"
    }
}

/// 4x4 blur over the Bayer tile, skipping pixels at a different distance,
/// then the intensity exponent.
#[rustfmt::skip]
mod occlusion_blur_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0) uniform sampler2D raw;
layout(set = 0, binding = 1, r32f) uniform writeonly image2D target;
layout(set = 0, binding = 2) readonly buffer Params {
    mat4 view_projection;
    mat4 inverse_view_projection;
    vec4 viewport;
    vec4 forward;
    vec4 settings;
} params;

void main() {
    ivec2 pixel = ivec2(gl_GlobalInvocationID.xy);
    ivec2 size = imageSize(target);
    if (any(greaterThanEqual(pixel, size))) {
        return;
    }
    vec2 center = texelFetch(raw, pixel, 0).rg;
    float tolerance = 0.1 * center.y + 0.01;
    float sum = 0.0;
    float count = 0.0;
    for (int y = -2; y < 2; ++y) {
        for (int x = -2; x < 2; ++x) {
            vec2 tap = texelFetch(raw, clamp(pixel + ivec2(x, y), ivec2(0), size - 1), 0).rg;
            if (abs(tap.y - center.y) <= tolerance) {
                sum += tap.x;
                count += 1.0;
            }
        }
    }
    float occlusion = count > 0.0 ? sum / count : center.x;
    imageStore(target, pixel, vec4(pow(clamp(occlusion, 0.0, 1.0), params.settings.y)));
}
"
    }
}

#[rustfmt::skip]
mod exposure_shader {
    vulkano_shaders::shader! {
        ty: "compute",
        src: r"
#version 450
layout(local_size_x = 256, local_size_y = 1, local_size_z = 1) in;
layout(set = 0, binding = 0) uniform sampler2D hdr;
layout(set = 0, binding = 1) buffer Exposure { float value; } exposure;
// blend is how far this frame moves toward the target: 1 adapts at once.
layout(push_constant) uniform Params {
    float key;
    float min_exposure;
    float max_exposure;
    float blend;
} params;
shared float sums[256];

// ponytail: a fixed 64x64 grid of bilinear taps, so a small bright spot
// between taps is missed; measure a bloom mip if that shows.
const uint GRID = 64u;

void main() {
    uint index = gl_LocalInvocationIndex;
    float sum = 0.0;
    for (uint tap = index; tap < GRID * GRID; tap += 256u) {
        vec2 uv = (vec2(tap % GRID, tap / GRID) + 0.5) / float(GRID);
        vec3 c = textureLod(hdr, uv, 0.0).rgb;
        sum += log2(max(dot(c, vec3(0.2126, 0.7152, 0.0722)), 1e-4));
    }
    sums[index] = sum;
    barrier();
    // Fixed-order tree reduction, so the same image gives the same value.
    for (uint stride = 128u; stride > 0u; stride >>= 1u) {
        if (index < stride) {
            sums[index] += sums[index + stride];
        }
        barrier();
    }
    if (index == 0u) {
        float average = exp2(sums[0] / float(GRID * GRID));
        float target = clamp(params.key / average,
            params.min_exposure, params.max_exposure);
        // Adapting in log space feels even in both directions.
        exposure.value = params.blend >= 1.0 ? target
            : exp2(mix(log2(exposure.value), log2(target), params.blend));
    }
}
"
    }
}
