use std::sync::Arc;
use vulkano::device::Device;
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::render_pass::{Framebuffer, FramebufferCreateInfo, RenderPass};
use vulkano::swapchain::{
    PresentMode, Surface, Swapchain, SwapchainCreateInfo,
};
use winit::window::Window;

pub fn create_swapchain_and_images(
    device: &Arc<Device>,
    surface: &Arc<Surface>,
    window: &Window,
) -> (Arc<Swapchain>, Vec<Arc<Image>>) {
    let caps = device
        .physical_device()
        .surface_capabilities(surface, Default::default())
        .unwrap();
    let formats = device
        .physical_device()
        .surface_formats(surface, Default::default())
        .unwrap();
    let (format, color_space) = formats
        .iter()
        .copied()
        .find(|(format, _)| {
            matches!(format, Format::B8G8R8A8_SRGB | Format::R8G8B8A8_SRGB)
        })
        .unwrap_or(formats[0]);
    let present_modes = device
        .physical_device()
        .surface_present_modes(surface, Default::default())
        .unwrap();
    let present_mode = [
        PresentMode::Mailbox,
        PresentMode::Immediate,
        PresentMode::Fifo,
    ]
    .into_iter()
    .find(|mode| present_modes.contains(mode))
    .expect("Vulkan surfaces must support FIFO presentation");
    let min_image_count = caps
        .max_image_count
        .map_or(caps.min_image_count + 1, |max| {
            (caps.min_image_count + 1).min(max)
        });

    let (sw, img) = Swapchain::new(
        device.clone(),
        surface.clone(),
        SwapchainCreateInfo {
            min_image_count,
            image_format: format,
            image_color_space: color_space,
            image_extent: window.inner_size().into(),
            image_usage: ImageUsage::COLOR_ATTACHMENT, // We'll draw to these images
            composite_alpha: caps
                .supported_composite_alpha
                .into_iter()
                .next()
                .unwrap(),
            present_mode,
            ..Default::default()
        },
    )
    .unwrap();
    (sw, img)
}

pub fn create_render_pass(
    device: Arc<Device>,
    swapchain: &Arc<Swapchain>,
) -> std::sync::Arc<RenderPass> {
    vulkano::ordered_passes_renderpass!(
        device.clone(),
        attachments: {
            color: {
                format: swapchain.image_format(),
                samples: 1,
                load_op: Clear,
                store_op: Store,
            },
            depth: {
                format: vulkano::format::Format::D16_UNORM,
                samples: 1,
                load_op: Clear,
                store_op: DontCare,
            }
        },
        passes: [ {
            color: [color],
            depth_stencil: {depth},
            input: []
        } ],
    )
    .unwrap()
}

pub fn create_framebuffers(
    images: &[Arc<Image>],
    render_pass: &Arc<RenderPass>,
    memory_allocator: &Arc<StandardMemoryAllocator>,
) -> Vec<Arc<Framebuffer>> {
    let extent = images[0].extent();
    let dims = [extent[0], extent[1]];

    // Depth buffer is required for 3D sorting
    let depth_image = Image::new(
        memory_allocator.clone(),
        ImageCreateInfo {
            format: Format::D16_UNORM,
            extent: [dims[0], dims[1], 1],
            usage: ImageUsage::DEPTH_STENCIL_ATTACHMENT
                | ImageUsage::TRANSIENT_ATTACHMENT,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .unwrap();
    let depth_view = ImageView::new_default(depth_image).unwrap();

    images
        .iter()
        .map(|image| {
            let view = ImageView::new_default(image.clone()).unwrap();
            Framebuffer::new(
                render_pass.clone(),
                FramebufferCreateInfo {
                    attachments: vec![view, depth_view.clone()],
                    ..Default::default()
                },
            )
            .unwrap()
        })
        .collect()
}
