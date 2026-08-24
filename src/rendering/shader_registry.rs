use std::collections::HashMap;
use std::sync::Arc;

use vulkano::device::Device;
use vulkano::pipeline::GraphicsPipeline;
use vulkano::render_pass::RenderPass;

use crate::rendering::pipeline::create_pipeline;
use crate::shaders::{fragment::*, vertex::vs};

/// Fragment shader variant that a user can assign to an object or scene.
/// Determines which fragment shader is used during rendering.
/// `Pbr` is the most powerful one
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShaderType {
    /// Full PBR (Cook-Torrance BRDF, fog, vignette, tone-mapping). This is the default.
    #[default]
    Pbr,
    /// Flat color output — no lighting calculations at all. Use for UI elements,
    /// skyboxes, or objects where lighting is meaningless.
    Unlit,
    /// Emissive glow with tone-mapping and vignette. Objects look like they emit light.
    Emissive,
    /// Visualize surface normals as RGB colors. Useful for debugging geometry.
    NormalDebug,
    /// Intentionally heavy shader (8 lights, FBM noise, SSS, AO).
    Heavy,
}

impl ShaderType {
    /// Returns all available shader types in the order they should be sorted for rendering.
    pub fn all() -> &'static [ShaderType] {
        &[
            ShaderType::Pbr,
            ShaderType::Unlit,
            ShaderType::Emissive,
            ShaderType::NormalDebug,
            ShaderType::Heavy,
        ]
    }

    /// Returns the sort key for this shader type (used to minimize pipeline switches).
    pub fn sort_key(&self) -> u32 {
        match self {
            ShaderType::Pbr => 0,
            ShaderType::Unlit => 1,
            ShaderType::Emissive => 2,
            ShaderType::NormalDebug => 3,
            ShaderType::Heavy => 4,
        }
    }
}

/// Manages one `GraphicsPipeline` per `ShaderType`.
/// All pipelines share the same vertex shader, vertex input state, descriptor set layout,
/// and render pass — only the fragment shader module differs.
pub struct ShaderRegistry {
    pipelines: HashMap<ShaderType, Arc<GraphicsPipeline>>,
    scene_shader: Option<ShaderType>,
}

impl ShaderRegistry {
    /// Create a new registry, loading all shader modules and building all pipelines.
    pub fn new(device: &Arc<Device>, render_pass: &Arc<RenderPass>) -> Self {
        let vs_module =
            vs::load(device.clone()).expect("Failed to load vertex shader");

        let mut pipelines = HashMap::new();

        // PBR (default)
        let fs_pbr = fs::load(device.clone())
            .expect("Failed to load PBR fragment shader");
        pipelines.insert(
            ShaderType::Pbr,
            create_pipeline(vs_module.clone(), fs_pbr, render_pass, device),
        );

        // Unlit
        let fs_unlit_mod = fs_unlit::load(device.clone())
            .expect("Failed to load Unlit fragment shader");
        pipelines.insert(
            ShaderType::Unlit,
            create_pipeline(
                vs_module.clone(),
                fs_unlit_mod,
                render_pass,
                device,
            ),
        );

        // Emissive
        let fs_emissive_mod = fs_emissive::load(device.clone())
            .expect("Failed to load Emissive fragment shader");
        pipelines.insert(
            ShaderType::Emissive,
            create_pipeline(
                vs_module.clone(),
                fs_emissive_mod,
                render_pass,
                device,
            ),
        );

        // NormalDebug
        let fs_normal_mod = fs_normal_debug::load(device.clone())
            .expect("Failed to load NormalDebug fragment shader");
        pipelines.insert(
            ShaderType::NormalDebug,
            create_pipeline(
                vs_module.clone(),
                fs_normal_mod,
                render_pass,
                device,
            ),
        );

        let fs_heavy_mod = fs_heavy::load(device.clone())
            .expect("Failed to load Heavy fragment shader");
        pipelines.insert(
            ShaderType::Heavy,
            create_pipeline(vs_module, fs_heavy_mod, render_pass, device),
        );

        Self {
            pipelines,
            scene_shader: None,
        }
    }

    /// Get the pipeline for a specific shader type.
    pub fn get_pipeline(
        &self,
        shader_type: ShaderType,
    ) -> &Arc<GraphicsPipeline> {
        self.pipelines
            .get(&shader_type)
            .expect("ShaderType pipeline not found in registry")
    }

    /// Get the default (PBR) pipeline. Used for descriptor set layout compatibility.
    pub fn default_pipeline(&self) -> &Arc<GraphicsPipeline> {
        self.get_pipeline(ShaderType::Pbr)
    }

    /// Resolve the effective shader for an object: scene override wins, then per-object shader.
    pub fn resolve_shader(&self, object_shader: ShaderType) -> ShaderType {
        self.scene_shader.unwrap_or(object_shader)
    }

    /// Set a scene-wide shader override. All objects will use this shader,
    /// ignoring their per-object shader setting.
    pub fn set_scene_shader(&mut self, shader: ShaderType) {
        self.scene_shader = Some(shader);
    }

    /// Clear the scene-wide shader override. Objects will use their per-object shader.
    pub fn clear_scene_shader(&mut self) {
        self.scene_shader = None;
    }

    /// Check if a scene-wide shader override is active.
    pub fn scene_shader(&self) -> Option<ShaderType> {
        self.scene_shader
    }
}
