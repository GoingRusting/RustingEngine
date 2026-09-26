//! Engine-owned egui painter: uploads egui textures and meshes and draws them
//! over a finished render target in its own render pass.
//!
//! The painter only needs a queue, an allocator, and the target format, so
//! the editor and games draw egui the same way over the scene renderer's
//! output.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use egui::epaint::{ClippedPrimitive, ImageData, Primitive};
use egui::{
    TextureFilter, TextureId, TextureOptions, TextureWrapMode, TexturesDelta,
};
use vulkano::buffer::allocator::{
    SubbufferAllocator, SubbufferAllocatorCreateInfo,
};
use vulkano::buffer::{Buffer, BufferContents, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, BufferImageCopy, CommandBufferUsage,
    CopyBufferToImageInfo, PrimaryCommandBufferAbstract, RenderPassBeginInfo,
    SubpassBeginInfo, SubpassContents,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::Queue;
use vulkano::format::{Format, NumericFormat};
use vulkano::image::sampler::{
    Filter, Sampler, SamplerAddressMode, SamplerCreateInfo,
};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::pipeline::graphics::color_blend::{
    AttachmentBlend, BlendFactor, ColorBlendAttachmentState, ColorBlendState,
};
use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::RasterizationState;
use vulkano::pipeline::graphics::subpass::PipelineSubpassType;
use vulkano::pipeline::graphics::vertex_input::{Vertex, VertexDefinition};
use vulkano::pipeline::graphics::viewport::{Scissor, Viewport, ViewportState};
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
use vulkano::pipeline::{
    DynamicState, GraphicsPipeline, Pipeline, PipelineBindPoint,
    PipelineLayout, PipelineShaderStageCreateInfo,
};
use vulkano::render_pass::{
    Framebuffer, FramebufferCreateInfo, RenderPass, Subpass,
};
use vulkano::sync::GpuFuture;

#[derive(Debug)]
pub struct EguiPaintError(String);

impl Display for EguiPaintError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for EguiPaintError {}

fn fail(error: impl ToString) -> EguiPaintError {
    EguiPaintError(error.to_string())
}

#[repr(C)]
#[derive(BufferContents, Vertex, Clone, Copy)]
struct EguiVertex {
    /// Logical points; the shader divides by the screen size in points.
    #[format(R32G32_SFLOAT)]
    position: [f32; 2],
    #[format(R32G32_SFLOAT)]
    tex_coords: [f32; 2],
    /// Premultiplied, sRGB-encoded, as egui produces it.
    #[format(R8G8B8A8_UNORM)]
    color: [u8; 4],
}

struct EguiTexture {
    image: Arc<Image>,
    set: Arc<DescriptorSet>,
}

/// Draws egui output onto an existing image, keeping what is below it.
pub struct EguiPainter {
    queue: Arc<Queue>,
    memory_allocator: Arc<StandardMemoryAllocator>,
    command_allocator: Arc<StandardCommandBufferAllocator>,
    descriptor_allocator: Arc<StandardDescriptorSetAllocator>,
    render_pass: Arc<RenderPass>,
    pipeline: Arc<GraphicsPipeline>,
    /// Per-frame vertex and index uploads. Arenas return once no in-flight
    /// command buffer holds them.
    mesh_allocator: SubbufferAllocator,
    textures: HashMap<TextureId, EguiTexture>,
    /// egui blends in sRGB space; an sRGB target gets the result decoded.
    output_srgb: bool,
}

impl EguiPainter {
    pub fn new(
        queue: Arc<Queue>,
        memory_allocator: Arc<StandardMemoryAllocator>,
        output_format: Format,
    ) -> Result<Self, EguiPaintError> {
        let device = queue.device().clone();
        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: output_format,
                    samples: 1,
                    load_op: Load,
                    store_op: Store,
                }
            },
            pass: {
                color: [color],
                depth_stencil: {}
            }
        )
        .map_err(fail)?;
        let vertex = vertex_shader::load(device.clone())
            .map_err(fail)?
            .entry_point("main")
            .ok_or_else(|| fail("egui vertex entry point is missing"))?;
        let fragment = fragment_shader::load(device.clone())
            .map_err(fail)?
            .entry_point("main")
            .ok_or_else(|| fail("egui fragment entry point is missing"))?;
        let vertex_input_state =
            EguiVertex::per_vertex().definition(&vertex).map_err(fail)?;
        let stages = [
            PipelineShaderStageCreateInfo::new(vertex),
            PipelineShaderStageCreateInfo::new(fragment),
        ];
        let layout = PipelineLayout::new(
            device.clone(),
            PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
                .into_pipeline_layout_create_info(device.clone())
                .map_err(fail)?,
        )
        .map_err(fail)?;
        let subpass = Subpass::from(render_pass.clone(), 0)
            .ok_or_else(|| fail("egui subpass is missing"))?;
        let pipeline = GraphicsPipeline::new(
            device.clone(),
            None,
            GraphicsPipelineCreateInfo {
                stages: stages.into_iter().collect(),
                vertex_input_state: Some(vertex_input_state),
                input_assembly_state: Some(InputAssemblyState::default()),
                viewport_state: Some(ViewportState::default()),
                rasterization_state: Some(RasterizationState::default()),
                multisample_state: Some(MultisampleState::default()),
                color_blend_state: Some(
                    ColorBlendState::with_attachment_states(
                        1,
                        ColorBlendAttachmentState {
                            // Premultiplied alpha, as egui outputs it.
                            blend: Some(AttachmentBlend {
                                src_color_blend_factor: BlendFactor::One,
                                src_alpha_blend_factor:
                                    BlendFactor::OneMinusDstAlpha,
                                dst_alpha_blend_factor: BlendFactor::One,
                                ..AttachmentBlend::alpha()
                            }),
                            ..Default::default()
                        },
                    ),
                ),
                dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                    .into_iter()
                    .collect(),
                subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
                ..GraphicsPipelineCreateInfo::layout(layout)
            },
        )
        .map_err(fail)?;
        Ok(Self {
            command_allocator: Arc::new(StandardCommandBufferAllocator::new(
                device.clone(),
                Default::default(),
            )),
            descriptor_allocator: Arc::new(
                StandardDescriptorSetAllocator::new(device, Default::default()),
            ),
            mesh_allocator: SubbufferAllocator::new(
                memory_allocator.clone(),
                SubbufferAllocatorCreateInfo {
                    buffer_usage: BufferUsage::VERTEX_BUFFER
                        | BufferUsage::INDEX_BUFFER,
                    memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
            ),
            output_srgb: output_format.numeric_format_color()
                == Some(NumericFormat::SRGB),
            queue,
            memory_allocator,
            render_pass,
            pipeline,
            textures: HashMap::new(),
        })
    }

    /// Largest texture side egui may request; pass it to `egui_winit::State`.
    #[must_use]
    pub fn max_texture_side(&self) -> usize {
        self.queue
            .device()
            .physical_device()
            .properties()
            .max_image_dimension2_d as usize
    }

    /// Shows `view` wherever egui draws texture `id` (use a
    /// `TextureId::User`), such as a 3D view rendered offscreen. Setting the
    /// same image again is free; a new image replaces the old one, which the
    /// recorded command buffers keep alive until they finish.
    pub fn set_native_texture(
        &mut self,
        id: TextureId,
        view: Arc<ImageView>,
    ) -> Result<(), EguiPaintError> {
        if self
            .textures
            .get(&id)
            .is_some_and(|texture| Arc::ptr_eq(&texture.image, view.image()))
        {
            return Ok(());
        }
        let image = view.image().clone();
        let set = DescriptorSet::new(
            self.descriptor_allocator.clone(),
            self.pipeline.layout().set_layouts()[0].clone(),
            [WriteDescriptorSet::image_view_sampler(
                0,
                view,
                self.sampler(TextureOptions::LINEAR)?,
            )],
            [],
        )
        .map_err(fail)?;
        self.textures.insert(id, EguiTexture { image, set });
        Ok(())
    }

    /// Applies `textures`, then draws `primitives` over `target` after
    /// `before`. Freed textures are dropped after the draw is recorded.
    pub fn paint(
        &mut self,
        before: Box<dyn GpuFuture>,
        target: Arc<ImageView>,
        pixels_per_point: f32,
        primitives: &[ClippedPrimitive],
        textures: &TexturesDelta,
    ) -> Result<Box<dyn GpuFuture>, EguiPaintError> {
        if !textures.set.is_empty() {
            // Uploads run in their own submission and finish before the
            // draw, so no in-flight frame holds a texture that is being
            // written. ponytail: this stalls the CPU on frames that change
            // textures (font atlas growth, new images); move to a per-frame
            // staging ring with barriers if that shows up in profiles.
            let mut uploads = AutoCommandBufferBuilder::primary(
                self.command_allocator.clone(),
                self.queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .map_err(fail)?;
            for (id, delta) in &textures.set {
                self.set_texture(&mut uploads, *id, delta)?;
            }
            uploads
                .build()
                .map_err(fail)?
                .execute(self.queue.clone())
                .map_err(fail)?
                .then_signal_fence_and_flush()
                .map_err(fail)?
                .wait(None)
                .map_err(fail)?;
        }
        let mut commands = AutoCommandBufferBuilder::primary(
            self.command_allocator.clone(),
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .map_err(fail)?;
        let extent = target.image().extent();
        let framebuffer = Framebuffer::new(
            self.render_pass.clone(),
            FramebufferCreateInfo {
                attachments: vec![target],
                ..Default::default()
            },
        )
        .map_err(fail)?;
        commands
            .begin_render_pass(
                RenderPassBeginInfo {
                    clear_values: vec![None],
                    ..RenderPassBeginInfo::framebuffer(framebuffer)
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .map_err(fail)?
            .bind_pipeline_graphics(self.pipeline.clone())
            .map_err(fail)?
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
            .map_err(fail)?
            .push_constants(
                self.pipeline.layout().clone(),
                0,
                vertex_shader::Screen {
                    size: [
                        extent[0] as f32 / pixels_per_point,
                        extent[1] as f32 / pixels_per_point,
                    ],
                    output_srgb: u32::from(self.output_srgb),
                },
            )
            .map_err(fail)?;
        for ClippedPrimitive {
            clip_rect,
            primitive,
        } in primitives
        {
            // ponytail: paint callbacks are skipped; add them when a panel
            // needs custom Vulkan drawing inside egui.
            let Primitive::Mesh(mesh) = primitive else {
                continue;
            };
            let Some(texture) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            if mesh.indices.is_empty() {
                continue;
            }
            // Clip rects are in points; round outward to whole pixels.
            let min_x = (clip_rect.min.x * pixels_per_point)
                .round()
                .clamp(0.0, extent[0] as f32);
            let min_y = (clip_rect.min.y * pixels_per_point)
                .round()
                .clamp(0.0, extent[1] as f32);
            let max_x = (clip_rect.max.x * pixels_per_point)
                .round()
                .clamp(min_x, extent[0] as f32);
            let max_y = (clip_rect.max.y * pixels_per_point)
                .round()
                .clamp(min_y, extent[1] as f32);
            if max_x <= min_x || max_y <= min_y {
                continue;
            }
            let vertices = self
                .mesh_allocator
                .allocate_slice::<EguiVertex>(mesh.vertices.len() as u64)
                .map_err(fail)?;
            for (out, vertex) in vertices
                .write()
                .map_err(fail)?
                .iter_mut()
                .zip(&mesh.vertices)
            {
                *out = EguiVertex {
                    position: [vertex.pos.x, vertex.pos.y],
                    tex_coords: [vertex.uv.x, vertex.uv.y],
                    color: vertex.color.to_array(),
                };
            }
            let indices = self
                .mesh_allocator
                .allocate_slice::<u32>(mesh.indices.len() as u64)
                .map_err(fail)?;
            indices
                .write()
                .map_err(fail)?
                .copy_from_slice(&mesh.indices);
            commands
                .set_scissor(
                    0,
                    [Scissor {
                        offset: [min_x as u32, min_y as u32],
                        extent: [
                            (max_x - min_x) as u32,
                            (max_y - min_y) as u32,
                        ],
                    }]
                    .into_iter()
                    .collect(),
                )
                .map_err(fail)?
                .bind_descriptor_sets(
                    PipelineBindPoint::Graphics,
                    self.pipeline.layout().clone(),
                    0,
                    texture.set.clone(),
                )
                .map_err(fail)?
                .bind_vertex_buffers(0, vertices)
                .map_err(fail)?
                .bind_index_buffer(indices)
                .map_err(fail)?;
            unsafe {
                commands
                    .draw_indexed(mesh.indices.len() as u32, 1, 0, 0, 0)
                    .map_err(fail)?;
            }
        }
        commands.end_render_pass(Default::default()).map_err(fail)?;
        // The recorded command buffer keeps freed images alive until it runs.
        for id in &textures.free {
            self.textures.remove(id);
        }
        let command_buffer = commands.build().map_err(fail)?;
        Ok(before
            .then_execute(self.queue.clone(), command_buffer)
            .map_err(fail)?
            .boxed())
    }

    fn set_texture(
        &mut self,
        commands: &mut AutoCommandBufferBuilder<
            vulkano::command_buffer::PrimaryAutoCommandBuffer,
        >,
        id: TextureId,
        delta: &egui::epaint::ImageDelta,
    ) -> Result<(), EguiPaintError> {
        let [width, height] = delta.image.size().map(|side| side as u32);
        if width == 0 || height == 0 {
            return Ok(());
        }
        let pixels: Vec<u8> = match &delta.image {
            ImageData::Color(image) => image
                .pixels
                .iter()
                .flat_map(|color| color.to_array())
                .collect(),
            ImageData::Font(image) => image
                .srgba_pixels(None)
                .flat_map(|color| color.to_array())
                .collect(),
        };
        let staging = Buffer::from_iter(
            self.memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            pixels,
        )
        .map_err(fail)?;
        let (image, offset) = match delta.pos {
            None => {
                let image = Image::new(
                    self.memory_allocator.clone(),
                    ImageCreateInfo {
                        format: Format::R8G8B8A8_SRGB,
                        extent: [width, height, 1],
                        usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                        ..Default::default()
                    },
                    AllocationCreateInfo {
                        memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                        ..Default::default()
                    },
                )
                .map_err(fail)?;
                let set = DescriptorSet::new(
                    self.descriptor_allocator.clone(),
                    self.pipeline.layout().set_layouts()[0].clone(),
                    [WriteDescriptorSet::image_view_sampler(
                        0,
                        ImageView::new_default(image.clone()).map_err(fail)?,
                        self.sampler(delta.options)?,
                    )],
                    [],
                )
                .map_err(fail)?;
                self.textures.insert(
                    id,
                    EguiTexture {
                        image: image.clone(),
                        set,
                    },
                );
                (image, [0, 0, 0])
            }
            Some([x, y]) => {
                let Some(texture) = self.textures.get(&id) else {
                    return Err(fail(format!(
                        "egui patched unknown texture {id:?}"
                    )));
                };
                (texture.image.clone(), [x as u32, y as u32, 0])
            }
        };
        commands
            .copy_buffer_to_image(CopyBufferToImageInfo {
                regions: [BufferImageCopy {
                    image_subresource: image.subresource_layers(),
                    image_offset: offset,
                    image_extent: [width, height, 1],
                    ..Default::default()
                }]
                .into(),
                ..CopyBufferToImageInfo::buffer_image(staging, image)
            })
            .map_err(fail)?;
        Ok(())
    }

    fn sampler(
        &self,
        options: TextureOptions,
    ) -> Result<Arc<Sampler>, EguiPaintError> {
        let filter = |filter| match filter {
            TextureFilter::Nearest => Filter::Nearest,
            TextureFilter::Linear => Filter::Linear,
        };
        let address_mode = match options.wrap_mode {
            TextureWrapMode::ClampToEdge => SamplerAddressMode::ClampToEdge,
            TextureWrapMode::Repeat => SamplerAddressMode::Repeat,
            TextureWrapMode::MirroredRepeat => {
                SamplerAddressMode::MirroredRepeat
            }
        };
        // ponytail: one sampler per texture and no mipmaps; egui's own
        // textures are single-level. Cache samplers if user images multiply.
        Sampler::new(
            self.queue.device().clone(),
            SamplerCreateInfo {
                mag_filter: filter(options.magnification),
                min_filter: filter(options.minification),
                address_mode: [address_mode; 3],
                ..Default::default()
            },
        )
        .map_err(fail)
    }
}

#[rustfmt::skip]
mod vertex_shader {
    vulkano_shaders::shader! {
        ty: "vertex",
        src: r"
#version 450
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 tex_coords;
layout(location = 2) in vec4 color;
layout(location = 0) out vec4 v_color;
layout(location = 1) out vec2 v_tex_coords;
layout(push_constant) uniform Screen {
    vec2 size;
    uint output_srgb;
} screen;
void main() {
    gl_Position = vec4(2.0 * position / screen.size - 1.0, 0.0, 1.0);
    v_color = color;
    v_tex_coords = tex_coords;
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
layout(location = 0) in vec4 v_color;
layout(location = 1) in vec2 v_tex_coords;
layout(location = 0) out vec4 f_color;
layout(set = 0, binding = 0) uniform sampler2D egui_texture;
layout(push_constant) uniform Screen {
    vec2 size;
    uint output_srgb;
} screen;
vec3 srgb_from_linear(vec3 linear) {
    vec3 lower = linear * 12.92;
    vec3 higher = 1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055;
    return mix(higher, lower, vec3(lessThan(linear, vec3(0.0031308))));
}
vec3 linear_from_srgb(vec3 srgb) {
    vec3 lower = srgb / 12.92;
    vec3 higher = pow((srgb + 0.055) / 1.055, vec3(2.4));
    return mix(higher, lower, vec3(lessThan(srgb, vec3(0.04045))));
}
void main() {
    // egui mixes texture, vertex color, and blending in sRGB space.
    vec4 texel = texture(egui_texture, v_tex_coords);
    vec4 color = v_color * vec4(srgb_from_linear(texel.rgb), texel.a);
    if (screen.output_srgb == 1u) {
        color.rgb = linear_from_srgb(color.rgb);
    }
    f_color = color;
}
"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::swapchain::OFFSCREEN_COLOR_FORMAT;
    use egui::epaint::{ColorImage, ImageDelta, Mesh};
    use egui::{Color32, Pos2, Rect};
    use vulkano::command_buffer::ClearColorImageInfo;

    const SIZE: u32 = 8;

    struct Target {
        base: &'static crate::rendering::HeadlessVulkanBase,
        memory_allocator: Arc<StandardMemoryAllocator>,
        painter: EguiPainter,
        image: Arc<Image>,
    }

    impl Target {
        /// An 8x8 sRGB target filled with blue.
        fn new() -> Self {
            let base = crate::rendering::test_support::headless_device();
            let memory_allocator = Arc::new(
                StandardMemoryAllocator::new_default(base.device.clone()),
            );
            let painter = EguiPainter::new(
                base.queue.clone(),
                memory_allocator.clone(),
                OFFSCREEN_COLOR_FORMAT,
            )
            .unwrap();
            let image = Image::new(
                memory_allocator.clone(),
                ImageCreateInfo {
                    format: OFFSCREEN_COLOR_FORMAT,
                    extent: [SIZE, SIZE, 1],
                    usage: ImageUsage::COLOR_ATTACHMENT
                        | ImageUsage::TRANSFER_SRC
                        | ImageUsage::TRANSFER_DST,
                    ..Default::default()
                },
                AllocationCreateInfo::default(),
            )
            .unwrap();
            let mut commands = AutoCommandBufferBuilder::primary(
                painter.command_allocator.clone(),
                base.queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .unwrap();
            commands
                .clear_color_image(ClearColorImageInfo {
                    clear_value: [0.0, 0.0, 1.0, 1.0].into(),
                    ..ClearColorImageInfo::image(image.clone())
                })
                .unwrap();
            vulkano::sync::now(base.device.clone())
                .then_execute(base.queue.clone(), commands.build().unwrap())
                .unwrap()
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            Self {
                base,
                memory_allocator,
                painter,
                image,
            }
        }

        fn paint(
            &mut self,
            pixels_per_point: f32,
            primitives: &[ClippedPrimitive],
            textures: &TexturesDelta,
        ) {
            let before = vulkano::sync::now(self.base.device.clone()).boxed();
            self.painter
                .paint(
                    before,
                    ImageView::new_default(self.image.clone()).unwrap(),
                    pixels_per_point,
                    primitives,
                    textures,
                )
                .unwrap()
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
        }

        /// Returns pixel `[x, y]` as `[r, g, b]`.
        fn pixel(&self, [x, y]: [u32; 2]) -> [u8; 3] {
            let pixels = crate::rendering::readback::read_back_image(
                &self.base.device,
                &self.base.queue,
                &self.memory_allocator,
                &self.painter.command_allocator,
                &self.image,
            );
            let at = ((y * SIZE + x) * 4) as usize;
            [pixels[at + 2], pixels[at + 1], pixels[at]]
        }
    }

    fn set(
        id: TextureId,
        pos: Option<[usize; 2]>,
        pixels: Vec<Color32>,
    ) -> (TextureId, ImageDelta) {
        let image = ColorImage {
            size: [pixels.len(), 1],
            pixels,
        };
        let delta = match pos {
            None => ImageDelta::full(image, TextureOptions::NEAREST),
            Some(pos) => {
                ImageDelta::partial(pos, image, TextureOptions::NEAREST)
            }
        };
        (id, delta)
    }

    /// A rect in points sampling texture `id` at `uv`, clipped to `clip`.
    fn rect(
        id: TextureId,
        rect: Rect,
        uv: Pos2,
        color: Color32,
        clip: Rect,
    ) -> ClippedPrimitive {
        let mut mesh = Mesh::with_texture(id);
        mesh.add_rect_with_uv(rect, Rect::from_min_max(uv, uv), color);
        ClippedPrimitive {
            clip_rect: clip,
            primitive: Primitive::Mesh(mesh),
        }
    }

    fn points(min: [f32; 2], max: [f32; 2]) -> Rect {
        Rect::from_min_max(Pos2::from(min), Pos2::from(max))
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn meshes_blend_over_the_target_in_points_and_respect_clip_rects() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut target = Target::new();
        let white = TextureId::Managed(0);
        let textures = TexturesDelta {
            set: vec![set(white, None, vec![Color32::WHITE])],
            free: Vec::new(),
        };
        // At 2 pixels per point the 8x8 target is 4x4 points. Half-transparent
        // red covers the left half and is clipped to the top half.
        let red = Color32::from_rgba_premultiplied(128, 0, 0, 128);
        target.paint(
            2.0,
            &[rect(
                white,
                points([0.0, 0.0], [2.0, 4.0]),
                Pos2::ZERO,
                red,
                points([0.0, 0.0], [4.0, 2.0]),
            )],
            &textures,
        );
        // Target blue is 1.0 linear. Red 128 decodes to 0.216 linear and adds to
        // blue scaled by 1 - alpha, 0.498, which encode to 128 and 188.
        let [r, g, b] = target.pixel([1, 1]);
        assert!(
            r.abs_diff(128) <= 2 && g == 0 && b.abs_diff(188) <= 2,
            "blended: {r} {g} {b}"
        );
        assert_eq!(
            target.pixel([6, 1]),
            [0, 0, 255],
            "right of the rect keeps the target"
        );
        assert_eq!(
            target.pixel([1, 6]),
            [0, 0, 255],
            "clipped part keeps the target"
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn texture_patches_update_one_region_and_freed_textures_stop_drawing() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut target = Target::new();
        let id = TextureId::Managed(1);
        let whole = points([0.0, 0.0], [8.0, 8.0]);
        // Samples the center of texel 1 of a 2x1 texture.
        let draw = rect(id, whole, Pos2::new(0.75, 0.5), Color32::WHITE, whole);
        let full = TexturesDelta {
            set: vec![set(id, None, vec![Color32::RED, Color32::GREEN])],
            free: Vec::new(),
        };
        target.paint(1.0, std::slice::from_ref(&draw), &full);
        assert_eq!(target.pixel([4, 4]), [0, 255, 0]);
        let patch = TexturesDelta {
            set: vec![set(id, Some([1, 0]), vec![Color32::WHITE])],
            free: vec![id],
        };
        target.paint(1.0, std::slice::from_ref(&draw), &patch);
        assert_eq!(target.pixel([4, 4]), [255, 255, 255], "patched texel");
        // Freed after the previous frame, so this mesh is skipped.
        target.paint(
            1.0,
            std::slice::from_ref(&rect(
                id,
                whole,
                Pos2::ZERO,
                Color32::BLACK,
                whole,
            )),
            &TexturesDelta::default(),
        );
        assert_eq!(target.pixel([4, 4]), [255, 255, 255]);
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn native_textures_show_an_image_rendered_elsewhere() {
        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let mut target = Target::new();
        // A 2x2 image cleared on the GPU, as the scene renderer leaves its
        // offscreen view.
        let native = |color: [f32; 4]| {
            let image = Image::new(
                target.memory_allocator.clone(),
                ImageCreateInfo {
                    format: OFFSCREEN_COLOR_FORMAT,
                    extent: [2, 2, 1],
                    usage: ImageUsage::SAMPLED | ImageUsage::TRANSFER_DST,
                    ..Default::default()
                },
                AllocationCreateInfo::default(),
            )
            .unwrap();
            let mut commands = AutoCommandBufferBuilder::primary(
                target.painter.command_allocator.clone(),
                target.base.queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .unwrap();
            commands
                .clear_color_image(ClearColorImageInfo {
                    clear_value: color.into(),
                    ..ClearColorImageInfo::image(image.clone())
                })
                .unwrap();
            vulkano::sync::now(target.base.device.clone())
                .then_execute(
                    target.base.queue.clone(),
                    commands.build().unwrap(),
                )
                .unwrap()
                .then_signal_fence_and_flush()
                .unwrap()
                .wait(None)
                .unwrap();
            ImageView::new_default(image).unwrap()
        };
        let red = native([1.0, 0.0, 0.0, 1.0]);
        let green = native([0.0, 1.0, 0.0, 1.0]);
        let id = TextureId::User(0);
        let left = points([0.0, 0.0], [4.0, 8.0]);
        let draw = rect(id, left, Pos2::new(0.5, 0.5), Color32::WHITE, left);
        target.painter.set_native_texture(id, red.clone()).unwrap();
        target.painter.set_native_texture(id, red).unwrap();
        target.paint(
            1.0,
            std::slice::from_ref(&draw),
            &TexturesDelta::default(),
        );
        assert_eq!(target.pixel([1, 4]), [255, 0, 0]);
        assert_eq!(target.pixel([6, 4]), [0, 0, 255], "outside the rect");
        target.painter.set_native_texture(id, green).unwrap();
        target.paint(
            1.0,
            std::slice::from_ref(&draw),
            &TexturesDelta::default(),
        );
        assert_eq!(target.pixel([1, 4]), [0, 255, 0], "replaced image");
    }
}
