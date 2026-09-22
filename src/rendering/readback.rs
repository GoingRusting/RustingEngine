use std::sync::Arc;
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo,
};
use vulkano::device::{Device, Queue};
use vulkano::image::Image;
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::sync::GpuFuture;

/// Copies `image` to a host-visible buffer, submits a fenced command buffer,
/// waits for it to complete, and returns the raw pixel bytes.
pub fn read_back_image(
    device: &Arc<Device>,
    queue: &Arc<Queue>,
    memory_allocator: &Arc<StandardMemoryAllocator>,
    command_buffer_allocator: &Arc<StandardCommandBufferAllocator>,
    image: &Arc<Image>,
) -> Vec<u8> {
    let extent = image.extent();
    let buffer_size = u64::from(extent[0])
        * u64::from(extent[1])
        * image.format().block_size();

    let destination = Buffer::new_slice::<u8>(
        memory_allocator.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_RANDOM_ACCESS,
            ..Default::default()
        },
        buffer_size,
    )
    .unwrap();

    let mut builder = AutoCommandBufferBuilder::primary(
        command_buffer_allocator.clone(),
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .unwrap();

    builder
        .copy_image_to_buffer(CopyImageToBufferInfo::image_buffer(
            image.clone(),
            destination.clone(),
        ))
        .unwrap();

    let command_buffer = builder.build().unwrap();

    let future = vulkano::sync::now(device.clone())
        .then_execute(queue.clone(), command_buffer)
        .unwrap()
        .then_signal_fence_and_flush()
        .unwrap();
    future.wait(None).unwrap();

    let pixels = destination.read().unwrap().to_vec();
    pixels
}

/// Copies a storage buffer to a host-visible buffer, submits a fenced
/// command buffer, waits for it to complete, and returns the CPU data —
/// e.g. to read back results after a compute dispatch.
pub fn read_back_buffer<T>(
    device: &Arc<Device>,
    queue: &Arc<Queue>,
    memory_allocator: &Arc<StandardMemoryAllocator>,
    command_buffer_allocator: &Arc<StandardCommandBufferAllocator>,
    source: vulkano::buffer::Subbuffer<[T]>,
) -> Vec<T>
where
    T: vulkano::buffer::BufferContents + Clone,
{
    let destination = Buffer::new_slice::<T>(
        memory_allocator.clone(),
        BufferCreateInfo {
            usage: BufferUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_RANDOM_ACCESS,
            ..Default::default()
        },
        source.len(),
    )
    .unwrap();

    let mut builder = AutoCommandBufferBuilder::primary(
        command_buffer_allocator.clone(),
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .unwrap();

    builder
        .copy_buffer(vulkano::command_buffer::CopyBufferInfo::buffers(
            source,
            destination.clone(),
        ))
        .unwrap();

    let command_buffer = builder.build().unwrap();

    let future = vulkano::sync::now(device.clone())
        .then_execute(queue.clone(), command_buffer)
        .unwrap()
        .then_signal_fence_and_flush()
        .unwrap();
    future.wait(None).unwrap();

    let values = destination.read().unwrap().to_vec();
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendering::swapchain::{
        create_offscreen_target, create_render_pass_for_format,
        OFFSCREEN_COLOR_FORMAT,
    };
    use crate::rendering::test_support::headless_device;
    use vulkano::VulkanLibrary;

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn reads_back_an_offscreen_color_image_with_the_right_byte_count() {
        if VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let base = headless_device();
        let memory_allocator =
            Arc::new(StandardMemoryAllocator::new_default(base.device.clone()));
        let command_buffer_allocator =
            Arc::new(StandardCommandBufferAllocator::new(
                base.device.clone(),
                Default::default(),
            ));
        let render_pass = create_render_pass_for_format(
            base.device.clone(),
            OFFSCREEN_COLOR_FORMAT,
        );
        let target =
            create_offscreen_target(&memory_allocator, &render_pass, [4, 4]);

        let pixels = read_back_image(
            &base.device,
            &base.queue,
            &memory_allocator,
            &command_buffer_allocator,
            &target.color_image,
        );

        // 4x4 RGBA8 image = 64 bytes. The image is never rendered to here,
        // so only the byte count is asserted, not pixel content.
        assert_eq!(pixels.len(), 4 * 4 * 4);
    }

    mod double_shader {
        vulkano_shaders::shader! {
            ty: "compute",
            path: "src/shaders/compute/test_double.comp",
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn reads_back_storage_buffer_results_after_a_compute_dispatch() {
        use crate::rendering::test_support::dispatch_and_read_back;

        if VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let base = headless_device();

        let input: Vec<u32> = (0..64).collect();
        let shader = double_shader::load(base.device.clone()).unwrap();
        let result = dispatch_and_read_back(
            base,
            shader.entry_point("main").unwrap(),
            input.clone(),
            [1, 1, 1],
        );

        let expected: Vec<u32> = input.iter().map(|v| v * 2).collect();
        assert_eq!(result, expected);
    }
}
