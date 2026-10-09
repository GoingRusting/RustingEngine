//! Per-tick hash of the simulation state listed in `docs/determinism.md`.
//! Two runs with the same inputs must produce the same hash sequence;
//! [`StateHashes`] keeps the recent ones for determinism checks and replays.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::{self, Write};

use bevy_ecs::prelude::{Entity, Has, Or, Resource, With, World};
use rusting_core::transform::Transform;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    App, AppError, BurstEmitter, Collider, ColliderShape, CollisionLayers,
    Counter, FrameTime, GpuPhysicsCommands, GpuProxyOf, Health, Joint, Name,
    ObjectState, PhysicsBody, PhysicsIdRegistry, PhysicsSettings, PhysicsWorld,
    Pickup, PlatformerController, PlayerController, RandomSeed, RigidBody,
    SceneId, Sleeping, Tween,
};

/// Ticks of history kept in [`StateHashes`] (about 17 seconds at 60 Hz).
pub const STATE_HASH_HISTORY: usize = 1024;

/// `(fixed tick, hash)` of the state after each recent fixed step, oldest
/// first. The tick is the number of steps completed, so the state after the
/// first step is tick 1.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct StateHashes {
    /// CPU-visible state, recorded as each step finishes.
    pub recent: VecDeque<(u64, u64)>,
    /// GPU body buffers, hashed on the GPU after each tick and delivered one
    /// to three frames later by [`record_gpu_state_hashes`].
    pub gpu: VecDeque<(u64, u64)>,
}

impl StateHashes {
    #[must_use]
    pub fn get(&self, tick: u64) -> Option<u64> {
        self.recent
            .iter()
            .find(|(recorded, _)| *recorded == tick)
            .map(|(_, hash)| *hash)
    }
}

/// FNV-1a over 64-bit words: xor then multiply by an odd constant is a
/// bijection, so two inputs that differ in one word never hash the same.
pub struct StateHasher {
    state: u64,
    /// See [`Self::entity`].
    places: HashMap<Entity, u64>,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self {
            state: 0xCBF2_9CE4_8422_2325,
            places: HashMap::new(),
        }
    }
}

impl StateHasher {
    pub fn word(&mut self, value: u64) {
        self.state = (self.state ^ value).wrapping_mul(0x0100_0000_01B3);
    }

    /// Feeds `entity`'s place among the simulating entities in entity
    /// order, not its bits: resources are entities too, so a resource only
    /// the windowed runtime inserts would shift every later entity's index
    /// and break replays. Entities outside that set feed `u64::MAX`.
    pub fn entity(&mut self, entity: Entity) {
        self.word(self.places.get(&entity).copied().unwrap_or(u64::MAX));
    }

    pub fn float(&mut self, value: f32) {
        self.word(u64::from(value.to_bits()));
    }

    pub fn floats(&mut self, values: &[f32]) {
        for value in values {
            self.float(*value);
        }
    }

    #[must_use]
    pub fn finish(&self) -> u64 {
        self.state
    }
}

/// Feeds `Debug` output, for small gameplay components whose shortest
/// round-trip float formatting still tells every value apart.
impl Write for StateHasher {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            self.word(u64::from(byte));
        }
        Ok(())
    }
}

/// Hashes the simulation state visible to the CPU: resources first, then
/// every simulating entity in entity order.
///
/// GPU-class bodies contribute only their configuration: their `Transform`
/// and `RigidBody` on the CPU are readback copies that arrive frames late,
/// and their real state lives in GPU buffers (see [`StateHashes::gpu`]).
#[must_use]
pub fn world_state_hash(world: &mut World) -> u64 {
    let mut hasher = StateHasher::default();
    hasher.word(resource_state_hash(world));
    hasher.places = entity_places(world);
    for (entity, hash) in entity_state_hashes(world) {
        hasher.entity(entity);
        hasher.word(hash);
    }
    hasher.finish()
}

/// Hash of the simulation resources: clock, seed, physics settings, solver
/// carry-over, physics ids, and queued GPU commands.
#[must_use]
pub fn resource_state_hash(world: &mut World) -> u64 {
    let mut hasher = StateHasher {
        places: entity_places(world),
        ..StateHasher::default()
    };
    if let Some(time) = world.get_resource::<FrameTime>() {
        hasher.word(time.fixed_tick);
        hasher.word(time.fixed_delta.as_nanos() as u64);
    }
    if let Some(seed) = world.get_resource::<RandomSeed>() {
        hasher.word(seed.0);
    }
    if let Some(settings) = world.get_resource::<PhysicsSettings>() {
        hasher.floats(&settings.gravity);
        hasher.word(u64::from(settings.enabled));
    }
    if let Some(physics) = world.get_resource::<PhysicsWorld>() {
        physics.hash_state(&mut hasher);
    }
    if let Some(registry) = world.get_resource::<PhysicsIdRegistry>() {
        registry.hash_state(&mut hasher);
    }
    if let Some(commands) = world.get_resource::<GpuPhysicsCommands>() {
        let _ =
            write!(hasher, "{:?}{:?}", commands.commands, commands.apply_ticks);
    }
    hasher.finish()
}

/// The place in entity order of each entity [`entity_state_hashes`]
/// hashes, and of each joint.
fn entity_places(world: &mut World) -> HashMap<Entity, u64> {
    let mut entities: Vec<Entity> = world
        .query_filtered::<Entity, Or<(
            With<RigidBody>,
            With<Collider>,
            With<PhysicsBody>,
            With<Joint>,
            With<PlayerController>,
            With<PlatformerController>,
            With<Tween>,
            With<Pickup>,
            With<Counter>,
            With<BurstEmitter>,
            With<Health>,
            With<ObjectState>,
        )>>()
        .iter(world)
        .collect();
    entities.sort_unstable();
    entities.into_iter().zip(0..).collect()
}

/// Hash of each simulating entity's components, sorted by entity.
#[must_use]
pub fn entity_state_hashes(world: &mut World) -> Vec<(Entity, u64)> {
    // Entity fields feed their place; see `StateHasher::entity`.
    let places = entity_places(world);
    let place =
        |entity: Entity| places.get(&entity).copied().unwrap_or(u64::MAX);
    let mut hashers = BTreeMap::<Entity, StateHasher>::new();
    let mut bodies =
        world.query_filtered::<(
            Entity,
            Option<&Transform>,
            Option<&RigidBody>,
            Option<&Collider>,
            Option<&CollisionLayers>,
            Option<&PhysicsBody>,
            Has<Sleeping>,
            Option<&GpuProxyOf>,
        ), Or<(With<RigidBody>, With<Collider>, With<PhysicsBody>)>>(
        );
    for (entity, transform, rigid, collider, layers, body, sleeping, proxy) in
        bodies.iter(world)
    {
        let hasher = hashers.entry(entity).or_default();
        let on_gpu = body.is_some_and(PhysicsBody::uses_gpu);
        if let Some(body) = body {
            hasher.word(body.simulation as u64);
            hasher.word(body.solver as u64);
            let _ = write!(hasher, "{:?}", body.custom_shader);
        }
        if let Some(transform) = transform.filter(|_| !on_gpu) {
            hasher.floats(&transform.position);
            hasher.floats(&transform.rotation);
            hasher.floats(&transform.scale);
        }
        if let Some(rigid) = rigid {
            hasher.word(rigid.kind as u64);
            hasher.float(rigid.mass);
            hasher.float(rigid.gravity_scale);
            if !on_gpu {
                hasher.floats(&rigid.linear_velocity);
                hasher.floats(&rigid.angular_velocity);
            }
        }
        if let Some(collider) = collider {
            hash_collider(hasher, collider);
        }
        if let Some(layers) = layers {
            hasher.word(u64::from(layers.memberships));
            hasher.word(u64::from(layers.filters));
        }
        hasher.word(u64::from(sleeping));
        if let Some(proxy) = proxy {
            hasher.word(place(proxy.0));
        }
    }

    let mut gameplay = world.query_filtered::<(
        Entity,
        Option<&PlayerController>,
        Option<&PlatformerController>,
        Option<&Tween>,
        Option<&Pickup>,
        Option<&Counter>,
        Option<&BurstEmitter>,
        Option<&Health>,
        Option<&ObjectState>,
    ), Or<(
        With<PlayerController>,
        With<PlatformerController>,
        With<Tween>,
        With<Pickup>,
        With<Counter>,
        With<BurstEmitter>,
        With<Health>,
        With<ObjectState>,
    )>>();
    for (
        entity,
        player,
        platformer,
        tween,
        pickup,
        counter,
        emitter,
        state,
        health,
    ) in gameplay.iter(world)
    {
        let hasher = hashers.entry(entity).or_default();
        let floors = [
            player.map(|player| player.floor),
            platformer.map(|platformer| platformer.floor),
        ];
        let walls =
            player.map(|player| player.wall.map(|wall| (wall, [0.0; 3])));
        for touching in floors.into_iter().chain([walls]).flatten() {
            let (body, at) =
                touching.unwrap_or((Entity::PLACEHOLDER, [0.0; 3]));
            hasher.word(place(body));
            hasher.floats(&at);
        }
        let player = player.map(|player| PlayerController {
            floor: None,
            wall: None,
            ..*player
        });
        let platformer = platformer.map(|platformer| PlatformerController {
            floor: None,
            ..*platformer
        });
        let _ = write!(
            hasher,
            "{player:?}{platformer:?}{tween:?}{pickup:?}{counter:?}{emitter:?}{state:?}{health:?}"
        );
    }
    hashers
        .into_iter()
        .map(|(entity, hasher)| (entity, hasher.finish()))
        .collect()
}

fn hash_collider(hasher: &mut StateHasher, collider: &Collider) {
    match collider.shape {
        ColliderShape::Box { half_extents } => {
            hasher.word(0);
            hasher.floats(&half_extents);
        }
        ColliderShape::Sphere { radius } => {
            hasher.word(1);
            hasher.float(radius);
        }
        ColliderShape::Capsule {
            half_height,
            radius,
        } => {
            hasher.word(2);
            hasher.floats(&[half_height, radius]);
        }
        ColliderShape::ConvexMesh => hasher.word(3),
        ColliderShape::TriangleMesh => hasher.word(4),
    }
    hasher.floats(&[collider.friction, collider.restitution]);
    hasher.word(u64::from(collider.sensor));
}

fn push_capped(history: &mut VecDeque<(u64, u64)>, entry: (u64, u64)) {
    if history.len() == STATE_HASH_HISTORY {
        history.pop_front();
    }
    history.push_back(entry);
}

/// Hashes the state after the fixed step that just finished and records it.
pub(super) fn record_state_hash(world: &mut World) {
    let hash = world_state_hash(world);
    let tick = world.resource::<FrameTime>().fixed_tick + 1;
    let mut hashes = world.get_resource_or_insert_with(StateHashes::default);
    push_capped(&mut hashes.recent, (tick, hash));
}

/// Records GPU hashes from `SceneRenderer::take_completed_physics_state_hashes`.
pub fn record_gpu_state_hashes(world: &mut World, gpu: &[(u64, u64)]) {
    let mut hashes = world.get_resource_or_insert_with(StateHashes::default);
    for entry in gpu {
        push_capped(&mut hashes.gpu, *entry);
    }
}

/// One entity's state hash with the names that identify it in reports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityStateHash {
    /// `Entity::to_bits`; the same scene loads into the same entities.
    pub entity: u64,
    pub name: Option<String>,
    pub scene_id: Option<Uuid>,
    pub hash: u64,
}

/// [`entity_state_hashes`] with each entity's `Name` and `SceneId`.
#[must_use]
pub fn named_entity_state_hashes(world: &mut World) -> Vec<EntityStateHash> {
    entity_state_hashes(world)
        .into_iter()
        .map(|(entity, hash)| EntityStateHash {
            entity: entity.to_bits(),
            name: world.get::<Name>(entity).map(|name| name.0.clone()),
            scene_id: world.get::<SceneId>(entity).map(|id| id.0),
            hash,
        })
        .collect()
}

/// A whole run's hashes: every tick's world hash, and each entity's hash
/// after the last tick. Headless runs write it as JSON to the file named by
/// `STATE_HASH_OUT_ENV`, so runs in separate processes can be compared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateHashReport {
    pub ticks: Vec<(u64, u64)>,
    pub entities: Vec<EntityStateHash>,
}

/// First tick whose hash differs between two tick lists, or the first tick
/// only one of them has.
#[must_use]
pub fn first_divergent_tick(
    first: &[(u64, u64)],
    second: &[(u64, u64)],
) -> Option<u64> {
    first
        .iter()
        .zip(second)
        .find(|(left, right)| left != right)
        .map(|(left, _)| left.0)
        .or_else(|| {
            let shorter = first.len().min(second.len());
            first
                .get(shorter)
                .or(second.get(shorter))
                .map(|tick| tick.0)
        })
}

/// Where two runs first differ; see [`compare_runs`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Divergence {
    /// First tick whose state hash differs.
    pub tick: u64,
    /// First entity, in entity order, whose state differs or that only one
    /// run has; `None` when only resources differ.
    pub entity: Option<DivergentEntity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DivergentEntity {
    /// `Entity::to_bits`.
    pub entity: u64,
    pub name: Option<String>,
    pub scene_id: Option<Uuid>,
}

/// Builds two apps with `build`, which must load the same scene and feed
/// the same inputs, and runs them in lockstep for `ticks` updates of one
/// `fixed_delta` each, comparing the state hash after every update.
/// Returns the first divergence, or `None` when every tick matched.
pub fn compare_runs(
    mut build: impl FnMut() -> App,
    ticks: u32,
) -> Result<Option<Divergence>, AppError> {
    let mut runs = [build(), build()];
    let latest = |app: &App| {
        app.world()
            .get_resource::<StateHashes>()
            .and_then(|hashes| hashes.recent.back().copied())
    };
    for _ in 0..ticks {
        for app in &mut runs {
            let delta = app.world().resource::<FrameTime>().fixed_delta;
            app.update(delta)?;
        }
        let [first, second] = &mut runs;
        let (left, right) = (latest(first), latest(second));
        if left != right {
            return Ok(Some(Divergence {
                tick: left.or(right).map_or(0, |(tick, _)| tick),
                entity: first_divergent_entity(
                    &named_entity_state_hashes(first.world_mut()),
                    &named_entity_state_hashes(second.world_mut()),
                ),
            }));
        }
    }
    Ok(None)
}

/// First entity, in the first list's order, whose hash differs or that
/// only one list has. Entities pair by scene ID when they have one, so two
/// scene revisions that add or remove entities still line up, and by
/// entity otherwise.
#[must_use]
pub fn first_divergent_entity(
    first: &[EntityStateHash],
    second: &[EntityStateHash],
) -> Option<DivergentEntity> {
    let key = |entry: &EntityStateHash| match entry.scene_id {
        Some(id) => (Some(id), 0),
        None => (None, entry.entity),
    };
    let seconds: BTreeMap<_, u64> = second
        .iter()
        .map(|entry| (key(entry), entry.hash))
        .collect();
    let firsts: BTreeMap<_, u64> =
        first.iter().map(|entry| (key(entry), entry.hash)).collect();
    let entry = first
        .iter()
        .find(|entry| seconds.get(&key(entry)) != Some(&entry.hash))
        .or_else(|| {
            second
                .iter()
                .find(|entry| !firsts.contains_key(&key(entry)))
        })?;
    Some(DivergentEntity {
        entity: entry.entity,
        name: entry.name.clone(),
        scene_id: entry.scene_id,
    })
}
