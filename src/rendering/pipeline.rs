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
            },
        )
        .attribute(
            0,
            VertexInputAttributeDescription {
                binding: 0,
                format: Format::R32G32B32_SFLOAT,
                offset: 0,
            },
        )
        .attribute(
            1,
            VertexInputAttributeDescription {
                binding: 0,
                format: Format::R32G32B32_SFLOAT,
                offset: 12,
            },
        )
        .attribute(
            2,
            VertexInputAttributeDescription {
                binding: 0,
                format: Format::R32G32_SFLOAT,
                offset: 24,
            },
        )
}

use std::sync::Arc;
use vulkano::device::Device;
use vulkano::pipeline::graphics::depth_stencil::DepthStencilState;
use vulkano::pipeline::graphics::input_assembly::InputAssemblyState;
use vulkano::pipeline::graphics::input_assembly::PrimitiveTopology;
use vulkano::pipeline::graphics::rasterization::FrontFace;
use vulkano::pipeline::graphics::rasterization::{CullMode, RasterizationState};
use vulkano::pipeline::graphics::viewport::ViewportState;
use vulkano::pipeline::layout::PipelineLayout;
use vulkano::pipeline::layout::PipelineLayoutCreateInfo;
use vulkano::pipeline::layout::PushConstantRange;
use vulkano::pipeline::Pipeline;
use vulkano::render_pass::RenderPass;
use vulkano::render_pass::Subpass;
use vulkano::shader::ShaderModule;

pub fn create_pipeline(
    vs: Arc<ShaderModule>,
    fs: Arc<ShaderModule>,
    render_pass: &Arc<RenderPass>,
    device: &Arc<Device>,
) -> std::sync::Arc<GraphicsPipeline> {
    use vulkano::descriptor_set::layout::DescriptorSetLayout;

    let temp_pipeline = GraphicsPipeline::start()
        .vertex_input_state(create_vertex_input_state())
        .vertex_shader(vs.entry_point("main").unwrap(), ())
        .input_assembly_state(InputAssemblyState::new().topology(PrimitiveTopology::TriangleList))
        .viewport_state(ViewportState::viewport_dynamic_scissor_irrelevant())
        .rasterization_state(
            RasterizationState::new()
                .cull_mode(CullMode::Back)
                .front_face(FrontFace::Clockwise),
        )
        .fragment_shader(fs.entry_point("main").unwrap(), ())
        .depth_stencil_state(DepthStencilState::simple_depth_test())
        .render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
        .build(device.clone())
        .unwrap();

    let set_layouts: Vec<Arc<DescriptorSetLayout>> = temp_pipeline.layout().set_layouts().to_vec();

    let pipeline_layout = PipelineLayout::new(
        device.clone(),
        PipelineLayoutCreateInfo {
            set_layouts,
            push_constant_ranges: vec![PushConstantRange {
                stages: vulkano::shader::ShaderStages::VERTEX,
                offset: 0,
                size: 8, // Two u32s: v_visible_list_offset and v_use_culling
            }],
            ..Default::default()
        },
    )
    .unwrap();

    let pipeline = GraphicsPipeline::start()
        .vertex_input_state(create_vertex_input_state())
        .vertex_shader(vs.entry_point("main").unwrap(), ())
        .input_assembly_state(InputAssemblyState::new().topology(PrimitiveTopology::TriangleList))
        .viewport_state(ViewportState::viewport_dynamic_scissor_irrelevant())
        .rasterization_state(
            RasterizationState::new()
                .cull_mode(CullMode::Back)
                .front_face(FrontFace::Clockwise),
        )
        .fragment_shader(fs.entry_point("main").unwrap(), ())
        .depth_stencil_state(DepthStencilState::simple_depth_test())
        .render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
        .with_pipeline_layout(device.clone(), pipeline_layout)
        .unwrap();

    pipeline
}
