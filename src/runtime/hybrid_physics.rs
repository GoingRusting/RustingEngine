//! CPU-side types shared with the hybrid GPU physics bridge.
//!
//! This module does not create Vulkan buffers. It defines stable body IDs,
//! programmable conditions, and the exact event format used for readback.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::prelude::{Changed, Commands, Query, ResMut, Resource, World};
use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

use super::{App, AppError, EventQueue, Plugin, ScheduleStage};
use crate::Transform;

/// Maximum number of simple instructions accepted for one condition.
///
/// A fixed limit prevents one object from creating an unexpectedly expensive
/// condition program. Custom compute code remains available for larger logic.
pub const MAX_GPU_CONDITION_INSTRUCTIONS: usize = 64;

/// Stable identity shared by ECS, GPU buffers, commands, and GPU events.
///
/// `slot` locates a table entry. `generation` changes whenever that slot is
/// reused, so a delayed event cannot accidentally target a newer body.
#[repr(C)]
#[derive(
    Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable,
)]
pub struct PhysicsId {
    pub slot: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug)]
struct PhysicsSlot {
    generation: u32,
    entity: Option<Entity>,
}

/// Owns the stable mapping between ECS entities and GPU physics IDs.
#[derive(Resource, Debug, Default)]
pub struct PhysicsIdRegistry {
    slots: Vec<PhysicsSlot>,
    free_slots: Vec<u32>,
    entity_ids: HashMap<Entity, PhysicsId>,
}

impl PhysicsIdRegistry {
    /// Returns the existing ID or creates a new stable ID for an entity.
    pub fn assign(&mut self, entity: Entity) -> PhysicsId {
        if let Some(id) = self.entity_ids.get(&entity) {
            return *id;
        }

        let id = if let Some(slot) = self.free_slots.pop() {
            let entry = &mut self.slots[slot as usize];
            entry.entity = Some(entity);
            PhysicsId {
                slot,
                generation: entry.generation,
            }
        } else {
            let slot = self.slots.len() as u32;
            let generation = 1;
            self.slots.push(PhysicsSlot {
                generation,
                entity: Some(entity),
            });
            PhysicsId { slot, generation }
        };

        self.entity_ids.insert(entity, id);
        id
    }

    /// Releases an entity ID and invalidates all delayed events using it.
    pub fn release(&mut self, entity: Entity) -> Option<PhysicsId> {
        let old_id = self.entity_ids.remove(&entity)?;
        let entry = &mut self.slots[old_id.slot as usize];
        entry.entity = None;
        entry.generation = next_generation(entry.generation);
        self.free_slots.push(old_id.slot);
        Some(old_id)
    }

    /// Resolves an ID only when both its slot and generation still match.
    #[must_use]
    pub fn resolve(&self, id: PhysicsId) -> Option<Entity> {
        let entry = self.slots.get(id.slot as usize)?;
        (entry.generation == id.generation)
            .then_some(entry.entity)
            .flatten()
    }

    /// Returns the GPU physics ID currently assigned to an ECS entity.
    #[must_use]
    pub fn id_for(&self, entity: Entity) -> Option<PhysicsId> {
        self.entity_ids.get(&entity).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entity_ids.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entity_ids.is_empty()
    }
}

fn next_generation(generation: u32) -> u32 {
    let next = generation.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

/// Numeric name used by shaders when they emit a registered gameplay event.
#[repr(transparent)]
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Hash, Serialize,
)]
pub struct GpuEventId(pub u32);

/// Assigns compact IDs to event names used by Rust and custom shaders.
#[derive(Resource, Debug, Default)]
pub struct GpuEventRegistry {
    names: Vec<String>,
    ids: HashMap<String, GpuEventId>,
}

impl GpuEventRegistry {
    /// Registers a name once and returns the same ID on later calls.
    pub fn register(&mut self, name: impl Into<String>) -> GpuEventId {
        let name = name.into();
        if let Some(id) = self.ids.get(&name) {
            return *id;
        }
        // Zero means "no event" inside GPU buffers.
        let id = GpuEventId((self.names.len() as u32).saturating_add(1));
        self.names.push(name.clone());
        self.ids.insert(name, id);
        id
    }

    #[must_use]
    pub fn id(&self, name: &str) -> Option<GpuEventId> {
        self.ids.get(name).copied()
    }

    #[must_use]
    pub fn name(&self, id: GpuEventId) -> Option<&str> {
        let index = id.0.checked_sub(1)? as usize;
        self.names.get(index).map(String::as_str)
    }
}

/// Physics value that a built-in condition can read on the GPU.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum GpuStateField {
    PositionX,
    PositionY,
    PositionZ,
    VelocityX,
    VelocityY,
    VelocityZ,
    AngularVelocityX,
    AngularVelocityY,
    AngularVelocityZ,
    ScaleX,
    ScaleY,
    ScaleZ,
    Mass,
    GravityScale,
    Speed,
    Custom(u8),
}

impl GpuStateField {
    fn gpu_code(self) -> u32 {
        match self {
            Self::PositionX => 0,
            Self::PositionY => 1,
            Self::PositionZ => 2,
            Self::VelocityX => 3,
            Self::VelocityY => 4,
            Self::VelocityZ => 5,
            Self::AngularVelocityX => 6,
            Self::AngularVelocityY => 7,
            Self::AngularVelocityZ => 8,
            Self::ScaleX => 9,
            Self::ScaleY => 10,
            Self::ScaleZ => 11,
            Self::Mass => 12,
            Self::GravityScale => 13,
            Self::Speed => 14,
            Self::Custom(index) => 0x100 + u32::from(index),
        }
    }
}

/// Comparison used by one leaf in a GPU condition.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum GpuComparison {
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Equal,
    NotEqual,
}

impl GpuComparison {
    fn gpu_code(self) -> u32 {
        match self {
            Self::Less => 0,
            Self::LessOrEqual => 1,
            Self::Greater => 2,
            Self::GreaterOrEqual => 3,
            Self::Equal => 4,
            Self::NotEqual => 5,
        }
    }
}

/// One node in a condition tree authored through normal Rust code.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
enum GpuConditionNode {
    Compare {
        field: GpuStateField,
        comparison: GpuComparison,
        value: f32,
    },
    Range {
        field: GpuStateField,
        minimum: f32,
        maximum: f32,
    },
    Colliding,
    Sleeping,
    TimerElapsed(f32),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Not(Box<Self>),
}

/// A condition that can be compiled into data evaluated by a compute shader.
///
/// This deliberately stays a Rust builder instead of introducing another
/// scripting language. Truly arbitrary logic can use the custom shader ABI.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GpuCondition {
    node: GpuConditionNode,
}

impl GpuCondition {
    #[must_use]
    pub fn field(field: GpuStateField) -> GpuFieldCondition {
        GpuFieldCondition { field }
    }

    #[must_use]
    pub fn position_y() -> GpuFieldCondition {
        Self::field(GpuStateField::PositionY)
    }

    #[must_use]
    pub fn velocity_y() -> GpuFieldCondition {
        Self::field(GpuStateField::VelocityY)
    }

    #[must_use]
    pub fn custom(index: u8) -> GpuFieldCondition {
        Self::field(GpuStateField::Custom(index))
    }

    #[must_use]
    pub fn colliding() -> Self {
        Self {
            node: GpuConditionNode::Colliding,
        }
    }

    #[must_use]
    pub fn sleeping() -> Self {
        Self {
            node: GpuConditionNode::Sleeping,
        }
    }

    #[must_use]
    pub fn timer_elapsed(seconds: f32) -> Self {
        Self {
            node: GpuConditionNode::TimerElapsed(seconds.max(0.0)),
        }
    }

    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self {
            node: GpuConditionNode::And(
                Box::new(self.node),
                Box::new(other.node),
            ),
        }
    }

    #[must_use]
    pub fn or(self, other: Self) -> Self {
        Self {
            node: GpuConditionNode::Or(
                Box::new(self.node),
                Box::new(other.node),
            ),
        }
    }

    #[must_use]
    pub fn inverted(self) -> Self {
        Self {
            node: GpuConditionNode::Not(Box::new(self.node)),
        }
    }

    /// Converts the tree into postfix instructions consumed by the GPU.
    pub fn compile(
        &self,
    ) -> Result<Vec<GpuConditionInstruction>, ConditionError> {
        let mut instructions = Vec::new();
        compile_node(&self.node, &mut instructions)?;
        if instructions.len() > MAX_GPU_CONDITION_INSTRUCTIONS {
            return Err(ConditionError::TooManyInstructions {
                count: instructions.len(),
                maximum: MAX_GPU_CONDITION_INSTRUCTIONS,
            });
        }
        Ok(instructions)
    }
}

impl std::ops::Not for GpuCondition {
    type Output = Self;

    fn not(self) -> Self::Output {
        self.inverted()
    }
}

/// Starts a comparison against one GPU physics value.
#[derive(Clone, Copy, Debug)]
pub struct GpuFieldCondition {
    field: GpuStateField,
}

impl GpuFieldCondition {
    fn compare(self, comparison: GpuComparison, value: f32) -> GpuCondition {
        GpuCondition {
            node: GpuConditionNode::Compare {
                field: self.field,
                comparison,
                value,
            },
        }
    }

    #[must_use]
    pub fn less_than(self, value: f32) -> GpuCondition {
        self.compare(GpuComparison::Less, value)
    }

    #[must_use]
    pub fn less_or_equal(self, value: f32) -> GpuCondition {
        self.compare(GpuComparison::LessOrEqual, value)
    }

    #[must_use]
    pub fn greater_than(self, value: f32) -> GpuCondition {
        self.compare(GpuComparison::Greater, value)
    }

    #[must_use]
    pub fn greater_or_equal(self, value: f32) -> GpuCondition {
        self.compare(GpuComparison::GreaterOrEqual, value)
    }

    #[must_use]
    pub fn equal_to(self, value: f32) -> GpuCondition {
        self.compare(GpuComparison::Equal, value)
    }

    #[must_use]
    pub fn not_equal_to(self, value: f32) -> GpuCondition {
        self.compare(GpuComparison::NotEqual, value)
    }

    #[must_use]
    pub fn inside(self, minimum: f32, maximum: f32) -> GpuCondition {
        GpuCondition {
            node: GpuConditionNode::Range {
                field: self.field,
                minimum: minimum.min(maximum),
                maximum: minimum.max(maximum),
            },
        }
    }
}

/// Operation codes shared with the condition compute shader.
mod condition_opcode {
    pub const COMPARE: u32 = 1;
    pub const RANGE: u32 = 2;
    pub const COLLIDING: u32 = 3;
    pub const SLEEPING: u32 = 4;
    pub const TIMER_ELAPSED: u32 = 5;
    pub const AND: u32 = 16;
    pub const OR: u32 = 17;
    pub const NOT: u32 = 18;
}

/// Fixed-size instruction stored in a GPU condition buffer.
///
/// The four integer words describe the operation. The four float words hold
/// thresholds or future parameters. Keeping this at 32 bytes makes its GLSL
/// `std430` layout simple and predictable.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GpuConditionInstruction {
    pub opcode: u32,
    pub operand: u32,
    pub flags: u32,
    pub reserved: u32,
    pub values: [f32; 4],
}

fn compile_node(
    node: &GpuConditionNode,
    output: &mut Vec<GpuConditionInstruction>,
) -> Result<(), ConditionError> {
    let instruction = match node {
        GpuConditionNode::Compare {
            field,
            comparison,
            value,
        } => GpuConditionInstruction {
            opcode: condition_opcode::COMPARE,
            operand: field.gpu_code(),
            flags: comparison.gpu_code(),
            values: [*value, 0.0, 0.0, 0.0],
            ..Default::default()
        },
        GpuConditionNode::Range {
            field,
            minimum,
            maximum,
        } => GpuConditionInstruction {
            opcode: condition_opcode::RANGE,
            operand: field.gpu_code(),
            values: [*minimum, *maximum, 0.0, 0.0],
            ..Default::default()
        },
        GpuConditionNode::Colliding => GpuConditionInstruction {
            opcode: condition_opcode::COLLIDING,
            ..Default::default()
        },
        GpuConditionNode::Sleeping => GpuConditionInstruction {
            opcode: condition_opcode::SLEEPING,
            ..Default::default()
        },
        GpuConditionNode::TimerElapsed(seconds) => GpuConditionInstruction {
            opcode: condition_opcode::TIMER_ELAPSED,
            values: [*seconds, 0.0, 0.0, 0.0],
            ..Default::default()
        },
        GpuConditionNode::And(left, right) => {
            compile_node(left, output)?;
            compile_node(right, output)?;
            GpuConditionInstruction {
                opcode: condition_opcode::AND,
                ..Default::default()
            }
        }
        GpuConditionNode::Or(left, right) => {
            compile_node(left, output)?;
            compile_node(right, output)?;
            GpuConditionInstruction {
                opcode: condition_opcode::OR,
                ..Default::default()
            }
        }
        GpuConditionNode::Not(inner) => {
            compile_node(inner, output)?;
            GpuConditionInstruction {
                opcode: condition_opcode::NOT,
                ..Default::default()
            }
        }
    };
    output.push(instruction);
    if output.len() > MAX_GPU_CONDITION_INSTRUCTIONS {
        return Err(ConditionError::TooManyInstructions {
            count: output.len(),
            maximum: MAX_GPU_CONDITION_INSTRUCTIONS,
        });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionError {
    TooManyInstructions { count: usize, maximum: usize },
}

impl std::fmt::Display for ConditionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyInstructions { count, maximum } => write!(
                formatter,
                "GPU condition has {count} instructions; maximum is {maximum}"
            ),
        }
    }
}

impl std::error::Error for ConditionError {}

/// Decides when a true/false condition produces an event.
#[repr(u32)]
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize,
)]
pub enum GpuEventMode {
    #[default]
    OnEnter = 0,
    OnExit = 1,
    WhileTrue = 2,
    Once = 3,
}

/// Selects the four floats copied into an event without another readback.
#[repr(u32)]
#[derive(
    Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize,
)]
pub enum GpuEventPayload {
    #[default]
    None = 0,
    Position = 1,
    Velocity = 2,
    AngularVelocity = 3,
    Contact = 4,
    Custom = 5,
}

/// One authored rule attached to one body or a prepared body group.
#[derive(
    Component, Clone, Debug, Default, Deserialize, PartialEq, Serialize,
)]
pub struct GpuPhysicsWatch {
    pub rules: Vec<GpuPhysicsRule>,
}

/// GPU rules assigned to reusable object classes.
///
/// One rule can target 10,000 objects without also changing unrelated GPU
/// bodies. Objects may belong to more than one class.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GpuPhysicsClassWatches {
    /// Rules stored under their authored class name.
    pub classes: BTreeMap<String, Vec<GpuPhysicsRule>>,
}

impl GpuPhysicsClassWatches {
    /// Adds one rule to a class without registering it twice.
    pub fn add(&mut self, class: impl Into<String>, rule: GpuPhysicsRule) {
        let class = class.into();
        let class = class.trim();
        if class.is_empty() {
            return;
        }
        let rules = self.classes.entry(class.to_owned()).or_default();
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
}

/// Configures which event is emitted when a GPU condition changes.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GpuPhysicsRule {
    pub event: String,
    pub condition: GpuCondition,
    pub mode: GpuEventMode,
    pub payload: GpuEventPayload,
    pub cooldown_seconds: f32,
}

impl GpuPhysicsRule {
    #[must_use]
    pub fn new(event: impl Into<String>, condition: GpuCondition) -> Self {
        Self {
            event: event.into(),
            condition,
            mode: GpuEventMode::OnEnter,
            payload: GpuEventPayload::None,
            cooldown_seconds: 0.0,
        }
    }

    #[must_use]
    pub fn mode(mut self, mode: GpuEventMode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub fn payload(mut self, payload: GpuEventPayload) -> Self {
        self.payload = payload;
        self
    }

    #[must_use]
    pub fn cooldown(mut self, seconds: f32) -> Self {
        self.cooldown_seconds = seconds.max(0.0);
        self
    }
}

/// Exact event bytes copied from a GPU readback buffer.
///
/// The tick is split into two words because 64-bit shader integers are not a
/// baseline feature on low-end Vulkan devices.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RawGpuPhysicsEvent {
    pub body_slot: u32,
    pub body_generation: u32,
    pub event_id: u32,
    pub flags: u32,
    pub tick_low: u32,
    pub tick_high: u32,
    pub payload_kind: u32,
    pub reserved: u32,
    pub payload: [f32; 4],
}

impl RawGpuPhysicsEvent {
    #[must_use]
    pub fn physics_id(self) -> PhysicsId {
        PhysicsId {
            slot: self.body_slot,
            generation: self.body_generation,
        }
    }

    #[must_use]
    pub fn tick(self) -> u64 {
        u64::from(self.tick_low) | (u64::from(self.tick_high) << 32)
    }
}

/// Safe Rust event delivered after a raw GPU event resolves to a live entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuPhysicsEvent {
    pub entity: Entity,
    pub physics_id: PhysicsId,
    pub event_id: GpuEventId,
    pub tick: u64,
    pub flags: u32,
    pub payload_kind: u32,
    pub payload: [f32; 4],
}

/// Reports how many readback events were accepted or rejected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuEventRouteReport {
    pub delivered: usize,
    pub stale: usize,
    pub unknown_event: usize,
}

/// One compiled rule copied into the renderer-facing physics snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractedGpuPhysicsRule {
    pub event_id: GpuEventId,
    pub instructions: Vec<GpuConditionInstruction>,
    pub mode: GpuEventMode,
    pub payload: GpuEventPayload,
    pub cooldown_seconds: f32,
}

/// GPU-owned body data prepared without exposing Vulkan buffers to ECS.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractedGpuPhysicsBody {
    pub entity: Entity,
    pub physics_id: PhysicsId,
    pub transform: Transform,
    pub rigid_body: super::RigidBody,
    /// Solver selected for this GPU body, including the Space attractor mode.
    pub solver: super::PhysicsSolver,
    pub rules: Vec<ExtractedGpuPhysicsRule>,
}

/// Returns a cheap signature when GPU bodies have no authored event rules.
///
/// Rule-free effect swarms remain GPU-owned after setup. Their complete CPU
/// descriptions do not need to be cloned on every render frame. Scenes with
/// class or object watches return `None` and keep full rule extraction.
pub(super) fn simple_gpu_physics_signature(world: &mut World) -> Option<u64> {
    if !world
        .resource::<GpuPhysicsClassWatches>()
        .classes
        .is_empty()
    {
        return None;
    }
    let has_object_rules = {
        let mut query = world.query::<&GpuPhysicsWatch>();
        query.iter(world).any(|watch| !watch.rules.is_empty())
    };
    if has_object_rules {
        return None;
    }

    let mut hasher = super::FastHasher::default();
    let mut count = 0_u64;
    let mut query = world.query::<(
        Entity,
        &PhysicsId,
        &Transform,
        &super::PhysicsBody,
        Option<&super::RigidBody>,
    )>();
    for (entity, id, transform, body, rigid_body) in query.iter(world) {
        if !body.uses_gpu() {
            continue;
        }
        count += 1;
        entity.to_bits().hash(&mut hasher);
        id.hash(&mut hasher);
        for value in transform
            .position
            .into_iter()
            .chain(transform.rotation)
            .chain(transform.scale)
        {
            value.to_bits().hash(&mut hasher);
        }
        let solver = match body.solver {
            super::PhysicsSolver::Full => 0_u8,
            super::PhysicsSolver::Simplified => 1,
            super::PhysicsSolver::NoCollision => 2,
            super::PhysicsSolver::Custom => 3,
            super::PhysicsSolver::Space => 4,
        };
        solver.hash(&mut hasher);
        body.custom_shader.hash(&mut hasher);
        let rigid_body = rigid_body.copied().unwrap_or_default();
        (rigid_body.kind as u8).hash(&mut hasher);
        rigid_body.mass.to_bits().hash(&mut hasher);
        rigid_body.gravity_scale.to_bits().hash(&mut hasher);
        for value in rigid_body
            .linear_velocity
            .into_iter()
            .chain(rigid_body.angular_velocity)
        {
            value.to_bits().hash(&mut hasher);
        }
    }
    count.hash(&mut hasher);
    Some(hasher.finish())
}

/// Collects GPU bodies and compiles their authored rules for rendering.
pub(super) fn extract_gpu_physics_bodies(
    world: &mut World,
) -> Vec<ExtractedGpuPhysicsBody> {
    let class_watches = world.resource::<GpuPhysicsClassWatches>().clone();
    let needs_class_lookup = !class_watches.classes.is_empty();
    let raw = {
        let mut query = world.query::<(
            Entity,
            &PhysicsId,
            &Transform,
            &super::PhysicsBody,
            Option<&super::RigidBody>,
            Option<&super::ObjectClasses>,
            Option<&GpuPhysicsWatch>,
        )>();
        query
            .iter(world)
            .filter(|(_, _, _, body, _, _, _)| body.uses_gpu())
            .map(
                |(entity, id, transform, body, rigid_body, classes, watch)| {
                    (
                        entity,
                        *id,
                        *transform,
                        body.solver,
                        rigid_body.copied().unwrap_or_default(),
                        needs_class_lookup
                            .then(|| classes.cloned().unwrap_or_default()),
                        watch.cloned().unwrap_or_default(),
                    )
                },
            )
            .collect::<Vec<_>>()
    };

    let mut extracted = Vec::with_capacity(raw.len());
    world.resource_scope(
        |_, mut events: bevy_ecs::prelude::Mut<GpuEventRegistry>| {
            for (
                entity,
                physics_id,
                transform,
                solver,
                rigid_body,
                classes,
                watch,
            ) in raw
            {
                let mut authored_rules = watch.rules;
                if let Some(classes) = classes {
                    for class in classes.names {
                        if let Some(class_rules) =
                            class_watches.classes.get(&class)
                        {
                            for rule in class_rules {
                                if !authored_rules.contains(rule) {
                                    authored_rules.push(rule.clone());
                                }
                            }
                        }
                    }
                }
                let rules = authored_rules
                    .into_iter()
                    .filter_map(|rule| {
                        let instructions = rule.condition.compile().ok()?;
                        Some(ExtractedGpuPhysicsRule {
                            event_id: events.register(rule.event),
                            instructions,
                            mode: rule.mode,
                            payload: rule.payload,
                            cooldown_seconds: rule.cooldown_seconds,
                        })
                    })
                    .collect();
                extracted.push(ExtractedGpuPhysicsBody {
                    entity,
                    physics_id,
                    transform,
                    rigid_body,
                    solver,
                    rules,
                });
            }
        },
    );
    extracted.sort_by_key(|body| body.physics_id.slot);
    extracted
}

/// Converts raw readback bytes into frame-bounded Rust gameplay events.
///
/// Delayed events with stale body generations are counted and ignored. This
/// is normal when an object is deleted while older GPU work is still running.
pub fn route_gpu_physics_events(
    world: &mut World,
    raw_events: &[RawGpuPhysicsEvent],
) -> GpuEventRouteReport {
    let resolved = {
        let ids = world.resource::<PhysicsIdRegistry>();
        raw_events
            .iter()
            .map(|raw| (*raw, ids.resolve(raw.physics_id())))
            .collect::<Vec<_>>()
    };
    let known_events = world.resource::<GpuEventRegistry>();
    let mut routed = Vec::with_capacity(resolved.len());
    let mut report = GpuEventRouteReport::default();
    for (raw, entity) in resolved {
        let Some(entity) = entity else {
            report.stale += 1;
            continue;
        };
        let event_id = GpuEventId(raw.event_id);
        if known_events.name(event_id).is_none() {
            report.unknown_event += 1;
            continue;
        }
        routed.push(GpuPhysicsEvent {
            entity,
            physics_id: raw.physics_id(),
            event_id,
            tick: raw.tick(),
            flags: raw.flags,
            payload_kind: raw.payload_kind,
            payload: raw.payload,
        });
        report.delivered += 1;
    }
    let mut events = world.resource_mut::<EventQueue<GpuPhysicsEvent>>();
    for event in routed {
        events.send(event);
    }
    report
}

/// Installs CPU-side resources needed by the asynchronous physics bridge.
#[derive(Clone, Copy, Debug, Default)]
pub struct HybridPhysicsPlugin;

impl Plugin for HybridPhysicsPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        if !app.world().contains_resource::<PhysicsIdRegistry>() {
            app.insert_resource(PhysicsIdRegistry::default());
        }
        if !app.world().contains_resource::<GpuEventRegistry>() {
            app.insert_resource(GpuEventRegistry::default());
        }
        if !app.world().contains_resource::<GpuPhysicsClassWatches>() {
            app.insert_resource(GpuPhysicsClassWatches::default());
        }
        app.add_event::<GpuPhysicsEvent>()
            .add_systems(ScheduleStage::PostUpdate, maintain_gpu_physics_ids);
        Ok(())
    }
}

/// Adds IDs only when a physics body changes and releases removed IDs.
///
/// GPU simulation does not change the ECS body component every render frame,
/// so a stable swarm costs no CPU work here after its setup frame.
fn maintain_gpu_physics_ids(
    mut commands: Commands,
    changed_bodies: Query<
        (Entity, &super::PhysicsBody),
        Changed<super::PhysicsBody>,
    >,
    mut removed_bodies: RemovedComponents<super::PhysicsBody>,
    mut registry: ResMut<PhysicsIdRegistry>,
) {
    // Removal messages also cover despawned entities. Releasing the registry
    // entry is enough because a despawned entity cannot keep its component.
    for entity in removed_bodies.read() {
        registry.release(entity);
    }

    for (entity, body) in &changed_bodies {
        if body.uses_gpu() {
            let id = registry.assign(entity);
            commands.entity(entity).insert(id);
        } else {
            registry.release(entity);
            commands.entity(entity).remove::<PhysicsId>();
        }
    }
}
