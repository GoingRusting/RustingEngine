//! Scene management module for the rendering engine.
//!
//! This module handles the creation, management, and rendering of 3D scenes.
//! It provides structures for managing render batches, instances, physics data,
//! and GPU buffers. The scene system supports instanced rendering with
//! GPU-accelerated physics simulation via compute shaders.
//!
//! Key components:
//! - `RenderScene`: Main scene container holding all renderable objects
//! - `RenderBatch`: Groups instances sharing the same mesh and shader
//! - `Instance`: Individual object with transform, material, and physics properties
//! - Compute dispatch system for parallel physics simulation

pub mod animation;
pub mod object;

use crate::scene::object::PhysicsPushConstants;
use std::collections::HashMap;
use std::sync::Arc;

/// Returns the push-constant vector used by one legacy compute profile.
///
/// Normal physics receives a directional gravity vector. Space mode instead
/// receives an attractor position in xyz and its strength in w.
fn physics_force_vector(shader: ComputeShaderType, cell_size: f32) -> [f32; 4] {
    if shader == ComputeShaderType::Space {
        [0.0, 0.0, 0.0, 500.0]
    } else {
        [0.0, -9.81, 0.0, cell_size]
    }
}
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer};
use vulkano::command_buffer::allocator::{
    StandardCommandBufferAllocator, StandardCommandBufferAllocatorCreateInfo,
};
use vulkano::command_buffer::RenderPassBeginInfo;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, CopyBufferInfoTyped,
    CopyBufferToImageInfo, PrimaryAutoCommandBuffer,
    PrimaryCommandBufferAbstract,
};
use vulkano::command_buffer::{SubpassBeginInfo, SubpassContents};
use vulkano::descriptor_set::{
    allocator::StandardDescriptorSetAllocator, DescriptorSet,
    WriteDescriptorSet,
};
use vulkano::device::Queue;
use vulkano::format::Format;
use vulkano::image::sampler::{
    Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode,
    LOD_CLAMP_NONE,
};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::pipeline::graphics::viewport::Viewport;
use vulkano::pipeline::{GraphicsPipeline, Pipeline, PipelineBindPoint};
use vulkano::render_pass::Framebuffer;
use vulkano::sync::GpuFuture;

use crate::geometry::Mesh;
use crate::rendering::compute_registry::ComputeShaderRegistry;
use crate::rendering::compute_registry::ComputeShaderType;
use crate::rendering::pipeline::UniformBufferObject;
use crate::rendering::shader_registry::{ShaderRegistry, ShaderType};
use crate::scene::object::{Instance, InstanceData, RenderBatch, Texture};
use vulkano::command_buffer::DrawIndexedIndirectCommand;

/// Push constants for mesh rendering.
/// These are passed directly to the shader for per-draw configuration.
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct MeshPushConstants {
    /// Offset into the visible list for culling - determines where to start reading instance indices
    pub visible_list_offset: u32,
    /// Culling mode: 0 = draw all instances directly, 1 = use indirect drawing with culling
    pub use_culling: u32,
}

/// Information needed to dispatch a compute shader.
/// Contains the shader type, which instances to process, and how many.
pub struct ComputeDispatchInfo {
    /// Which compute shader to use for this batch
    pub compute_shader: ComputeShaderType,
    /// Starting offset in the instance buffer for this batch
    pub offset: u32,
    /// Number of instances this shader should process
    pub count: u32,
}

/// Main scene container that holds all renderable objects and GPU resources.
///
/// The scene manages:
/// - Render batches (groups of instances with same mesh/shader)
/// - Frame data (one set per frame in flight)
/// - Texture resources and samplers
/// - Physics buffers (read/write for ping-pong updates)
/// - Spatial grid structures for collision detection
pub struct RenderScene {
    /// All render batches in the scene - each batch shares mesh geometry and shader
    pub batches: Vec<RenderBatch>,
    /// Per-frame data for double/triple buffering - holds uniform buffers
    pub frames: Vec<FrameData>,
    /// Position of the main light source in world space
    pub light_pos: [f32; 3],
    /// RGB color of the light
    pub light_color: [f32; 3],
    /// Brightness multiplier for the light
    pub light_intensity: f32,
    /// Image views for all textures in the scene
    pub texture_views: Vec<Arc<ImageView>>,
    /// Sampler used to sample textures - handles filtering and addressing modes
    pub texture_sampler: Arc<Sampler>,
    /// Allocator for descriptor sets - manages GPU memory for descriptors
    pub descriptor_set_allocator: Arc<StandardDescriptorSetAllocator>,
    /// Cached descriptor sets organized by frame and texture index
    pub descriptor_sets: Vec<Vec<Arc<DescriptorSet>>>,
    /// Physics state buffer read by compute shaders - contains instance transforms/velocities
    pub physics_read: Subbuffer<[InstanceData]>,
    /// Physics state buffer written by compute shaders - double-buffered with physics_read
    pub physics_write: Subbuffer<[InstanceData]>,
    /// Indices of objects classified as "large" - need special collision handling
    pub big_objects_indices: Subbuffer<[u32]>,
    /// Number of large objects currently in the scene
    pub num_big_objects: u32,
    /// Count of objects in each grid cell - used for spatial partitioning
    pub grid_counts: Subbuffer<[u32]>,
    /// List of object indices organized by grid cell - enables fast spatial queries
    pub grid_objects: Subbuffer<[u32]>,
    /// Indices of instances that passed visibility test - used by culling
    pub visible_indices: Subbuffer<[u32]>,
    /// Total number of instances across all batches
    pub total_instances: u32,
    /// Maximum radius of small objects - used to calculate grid cell size
    pub max_object_radius: f32,
    /// Monotonically increasing source for stable instance handles.
    next_instance_id: u64,
}

/// Per-frame data storage.
/// Each frame in flight has its own uniform buffer for view/projection matrices.
pub struct FrameData {
    /// Uniform buffer containing view matrix, projection matrix, camera position, and light data
    pub uniform_buffer: Subbuffer<UniformBufferObject>,
}

/// Handle to a specific instance in the scene.
/// Used to remove or modify instances after they've been added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InstanceHandle {
    /// Stable identity that remains valid when batches are sorted or compacted.
    id: u64,
}

impl RenderScene {
    /// Adds a new instance to the scene.
    ///
    /// The instance is placed into an existing batch if one exists with matching
    /// mesh and shader, otherwise a new batch is created. Returns a handle that
    /// can be used to remove or modify the instance later.
    ///
    /// # Arguments
    /// * `mesh` - The mesh geometry for this instance
    /// * `instance` - The instance data including transform, material, and physics
    /// * `allocator` - Memory allocator for creating buffers
    ///
    /// # Returns
    /// An `InstanceHandle` that references this instance in the scene
    pub fn add_instance(
        &mut self,
        mesh: Mesh,
        instance: Instance,
        allocator: &Arc<StandardMemoryAllocator>,
    ) -> InstanceHandle {
        let id = self.next_instance_id;
        self.next_instance_id = self
            .next_instance_id
            .checked_add(1)
            .expect("instance handle ID space exhausted");

        let shader = instance.shader;
        let compute_shader = instance.physics.compute_shader;
        let base_color_texture = instance.base_color_texture;
        let metallic_roughness_texture = instance.metallic_roughness_texture;
        for batch in &mut self.batches {
            if batch.mesh.vertices.buffer() == mesh.vertices.buffer()
                && batch.shader == shader
                && batch.compute_shader == compute_shader
                && batch.base_color_texture == base_color_texture
                && batch.metallic_roughness_texture
                    == metallic_roughness_texture
            {
                batch.instances.push(instance);
                batch.instance_ids.push(id);

                return InstanceHandle { id };
            }
        }

        self.batches.push(RenderBatch {
            mesh,
            instance_ids: vec![id],
            base_color_texture,
            metallic_roughness_texture,
            shader,
            compute_shader,
            instances: vec![instance],
            base_instance_offset: self.total_instances,
            indirect_buffer: Buffer::new_slice::<DrawIndexedIndirectCommand>(
                allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::INDIRECT_BUFFER
                        | BufferUsage::STORAGE_BUFFER
                        | BufferUsage::TRANSFER_DST,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                1,
            )
            .unwrap(),
        });

        InstanceHandle { id }
    }

    /// Removes an instance from the scene using its handle.
    ///
    /// The instance is removed from its batch. If the batch becomes empty,
    /// the batch is also removed. Uses swap-remove for efficiency.
    ///
    /// # Arguments
    /// * `handle` - The handle returned by `add_instance`
    pub fn remove_instance(&mut self, handle: InstanceHandle) {
        let Some(batch_index) = self
            .batches
            .iter()
            .position(|batch| batch.instance_ids.contains(&handle.id))
        else {
            return;
        };

        let instance_index = self.batches[batch_index]
            .instance_ids
            .iter()
            .position(|id| *id == handle.id)
            .expect("located batch must contain the instance ID");

        let batch = &mut self.batches[batch_index];
        batch.instances.swap_remove(instance_index);
        batch.instance_ids.swap_remove(instance_index);

        if batch.instances.is_empty() {
            self.batches.swap_remove(batch_index);
        }
    }

    /// Creates a new render scene with all required GPU resources.
    ///
    /// This initializes:
    /// - Default white texture (used when no texture is specified)
    /// - Texture sampler with linear filtering
    /// - Physics buffers (double-buffered for ping-pong updates)
    /// - Spatial grid structures for collision detection
    /// - Per-frame uniform buffers
    ///
    /// # Arguments
    /// * `memory_allocator` - For creating GPU buffers
    /// * `descriptor_set_allocator` - For creating descriptor sets
    /// * `_pipeline` - Graphics pipeline (unused, kept for compatibility)
    /// * `queue` - Queue family to execute commands on
    /// * `frames_in_flight` - Number of frames to buffer (usually 2 or 3)
    /// * `max_instances` - Maximum number of instances supported
    ///
    /// # Returns
    /// A new `RenderScene` ready to receive instances
    pub fn new(
        memory_allocator: &Arc<StandardMemoryAllocator>,
        descriptor_set_allocator: &Arc<StandardDescriptorSetAllocator>,
        _pipeline: &Arc<GraphicsPipeline>,
        queue: &Arc<Queue>,
        frames_in_flight: usize,
        max_instances: usize,
    ) -> Self {
        let mut frames = Vec::new();
        let default_texture = Self::create_texture_image(
            memory_allocator,
            queue,
            &[255, 255, 255, 255],
            1,
            1,
        );
        let default_texture_view =
            ImageView::new_default(default_texture).unwrap();
        let texture_sampler = Sampler::new(
            queue.device().clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Linear,
                lod: 0.0..=LOD_CLAMP_NONE,
                address_mode: [SamplerAddressMode::Repeat; 3],
                ..Default::default()
            },
        )
        .unwrap();

        let physics_read = Buffer::new_slice::<InstanceData>(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER
                    | BufferUsage::TRANSFER_DST
                    | BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
            max_instances as u64,
        )
        .unwrap();
        let physics_write = Buffer::new_slice::<InstanceData>(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER
                    | BufferUsage::TRANSFER_DST
                    | BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
            max_instances as u64,
        )
        .unwrap();

        for _ in 0..frames_in_flight {
            let uniform_buffer = Buffer::from_data(
                memory_allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::UNIFORM_BUFFER,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                UniformBufferObject::default(),
            )
            .unwrap();

            frames.push(FrameData { uniform_buffer });
        }
        let hash_size = 65521;
        let max_per_cell = 128;

        let big_objects_indices = Buffer::new_slice::<u32>(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
            1024,
        )
        .unwrap();

        let grid_counts = Buffer::new_slice::<u32>(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
            hash_size,
        )
        .unwrap();

        let grid_objects = Buffer::new_slice::<u32>(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
            hash_size * max_per_cell,
        )
        .unwrap();

        let visible_indices = Buffer::new_slice::<u32>(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER | BufferUsage::TRANSFER_DST,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
            1_000_000,
        )
        .unwrap();

        Self {
            batches: Vec::new(),
            frames,
            light_pos: [0.0, 10.0, 0.0],
            light_color: [1.0, 1.0, 1.0],
            light_intensity: 50.0,
            texture_views: vec![default_texture_view],
            texture_sampler,
            descriptor_set_allocator: descriptor_set_allocator.clone(),
            descriptor_sets: vec![Vec::new(); frames_in_flight],
            total_instances: 0,
            physics_read,
            physics_write,
            big_objects_indices,
            num_big_objects: 0,
            grid_counts,
            grid_objects,
            visible_indices,
            max_object_radius: 0.0,
            next_instance_id: 0,
        }
    }

    /// Uploads all instance data to GPU memory and prepares for rendering.
    ///
    /// This function:
    /// 1. Gathers all instance data from batches into a flat array
    /// 2. Classifies objects as "big" or "small" based on radius
    /// 3. Copies data to physics buffers via staging buffers
    /// 4. Updates indirect draw buffers for each batch
    /// 5. Returns compute dispatch info for physics simulation
    ///
    /// # Arguments
    /// * `allocator` - Memory allocator for staging buffers
    /// * `queue` - Queue to execute transfer commands
    /// * `compute_registry` - Registry of compute shaders
    ///
    /// # Returns
    /// Vector of `ComputeDispatchInfo` describing each compute shader dispatch
    pub fn upload_to_gpu(
        &mut self,
        allocator: &Arc<StandardMemoryAllocator>,
        queue: &Arc<Queue>,
        compute_registry: &ComputeShaderRegistry,
    ) -> Vec<ComputeDispatchInfo> {
        let total_instances = self
            .batches
            .iter()
            .map(|batch| batch.instances.len())
            .sum::<usize>();
        self.total_instances = total_instances as u32;

        if total_instances == 0 {
            return vec![];
        }

        let override_shader = compute_registry.scene_shader_optional();
        if override_shader.is_none() {
            self.batches.sort_by_key(|b| {
                (b.compute_shader.sort_key(), b.shader.sort_key())
            });
        }

        let mut current_offset = 0;
        for batch in &mut self.batches {
            batch.base_instance_offset = current_offset;
            current_offset += batch.instances.len() as u32;
        }

        let mut flat_data = Vec::with_capacity(total_instances);
        let mut big_indices: Vec<u32> = Vec::new();
        let mut max_small_radius = 0.1;
        let threshold = 2.5;
        let mut current_idx = 0;

        for batch in &self.batches {
            for inst in &batch.instances {
                let m = inst.model_matrix;
                let scale = f32::max(
                    f32::max(
                        (m[0][0].powi(2) + m[0][1].powi(2) + m[0][2].powi(2))
                            .sqrt(),
                        (m[1][0].powi(2) + m[1][1].powi(2) + m[1][2].powi(2))
                            .sqrt(),
                    ),
                    (m[2][0].powi(2) + m[2][1].powi(2) + m[2][2].powi(2))
                        .sqrt(),
                );

                // Subdivided sphere primitives have unit radius, while box extents
                // are half their model scale. Keep the broad phase consistent with
                // the collision shaders' convention.
                let radius = if inst.physics.collision_type
                    == crate::core::collisions::CollisionType::Sphere
                {
                    scale
                } else {
                    scale * 0.5
                };

                if radius > threshold {
                    big_indices.push(current_idx as u32);
                } else {
                    if radius > max_small_radius {
                        max_small_radius = radius;
                    }
                }

                flat_data.push(InstanceData {
                    model: inst.model_matrix,
                    color: [
                        inst.color[0],
                        inst.color[1],
                        inst.color[2],
                        inst.emissive,
                    ],
                    mat_props: [inst.roughness, inst.metalness, 0.0, 0.0],
                    velocity: [
                        inst.physics.linear_velocity[0],
                        inst.physics.linear_velocity[1],
                        inst.physics.linear_velocity[2],
                        inst.physics.bounciness,
                    ],
                    angular_velocity: [
                        inst.physics.angular_velocity[0],
                        inst.physics.angular_velocity[1],
                        inst.physics.angular_velocity[2],
                        inst.physics.friction,
                    ],
                    physic_props: [
                        inst.physics.collision_type.sort_key(),
                        inst.physics.mass,
                        inst.physics.gravity_scale,
                        0.0,
                    ],
                });
                current_idx += 1;
            }
        }

        self.max_object_radius = max_small_radius;
        self.num_big_objects = big_indices.len() as u32;

        let staging = Buffer::from_iter(
            allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            flat_data,
        )
        .unwrap();

        let cmd_allocator = Arc::new(StandardCommandBufferAllocator::new(
            queue.device().clone(),
            Default::default(),
        ));
        let mut builder = AutoCommandBufferBuilder::primary(
            cmd_allocator,
            queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();

        let copy_count = self.total_instances as u64;
        builder
            .copy_buffer(CopyBufferInfoTyped::buffers(
                staging.clone(),
                self.physics_read.clone().slice(0..copy_count),
            ))
            .unwrap();
        builder
            .copy_buffer(CopyBufferInfoTyped::buffers(
                staging.clone(),
                self.physics_write.clone().slice(0..copy_count),
            ))
            .unwrap();

        let mut indirect_data: Vec<DrawIndexedIndirectCommand> = Vec::new();
        for batch in &self.batches {
            indirect_data.push(DrawIndexedIndirectCommand {
                index_count: batch.mesh.index_count,
                instance_count: batch.instances.len() as u32,
                first_index: 0,
                vertex_offset: 0,
                first_instance: batch.base_instance_offset,
            });
        }

        let _indirect_staging = Buffer::from_iter(
            allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            indirect_data,
        )
        .unwrap();

        for batch in &self.batches {
            let cmd = DrawIndexedIndirectCommand {
                index_count: batch.mesh.index_count,
                instance_count: batch.instances.len() as u32,
                first_index: 0,
                vertex_offset: 0,
                first_instance: batch.base_instance_offset,
            };
            let single_staging = Buffer::from_iter(
                allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_SRC,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                std::iter::once(cmd),
            )
            .unwrap();
            builder
                .copy_buffer(CopyBufferInfoTyped::buffers(
                    single_staging,
                    batch.indirect_buffer.clone(),
                ))
                .unwrap();
        }

        if !big_indices.is_empty() {
            let big_staging = Buffer::from_iter(
                allocator.clone(),
                BufferCreateInfo {
                    usage: BufferUsage::TRANSFER_SRC,
                    ..Default::default()
                },
                AllocationCreateInfo {
                    memory_type_filter: MemoryTypeFilter::PREFER_HOST
                        | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                    ..Default::default()
                },
                big_indices,
            )
            .unwrap();
            builder
                .copy_buffer(CopyBufferInfoTyped::buffers(
                    big_staging,
                    self.big_objects_indices
                        .clone()
                        .slice(0..self.num_big_objects as u64),
                ))
                .unwrap();
        }

        builder
            .build()
            .unwrap()
            .execute(queue.clone())
            .unwrap()
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();

        if let Some(shader_type) = override_shader {
            if shader_type == ComputeShaderType::Static {
                Vec::new()
            } else {
                vec![ComputeDispatchInfo {
                    compute_shader: shader_type,
                    offset: 0,
                    count: self.total_instances,
                }]
            }
        } else {
            let mut dispatches = Vec::new();
            for batch in &self.batches {
                let count = batch.instances.len() as u32;
                if count > 0
                    && batch.compute_shader != ComputeShaderType::Static
                {
                    dispatches.push(ComputeDispatchInfo {
                        compute_shader: batch.compute_shader,
                        offset: batch.base_instance_offset,
                        count,
                    });
                }
            }
            dispatches
        }
    }

    /// Ensures descriptor sets are created and cached for all textures.
    ///
    /// Descriptor sets bind uniform buffers, textures, and physics buffers
    /// to graphics pipeline slots. Creates read (physics_read) and write
    /// (physics_write) versions for each texture to support ping-pong.
    ///
    /// # Arguments
    /// * `pipeline` - The graphics pipeline to create descriptor sets for
    /// * `target_tex_count` - Number of textures to create sets for
    pub fn ensure_descriptor_cache(
        &mut self,
        pipeline: &Arc<GraphicsPipeline>,
        target_tex_count: usize,
    ) {
        let layout = pipeline.layout().set_layouts()[0].clone();

        for (frame_i, frame) in self.frames.iter().enumerate() {
            let total_sets_needed = target_tex_count * 2;

            if self.descriptor_sets[frame_i].len() == total_sets_needed {
                continue;
            }

            self.descriptor_sets[frame_i].clear();

            for tex_idx in 0..target_tex_count {
                // Buffer read
                let set_a = DescriptorSet::new(
                    self.descriptor_set_allocator.clone(),
                    layout.clone(),
                    [
                        WriteDescriptorSet::buffer(
                            0,
                            frame.uniform_buffer.clone(),
                        ),
                        WriteDescriptorSet::image_view_sampler(
                            1,
                            self.texture_views[tex_idx].clone(),
                            self.texture_sampler.clone(),
                        ),
                        WriteDescriptorSet::buffer(
                            2,
                            self.physics_read.clone(),
                        ),
                        WriteDescriptorSet::buffer(
                            3,
                            self.visible_indices.clone(),
                        ),
                    ],
                    [],
                )
                .unwrap();

                // Buffer write
                let set_b = DescriptorSet::new(
                    self.descriptor_set_allocator.clone(),
                    layout.clone(),
                    [
                        WriteDescriptorSet::buffer(
                            0,
                            frame.uniform_buffer.clone(),
                        ),
                        WriteDescriptorSet::image_view_sampler(
                            1,
                            self.texture_views[tex_idx].clone(),
                            self.texture_sampler.clone(),
                        ),
                        WriteDescriptorSet::buffer(
                            2,
                            self.physics_write.clone(),
                        ),
                        WriteDescriptorSet::buffer(
                            3,
                            self.visible_indices.clone(),
                        ),
                    ],
                    [],
                )
                .unwrap();

                self.descriptor_sets[frame_i].push(set_a);
                self.descriptor_sets[frame_i].push(set_b);
            }
        }
    }

    /// Draw all batches using a single pipeline (backward compatible).
    /// Used by main.rs which creates its own pipeline directly.
    pub fn record_draws(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        pipeline: &Arc<GraphicsPipeline>,
        frame_index: usize,
        physics_buffer_index: usize,
    ) {
        let mut current_offset = 0;

        builder.bind_pipeline_graphics(pipeline.clone()).unwrap();

        for batch in &self.batches {
            let count = batch.instances.len() as u32;
            if count == 0 {
                continue;
            }

            builder
                .bind_vertex_buffers(0, (batch.mesh.vertices.clone(),))
                .unwrap();

            let requested_tex = batch.base_color_texture.unwrap_or(0);
            let descriptor_idx = (requested_tex * 2) + physics_buffer_index;

            builder
                .bind_descriptor_sets(
                    PipelineBindPoint::Graphics,
                    pipeline.layout().clone(),
                    0,
                    self.descriptor_sets[frame_index][descriptor_idx].clone(),
                )
                .unwrap();

            if let Some(indices) = &batch.mesh.indices {
                builder.bind_index_buffer(indices.clone()).unwrap();
                unsafe {
                    builder.draw_indexed(
                        batch.mesh.index_count,
                        count,
                        0,
                        0,
                        current_offset,
                    )
                }
                .unwrap();
            } else {
                unsafe {
                    builder.draw(
                        batch.mesh.vertex_count,
                        count,
                        0,
                        current_offset,
                    )
                }
                .unwrap();
            }

            current_offset += count;
        }
    }

    /// Draw all batches using the multi-shader registry.
    /// Sorts batches by shader type to minimize pipeline switches.
    pub fn record_draws_multi(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        registry: &ShaderRegistry,
        frame_index: usize,
        physics_buffer_index: usize,
        use_culling: bool,
    ) {
        let mut last_shader: Option<ShaderType> = None;

        for batch in &self.batches {
            if batch.instances.is_empty() {
                continue;
            }

            let effective_shader = registry.resolve_shader(batch.shader);
            let pipeline = registry.get_pipeline(effective_shader);

            if last_shader != Some(effective_shader) {
                builder.bind_pipeline_graphics(pipeline.clone()).unwrap();
                last_shader = Some(effective_shader);
            }

            let effective_culling = use_culling && batch.mesh.indices.is_some();

            builder
                .push_constants(
                    pipeline.layout().clone(),
                    0,
                    MeshPushConstants {
                        visible_list_offset: batch.base_instance_offset,
                        use_culling: if effective_culling { 1 } else { 0 },
                    },
                )
                .unwrap();

            builder
                .bind_vertex_buffers(0, (batch.mesh.vertices.clone(),))
                .unwrap();

            let requested_tex = batch.base_color_texture.unwrap_or(0);
            let descriptor_idx = (requested_tex * 2) + physics_buffer_index;

            builder
                .bind_descriptor_sets(
                    PipelineBindPoint::Graphics,
                    pipeline.layout().clone(),
                    0,
                    self.descriptor_sets[frame_index][descriptor_idx].clone(),
                )
                .unwrap();

            if let Some(indices) = &batch.mesh.indices {
                builder.bind_index_buffer(indices.clone()).unwrap();

                if effective_culling {
                    unsafe {
                        builder.draw_indexed_indirect(
                            batch.indirect_buffer.clone(),
                        )
                    }
                    .unwrap();
                } else {
                    unsafe {
                        builder.draw_indexed(
                            batch.mesh.index_count,
                            batch.instances.len() as u32,
                            0,
                            0,
                            batch.base_instance_offset,
                        )
                    }
                    .unwrap();
                }
            } else {
                unsafe {
                    builder.draw(
                        batch.mesh.vertex_count,
                        batch.instances.len() as u32,
                        0,
                        batch.base_instance_offset,
                    )
                }
                .unwrap();
            }
        }
    }

    /// Updates the uniform buffer with camera and light data for a specific frame.
    ///
    /// Call this each frame before rendering to update the view/projection
    /// matrices and camera position in the GPU uniform buffer.
    ///
    /// # Arguments
    /// * `frame_index` - Which frame in flight to update
    /// * `view` - View matrix (camera transformation)
    /// * `proj` - Projection matrix (perspective transformation)
    /// * `eye_pos` - Camera position in world space
    pub fn prepare_frame_ubo(
        &mut self,
        frame_index: usize,
        view: [[f32; 4]; 4],
        proj: [[f32; 4]; 4],
        eye_pos: [f32; 3],
    ) {
        let mut ubo = self.frames[frame_index].uniform_buffer.write().unwrap();
        ubo.view = view;
        ubo.proj = proj;
        ubo.eye_pos = eye_pos;
        ubo.light_pos = self.light_pos;
        ubo.light_color = self.light_color;
        ubo.light_intensity = self.light_intensity;
    }

    /// Creates a GPU texture from raw RGBA pixel data.
    ///
    /// This creates an immutable image and uploads it to GPU memory.
    /// The image is uploaded via a one-time command buffer.
    ///
    /// # Arguments
    /// * `memory_allocator` - For creating the image
    /// * `queue` - Queue to execute the upload command
    /// * `pixels_rgba` - Raw RGBA pixel data (4 bytes per pixel)
    /// * `width` - Image width in pixels
    /// * `height` - Image height in pixels
    ///
    /// # Returns
    /// The created immutable image wrapped in Arc
    fn create_texture_image(
        memory_allocator: &Arc<StandardMemoryAllocator>,
        queue: &Arc<Queue>,
        pixels_rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Arc<Image> {
        let cb_allocator = Arc::new(StandardCommandBufferAllocator::new(
            queue.device().clone(),
            StandardCommandBufferAllocatorCreateInfo::default(),
        ));
        let mut upload_builder = AutoCommandBufferBuilder::primary(
            cb_allocator,
            queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .unwrap();
        let staging = Buffer::from_iter(
            memory_allocator.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            pixels_rgba.iter().copied(),
        )
        .unwrap();
        let image = Image::new(
            memory_allocator.clone(),
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
        .unwrap();
        upload_builder
            .copy_buffer_to_image(CopyBufferToImageInfo::buffer_image(
                staging,
                image.clone(),
            ))
            .unwrap();
        let upload_cmd = upload_builder.build().unwrap();
        vulkano::sync::now(queue.device().clone())
            .then_execute(queue.clone(), upload_cmd)
            .unwrap()
            .then_signal_fence_and_flush()
            .unwrap()
            .wait(None)
            .unwrap();
        image
    }

    /// Converts a Texture to RGBA8 format.
    ///
    /// Handles both RGBA (4 bytes/pixel) and RGB (3 bytes/pixel) input.
    /// If the format is neither, returns a white texture.
    ///
    /// # Arguments
    /// * `tex` - Input texture in any supported format
    ///
    /// # Returns
    /// Vector of RGBA8 bytes (4 bytes per pixel)
    fn to_rgba8(tex: &Texture) -> Vec<u8> {
        match tex.pixels.len() as u32 {
            len if len == tex.width * tex.height * 4 => tex.pixels.clone(),
            len if len == tex.width * tex.height * 3 => {
                let mut out =
                    Vec::with_capacity((tex.width * tex.height * 4) as usize);
                for rgb in tex.pixels.chunks_exact(3) {
                    out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
                out
            }
            _ => vec![255, 255, 255, 255],
        }
    }

    /// Adds custom textures to the scene.
    ///
    /// Each texture is uploaded to GPU memory and an image view is created.
    /// Also rebuilds the descriptor cache to include the new textures.
    ///
    /// # Arguments
    /// * `pipeline` - Graphics pipeline for descriptor set creation
    /// * `textures` - Array of textures to add
    /// * `queue` - Queue for upload commands
    /// * `memory_allocator` - For creating GPU resources
    pub fn set_textures(
        &mut self,
        pipeline: &Arc<GraphicsPipeline>,
        textures: &[Texture],
        queue: &Arc<Queue>,
        memory_allocator: &Arc<StandardMemoryAllocator>,
    ) {
        for tex in textures {
            if tex.width == 0 || tex.height == 0 {
                continue;
            }
            let pixels_rgba = Self::to_rgba8(tex);
            let image = Self::create_texture_image(
                memory_allocator,
                queue,
                &pixels_rgba,
                tex.width,
                tex.height,
            );
            let view = ImageView::new_default(image).unwrap();
            self.texture_views.push(view);
        }
        self.ensure_descriptor_cache(pipeline, self.texture_views.len());
    }

    /// Sets the main light source parameters.
    ///
    /// # Arguments
    /// * `position` - Light position in world space (XYZ)
    /// * `color` - Light color as RGB values
    /// * `intensity` - Light brightness multiplier
    pub fn set_light(
        &mut self,
        position: [f32; 3],
        color: [f32; 3],
        intensity: f32,
    ) {
        self.light_pos = position;
        self.light_color = color;
        self.light_intensity = intensity;
    }
}

/// Records a single compute shader dispatch for physics simulation.
///
/// This is the simple version that dispatches one compute shader type
/// to process all instances. Used when all objects share the same physics behavior.
///
/// # Arguments
/// * `builder` - Command buffer to record the dispatch to
/// * `compute_pipeline` - The compute pipeline to use
/// * `compute_set` - Descriptor set with physics buffers
/// * `max_instances` - Maximum instances the buffer can hold
/// * `dt` - Delta time since last frame (seconds)
/// * `total_objects` - Number of active objects
/// * `num_big_objects` - Count of large objects requiring special handling
pub fn record_compute_physics(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    compute_pipeline: &Arc<vulkano::pipeline::ComputePipeline>,
    compute_set: &Arc<DescriptorSet>,
    max_instances: u32,
    dt: f32,
    total_objects: u32,
    num_big_objects: u32,
) {
    let workgroups_x = max_instances.div_ceil(256);
    if workgroups_x == 0 {
        return;
    }
    builder
        .bind_pipeline_compute(compute_pipeline.clone())
        .unwrap()
        .bind_descriptor_sets(
            vulkano::pipeline::PipelineBindPoint::Compute,
            compute_pipeline.layout().clone(),
            0,
            compute_set.clone(),
        )
        .unwrap()
        .push_constants(
            compute_pipeline.layout().clone(),
            0,
            crate::scene::object::PhysicsPushConstants {
                dt,
                total_objects,
                offset: 0,
                count: max_instances,
                num_big_objects,
                _pad: [0, 0, 0],
                global_gravity: [0.0, -9.81, 0.0, 2.0],
            },
        )
        .unwrap();
    unsafe { builder.dispatch([workgroups_x, 1, 1]) }.unwrap();
}

#[allow(clippy::too_many_arguments)]
pub fn record_compute_physics_multi(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    registry: &crate::rendering::compute_registry::ComputeShaderRegistry,
    compute_sets: &HashMap<
        ComputeShaderType,
        (Arc<DescriptorSet>, Arc<DescriptorSet>),
    >,
    grid_build_sets: &(Arc<DescriptorSet>, Arc<DescriptorSet>),
    grid_counts: &Subbuffer<[u32]>,
    dispatches: &[ComputeDispatchInfo],
    dt: f32,
    total_objects: u32,
    cell_size: f32,
    num_big_objects: u32,
    ping_pong: bool,
) {
    builder.fill_buffer(grid_counts.clone(), 0u32).unwrap();
    let build_pipeline = registry.get_pipeline(ComputeShaderType::GridBuild);
    let grid_set = if ping_pong {
        &grid_build_sets.1
    } else {
        &grid_build_sets.0
    };
    builder
        .bind_pipeline_compute(build_pipeline.clone())
        .unwrap()
        .bind_descriptor_sets(
            PipelineBindPoint::Compute,
            build_pipeline.layout().clone(),
            0,
            grid_set.clone(),
        )
        .unwrap()
        .push_constants(
            build_pipeline.layout().clone(),
            0,
            PhysicsPushConstants {
                dt,
                total_objects,
                offset: 0,
                count: total_objects,
                num_big_objects,
                _pad: [0, 0, 0],
                global_gravity: [0.0, -9.81, 0.0, cell_size], // w = CELL_SIZE (must be >= 2 * max_object_radius)
            },
        )
        .unwrap();
    unsafe { builder.dispatch([total_objects.div_ceil(256), 1, 1]) }.unwrap();

    let mut last_bound = None;

    for dispatch in dispatches {
        let shader_to_use = dispatch.compute_shader;
        let compute_pipeline = registry.get_pipeline(shader_to_use);

        if shader_to_use == ComputeShaderType::GridBuild {
            continue;
        } else {
            let (set_0, set_1) = compute_sets.get(&shader_to_use).unwrap();
            let compute_set = if ping_pong { set_1 } else { set_0 };
            if last_bound != Some(shader_to_use) {
                builder
                    .bind_pipeline_compute(compute_pipeline.clone())
                    .unwrap();
                builder
                    .bind_descriptor_sets(
                        PipelineBindPoint::Compute,
                        compute_pipeline.layout().clone(),
                        0,
                        compute_set.clone(),
                    )
                    .unwrap();
                last_bound = Some(shader_to_use);
            }
            let workgroups_x = dispatch.count.div_ceil(256);
            if workgroups_x > 0 {
                builder
                    .push_constants(
                        compute_pipeline.layout().clone(),
                        0,
                        PhysicsPushConstants {
                            dt,
                            total_objects,
                            offset: dispatch.offset,
                            count: dispatch.count,
                            num_big_objects,
                            _pad: [0, 0, 0],
                            global_gravity: physics_force_vector(
                                shader_to_use,
                                cell_size,
                            ),
                        },
                    )
                    .unwrap();
                unsafe { builder.dispatch([workgroups_x, 1, 1]) }.unwrap();
            }
        }
    }
}
pub fn begin_render_pass_only(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    framebuffers: &[Arc<Framebuffer>],
    img_index: u32,
    dims: [u32; 2],
    pipeline: &Arc<GraphicsPipeline>,
) {
    builder
        .begin_render_pass(
            RenderPassBeginInfo {
                clear_values: vec![
                    Some([0.01, 0.01, 0.02, 1.0].into()), // Dark blue clear
                    Some(1.0.into()),
                ],
                ..RenderPassBeginInfo::framebuffer(
                    framebuffers[img_index as usize].clone(),
                )
            },
            SubpassBeginInfo {
                contents: SubpassContents::Inline,
                ..Default::default()
            },
        )
        .unwrap()
        .set_viewport(
            0,
            vec![Viewport {
                offset: [0.0, 0.0],
                extent: [dims[0] as f32, dims[1] as f32],
                depth_range: 0.0..=1.0,
            }]
            .into(),
        )
        .unwrap()
        .bind_pipeline_graphics(pipeline.clone())
        .unwrap();
}

#[allow(clippy::too_many_arguments)]
pub fn record_compute_physics_spatial(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    registry: &ComputeShaderRegistry,
    compute_sets: &HashMap<
        ComputeShaderType,
        (Arc<DescriptorSet>, Arc<DescriptorSet>),
    >,
    grid_build_sets: &(Arc<DescriptorSet>, Arc<DescriptorSet>),
    grid_counts: &Subbuffer<[u32]>,
    dispatches: &[ComputeDispatchInfo],
    dt: f32,
    total_objects: u32,
    cell_size: f32,
    num_big_objects: u32,
    ping_pong: bool,
) {
    let build_pipeline = registry.get_pipeline(ComputeShaderType::GridBuild);

    let mut read_index: usize = 0;

    for dispatch in dispatches {
        builder.fill_buffer(grid_counts.clone(), 0u32).unwrap();

        let grid_set = if read_index == 0 {
            &grid_build_sets.0
        } else {
            &grid_build_sets.1
        };

        builder
            .bind_pipeline_compute(build_pipeline.clone())
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                build_pipeline.layout().clone(),
                0,
                grid_set.clone(),
            )
            .unwrap()
            .push_constants(
                build_pipeline.layout().clone(),
                0,
                PhysicsPushConstants {
                    dt,
                    total_objects,
                    offset: 0,
                    count: total_objects,
                    num_big_objects,
                    _pad: [0, 0, 0],
                    global_gravity: [0.0, -9.81, 0.0, cell_size],
                },
            )
            .unwrap();
        unsafe { builder.dispatch([total_objects.div_ceil(256), 1, 1]) }
            .unwrap();

        let compute_pipeline = registry.get_pipeline(dispatch.compute_shader);
        let (set_0, set_1) =
            compute_sets.get(&dispatch.compute_shader).unwrap();
        let compute_set = if ping_pong { set_1 } else { set_0 };

        builder
            .bind_pipeline_compute(compute_pipeline.clone())
            .unwrap()
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                compute_pipeline.layout().clone(),
                0,
                compute_set.clone(),
            )
            .unwrap()
            .push_constants(
                compute_pipeline.layout().clone(),
                0,
                PhysicsPushConstants {
                    dt,
                    total_objects,
                    offset: dispatch.offset,
                    count: dispatch.count,
                    num_big_objects,
                    _pad: [0, 0, 0],
                    global_gravity: physics_force_vector(
                        dispatch.compute_shader,
                        2.0,
                    ),
                },
            )
            .unwrap();
        unsafe { builder.dispatch([dispatch.count.div_ceil(256), 1, 1]) }
            .unwrap();

        read_index ^= 1;
    }
}
