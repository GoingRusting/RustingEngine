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

    mod sim_math_shader {
        vulkano_shaders::shader! {
            ty: "compute",
            include: ["src/shaders"],
            path: "src/shaders/compute/sim_math_test.comp",
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn shader_sim_math_matches_the_rust_reference_bit_for_bit() {
        use crate::rendering::test_support::dispatch_and_read_back;
        use crate::runtime::sim_math;

        if VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let base = headless_device();
        // p * p + c is 0 with separate rounding and 2^-24 when fused, so
        // the first case fails on a driver that contracts `sim_dot`.
        let p = 1.0 + 2f32.powi(-12);
        let mut cases =
            vec![([p, 1.0, 0.0], [p, -(1.0 + 2f32.powi(-11)), 0.0], 3.0)];
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            // Positive and negative values spread over 2^-20 .. 2^20.
            let magnitude = 2f32.powf((seed % 4000) as f32 / 100.0 - 20.0);
            if seed & 1 == 0 {
                magnitude
            } else {
                -magnitude
            }
        };
        while cases.len() < 64 {
            let v = [next(), next(), next()];
            let u = [next(), next(), next()];
            cases.push((v, u, next().abs()));
        }
        // Inputs for sim_to_int: GLSL int() is undefined for the specials.
        let specials = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            2f32.powi(31),
            -(2f32.powi(31)),
            3e9,
            -3e9,
            -0.5,
            1.5,
            -2.5,
        ];
        let ints: Vec<f32> = (0..cases.len())
            .map(|index| {
                specials
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| next() * 4096.0)
            })
            .collect();
        let input: Vec<u32> = cases
            .iter()
            .zip(&ints)
            .flat_map(|((v, u, s), t)| {
                v.iter()
                    .chain(u)
                    .chain([s, t, &0.0])
                    .map(|value| value.to_bits())
            })
            .collect();
        let shader = sim_math_shader::load(base.device.clone()).unwrap();
        let result = dispatch_and_read_back(
            base,
            shader.entry_point("main").unwrap(),
            input,
            [1, 1, 1],
        );
        let expected: Vec<u32> = cases
            .iter()
            .zip(&ints)
            .flat_map(|(&(v, u, s), &t)| {
                let row = [v[0], u[0], s];
                [
                    sim_math::recip(s),
                    sim_math::rsqrt(s),
                    sim_math::sqrt(s),
                    sim_math::div(v[0], s),
                    sim_math::dot(v, u),
                    sim_math::length(v),
                    sim_math::normalize(v)[1],
                    sim_math::dot(row, u),
                ]
                .map(f32::to_bits)
                .into_iter()
                .chain([sim_math::to_int(t).cast_unsigned()])
            })
            .collect();
        assert_eq!(f32::from_bits(expected[4]), 0.0, "reference must not fuse");
        assert_eq!(result, expected);
    }
}
