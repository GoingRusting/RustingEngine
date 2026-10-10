//! Runs `cloth.comp` on a queue and returns finished cloth state without
//! waiting, like the rigid GPU physics readback in [`super::scene_renderer`]:
//! each submission signals a fence, and [`GpuClothRunner::take_completed`]
//! returns only those whose fence has signaled, stamped with the body's
//! [`PhysicsId`] and the fixed tick it reached. Packing and the CPU
//! reference live in [`crate::runtime::gpu_cloth`].

use std::sync::Arc;
use std::time::Duration;

use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{AutoCommandBufferBuilder, CommandBufferUsage};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{DescriptorSet, WriteDescriptorSet};
use vulkano::device::{Device, Queue};
use vulkano::memory::allocator::{
    AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator,
};
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
use vulkano::pipeline::{
    ComputePipeline, Pipeline, PipelineBindPoint, PipelineLayout,
    PipelineShaderStageCreateInfo,
};
use vulkano::sync::future::FenceSignalFuture;
use vulkano::sync::GpuFuture;

use bevy_ecs::prelude::{Entity, World};

use crate::runtime::gpu_cloth::{
    floor_contact, pack, pack_soft_body, unpack, unpack_soft_body, GpuCloth,
};
use crate::runtime::{
    route_gpu_physics_events, ClothVolume, FrameTime, GpuEventPayload,
    PhysicsId, PhysicsIdRegistry, RawGpuPhysicsEvent, SoftBodyVolume,
};

mod shader {
    vulkano_shaders::shader! {
        ty: "compute",
        include: ["src/shaders"],
        path: "src/shaders/compute/cloth.comp",
    }
}

/// Cloth words a finished submission produced; read them with
/// [`crate::runtime::gpu_cloth::unpack`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuClothState {
    pub physics_id: PhysicsId,
    /// Fixed tick the cloth reached in this submission.
    pub tick: u64,
    pub words: Vec<u32>,
}

struct Pending {
    fence: FenceSignalFuture<Box<dyn GpuFuture>>,
    buffer: Subbuffer<[u32]>,
    physics_id: PhysicsId,
    tick: u64,
}

pub struct GpuClothRunner {
    device: Arc<Device>,
    queue: Arc<Queue>,
    pipeline: Arc<ComputePipeline>,
    memory: Arc<StandardMemoryAllocator>,
    commands: Arc<StandardCommandBufferAllocator>,
    descriptors: Arc<StandardDescriptorSetAllocator>,
    pending: Vec<Pending>,
}

impl GpuClothRunner {
    pub fn new(device: Arc<Device>, queue: Arc<Queue>) -> Result<Self, String> {
        let error = |error: &dyn std::fmt::Display| error.to_string();
        let entry_point = shader::load(device.clone())
            .map_err(|e| error(&e))?
            .entry_point("main")
            .ok_or("cloth.comp has no main")?;
        let stage = PipelineShaderStageCreateInfo::new(entry_point);
        let layout = PipelineLayout::new(
            device.clone(),
            PipelineDescriptorSetLayoutCreateInfo::from_stages([&stage])
                .into_pipeline_layout_create_info(device.clone())
                .map_err(|e| format!("{e:?}"))?,
        )
        .map_err(|e| error(&e))?;
        let pipeline = ComputePipeline::new(
            device.clone(),
            None,
            ComputePipelineCreateInfo::stage_layout(stage, layout),
        )
        .map_err(|e| error(&e))?;
        Ok(Self {
            memory: Arc::new(StandardMemoryAllocator::new_default(
                device.clone(),
            )),
            commands: Arc::new(StandardCommandBufferAllocator::new(
                device.clone(),
                Default::default(),
            )),
            descriptors: Arc::new(StandardDescriptorSetAllocator::new(
                device.clone(),
                Default::default(),
            )),
            device,
            queue,
            pipeline,
            pending: Vec::new(),
        })
    }

    /// Submits `words` from [`crate::runtime::gpu_cloth::pack`]; they reach
    /// `tick` when done. Returns at once.
    pub fn submit(
        &mut self,
        physics_id: PhysicsId,
        tick: u64,
        words: Vec<u32>,
    ) -> Result<(), String> {
        let error = |error: &dyn std::fmt::Display| error.to_string();
        // ponytail: one host-visible buffer per submission; keep a
        // device-local buffer per cloth and copy only positions back if the
        // traffic shows up in profiles.
        let buffer = Buffer::from_iter(
            self.memory.clone(),
            BufferCreateInfo {
                usage: BufferUsage::STORAGE_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_RANDOM_ACCESS,
                ..Default::default()
            },
            words,
        )
        .map_err(|e| format!("{e:?}"))?;
        let set = DescriptorSet::new(
            self.descriptors.clone(),
            self.pipeline.layout().set_layouts()[0].clone(),
            [WriteDescriptorSet::buffer(0, buffer.clone())],
            [],
        )
        .map_err(|e| format!("{e:?}"))?;
        let mut builder = AutoCommandBufferBuilder::primary(
            self.commands.clone(),
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .map_err(|e| format!("{e:?}"))?;
        builder
            .bind_pipeline_compute(self.pipeline.clone())
            .map_err(|e| format!("{e:?}"))?
            .bind_descriptor_sets(
                PipelineBindPoint::Compute,
                self.pipeline.layout().clone(),
                0,
                set,
            )
            .map_err(|e| format!("{e:?}"))?;
        // The kernel steps every particle from one workgroup.
        unsafe { builder.dispatch([1, 1, 1]) }.map_err(|e| format!("{e:?}"))?;
        let command_buffer = builder.build().map_err(|e| format!("{e:?}"))?;
        let fence = vulkano::sync::now(self.device.clone())
            .then_execute(self.queue.clone(), command_buffer)
            .map_err(|e| error(&e))?
            .boxed()
            .then_signal_fence_and_flush()
            .map_err(|e| format!("{e:?}"))?;
        self.pending.push(Pending {
            fence,
            buffer,
            physics_id,
            tick,
        });
        Ok(())
    }

    /// Submissions not yet returned by [`Self::take_completed`].
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// State of every finished submission, in submission order. Never
    /// waits for unfinished work.
    pub fn take_completed(&mut self) -> Vec<GpuClothState> {
        let mut done = Vec::new();
        let mut index = 0;
        while index < self.pending.len() {
            if !self.pending[index].fence.is_signaled().unwrap_or(false) {
                index += 1;
                continue;
            }
            let pending = self.pending.remove(index);
            // The zero timeout releases the buffer's GPU lock; without it
            // the read below fails.
            if pending.fence.wait(Some(Duration::ZERO)).is_err() {
                continue;
            }
            // A failed read (lost device) drops the submission.
            if let Ok(words) = pending.buffer.read() {
                done.push(GpuClothState {
                    physics_id: pending.physics_id,
                    tick: pending.tick,
                    words: words.to_vec(),
                });
            };
        }
        done
    }

    /// Blocks until every submission has finished. **Slow:** for tests and
    /// tools only, never the per-frame path.
    pub fn block_until_complete(&mut self) {
        for pending in &self.pending {
            let _ = pending.fence.wait(None);
        }
    }
}

/// Writes finished states into their [`ClothVolume`]s or
/// [`SoftBodyVolume`]s and sends each
/// cloth's [`GpuCloth::floor_event`] through [`route_gpu_physics_events`]
/// when a free particle first reaches the floor. States of despawned cloths
/// are dropped.
pub fn apply_gpu_cloth_states(world: &mut World, states: &[GpuClothState]) {
    let mut events = Vec::new();
    for state in states {
        let entity = world
            .resource::<PhysicsIdRegistry>()
            .resolve(state.physics_id);
        let Some(mut entity) =
            entity.and_then(|entity| world.get_entity_mut(entity).ok())
        else {
            continue;
        };
        let Some(mut gpu) = entity.get_mut::<GpuCloth>() else {
            continue;
        };
        let contact = floor_contact(&state.words);
        if let (Some(event), Some((index, [x, y, z])), false) =
            (gpu.floor_event, contact, gpu.on_floor)
        {
            events.push(RawGpuPhysicsEvent {
                body_slot: state.physics_id.slot,
                body_generation: state.physics_id.generation,
                event_id: event.0,
                tick_low: state.tick as u32,
                tick_high: (state.tick >> 32) as u32,
                payload_kind: GpuEventPayload::Contact as u32,
                payload: [x, y, z, index as f32],
                ..Default::default()
            });
        }
        gpu.in_flight = false;
        gpu.tick = Some(state.tick);
        gpu.on_floor = contact.is_some();
        if let Some(mut volume) = entity.get_mut::<ClothVolume>() {
            unpack(&state.words, &mut volume.cloth);
        } else if let Some(mut volume) = entity.get_mut::<SoftBodyVolume>() {
            unpack_soft_body(&state.words, &mut volume.body);
        }
    }
    if !events.is_empty() {
        route_gpu_physics_events(world, &events);
    }
}

/// Applies finished GPU work with [`apply_gpu_cloth_states`], then submits
/// every [`GpuCloth`] cloth or soft body with nothing on the GPU from the tick its particles
/// show up to the current fixed tick. Each cloth has at most one submission
/// in flight, so the volume never runs behind more than the GPU latency,
/// and a run of N steps gives the same bits as N runs of one step. Call it
/// once per frame, with completed rigid readback. Returns how many cloths
/// it submitted.
// ponytail: a despawned cloth keeps its PhysicsId slot; release it on
// removal if scenes churn many GPU cloths.
pub fn service_gpu_cloths(
    world: &mut World,
    runner: &mut GpuClothRunner,
) -> Result<usize, String> {
    world.init_resource::<PhysicsIdRegistry>();
    let completed = runner.take_completed();
    apply_gpu_cloth_states(world, &completed);

    let time = world.resource::<FrameTime>();
    let (tick, dt) = (time.fixed_tick, time.fixed_delta.as_secs_f32());
    let mut ready: Vec<(Entity, Option<PhysicsId>)> = world
        .query::<(Entity, &GpuCloth, Option<&PhysicsId>)>()
        .iter(world)
        .filter(|(_, gpu, _)| !gpu.in_flight)
        .map(|(entity, _, id)| (entity, id.copied()))
        .collect();
    ready.sort_unstable_by_key(|&(entity, _)| entity);
    let mut submitted = 0;
    for (entity, id) in ready {
        let id = match id {
            Some(id) => id,
            None => {
                let id =
                    world.resource_mut::<PhysicsIdRegistry>().assign(entity);
                world.entity_mut(entity).insert(id);
                id
            }
        };
        let mut entity = world.entity_mut(entity);
        let from = entity.get::<GpuCloth>().unwrap().tick.unwrap_or(tick);
        let steps = tick.saturating_sub(from);
        if steps == 0 {
            entity.get_mut::<GpuCloth>().unwrap().tick = Some(tick);
            continue;
        }
        let words = if let Some(volume) = entity.get::<ClothVolume>() {
            pack(&volume.cloth, &volume.settings, dt, steps as u32)?
        } else if let Some(volume) = entity.get::<SoftBodyVolume>() {
            pack_soft_body(&volume.body, &volume.settings, dt, steps as u32)?
        } else {
            continue;
        };
        runner.submit(id, tick, words)?;
        entity.get_mut::<GpuCloth>().unwrap().in_flight = true;
        submitted += 1;
    }
    Ok(submitted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::gpu_cloth::{pack, step_on_cpu, unpack};
    use crate::runtime::{Cloth, ClothSettings};

    fn hanging() -> (Cloth, ClothSettings) {
        let mut cloth = Cloth::grid(
            [0.0, 2.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
            [8, 8],
            0.125,
            0.3,
        )
        .unwrap();
        for i in 0..9 {
            cloth.pin(i);
        }
        let settings = ClothSettings {
            substeps: 8,
            floor: Some(1.5),
            ..ClothSettings::default()
        };
        (cloth, settings)
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn submissions_return_cpu_reference_bits_tagged_with_body_and_tick() {
        use crate::rendering::test_support::headless_device;

        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let base = headless_device();
        let mut runner =
            GpuClothRunner::new(base.device.clone(), base.queue.clone())
                .unwrap();
        let (mut cloth, settings) = hanging();
        let body = PhysicsId {
            slot: 3,
            generation: 1,
        };
        // Two ticks of 30 steps each, the second from the first's result,
        // as a game feeding back what it read.
        let mut expected = Vec::new();
        for tick in [30, 60] {
            let words = pack(&cloth, &settings, 1.0 / 60.0, 30).unwrap();
            let mut reference = words.clone();
            step_on_cpu(&mut reference);
            unpack(&reference, &mut cloth);
            expected.push(GpuClothState {
                physics_id: body,
                tick,
                words: reference,
            });
            runner.submit(body, tick, words).unwrap();
        }
        assert_eq!(runner.in_flight(), 2);
        runner.block_until_complete();
        assert_eq!(runner.take_completed(), expected);
        assert_eq!(runner.in_flight(), 0);
        assert!(runner.take_completed().is_empty());
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn a_soft_block_steps_on_the_gpu_with_the_cpu_reference_bits() {
        use crate::rendering::test_support::headless_device;
        use crate::runtime::gpu_cloth::pack_soft_body;
        use crate::runtime::{SoftBody, SoftBodySettings};

        if vulkano::VulkanLibrary::new().is_err() {
            eprintln!("skipping: no Vulkan driver present");
            return;
        }
        let base = headless_device();
        let mut runner =
            GpuClothRunner::new(base.device.clone(), base.queue.clone())
                .unwrap();
        let body = SoftBody::block([-0.5, 1.0, -0.5], [4, 4, 4], 0.25, 1000.0)
            .unwrap();
        let words =
            pack_soft_body(&body, &SoftBodySettings::default(), 1.0 / 60.0, 90)
                .unwrap();
        let mut reference = words.clone();
        step_on_cpu(&mut reference);
        let id = PhysicsId {
            slot: 1,
            generation: 0,
        };
        runner.submit(id, 90, words).unwrap();
        runner.block_until_complete();
        let done = runner.take_completed();
        assert_eq!(done.len(), 1);
        assert!(done[0].words == reference, "GPU and CPU bits differ");
    }

    #[test]
    fn a_cloth_reaching_the_floor_sends_its_event_once() {
        use crate::runtime::{
            App, ClothVolume, EventQueue, GpuEventRegistry, GpuPhysicsEvent,
            HybridPhysicsPlugin,
        };

        let mut app = App::new();
        app.add_plugin(HybridPhysicsPlugin).unwrap();
        // Unpinned, the sheet drops onto the floor.
        let (mut cloth, mut settings) = hanging();
        cloth.inverse_masses.fill(1.0);
        settings.floor = Some(0.0);
        let event = app
            .world_mut()
            .resource_mut::<GpuEventRegistry>()
            .register("cloth landed");
        let entity = app.spawn((
            ClothVolume {
                settings,
                cloth: cloth.clone(),
                attachments: Vec::new(),
                skin: None,
            },
            GpuCloth {
                floor_event: Some(event),
                ..GpuCloth::default()
            },
        ));
        let id = app
            .world_mut()
            .resource_mut::<PhysicsIdRegistry>()
            .assign(entity);
        let state = |tick: u64, steps: u32| {
            let mut words = pack(&cloth, &settings, 1.0 / 60.0, steps).unwrap();
            step_on_cpu(&mut words);
            GpuClothState {
                physics_id: id,
                tick,
                words,
            }
        };
        let (airborne, landed) = (state(1, 1), state(120, 120));
        for states in [vec![airborne], vec![landed.clone(), landed]] {
            apply_gpu_cloth_states(app.world_mut(), &states);
        }
        app.update(std::time::Duration::ZERO).unwrap();
        let events: Vec<GpuPhysicsEvent> = app
            .world()
            .resource::<EventQueue<GpuPhysicsEvent>>()
            .iter()
            .copied()
            .collect();
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!((events[0].entity, events[0].tick), (entity, 120));
        assert_eq!(events[0].payload_kind, GpuEventPayload::Contact as u32);
        assert_eq!(events[0].payload[1], 0.0);
        let gpu = app.world().get::<GpuCloth>(entity).unwrap();
        assert!(gpu.on_floor && !gpu.in_flight);
    }
}
