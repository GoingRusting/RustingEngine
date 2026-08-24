use crate::scene::object::PhysicsPushConstants;
use std::sync::Arc;
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, PrimaryAutoCommandBuffer, RenderPassBeginInfo,
    SubpassContents,
};
use vulkano::descriptor_set::PersistentDescriptorSet;
use vulkano::device::Queue;
use vulkano::pipeline::graphics::viewport::Viewport;
use vulkano::pipeline::{ComputePipeline, GraphicsPipeline, Pipeline, PipelineBindPoint};
use vulkano::render_pass::Framebuffer;

#[allow(clippy::too_many_arguments)]
pub fn process_render(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    framebuffers: &[Arc<Framebuffer>],
    img_index: u32,
    dims: [u32; 2],
    pipeline: &Arc<GraphicsPipeline>,
    compute_pipeline: &Arc<ComputePipeline>,
    compute_set: &Arc<PersistentDescriptorSet>,
    total_objects: u32,
    dt: f32,
    num_big_objects: u32,
) {
    if total_objects > 0 {
        let workgroups_x = total_objects.div_ceil(256);

        builder
            .bind_pipeline_compute(compute_pipeline.clone())
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                compute_pipeline.layout().clone(),
                0,
                compute_set.clone(),
            )
            .push_constants(
                compute_pipeline.layout().clone(),
                0,
                PhysicsPushConstants {
                    dt,
                    total_objects,
                    offset: 0,
                    count: total_objects,
                    num_big_objects,
                    _pad: [0, 0, 0],
                    global_gravity: [0.0, -9.81, 0.0, 2.0],
                },
            )
            .dispatch([workgroups_x, 1, 1])
            .unwrap();
    }

    builder
        .begin_render_pass(
            RenderPassBeginInfo {
                clear_values: vec![Some([0.0, 0.0, 0.0, 1.0].into()), Some(1.0.into())],
                ..RenderPassBeginInfo::framebuffer(framebuffers[img_index as usize].clone())
            },
            SubpassContents::Inline,
        )
        .unwrap()
        .set_viewport(
            0,
            vec![Viewport {
                origin: [0.0, 0.0],
                dimensions: [dims[0] as f32, dims[1] as f32],
                depth_range: 0.0..1.0,
            }],
        )
        .bind_pipeline_graphics(pipeline.clone());
}

pub fn create_builder(
    cb_allocator: &StandardCommandBufferAllocator,
    queue: &Arc<Queue>,
) -> AutoCommandBufferBuilder<PrimaryAutoCommandBuffer> {
    AutoCommandBufferBuilder::primary(
        cb_allocator,
        queue.queue_family_index(),
        CommandBufferUsage::OneTimeSubmit,
    )
    .unwrap()
}
