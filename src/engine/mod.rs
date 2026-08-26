//! High-level game engine API for the rendering engine.
//!
//! This module provides the main `Engine` struct that wraps all low-level Vulkan
//! complexity into a simple, easy-to-use API. It handles:
//!
//! - Window creation and event handling via Winit
//! - Vulkan initialization and swapchain management
//! - Scene management (adding cubes, spheres, GLTF models)
//! - Camera controls with keyboard/mouse input
//! - Physics simulation loop at fixed timestep
//! - Rendering with optional frustum culling
//!
//! # Quick Start
//!
//! ```ignore
//! let mut engine = Engine::new("My Game");
//! engine.add_cube(
//!     Transform { position: [0.0, 0.0, 0.0], ..Default::default() },
//!     &Material::standard().build(),
//!     &Physics::default()
//! );
//! engine.run();
//! ```

use crate::core::{Material, Physics, Transform};
use crate::geometry::gltf_loader::load_gltf_scene;
use crate::geometry::shapes::{create_cube, create_sphere_subdivided};
use crate::rendering::camera::create_projection_matrix;
use crate::rendering::compute_registry::{
    ComputeShaderRegistry, ComputeShaderType,
};
use crate::rendering::frame_pacer::select_present_mode;
use crate::rendering::init_vulkan;
use crate::rendering::render::create_builder;
use crate::rendering::shader_registry::{ShaderRegistry, ShaderType};
use crate::rendering::swapchain::{create_framebuffers, create_render_pass};
use crate::rendering::VulkanBase;
use crate::runtime::RenderSettings;
use crate::scene::object::Instance;
use crate::scene::{
    begin_render_pass_only, record_compute_physics_multi, RenderScene,
};
use nalgebra::Matrix4;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vulkano::sync::GpuFuture;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event::{DeviceEvent, DeviceId};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::CursorGrabMode;
use winit::window::WindowId;

use crate::rendering::compute_registry::CullPushConstants;
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::image::ImageUsage;
use vulkano::pipeline::Pipeline;
use vulkano::pipeline::PipelineBindPoint;
use vulkano::swapchain::{
    Swapchain, SwapchainCreateInfo, SwapchainPresentInfo,
};
use vulkano::sync;

/// Perspective camera for 3D rendering.
///
/// Controls the view matrix based on position, yaw/pitch angles, and handles
/// keyboard input for WASD movement and mouse look.
pub struct PerspectiveCamera {
    /// Camera position in world space (X, Y, Z)
    pub position: [f32; 3],
    /// Horizontal rotation angle in radians
    pub yaw: f32,
    /// Vertical rotation angle in radians
    pub pitch: f32,
    /// Field of view in radians
    pub fov: f32,
    /// Aspect ratio (width / height)
    pub aspect: f32,
    /// Near clipping plane distance
    pub near: f32,
    /// Far clipping plane distance
    pub far: f32,
}

impl PerspectiveCamera {
    /// Creates a new perspective camera with default position and orientation.
    ///
    /// # Arguments
    /// * `fov` - Vertical field of view in degrees
    /// * `aspect` - Aspect ratio (width / height)
    /// * `near` - Near clipping plane
    /// * `far` - Far clipping plane
    pub fn new(fov: f32, aspect: f32, near: f32, far: f32) -> Self {
        Self {
            position: [0.0, 5.0, 20.0],
            yaw: 90.0f32.to_radians(),
            pitch: 0.0,
            fov: fov.to_radians(),
            aspect,
            near,
            far,
        }
    }

    /// Updates camera position and orientation based on keyboard/mouse input.
    ///
    /// Handles WASD movement, Space/LControl for vertical movement, and mouse
    /// look when the mouse is captured. Returns the view matrix.
    ///
    /// # Arguments
    /// * `keys` - Set of currently pressed keys
    /// * `sprint` - Movement speed multiplier (2.0 for sprint, 1.0 for normal)
    /// * `dt` - Time since last frame in seconds
    /// * `mouse_captured` - Whether mouse look is active
    ///
    /// # Returns
    /// The view matrix transforming world coordinates to camera space
    pub fn update(
        &mut self,
        keys: &HashSet<KeyCode>,
        sprint: f32,
        dt: f32,
        mouse_captured: bool,
    ) -> [[f32; 4]; 4] {
        let (yaw_sin, yaw_cos) = self.yaw.sin_cos();
        let (pitch_sin, pitch_cos) = self.pitch.sin_cos();

        let forward = [-self.yaw.cos(), 0.0, -self.yaw.sin()];
        let len = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
        let forward = [forward[0] / len, 0.0, forward[2] / len];

        let right = [-self.yaw.sin(), 0.0, self.yaw.cos()];
        let view_forward =
            [yaw_cos * pitch_cos, pitch_sin, yaw_sin * pitch_cos];

        if mouse_captured {
            let speed = 50.0 * sprint * dt;
            if keys.contains(&KeyCode::KeyW) {
                for (position, direction) in
                    self.position.iter_mut().zip(forward)
                {
                    *position += direction * speed;
                }
            }
            if keys.contains(&KeyCode::KeyS) {
                for (position, direction) in
                    self.position.iter_mut().zip(forward)
                {
                    *position -= direction * speed;
                }
            }
            if keys.contains(&KeyCode::KeyA) {
                for (position, direction) in self.position.iter_mut().zip(right)
                {
                    *position -= direction * speed;
                }
            }
            if keys.contains(&KeyCode::KeyD) {
                for (position, direction) in self.position.iter_mut().zip(right)
                {
                    *position += direction * speed;
                }
            }
            if keys.contains(&KeyCode::Space) {
                self.position[1] += speed;
            }
            if keys.contains(&KeyCode::ControlLeft) {
                self.position[1] -= speed;
            }
        }

        let target = [
            self.position[0] + view_forward[0],
            self.position[1] + view_forward[1],
            self.position[2] + view_forward[2],
        ];

        crate::rendering::camera::create_look_at(
            self.position,
            target,
            [0.0, 1.0, 0.0],
        )
    }
}

/// High-level engine structure for building and rendering scenes.
///
/// Handles Vulkan context, swapchain, render pass, rendering pipeline, inputs,
/// and the physics simulation loop.
pub struct Engine {
    /// Rendering and frame-rate options used by the legacy engine facade.
    pub render_settings: RenderSettings,
    /// The camera - controls view matrix and receives input
    pub camera: Arc<Mutex<PerspectiveCamera>>,
    /// The render scene - holds all objects, batches, and GPU resources
    scene: Arc<Mutex<RenderScene>>,
    /// Winit event loop - takes ownership when run() is called
    event_loop: Option<EventLoop<()>>,
    /// Base Vulkan resources (device, queue, window, surface)
    base: VulkanBase,
    /// Vulkan swapchain - manages presentation
    swapchain: Arc<Swapchain>,
    /// Swapchain images - the render targets
    images: Vec<Arc<vulkano::image::Image>>,
    /// Render pass - defines the rendering pipeline stages
    render_pass: Arc<vulkano::render_pass::RenderPass>,
    /// Graphics shader registry - maps shader types to pipelines
    registry: ShaderRegistry,
    /// Compute shader registry - maps physics types to pipelines
    compute_registry: ComputeShaderRegistry,
    /// Memory allocator for GPU buffers
    memory_allocator: Arc<vulkano::memory::allocator::StandardMemoryAllocator>,
    /// Descriptor set allocator for pipeline resources
    descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
    /// Command buffer allocator - manages GPU command buffers
    command_buffer_allocator: Arc<StandardCommandBufferAllocator>,
    /// Cached cube mesh - reused for all cube instances
    cached_cube_mesh: Option<crate::geometry::Mesh>,
    /// Cached sphere meshes, keyed by subdivision level
    cached_sphere_meshes: HashMap<u32, crate::geometry::Mesh>,
    /// Global loaded textures map: Path/Name -> Texture ID
    pub textures_cache: HashMap<String, usize>,
    /// Global GLTF Cache: Path -> parsed models and relative transforms
    pub gltf_cache: HashMap<String, Vec<(crate::geometry::Mesh, Instance)>>,
}

impl Engine {
    /// Creates a new Engine and initializes Vulkan, Winit, and rendering pipelines.
    ///
    /// This sets up:
    /// - Window and Vulkan device/queue
    /// - Swapchain and render pass
    /// - Shader registries (graphics and compute)
    /// - Memory and descriptor set allocators
    /// - Empty scene ready for objects
    ///
    /// # Arguments
    /// * `title` - The window title.
    ///
    /// # Examples
    /// ```no_run
    /// let engine = rusting_engine::Engine::new("My Game");
    /// ```
    pub fn new(title: &str) -> Self {
        Self::with_render_settings(title, RenderSettings::default())
    }

    /// Creates an engine with explicit rendering and frame-rate settings.
    pub fn with_render_settings(
        title: &str,
        render_settings: RenderSettings,
    ) -> Self {
        let event_loop = EventLoop::new().expect("Failed to create event loop");
        let base = init_vulkan(&event_loop, title);
        let dims = base.window.inner_size();

        let physical = base.device.physical_device();
        let capabilities = physical
            .surface_capabilities(&base.surface, Default::default())
            .expect("Failed to query surface capabilities");
        let (image_format, image_color_space) = physical
            .surface_formats(&base.surface, Default::default())
            .expect("Failed to query surface formats")
            .into_iter()
            .find(|(format, _)| {
                matches!(
                    format,
                    vulkano::format::Format::B8G8R8A8_SRGB
                        | vulkano::format::Format::R8G8B8A8_SRGB
                )
            })
            .unwrap_or_else(|| {
                physical
                    .surface_formats(&base.surface, Default::default())
                    .expect("Failed to query surface formats")[0]
            });
        let present_mode = select_present_mode(
            &base.queue,
            &base.surface,
            render_settings.vsync,
        );
        let mut min_image_count =
            capabilities.min_image_count.saturating_add(1);
        if let Some(max_image_count) = capabilities.max_image_count {
            min_image_count = min_image_count.min(max_image_count);
        }

        let (swapchain, images) = Swapchain::new(
            base.device.clone(),
            base.surface.clone(),
            SwapchainCreateInfo {
                min_image_count,
                image_format,
                image_color_space,
                image_extent: [dims.width, dims.height],
                image_usage: ImageUsage::COLOR_ATTACHMENT,
                composite_alpha: capabilities
                    .supported_composite_alpha
                    .into_iter()
                    .next()
                    .expect("Surface supports no composite alpha mode"),
                present_mode,
                ..Default::default()
            },
        )
        .unwrap();

        let render_pass = create_render_pass(base.device.clone(), &swapchain);
        let registry = ShaderRegistry::new(&base.device, &render_pass);

        let cb_allocator = Arc::new(StandardCommandBufferAllocator::new(
            base.device.clone(),
            Default::default(),
        ));
        let ds_allocator = Arc::new(StandardDescriptorSetAllocator::new(
            base.device.clone(),
            Default::default(),
        ));
        let mem_allocator = Arc::new(
            vulkano::memory::allocator::StandardMemoryAllocator::new_default(
                base.device.clone(),
            ),
        );

        let scene = RenderScene::new(
            &mem_allocator,
            &ds_allocator,
            registry.default_pipeline(),
            &base.queue,
            3,
            1_000_000, // 1m
        );

        let compute_registry = ComputeShaderRegistry::new(&base.device);

        Self {
            render_settings,
            event_loop: Some(event_loop),
            base,
            swapchain,
            images,
            render_pass,
            registry,
            compute_registry,
            memory_allocator: mem_allocator,
            descriptor_set_allocator: ds_allocator,
            command_buffer_allocator: cb_allocator,
            scene: Arc::new(Mutex::new(scene)),
            camera: Arc::new(Mutex::new(PerspectiveCamera::new(
                45.0,
                dims.width as f32 / dims.height as f32,
                0.1,
                1000.0,
            ))),
            cached_cube_mesh: None,
            cached_sphere_meshes: HashMap::new(),
            textures_cache: HashMap::new(),
            gltf_cache: HashMap::new(),
        }
    }

    /// Sets the main light source in the scene.
    ///
    /// This configures the directional/point light used for shading.
    ///
    /// # Arguments
    /// * `pos` - Light position in world space (X, Y, Z)
    /// * `color` - Light color as RGB values (typically 0.0-1.0)
    /// * `intensity` - Light brightness multiplier
    pub fn set_light(
        &mut self,
        pos: [f32; 3],
        color: [f32; 3],
        intensity: f32,
    ) {
        self.scene.lock().unwrap().set_light(pos, color, intensity);
    }

    /// Adds a cube to the scene.
    ///
    /// Creates a cube mesh with the given transform, material, and physics properties.
    /// The mesh is cached and reused for all subsequent cube additions.
    ///
    /// # Arguments
    /// * `transform` - Position, rotation, and scale of the cube
    /// * `mat` - Material properties (color, shader, roughness, metalness)
    /// * `phys` - Physics properties (collision type, mass, bounciness, etc.)
    pub fn add_cube(
        &mut self,
        transform: Transform,
        mat: &Material,
        phys: &Physics,
    ) {
        // Cache mesh on first use, then reuse
        let mesh = if let Some(cached) = &self.cached_cube_mesh {
            cached.clone()
        } else {
            let m = create_cube(&self.memory_allocator);
            self.cached_cube_mesh = Some(m.clone());
            m
        };

        let mut inst = Instance {
            model_matrix: transform.to_matrix(),
            physics: *phys,
            ..Default::default()
        };
        inst.apply_material(mat);
        self.scene.lock().unwrap().add_instance(
            mesh,
            inst,
            &self.memory_allocator,
        );
    }

    /// Adds a sphere to the scene.
    ///
    /// Creates a sphere mesh with the given transform, material, and physics properties.
    /// The mesh is cached and reused for all subsequent sphere additions.
    ///
    /// # Arguments
    /// * `transform` - Position, rotation, and scale of the sphere
    /// * `mat` - Material properties (color, shader, roughness, metalness)
    /// * `phys` - Physics properties (collision type, mass, bounciness, etc.)
    /// * `subdiv` - Number of subdivisions (higher = smoother sphere, costs more)
    pub fn add_sphere(
        &mut self,
        transform: Transform,
        mat: &Material,
        phys: &Physics,
        subdiv: u32,
    ) {
        // Each subdivision level has different geometry and needs its own cache entry.
        let mesh = if let Some(cached) = self.cached_sphere_meshes.get(&subdiv)
        {
            cached.clone()
        } else {
            let m = create_sphere_subdivided(&self.memory_allocator, subdiv);
            self.cached_sphere_meshes.insert(subdiv, m.clone());
            m
        };

        let mut inst = Instance {
            model_matrix: transform.to_matrix(),
            physics: *phys,
            ..Default::default()
        };
        inst.apply_material(mat);
        self.scene.lock().unwrap().add_instance(
            mesh,
            inst,
            &self.memory_allocator,
        );
    }

    /// Adds a GLTF model to the scene.
    ///
    /// Loads a 3D model from a GLTF file and adds all its meshes as instances.
    /// Textures from the model are automatically uploaded and assigned indices.
    ///
    /// # Arguments
    /// * `transform` - Position, rotation, scale of the model
    /// * `mat` - Material properties to apply to all meshes
    /// * `phys` - Physics properties for collision simulation
    /// * `path` - Path to the .gltf or .glb file
    pub fn load_texture(&mut self, path: &str) -> usize {
        if let Some(&id) = self.textures_cache.get(path) {
            return id;
        }

        // Load image using the image crate
        let img = image::open(path)
            .unwrap_or_else(|e| {
                panic!("Failed to load texture {}: {}", path, e)
            })
            .into_rgba8();
        let width = img.width();
        let height = img.height();

        let tex = crate::scene::object::Texture {
            width,
            height,
            pixels: img.into_raw(),
        };

        let base_texture_count = self.scene.lock().unwrap().texture_views.len();
        let pipeline = self.registry.default_pipeline();

        self.scene.lock().unwrap().set_textures(
            pipeline,
            &[tex],
            &self.base.queue,
            &self.memory_allocator,
        );

        self.textures_cache
            .insert(path.to_string(), base_texture_count);
        base_texture_count
    }

    /// Adds a GLTF model to the scene.
    ///
    /// Loads a 3D model from a GLTF file and adds all its meshes as instances.
    /// Textures from the model are automatically uploaded and assigned indices.
    ///
    /// # Arguments
    /// * `transform` - Position, rotation, scale of the model
    /// * `mat` - Material properties to apply to all meshes
    /// * `phys` - Physics properties for collision simulation
    /// * `path` - Path to the .gltf or .glb file
    pub fn add_gltf(
        &mut self,
        transform: Transform,
        mat: &Material,
        phys: &Physics,
        path: &str,
    ) {
        if !self.gltf_cache.contains_key(path) {
            let (mut objects, textures) =
                load_gltf_scene(&self.memory_allocator, path);
            let base_texture_count =
                self.scene.lock().unwrap().texture_views.len();
            let pipeline = self.registry.default_pipeline();

            if !textures.is_empty() {
                self.scene.lock().unwrap().set_textures(
                    pipeline,
                    &textures,
                    &self.base.queue,
                    &self.memory_allocator,
                );
            }

            for (_mesh, instance) in &mut objects {
                if let Some(tex_idx) = instance.base_color_texture {
                    instance.base_color_texture =
                        Some(tex_idx + base_texture_count);
                }
                if let Some(tex_idx) = instance.metallic_roughness_texture {
                    instance.metallic_roughness_texture =
                        Some(tex_idx + base_texture_count);
                }
            }
            self.gltf_cache.insert(path.to_string(), objects);
        }

        let objects = self.gltf_cache.get(path).unwrap().clone();

        for (mesh, mut instance) in objects {
            // Apply new transform onto the base model transform
            let transform_matrix = Matrix4::from(transform.to_matrix());
            let instance_matrix = Matrix4::from(instance.model_matrix);
            let combined_matrix = transform_matrix * instance_matrix;
            instance.model_matrix = combined_matrix.into();
            instance.physics = *phys;
            instance.shader = mat.shader;

            // Allow override of material properties and textures
            instance.color = mat.color;
            instance.roughness = mat.roughness;
            instance.metalness = mat.metalness;
            instance.emissive = mat.emissive;
            if mat.base_color_texture.is_some() {
                instance.base_color_texture = mat.base_color_texture;
            }
            if mat.metallic_roughness_texture.is_some() {
                instance.metallic_roughness_texture =
                    mat.metallic_roughness_texture;
            }

            self.scene.lock().unwrap().add_instance(
                mesh.clone(),
                instance,
                &self.memory_allocator,
            );
        }
    }

    /// Sets a scene-wide graphics shader override.
    ///
    /// All objects will use the specified shader instead of their per-object shader.
    /// Useful for post-processing effects or uniform visual style.
    ///
    /// # Arguments
    /// * `shader` - The shader type to use for all objects
    pub fn set_scene_shader(&mut self, shader: ShaderType) {
        self.registry.set_scene_shader(shader);
    }

    /// Clears the scene-wide graphics shader override.
    ///
    /// Objects will revert to using their individual per-object shader.
    pub fn clear_scene_shader(&mut self) {
        self.registry.clear_scene_shader();
    }

    /// Sets a scene-wide physics shader override.
    ///
    /// All objects will use the specified physics simulation instead of their
    /// per-object physics type. Useful for testing different physics behaviors.
    ///
    /// # Arguments
    /// * `shader` - The compute shader type to use for physics
    pub fn set_scene_physic(&mut self, shader: ComputeShaderType) {
        self.compute_registry.set_scene_shader(shader);
    }

    /// Clears the scene-wide physics shader override.
    ///
    /// Objects will revert to using their individual per-object physics type.
    pub fn clear_scene_physic(&mut self) {
        self.compute_registry.clear_scene_shader();
    }

    /// Starts the engine's main game loop.
    ///
    /// This method blocks until the window is closed. It handles:
    /// - Window events (resize, close, keyboard, mouse)
    /// - Physics simulation at fixed 60 FPS timestep
    /// - Optional frustum culling via compute shader
    /// - Rendering with triple-buffered frames
    ///
    /// Press Escape to capture/release mouse for camera look.
    /// Press C to toggle frustum culling.
    /// Hold Shift to sprint.
    pub fn run(mut self) {
        let requested_present_mode = select_present_mode(
            &self.base.queue,
            &self.base.surface,
            self.render_settings.vsync,
        );
        if self.swapchain.create_info().present_mode != requested_present_mode {
            let (swapchain, images) = self
                .swapchain
                .recreate(SwapchainCreateInfo {
                    present_mode: requested_present_mode,
                    ..self.swapchain.create_info()
                })
                .expect("Failed to apply the configured presentation mode");
            self.swapchain = swapchain;
            self.images = images;
        }

        eprintln!("[DBG] run() start");
        eprintln!("[DBG] starting upload");
        let (
            physics_read,
            physics_write,
            solid_obj_count,
            dispatches,
            visible_indices_buffer,
        ) = {
            let mut s = self.scene.lock().unwrap();
            let d = s.upload_to_gpu(
                &self.memory_allocator,
                &self.base.queue,
                &self.compute_registry,
            );
            eprintln!("[DBG] upload done {}", d.len());
            let tex_count = s.texture_views.len();
            s.ensure_descriptor_cache(
                self.registry.default_pipeline(),
                tex_count,
            );
            (
                s.physics_read.clone(),
                s.physics_write.clone(),
                s.total_instances,
                d,
                s.visible_indices.clone(), // Use scene's buffer for culling
            )
        };
        eprintln!("[DBG] creating compute_sets");

        let mut compute_sets: HashMap<
            ComputeShaderType,
            (Arc<DescriptorSet>, Arc<DescriptorSet>),
        > = HashMap::new();

        let mut used_shaders = HashSet::new();
        for dispatch in &dispatches {
            used_shaders.insert(dispatch.compute_shader);
        }

        if let Some(scene_shader) =
            self.compute_registry.scene_shader_optional()
        {
            used_shaders.insert(scene_shader);
        }

        for shader in used_shaders {
            let compute_layout = self
                .compute_registry
                .get_pipeline(shader)
                .layout()
                .set_layouts()[0]
                .clone();

            let bindings = shader.needs_bindings();
            let scene = self.scene.lock().unwrap();

            let mut writes_0: Vec<WriteDescriptorSet> = vec![];
            let mut writes_1: Vec<WriteDescriptorSet> = vec![];

            if bindings.needs_read_buffer {
                writes_0
                    .push(WriteDescriptorSet::buffer(0, physics_read.clone()));
                writes_1
                    .push(WriteDescriptorSet::buffer(0, physics_write.clone()));
            }
            if bindings.needs_write_buffer {
                writes_0
                    .push(WriteDescriptorSet::buffer(1, physics_write.clone()));
                writes_1
                    .push(WriteDescriptorSet::buffer(1, physics_read.clone()));
            }
            if bindings.needs_grid_counts {
                writes_0.push(WriteDescriptorSet::buffer(
                    2,
                    scene.grid_counts.clone(),
                ));
                writes_1.push(WriteDescriptorSet::buffer(
                    2,
                    scene.grid_counts.clone(),
                ));
            }
            if bindings.needs_grid_objects {
                writes_0.push(WriteDescriptorSet::buffer(
                    3,
                    scene.grid_objects.clone(),
                ));
                writes_1.push(WriteDescriptorSet::buffer(
                    3,
                    scene.grid_objects.clone(),
                ));
            }
            if bindings.needs_big_indices {
                writes_0.push(WriteDescriptorSet::buffer(
                    4,
                    scene.big_objects_indices.clone(),
                ));
                writes_1.push(WriteDescriptorSet::buffer(
                    4,
                    scene.big_objects_indices.clone(),
                ));
            }

            let set_0 = DescriptorSet::new(
                self.descriptor_set_allocator.clone(),
                compute_layout.clone(),
                writes_0,
                [],
            )
            .unwrap();

            let set_1 = DescriptorSet::new(
                self.descriptor_set_allocator.clone(),
                compute_layout.clone(),
                writes_1,
                [],
            )
            .unwrap();

            compute_sets.insert(shader, (set_0, set_1));
        }
        eprintln!("[DBG] compute_sets done");

        let grid_build_sets = {
            let scene = self.scene.lock().unwrap();
            let grid_layout = self
                .compute_registry
                .get_pipeline(ComputeShaderType::GridBuild)
                .layout()
                .set_layouts()[0]
                .clone();

            let gb_set_0 = DescriptorSet::new(
                self.descriptor_set_allocator.clone(),
                grid_layout.clone(),
                [
                    WriteDescriptorSet::buffer(0, physics_read.clone()),
                    WriteDescriptorSet::buffer(2, scene.grid_counts.clone()),
                    WriteDescriptorSet::buffer(3, scene.grid_objects.clone()),
                ],
                [],
            )
            .unwrap();

            let gb_set_1 = DescriptorSet::new(
                self.descriptor_set_allocator.clone(),
                grid_layout,
                [
                    WriteDescriptorSet::buffer(0, physics_write.clone()),
                    WriteDescriptorSet::buffer(2, scene.grid_counts.clone()),
                    WriteDescriptorSet::buffer(3, scene.grid_objects.clone()),
                ],
                [],
            )
            .unwrap();

            (gb_set_0, gb_set_1)
        };
        eprintln!("[DBG] grid_build_sets done, starting event loop");

        let mut framebuffers = create_framebuffers(
            &self.images,
            &self.render_pass,
            &self.memory_allocator,
        );
        let mut inputs = InputState::default();
        let mut last_frame_instant = Instant::now();
        let mut accumulator = 0.0;
        let fixed_dt = 1.0 / 60.0;
        let mut compute_ping_pong = false;
        let mut recreate_swapchain = false;
        let mut frame_index = 0;
        let mut fps_timer = Instant::now();
        let mut frame_count = 0;
        let mut next_frame = Instant::now();
        // Retain the submitted frame so swapchain images and per-frame GPU
        // resources are not reused while the device is still reading them.
        let mut previous_frame_end: Option<Box<dyn GpuFuture>> =
            Some(sync::now(self.base.device.clone()).boxed());

        let event_loop = self.event_loop.take().unwrap();
        event_loop.set_control_flow(ControlFlow::Poll);
        let window_id = self.base.window.id();
        let mut handler = EngineEventHandler::new(
            window_id,
            move |event: EngineEvent, active_event_loop: &ActiveEventLoop| {
                match event {
                    EngineEvent::Window(WindowEvent::CloseRequested) => {
                        active_event_loop.exit()
                    }
                    EngineEvent::Window(WindowEvent::Resized(_)) => {
                        recreate_swapchain = true
                    }
                    EngineEvent::Device(DeviceEvent::MouseMotion { delta })
                        if inputs.mouse_captured =>
                    {
                        let mut cam = self.camera.lock().unwrap();
                        cam.yaw -= delta.0 as f32 * 0.001;
                        cam.pitch += delta.1 as f32 * 0.001;
                        cam.pitch = cam.pitch.clamp(-1.5, 1.5);
                    }
                    EngineEvent::Window(WindowEvent::KeyboardInput {
                        event,
                        ..
                    }) => {
                        if let PhysicalKey::Code(code) = event.physical_key {
                            if event.state
                                == winit::event::ElementState::Pressed
                            {
                                if code == KeyCode::Escape {
                                    inputs.mouse_captured =
                                        !inputs.mouse_captured;
                                    let _ = self.base.window.set_cursor_grab(
                                        if inputs.mouse_captured {
                                            CursorGrabMode::Locked
                                        } else {
                                            CursorGrabMode::None
                                        },
                                    );
                                    self.base.window.set_cursor_visible(
                                        !inputs.mouse_captured,
                                    );
                                }
                                if code == KeyCode::KeyC {
                                    inputs.cull_enabled = !inputs.cull_enabled;
                                    eprintln!(
                                        "[DBG] Culling {}",
                                        if inputs.cull_enabled {
                                            "ENABLED"
                                        } else {
                                            "DISABLED"
                                        }
                                    );
                                }
                                inputs.keys.insert(code);
                            } else {
                                inputs.keys.remove(&code);
                            }
                        }
                    }

                    EngineEvent::AboutToWait => {
                        if self.render_settings.limit_fps {
                            let interval = Duration::from_secs_f64(
                                1.0 / f64::from(
                                    self.render_settings.max_fps.max(1),
                                ),
                            );
                            let now = Instant::now();
                            if now < next_frame {
                                active_event_loop.set_control_flow(
                                    ControlFlow::WaitUntil(next_frame),
                                );
                                return;
                            }
                            next_frame = now + interval;
                            active_event_loop.set_control_flow(
                                ControlFlow::WaitUntil(next_frame),
                            );
                        } else {
                            next_frame = Instant::now();
                            active_event_loop
                                .set_control_flow(ControlFlow::Poll);
                        }

                        frame_index = (frame_index + 1) % 3;

                        let frame_start = std::time::Instant::now();
                        let now = Instant::now();
                        let mut delta_time = now
                            .duration_since(last_frame_instant)
                            .as_secs_f32();
                        last_frame_instant = now;
                        if delta_time > 0.05 {
                            delta_time = 0.05;
                        }
                        accumulator += delta_time;

                        if recreate_swapchain {
                            let new_size = self.base.window.inner_size();
                            if new_size.width > 0 && new_size.height > 0 {
                                let (new_sw, new_img) = self
                                    .swapchain
                                    .recreate(SwapchainCreateInfo {
                                        image_extent: new_size.into(),
                                        ..self.swapchain.create_info()
                                    })
                                    .unwrap();
                                self.swapchain = new_sw;
                                framebuffers = create_framebuffers(
                                    &new_img,
                                    &self.render_pass,
                                    &self.memory_allocator,
                                );
                                self.camera.lock().unwrap().aspect =
                                    new_size.width as f32
                                        / new_size.height as f32;
                            }
                            recreate_swapchain = false;
                        }

                        let (img_index, suboptimal, acquire_future) =
                            match vulkano::swapchain::acquire_next_image(
                                self.swapchain.clone(),
                                None,
                            ) {
                                Ok(r) => r,
                                Err(vulkano::Validated::Error(
                                    vulkano::VulkanError::OutOfDate,
                                )) => {
                                    recreate_swapchain = true;
                                    return;
                                }
                                Err(e) => panic!("{e}"),
                            };
                        if suboptimal {
                            recreate_swapchain = true;
                        }

                        let (proj, view, cam_pos) = {
                            let mut cam = self.camera.lock().unwrap();
                            let sprint =
                                if inputs.keys.contains(&KeyCode::ShiftLeft) {
                                    2.0
                                } else {
                                    1.0
                                };
                            let view = cam.update(
                                &inputs.keys,
                                sprint,
                                delta_time,
                                inputs.mouse_captured,
                            );
                            let proj = create_projection_matrix(
                                cam.aspect, cam.fov, cam.near, cam.far,
                            );
                            let cam_pos = cam.position;
                            (proj, view, cam_pos)
                        };

                        {
                            let mut s = self.scene.lock().unwrap();
                            s.prepare_frame_ubo(
                                frame_index,
                                view,
                                proj,
                                cam_pos,
                            );
                            let tex_count = s.texture_views.len();
                            s.ensure_descriptor_cache(
                                self.registry.default_pipeline(),
                                tex_count,
                            );
                        }

                        let mut comp_builder = create_builder(
                            &self.command_buffer_allocator,
                            &self.base.queue,
                        );
                        let mut physics_ran = false;

                        while accumulator >= fixed_dt {
                            let scene = self.scene.lock().unwrap();
                            let cell_size = scene.max_object_radius * 2.0 + 0.2;
                            record_compute_physics_multi(
                                &mut comp_builder,
                                &self.compute_registry,
                                &compute_sets,
                                &grid_build_sets,
                                &scene.grid_counts,
                                &dispatches,
                                fixed_dt,
                                solid_obj_count,
                                cell_size,
                                scene.num_big_objects,
                                compute_ping_pong,
                            );

                            compute_ping_pong = !compute_ping_pong;
                            accumulator -= fixed_dt;
                            physics_ran = true;
                        }

                        let physics_idx = if compute_ping_pong { 1 } else { 0 };

                        if physics_ran {
                            let comp_cb = comp_builder.build().unwrap();
                            let comp_future =
                                sync::now(self.base.device.clone())
                                    .then_execute(
                                        self.base.queue.clone(),
                                        comp_cb,
                                    )
                                    .unwrap()
                                    .then_signal_fence_and_flush()
                                    .unwrap();

                            comp_future.wait(None).unwrap();
                        }

                        let mut comp_builder = create_builder(
                            &self.command_buffer_allocator,
                            &self.base.queue,
                        );

                        if inputs.cull_enabled {
                            let cull_start = std::time::Instant::now();

                            let view_proj = {
                                let p = cgmath::Matrix4::from(proj);
                                let v = cgmath::Matrix4::from(view);
                                let vp: [[f32; 4]; 4] = (p * v).into();
                                vp
                            };

                            let current_physics_buffer = if physics_idx == 0 {
                                physics_read.clone()
                            } else {
                                physics_write.clone()
                            };

                            let mut s = self.scene.lock().unwrap();
                            let mut current_physics_offset = 0u32;

                            for batch in &mut s.batches {
                                let count = batch.instances.len() as u32;
                                if count == 0 {
                                    continue;
                                }

                                {
                                    let mut guard =
                                        batch.indirect_buffer.write().unwrap();
                                    guard[0].instance_count = 0;
                                }

                                let cull_pipeline = self
                                    .compute_registry
                                    .get_pipeline(ComputeShaderType::Cull);
                                let cull_set = DescriptorSet::new(
                                    self.descriptor_set_allocator.clone(),
                                    cull_pipeline.layout().set_layouts()[0]
                                        .clone(),
                                    [
                                        WriteDescriptorSet::buffer(
                                            0,
                                            current_physics_buffer.clone(),
                                        ),
                                        WriteDescriptorSet::buffer(
                                            1,
                                            visible_indices_buffer.clone(),
                                        ),
                                        WriteDescriptorSet::buffer(
                                            2,
                                            batch.indirect_buffer.clone(),
                                        ),
                                    ],
                                    [],
                                )
                                .unwrap();

                                comp_builder
                                    .bind_pipeline_compute(
                                        cull_pipeline.clone(),
                                    )
                                    .unwrap()
                                    .bind_descriptor_sets(
                                        PipelineBindPoint::Compute,
                                        cull_pipeline.layout().clone(),
                                        0,
                                        cull_set,
                                    )
                                    .unwrap()
                                    .push_constants(
                                        cull_pipeline.layout().clone(),
                                        0,
                                        CullPushConstants {
                                            view_proj,
                                            batch_offset:
                                                current_physics_offset,
                                            batch_count: count,
                                            visible_list_offset: batch
                                                .base_instance_offset,
                                        },
                                    )
                                    .unwrap();
                                unsafe {
                                    comp_builder.dispatch([
                                        count.div_ceil(256),
                                        1,
                                        1,
                                    ])
                                }
                                .unwrap();

                                current_physics_offset += count;
                            }

                            let cull_cb = comp_builder.build().unwrap();

                            let cull_future =
                                sync::now(self.base.device.clone())
                                    .then_execute(
                                        self.base.queue.clone(),
                                        cull_cb,
                                    )
                                    .unwrap()
                                    .then_signal_fence_and_flush()
                                    .unwrap();

                            cull_future.wait(None).unwrap();

                            if frame_count <= 3 {
                                let visible: u32 = s
                                    .batches
                                    .iter()
                                    .map(|b| {
                                        let guard =
                                            b.indirect_buffer.read().unwrap();
                                        guard[0].instance_count
                                    })
                                    .sum();
                                eprintln!(
                                "[DBG] Culling took {}us — visible: {} / {}",
                                cull_start.elapsed().as_micros(),
                                visible,
                                solid_obj_count,
                            );
                            }

                            drop(s);
                        }

                        let mut render_builder = create_builder(
                            &self.command_buffer_allocator,
                            &self.base.queue,
                        );
                        {
                            let mut s = self.scene.lock().unwrap();
                            begin_render_pass_only(
                                &mut render_builder,
                                &framebuffers,
                                img_index,
                                self.base.window.inner_size().into(),
                                self.registry.default_pipeline(),
                            );
                            s.record_draws_multi(
                                &mut render_builder,
                                &self.registry,
                                frame_index,
                                physics_idx,
                                inputs.cull_enabled,
                            );
                            render_builder
                                .end_render_pass(Default::default())
                                .unwrap();
                        }

                        let render_cb = render_builder.build().unwrap();

                        previous_frame_end.as_mut().unwrap().cleanup_finished();
                        let future = previous_frame_end
                            .take()
                            .unwrap()
                            .join(acquire_future)
                            .then_execute(self.base.queue.clone(), render_cb)
                            .unwrap()
                            .then_swapchain_present(
                                self.base.queue.clone(),
                                SwapchainPresentInfo::swapchain_image_index(
                                    self.swapchain.clone(),
                                    img_index,
                                ),
                            )
                            .then_signal_fence_and_flush();

                        match future {
                            Ok(future) => {
                                previous_frame_end = Some(future.boxed());
                                frame_count += 1;
                                if fps_timer.elapsed().as_secs_f32() >= 2.0 {
                                    println!(
                                        "Presented FPS: {:.0}",
                                        frame_count as f32
                                            / fps_timer.elapsed().as_secs_f32()
                                    );
                                    frame_count = 0;
                                    fps_timer = Instant::now();
                                }
                                if frame_count <= 2 {
                                    eprintln!(
                                        "[DBG] CPU frame submission: {}us",
                                        frame_start.elapsed().as_micros()
                                    );
                                }
                            }
                            Err(vulkano::Validated::Error(
                                vulkano::VulkanError::OutOfDate,
                            )) => {
                                recreate_swapchain = true;
                                previous_frame_end = Some(
                                    sync::now(self.base.device.clone()).boxed(),
                                );
                            }
                            Err(e) => {
                                eprintln!("[DBG] Flush error: {:?}", e);
                                previous_frame_end = Some(
                                    sync::now(self.base.device.clone()).boxed(),
                                );
                            }
                        }
                    }
                    _ => (),
                }
            },
        );
        event_loop
            .run_app(&mut handler)
            .expect("Engine event loop failed");
    }
}

enum EngineEvent {
    Window(WindowEvent),
    Device(DeviceEvent),
    AboutToWait,
}

struct EngineEventHandler<F> {
    window_id: WindowId,
    callback: F,
}

impl<F> EngineEventHandler<F> {
    fn new(window_id: WindowId, callback: F) -> Self {
        Self {
            window_id,
            callback,
        }
    }
}

impl<F> ApplicationHandler for EngineEventHandler<F>
where
    F: FnMut(EngineEvent, &ActiveEventLoop),
{
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if window_id == self.window_id {
            (self.callback)(EngineEvent::Window(event), event_loop);
        }
    }

    fn device_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        (self.callback)(EngineEvent::Device(event), event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        (self.callback)(EngineEvent::AboutToWait, event_loop);
    }
}

/// Tracks keyboard and mouse input state for the current frame.
/// Used internally by the engine to handle user input.
#[derive(Default)]
struct InputState {
    /// Set of currently pressed keyboard keys
    keys: HashSet<KeyCode>,
    /// Whether the mouse is captured (for camera look)
    mouse_captured: bool,
    /// Whether frustum culling is enabled
    cull_enabled: bool,
}
