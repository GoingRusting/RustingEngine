use crate::geometry::VertexPosColorUv;
use vulkano::format::Format;
use vulkano::pipeline::graphics::vertex_input::VertexInputAttributeDescription;
use vulkano::pipeline::graphics::vertex_input::VertexInputBindingDescription;
use vulkano::pipeline::graphics::vertex_input::VertexInputRate;
use vulkano::pipeline::graphics::vertex_input::VertexInputState;
use vulkano::pipeline::graphics::GraphicsPipeline;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UniformBufferObject {
    pub view: [[f32; 4]; 4],   // 64 bytes
    pub proj: [[f32; 4]; 4],   // 64 bytes
    pub eye_pos: [f32; 3],     // 12 bytes
    pub padding_1: f32,        // 4 bytes
    pub light_pos: [f32; 3],   // 12 bytes
    pub padding_2: f32,        // 4 bytes
    pub light_color: [f32; 3], // 12 bytes
    pub light_intensity: f32,  // 4 bytes
}

impl Default for UniformBufferObject {
    fn default() -> Self {
        Self {
            view: [[0.0; 4]; 4],
            proj: [[0.0; 4]; 4],
            eye_pos: [0.0; 3],
            padding_1: 0.0,
            light_pos: [0.0, 10.0, 0.0],
            padding_2: 0.0,
            light_color: [1.0, 1.0, 1.0],
            light_intensity: 50.0,
        }
    }
}

pub fn create_vertex_input_state() -> VertexInputState {
    VertexInputState::new()
        .binding(
            0,
            VertexInputBindingDescription {
                stride: std::mem::size_of::<VertexPosColorUv>() as u32,
                input_rate: VertexInputRate::Vertex,
                ..Default::default()
            },
        )
        .attribute(
            0,
            VertexInputAttributeDescription {
                binding: 0,
                format: Format::R32G32B32_SFLOAT,
                offset: 0,
                ..Default::default()
            },
        )
        .attribute(
            1,
            VertexInputAttributeDescription {
                binding: 0,
                format: Format::R32G32B32_SFLOAT,
                offset: 12,
                ..Default::default()
            },
        )
        .attribute(
            2,
            VertexInputAttributeDescription {
                binding: 0,
                format: Format::R32G32_SFLOAT,
                offset: 24,
                ..Default::default()
            },
        )
}

use std::sync::Arc;
use vulkano::device::Device;
use vulkano::pipeline::graphics::color_blend::{
    ColorBlendAttachmentState, ColorBlendState,
};
use vulkano::pipeline::graphics::depth_stencil::{
    DepthState, DepthStencilState,
};
use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
use vulkano::pipeline::graphics::input_assembly::PrimitiveTopology;
use vulkano::pipeline::graphics::multisample::MultisampleState;
use vulkano::pipeline::graphics::rasterization::FrontFace;
use vulkano::pipeline::graphics::rasterization::{
    CullMode, RasterizationState,
};
use vulkano::pipeline::graphics::subpass::PipelineSubpassType;
use vulkano::pipeline::graphics::viewport::ViewportState;
use vulkano::pipeline::graphics::GraphicsPipelineCreateInfo;
use vulkano::pipeline::layout::{
    PipelineDescriptorSetLayoutCreateInfo, PipelineLayout,
};
use vulkano::pipeline::{DynamicState, PipelineShaderStageCreateInfo};
use vulkano::render_pass::RenderPass;
use vulkano::render_pass::Subpass;
use vulkano::shader::ShaderModule;

pub fn create_pipeline(
    vs: Arc<ShaderModule>,
    fs: Arc<ShaderModule>,
    render_pass: &Arc<RenderPass>,
    device: &Arc<Device>,
) -> std::sync::Arc<GraphicsPipeline> {
    let stages = [
        PipelineShaderStageCreateInfo::new(vs.entry_point("main").unwrap()),
        PipelineShaderStageCreateInfo::new(fs.entry_point("main").unwrap()),
    ];
    let pipeline_layout = PipelineLayout::new(
        device.clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages(stages.iter())
            .into_pipeline_layout_create_info(device.clone())
            .unwrap(),
    )
    .unwrap();

    GraphicsPipeline::new(
        device.clone(),
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(create_vertex_input_state()),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState {
                cull_mode: CullMode::Back,
                front_face: FrontFace::Clockwise,
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
            subpass: Some(PipelineSubpassType::BeginRenderPass(
                Subpass::from(render_pass.clone(), 0).unwrap(),
            )),
            ..GraphicsPipelineCreateInfo::layout(pipeline_layout)
        },
    )
    .unwrap()
}
