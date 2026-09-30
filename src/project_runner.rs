//! Windowed runtime runner for native Rust game projects.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Mut, Resource, World};
use vulkano::format::Format;
use vulkano::swapchain::Surface;
use vulkano::VulkanError;
use vulkano_util::context::VulkanoContext;
use vulkano_util::window::{VulkanoWindows, WindowDescriptor};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::rendering::frame_pacer::{select_present_mode, FramePacer};
use crate::rendering::scene_renderer::{SceneRenderOptions, SceneRenderer};
use crate::runtime::{
    apply_gpu_state_samples, load_scene, load_scene_document,
    record_gpu_state_hashes, route_gpu_physics_events, scene_document,
    write_atomic, AppError, EventQueue, FrameTime, GpuEventRegistry,
    GpuPhysicsClassWatches, GpuPhysicsEvent, GpuPhysicsEventsLost,
    GpuPhysicsRule, GpuPhysicsWatch, HybridPhysicsPlugin, MeshRenderer, Name,
    PhysicsBackendStatus, Plugin, RenderExtractPlugin, RenderSettings,
    RenderWorld, RuntimeInput, SceneDocument, SceneLoadMode, ScheduleStage,
};
use crate::{App, AssetPlugin, AssetServer, Transform};

/// Result returned by the convenient native game entry point.
pub type GameResult<T = ()> = Result<T, Box<dyn Error>>;

/// Options applied when one class becomes GPU simulated.
#[derive(Clone, Debug)]
pub struct GpuBodySettings {
    /// Compute solver selected for every matching object.
    pub solver: crate::runtime::PhysicsSolver,
    /// Project-relative shader used when `solver` is Custom.
    pub custom_shader: Option<String>,
    /// Mass, velocity, and gravity values copied into GPU body state.
    pub rigid_body: crate::runtime::RigidBody,
    /// Collision shape and surface values used by collision solvers.
    pub collider: crate::runtime::Collider,
    /// Collision groups used when collision solvers are connected.
    pub collision_layers: crate::runtime::CollisionLayers,
}

impl Default for GpuBodySettings {
    fn default() -> Self {
        Self {
            solver: crate::runtime::PhysicsSolver::Full,
            custom_shader: None,
            rigid_body: crate::runtime::RigidBody::default(),
            collider: crate::runtime::Collider::default(),
            collision_layers: crate::runtime::CollisionLayers::default(),
        }
    }
}

/// Reusable settings for cubes created by native Rust game code.
#[derive(Clone, Debug, Default)]
pub struct CubeSpawn {
    /// Classes assigned to every cube created with this template.
    pub classes: crate::runtime::ObjectClasses,
}

impl CubeSpawn {
    /// Creates an empty cube template.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one class to every cube created with this template.
    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.classes.add(class);
        self
    }
}

/// Reusable settings for spheres created by native Rust game code.
#[derive(Clone, Debug, Default)]
pub struct SphereSpawn {
    /// Classes assigned to every sphere created with this template.
    pub classes: crate::runtime::ObjectClasses,
    /// Number of vertical sphere subdivisions used by the shared mesh.
    pub subdivisions: u32,
}

impl SphereSpawn {
    /// Creates a sphere template with moderate mesh quality.
    #[must_use]
    pub fn new() -> Self {
        Self {
            subdivisions: 16,
            ..Self::default()
        }
    }

    /// Changes the sphere mesh quality. Meshes are cached by this value.
    #[must_use]
    pub fn subdivisions(mut self, value: u32) -> Self {
        self.subdivisions = value.clamp(2, 128);
        self
    }

    /// Adds one class to every sphere created with this template.
    #[must_use]
    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.classes.add(class);
        self
    }
}

/// Cached procedural sphere meshes shared by all native-spawned spheres.
#[derive(Resource, Clone, Default)]
struct SphereMeshCache(
    HashMap<u32, crate::assets::Handle<crate::assets::MeshAsset>>,
);

/// The scene as it was loaded when the game started, for
/// [`GameScene::restart`].
#[derive(Resource)]
struct StartingScene(SceneDocument);

/// The folder holding the game's `project.json`, which
/// [`GameScene::load_scene`] resolves scene paths against.
#[derive(Resource)]
struct ProjectFolder(PathBuf);

/// A copy of the scene taken by [`GameScene::snapshot`], for
/// [`GameScene::restore`].
#[derive(Clone)]
pub struct GameSnapshot {
    scene: SceneDocument,
    once: GameOnceState,
    physics: Option<crate::runtime::PhysicsWorld>,
    next_order: Option<crate::runtime::NextSpawnOrder>,
    /// Each scene object's id, entity, solve order and sleep state.
    objects:
        Vec<(uuid::Uuid, Entity, Option<crate::runtime::SpawnOrder>, bool)>,
}

/// Keys already executed through [`GameScene::once`].
#[derive(Resource, Clone, Default)]
struct GameOnceState(HashSet<String>);

/// Convenient access to objects in the loaded scene.
///
/// This is a small API over the ECS, not another scripting language. Advanced
/// systems can still query the ECS world directly.
pub struct GameScene<'world> {
    /// ECS world that owns all scene objects and components.
    world: &'world mut World,
}

impl GameScene<'_> {
    /// Runs setup code once per round: [`Self::restart`] runs it again.
    ///
    /// This keeps large procedural scene creation out of the per-frame path
    /// while preserving the short `rusting_game!` API.
    pub fn once(
        &mut self,
        key: impl Into<String>,
        action: impl FnOnce(&mut GameScene<'_>),
    ) {
        let key = key.into();
        let should_run = self
            .world
            .get_resource_or_insert_with(GameOnceState::default)
            .0
            .insert(key);
        if should_run {
            action(self);
        }
    }

    /// Puts every scene object back as the game started: objects spawned
    /// since are removed, and moved, hidden or despawned ones return with
    /// their starting components. [`Self::once`] blocks run again. Time,
    /// input and resources carry on. Returns false when the game was not
    /// started from a scene file.
    pub fn restart(&mut self) -> bool {
        let Some(start) = self.world.remove_resource::<StartingScene>() else {
            return false;
        };
        let loaded =
            load_scene_document(self.world, &start.0, SceneLoadMode::Replace);
        self.world.insert_resource(start);
        // ponytail: a start that loaded once reloads the same way, so a
        // failure here is an engine bug, not a game error.
        loaded.expect("the starting scene reloads");
        forget_old_scene(self.world);
        true
    }

    /// Replaces every scene object with the scene at `path`, relative to
    /// the project folder (the one holding `project.json`), for example
    /// `"scenes/level_2.rscene"`. The new scene becomes the one
    /// [`Self::restart`] returns to, and [`Self::once`] blocks run again.
    /// Time, input and resources carry on. Counters are scene objects, so
    /// read any value to keep before loading and set it again after.
    ///
    /// # Errors
    /// Returns an error when the file is missing or is not a valid scene;
    /// the current scene then stays as it was.
    pub fn load_scene(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<(), crate::runtime::SceneIoError> {
        let path = match self.world.get_resource::<ProjectFolder>() {
            Some(folder) => folder.0.join(path),
            None => path.as_ref().to_path_buf(),
        };
        load_scene(self.world, &path, SceneLoadMode::Replace)?;
        let start = scene_document(self.world, "start")?;
        self.world.insert_resource(StartingScene(start));
        forget_old_scene(self.world);
        Ok(())
    }

    /// Copies every scene object, the `once` keys and the physics solver
    /// state, to put back later with [`Self::restore`] while the game runs
    /// (a rewind, a checkpoint).
    ///
    /// # Errors
    /// Returns an error when a scene object cannot be saved, as for saving
    /// the scene to a file.
    pub fn snapshot(
        &mut self,
    ) -> Result<GameSnapshot, crate::runtime::SceneIoError> {
        let scene = scene_document(self.world, "snapshot")?;
        let mut query = self.world.query::<(
            Entity,
            &crate::runtime::SceneId,
            Option<&crate::runtime::SpawnOrder>,
            bevy_ecs::query::Has<crate::runtime::Sleeping>,
        )>();
        let objects = query
            .iter(self.world)
            .map(|(entity, id, order, asleep)| {
                (id.0, entity, order.copied(), asleep)
            })
            .collect();
        Ok(GameSnapshot {
            scene,
            once: self
                .world
                .get_resource::<GameOnceState>()
                .cloned()
                .unwrap_or_default(),
            physics: self
                .world
                .get_resource::<crate::runtime::PhysicsWorld>()
                .cloned(),
            next_order: self
                .world
                .get_resource::<crate::runtime::NextSpawnOrder>()
                .copied(),
            objects,
        })
    }

    /// Puts the scene back as it was at [`Self::snapshot`]: objects spawned
    /// since are removed and the others return with their components,
    /// velocities and sleep state. Physics then continues exactly as it did
    /// after the snapshot. Time, input, resources and the scene
    /// [`Self::restart`] returns to carry on. Return from `update` after
    /// this, as after `restart`.
    ///
    /// # Errors
    /// Returns an error when the copy cannot be loaded; the scene then
    /// stays as it was.
    pub fn restore(
        &mut self,
        snapshot: &GameSnapshot,
    ) -> Result<(), crate::runtime::SceneIoError> {
        load_scene_document(
            self.world,
            &snapshot.scene,
            SceneLoadMode::Replace,
        )?;
        let mut query =
            self.world.query::<(Entity, &crate::runtime::SceneId)>();
        let now: HashMap<uuid::Uuid, Entity> = query
            .iter(self.world)
            .map(|(entity, id)| (id.0, entity))
            .collect();
        let mut renamed = HashMap::new();
        for (id, old, order, asleep) in &snapshot.objects {
            let Some(&entity) = now.get(id) else {
                continue;
            };
            renamed.insert(*old, entity);
            let mut entity = self.world.entity_mut(entity);
            match order {
                Some(order) => entity.insert(*order),
                None => entity.remove::<crate::runtime::SpawnOrder>(),
            };
            if *asleep {
                entity.insert(crate::runtime::Sleeping);
            }
        }
        if let Some(next) = snapshot.next_order {
            self.world.insert_resource(next);
        }
        if let Some(physics) = &snapshot.physics {
            let mut physics = physics.clone();
            physics.rename_bodies(&renamed);
            self.world.insert_resource(physics);
        }
        self.world.insert_resource(snapshot.once.clone());
        self.world.remove_resource::<SceneNameIndex>();
        Ok(())
    }

    /// egui context for this frame's runtime UI, drawn over the game view.
    /// See [`crate::runtime::RuntimeUi`].
    #[cfg(feature = "ui")]
    #[must_use]
    pub fn ui(&self) -> egui::Context {
        self.world
            .resource::<crate::runtime::RuntimeUi>()
            .context()
            .clone()
    }

    /// Creates one visible built-in cube with a unique object name.
    pub fn spawn_cube(
        &mut self,
        name: impl Into<String>,
        transform: Transform,
        template: &CubeSpawn,
    ) -> Entity {
        let (mesh, material) = {
            let assets = self.world.resource::<AssetServer>();
            (assets.fallback_mesh, assets.fallback_material)
        };
        self.spawn_renderable(
            name.into(),
            transform,
            mesh,
            material,
            template.classes.clone(),
        )
    }

    /// Creates a cube using a material handle from the asset server.
    pub fn spawn_cube_with_material(
        &mut self,
        name: impl Into<String>,
        transform: Transform,
        template: &CubeSpawn,
        material: crate::assets::Handle<crate::assets::MaterialAsset>,
    ) -> Entity {
        let mesh = self.world.resource::<AssetServer>().fallback_mesh;
        self.spawn_renderable(
            name.into(),
            transform,
            mesh,
            material,
            template.classes.clone(),
        )
    }

    /// Registers a material and returns its generational asset handle.
    pub fn create_material(
        &mut self,
        material: crate::assets::MaterialAsset,
    ) -> crate::assets::Handle<crate::assets::MaterialAsset> {
        self.world
            .resource_mut::<AssetServer>()
            .materials
            .insert(material)
    }

    /// Changes the color used to clear the game render target.
    pub fn set_background_color(&mut self, color: [f32; 4]) {
        self.world.resource_mut::<RenderSettings>().background_color = color;
    }

    /// Turns screen-space reflections on or off. Off saves the scene copy,
    /// mip chain and depth pyramid passes when nothing refracts; a game with
    /// no mirror-like surfaces gains about half a millisecond a frame.
    pub fn set_reflections(&mut self, enabled: bool) {
        self.world.resource_mut::<RenderSettings>().reflections = enabled;
    }

    /// Creates one visible procedural sphere with a unique object name.
    pub fn spawn_sphere(
        &mut self,
        name: impl Into<String>,
        transform: Transform,
        template: &SphereSpawn,
    ) -> Entity {
        let subdivisions = template.subdivisions;
        let mesh = self
            .world
            .get_resource::<SphereMeshCache>()
            .and_then(|cache| cache.0.get(&subdivisions).copied())
            .unwrap_or_else(|| {
                let mesh =
                    self.world.resource_mut::<AssetServer>().meshes.insert(
                        crate::assets::procedural_sphere_mesh(subdivisions),
                    );
                self.world
                    .get_resource_or_insert_with(SphereMeshCache::default)
                    .0
                    .insert(subdivisions, mesh);
                mesh
            });
        self.spawn_renderable(
            name.into(),
            transform,
            mesh,
            self.world.resource::<AssetServer>().fallback_material,
            template.classes.clone(),
        )
    }

    /// Creates a sphere using a material handle from the asset server.
    pub fn spawn_sphere_with_material(
        &mut self,
        name: impl Into<String>,
        transform: Transform,
        template: &SphereSpawn,
        material: crate::assets::Handle<crate::assets::MaterialAsset>,
    ) -> Entity {
        let subdivisions = template.subdivisions;
        let mesh = self
            .world
            .get_resource::<SphereMeshCache>()
            .and_then(|cache| cache.0.get(&subdivisions).copied())
            .unwrap_or_else(|| {
                let mesh =
                    self.world.resource_mut::<AssetServer>().meshes.insert(
                        crate::assets::procedural_sphere_mesh(subdivisions),
                    );
                self.world
                    .get_resource_or_insert_with(SphereMeshCache::default)
                    .0
                    .insert(subdivisions, mesh);
                mesh
            });
        self.spawn_renderable(
            name.into(),
            transform,
            mesh,
            material,
            template.classes.clone(),
        )
    }

    /// Inserts the shared components used by cube and sphere primitives.
    fn spawn_renderable(
        &mut self,
        name: String,
        transform: Transform,
        mesh: crate::assets::Handle<crate::assets::MeshAsset>,
        material: crate::assets::Handle<crate::assets::MaterialAsset>,
        classes: crate::runtime::ObjectClasses,
    ) -> Entity {
        // The index keeps names of despawned or renamed objects, so only a
        // live entity that still carries the name blocks the spawn.
        if find_named_entity(self.world, &name).is_some() {
            panic!("scene object `{name}` already exists");
        }
        let order = crate::runtime::next_spawn_order(self.world);
        let entity = self
            .world
            .spawn((
                crate::runtime::SceneId::new(),
                order,
                Name(name.clone()),
                transform,
                crate::runtime::MeshRenderer {
                    mesh,
                    material,
                    cast_shadows: true,
                    receive_shadows: true,
                },
                crate::runtime::Visibility::default(),
            ))
            .id();
        if !classes.names.is_empty() {
            self.world.entity_mut(entity).insert(classes);
        }
        self.world
            .resource_mut::<SceneNameIndex>()
            .entities
            .insert(name, entity);
        entity
    }

    /// Enables GPU physics for every object in one class.
    ///
    /// Call this from [`Self::once`] after procedural objects are spawned.
    pub fn apply_gpu_physics_to_class(
        &mut self,
        class: &str,
        settings: &GpuBodySettings,
    ) -> usize {
        let entities = {
            let mut query = self
                .world
                .query::<(Entity, &crate::runtime::ObjectClasses)>();
            query
                .iter(self.world)
                .filter_map(|(entity, classes)| {
                    classes.contains(class).then_some(entity)
                })
                .collect::<Vec<_>>()
        };
        for entity in &entities {
            self.world.entity_mut(*entity).insert((
                crate::runtime::PhysicsBody {
                    simulation: crate::runtime::SimulationClass::Gpu,
                    solver: settings.solver,
                    custom_shader: settings.custom_shader.clone(),
                },
                settings.rigid_body,
                settings.collider,
                settings.collision_layers,
            ));
        }
        entities.len()
    }

    /// Applies `edit` to the rigid body of `name` and wakes it. Returns
    /// false when the object or its rigid body does not exist.
    fn edit_body(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut crate::runtime::RigidBody),
    ) -> bool {
        let Some(entity) = find_named_entity(self.world, name) else {
            return false;
        };
        let Some(mut rigid_body) =
            self.world.get_mut::<crate::runtime::RigidBody>(entity)
        else {
            return false;
        };
        edit(&mut rigid_body);
        self.world
            .entity_mut(entity)
            .remove::<crate::runtime::Sleeping>();
        true
    }

    /// Sets a body's linear velocity, in metres per second, and wakes it.
    ///
    /// A CPU body moves with it from the next physics step. A GPU body reads
    /// it at the next GPU extraction, after which the compute shader owns it,
    /// so set it during setup.
    pub fn set_linear_velocity(
        &mut self,
        name: &str,
        velocity: [f32; 3],
    ) -> bool {
        self.edit_body(name, |body| body.linear_velocity = velocity)
    }

    /// A body's linear velocity in metres per second, or `None` when the
    /// object or its rigid body does not exist.
    #[must_use]
    pub fn linear_velocity(&mut self, name: &str) -> Option<[f32; 3]> {
        let entity = find_named_entity(self.world, name)?;
        self.world
            .get::<crate::runtime::RigidBody>(entity)
            .map(|body| body.linear_velocity)
    }

    /// Sets a body's angular velocity, in radians per second about each
    /// world axis, and wakes it. GPU bodies read it as for
    /// [`Self::set_linear_velocity`].
    pub fn set_angular_velocity(
        &mut self,
        name: &str,
        velocity: [f32; 3],
    ) -> bool {
        self.edit_body(name, |body| body.angular_velocity = velocity)
    }

    /// A body's angular velocity in radians per second, or `None` when the
    /// object or its rigid body does not exist.
    #[must_use]
    pub fn angular_velocity(&mut self, name: &str) -> Option<[f32; 3]> {
        let entity = find_named_entity(self.world, name)?;
        self.world
            .get::<crate::runtime::RigidBody>(entity)
            .map(|body| body.angular_velocity)
    }

    /// Stops a body completely: zeroes both velocities, forgets the
    /// solver's contact impulses and sleep count for it, and wakes it. Put
    /// it back in place first with [`GameObject::set_position`] and
    /// [`GameObject::set_rotation`]. Returns false when the object or its
    /// rigid body does not exist.
    pub fn reset_body(&mut self, name: &str) -> bool {
        let reset = self.edit_body(name, |body| {
            body.linear_velocity = [0.0; 3];
            body.angular_velocity = [0.0; 3];
        });
        if let (true, Some(entity)) =
            (reset, find_named_entity(self.world, name))
        {
            if let Some(mut physics) = self
                .world
                .get_resource_mut::<crate::runtime::PhysicsWorld>()
            {
                physics.forget_body(entity);
            }
        }
        reset
    }

    /// Changes a body's kind, for example a `Kinematic` template copied
    /// with [`Self::spawn_copy`] into a `Dynamic` projectile. A body that
    /// becomes `Kinematic` or `Fixed` stops where it is: kinematic bodies
    /// move by their velocity, so an old one would carry it away. Returns
    /// false when the object or its rigid body does not exist.
    pub fn set_body_kind(
        &mut self,
        name: &str,
        kind: crate::runtime::RigidBodyKind,
    ) -> bool {
        self.edit_body(name, |body| {
            if body.kind != kind
                && kind != crate::runtime::RigidBodyKind::Dynamic
            {
                body.linear_velocity = [0.0; 3];
                body.angular_velocity = [0.0; 3];
            }
            body.kind = kind;
        })
    }

    /// How the object called `name` was when the current scene loaded (by
    /// [`Self::restart`] and [`Self::load_scene`] too), so a round can put
    /// it back without reloading. `None` for objects spawned during play
    /// and when the game was not started from a scene file.
    #[must_use]
    pub fn initial(&self, name: &str) -> Option<InitialState> {
        use crate::runtime::SceneMaterial;
        let start = self.world.get_resource::<StartingScene>()?;
        let entity = start
            .0
            .entities
            .iter()
            .find(|entity| entity.name.as_deref() == Some(name))?;
        let material =
            entity
                .mesh_renderer
                .as_ref()
                .and_then(|renderer| match &renderer.material {
                    SceneMaterial::Inline(material) => Some(material),
                    SceneMaterial::BuiltinError => None,
                });
        Some(InitialState {
            transform: entity
                .transform
                .map(Transform::from)
                .unwrap_or_default(),
            color: material.map(|material| material.base_color),
            emissive: material.map(|material| material.emissive),
            body_kind: entity.rigid_body.map(|body| body.kind),
            visible: entity.visible.unwrap_or(true),
        })
    }

    /// Points the player controller on `name` at `yaw` and `pitch`, in
    /// radians (yaw 0 faces -Z). Returns false when the object has no
    /// player controller.
    pub fn set_look(&mut self, name: &str, yaw: f32, pitch: f32) -> bool {
        let Some(entity) = find_named_entity(self.world, name) else {
            return false;
        };
        let Some(mut player) = self
            .world
            .get_mut::<crate::runtime::PlayerController>(entity)
        else {
            return false;
        };
        player.yaw = yaw;
        player.pitch = pitch;
        true
    }

    /// Returns a scene object by name.
    ///
    /// # Panics
    ///
    /// Panics with a descriptive message if the object or its transform does
    /// not exist. Use [`Self::try_object`] when absence is expected.
    pub fn object(&mut self, name: &str) -> GameObject<'_> {
        self.try_object(name).unwrap_or_else(|| {
            panic!("scene object `{name}` does not exist or has no Transform")
        })
    }

    /// The ECS world, for anything this API does not cover.
    pub fn world(&mut self) -> &mut World {
        self.world
    }

    /// The `rusting.counter` called `name`, to read or change its `value`.
    /// With duplicate names, the one with the lowest scene ID wins, as for
    /// pickups and HUD text.
    pub fn counter(
        &mut self,
        name: &str,
    ) -> Option<Mut<'_, crate::runtime::Counter>> {
        let mut counters = self.world.query::<(
            &mut crate::runtime::Counter,
            Option<&crate::runtime::SceneId>,
        )>();
        crate::runtime::find_counter(counters.iter_mut(self.world), name)
    }

    /// The value of the counter called `name`, or 0 when there is none.
    #[must_use]
    pub fn counter_value(&mut self, name: &str) -> i32 {
        self.counter(name).map_or(0, |counter| counter.value)
    }

    /// Adds `amount` (which may be negative) to the counter called `name`
    /// and returns its new value; 0 when there is no such counter.
    pub fn add_to_counter(&mut self, name: &str, amount: i32) -> i32 {
        self.counter(name).map_or(0, |mut counter| {
            counter.value += amount;
            counter.value
        })
    }

    /// Sets the counter called `name` to `value`; does nothing when there is
    /// no such counter.
    pub fn set_counter(&mut self, name: &str, value: i32) {
        if let Some(mut counter) = self.counter(name) {
            counter.value = value;
        }
    }

    /// A value in `[0, 1)` from the run's seed, the fixed tick and
    /// `stream`. The same seed gives the same game, so scenarios repeat;
    /// draw with a different `stream` for each value needed in one tick.
    #[must_use]
    pub fn random(&self, stream: u64) -> f32 {
        let tick = self
            .world
            .get_resource::<FrameTime>()
            .map_or(0, |time| time.fixed_tick);
        let seed = self
            .world
            .get_resource::<crate::runtime::RandomSeed>()
            .copied()
            .unwrap_or_default();
        seed.unit(tick, crate::runtime::RandomSeed::stream("game", stream))
    }

    /// True when the counter called `name` has reached its target.
    #[must_use]
    pub fn counter_complete(&mut self, name: &str) -> bool {
        self.counter(name).is_some_and(|counter| counter.complete())
    }

    /// True on the frame `action` was pressed. Actions come from
    /// `rusting.input_action` components, the built-in `player.*` actions,
    /// or [`crate::runtime::ActionMap`] bindings made in code.
    #[must_use]
    pub fn pressed(&self, action: &str) -> bool {
        self.world
            .resource::<crate::runtime::ActionMap>()
            .just_pressed(self.world.resource::<RuntimeInput>(), action)
    }

    /// True while `action` is held down.
    #[must_use]
    pub fn held(&self, action: &str) -> bool {
        self.world
            .resource::<crate::runtime::ActionMap>()
            .held(self.world.resource::<RuntimeInput>(), action)
    }

    /// First collider on a ray from `origin` along `direction` within
    /// `max_distance` metres, sensors included. A ray starting inside a
    /// collider passes through it.
    #[must_use]
    pub fn raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
    ) -> Option<RayHit> {
        let hit = self
            .world
            .get_resource::<crate::runtime::PhysicsWorld>()?
            .raycast(origin, direction, max_distance, u32::MAX)?;
        Some(RayHit {
            name: self
                .world
                .get::<Name>(hit.entity)
                .map(|name| name.0.clone())
                .unwrap_or_default(),
            point: hit.point,
            normal: hit.normal,
            distance: hit.distance,
        })
    }

    /// Casts [`Self::raycast`] from the active camera through the center of
    /// the view: what a first-person crosshair points at. Uses the camera's
    /// transforms as they are now, so an aim set this frame counts.
    #[must_use]
    pub fn aim(&mut self, max_distance: f32) -> Option<RayHit> {
        let (origin, forward) = self.camera_ray()?;
        self.raycast(origin, forward, max_distance)
    }

    /// The active camera's world position and unit forward direction, from
    /// its transforms as they are now: where a thrown or fired object
    /// starts and which way it goes. `None` without an active camera.
    #[must_use]
    pub fn camera_ray(&mut self) -> Option<([f32; 3], [f32; 3])> {
        let (camera, _) = self.active_camera()?;
        let matrix = world_matrix(self.world, camera);
        let forward = matrix
            .transform_vector(&-nalgebra::Vector3::z())
            .normalize();
        let origin = matrix.transform_point(&nalgebra::Point3::origin());
        Some((origin.coords.into(), forward.into()))
    }

    /// The world ray under the mouse cursor, from the active camera: where
    /// a top-down or point-and-click game aims. Pass it to
    /// [`Self::raycast`] to find the ground point under the cursor. `None`
    /// before the cursor has entered the window or without an active
    /// camera.
    #[must_use]
    pub fn pointer_ray(&mut self) -> Option<([f32; 3], [f32; 3])> {
        let input = self.world.resource::<RuntimeInput>();
        let (cursor, size) = (input.cursor_position()?, input.viewport_size());
        if size.contains(&0.0) {
            return None;
        }
        let (entity, camera) = self.active_camera()?;
        let transform = crate::runtime::GlobalTransform {
            matrix: world_matrix(self.world, entity).into(),
        };
        let ray = crate::runtime::picking::scene_ray(
            cursor, [0.0; 2], size, camera, transform,
        )?;
        Some((ray.origin.into(), ray.direction.into()))
    }

    /// The highest-priority active camera.
    fn active_camera(&mut self) -> Option<(Entity, crate::runtime::Camera)> {
        let mut cameras =
            self.world.query::<(Entity, &crate::runtime::Camera)>();
        cameras
            .iter(self.world)
            .filter(|(_, camera)| camera.active)
            .max_by_key(|(entity, camera)| (camera.priority, *entity))
            .map(|(entity, camera)| (entity, *camera))
    }

    /// Removes the named object and its children. Returns false when no
    /// object has that name.
    pub fn despawn(&mut self, name: &str) -> bool {
        let Some(entity) = find_named_entity(self.world, name) else {
            return false;
        };
        let _ = rusting_core::hierarchy::clear_parent(self.world, entity);
        despawn_tree(self.world, entity);
        true
    }

    /// Copies the named object and its children, with every component, as a
    /// new object called `name` at local `position` under the same parent.
    /// Use it to spawn enemies or projectiles from an object authored in the
    /// scene: keep that template hidden and out of the way, then show each
    /// copy with [`Self::set_visible`]. A copied child is named
    /// `"<name>/<child name>"`, so names stay unique. Returns `None` when
    /// `template` does not exist.
    ///
    /// # Panics
    ///
    /// Panics if an object called `name` already exists.
    pub fn spawn_copy(
        &mut self,
        template: &str,
        name: impl Into<String>,
        position: [f32; 3],
    ) -> Option<Entity> {
        let name = name.into();
        let template = find_named_entity(self.world, template)?;
        if find_named_entity(self.world, &name).is_some() {
            panic!("scene object `{name}` already exists");
        }
        let copy = copy_tree(self.world, template, Some(&name));
        if let Some(mut transform) = self.world.get_mut::<Transform>(copy) {
            transform.position = position;
        }
        if let Some(parent) = self
            .world
            .get::<crate::runtime::Parent>(template)
            .map(|p| p.0)
        {
            let _ =
                rusting_core::hierarchy::set_parent(self.world, copy, parent);
        }
        Some(copy)
    }

    /// Names of the objects in `class`, sorted, so game code can loop over
    /// spawned copies. Unnamed objects are left out.
    #[must_use]
    pub fn in_class(&mut self, class: &str) -> Vec<String> {
        let mut query = self
            .world
            .query::<(&Name, &crate::runtime::ObjectClasses)>();
        let mut names: Vec<_> = query
            .iter(self.world)
            .filter(|(_, classes)| classes.contains(class))
            .map(|(name, _)| name.0.clone())
            .collect();
        names.sort();
        names
    }

    /// A hash of the positions, rotations, scales and velocities of the
    /// named objects in `class`, in name order. Unlike the full state hash
    /// it leaves out the clock and everything else, so two rounds that
    /// leave the class in the same state hash the same at any tick. Equal
    /// floats bit for bit give equal hashes; any drift changes it.
    #[must_use]
    pub fn state_hash(&mut self, class: &str) -> u64 {
        let mut hasher = crate::runtime::StateHasher::default();
        for name in self.in_class(class) {
            let Some(entity) = find_named_entity(self.world, &name) else {
                continue;
            };
            std::fmt::Write::write_str(&mut hasher, &name).ok();
            if let Some(transform) = self.world.get::<Transform>(entity) {
                hasher.floats(&transform.position);
                hasher.floats(&transform.rotation);
                hasher.floats(&transform.scale);
            }
            if let Some(body) =
                self.world.get::<crate::runtime::RigidBody>(entity)
            {
                hasher.floats(&body.linear_velocity);
                hasher.floats(&body.angular_velocity);
            }
        }
        hasher.finish()
    }

    /// Shows or hides the named object. Hidden objects still collide.
    pub fn set_visible(&mut self, name: &str, visible: bool) {
        if let Some(entity) = find_named_entity(self.world, name) {
            self.world
                .entity_mut(entity)
                .insert(crate::runtime::Visibility { visible });
        }
    }

    /// The named object's base color (RGBA, linear), if it has a mesh.
    #[must_use]
    pub fn color(&mut self, name: &str) -> Option<[f32; 4]> {
        let entity = find_named_entity(self.world, name)?;
        let handle = self.world.get::<MeshRenderer>(entity)?.material;
        let assets = self.world.get_resource::<AssetServer>()?;
        Some(assets.materials.get(handle)?.base_color)
    }

    /// Changes the named object's base color (RGBA, linear). Other objects
    /// that shared its material keep theirs.
    pub fn set_color(&mut self, name: &str, color: [f32; 4]) {
        self.edit_material(name, |material| material.base_color = color);
    }

    /// Changes the named object's emitted light (RGB, linear; above 1
    /// glows with bloom). Other objects that shared its material keep theirs.
    pub fn set_emissive(&mut self, name: &str, emissive: [f32; 3]) {
        self.edit_material(name, |material| material.emissive = emissive);
    }

    /// Gives the named object a changed copy of its material. Equal
    /// materials are shared, as the scene loader shares them, so switching
    /// between a few colors does not grow the material list.
    fn edit_material(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut crate::assets::MaterialAsset),
    ) {
        let Some(entity) = find_named_entity(self.world, name) else {
            return;
        };
        let Some(handle) = self
            .world
            .get::<MeshRenderer>(entity)
            .map(|renderer| renderer.material)
        else {
            return;
        };
        let Some(mut assets) = self.world.get_resource_mut::<AssetServer>()
        else {
            return;
        };
        let Some(mut material) = assets.materials.get(handle).cloned() else {
            return;
        };
        edit(&mut material);
        // ponytail: linear scan of every material; index them by value if
        // a game recolors thousands of objects per frame.
        let shared = assets.materials.iter().find_map(|(handle, existing)| {
            (*existing == material).then_some(handle)
        });
        let handle =
            shared.unwrap_or_else(|| assets.materials.insert(material));
        if let Some(mut renderer) = self.world.get_mut::<MeshRenderer>(entity) {
            renderer.material = handle;
        }
    }

    /// The character in the named `rusting.tile_map` cell under world
    /// `position` (x and y; z is ignored), `.` for an empty cell or one past
    /// the grid. `None` when the map does not exist or `position` is left of
    /// or above the map origin.
    #[must_use]
    pub fn tile(&mut self, map: &str, position: [f32; 3]) -> Option<char> {
        let entity = find_named_entity(self.world, map)?;
        let origin = self.world.get::<Transform>(entity)?.position;
        let map = self.world.get::<crate::runtime::TileMap>(entity)?;
        let (column, row) =
            map.cell_at([position[0] - origin[0], position[1] - origin[1]])?;
        Some(map.cell(column, row))
    }

    /// Writes `character` into the named tile map's cell under world
    /// `position`, growing the grid as needed; the map's tiles and colliders
    /// rebuild on the next fixed step. Returns false when the map does not
    /// exist, `position` is outside it, or the cell already held it.
    pub fn set_tile(
        &mut self,
        map: &str,
        position: [f32; 3],
        character: char,
    ) -> bool {
        let Some(entity) = find_named_entity(self.world, map) else {
            return false;
        };
        let Some(origin) =
            self.world.get::<Transform>(entity).map(|t| t.position)
        else {
            return false;
        };
        let Some(mut map) =
            self.world.get_mut::<crate::runtime::TileMap>(entity)
        else {
            return false;
        };
        let local = [position[0] - origin[0], position[1] - origin[1]];
        let Some((column, row)) = map.cell_at(local) else {
            return false;
        };
        // Compare first so an unchanged cell does not mark the map changed.
        map.cell(column, row) != character
            && map.set_cell(column, row, character)
    }

    /// Fires the named object's `rusting.sound_cue` and
    /// `rusting.burst_emitter`, the way a pickup does when collected.
    pub fn trigger(&mut self, name: &str) {
        let Some(entity) = find_named_entity(self.world, name) else {
            return;
        };
        if let Some(mut cue) =
            self.world.get_mut::<crate::runtime::SoundCue>(entity)
        {
            cue.trigger();
        }
        if let Some(mut emitter) =
            self.world.get_mut::<crate::runtime::BurstEmitter>(entity)
        {
            emitter.trigger();
        }
    }

    /// Names of the objects touching `name` in the last physics step, sensors
    /// included. Empty when `name` does not exist.
    #[must_use]
    pub fn touching(&mut self, name: &str) -> Vec<String> {
        let Some(entity) = find_named_entity(self.world, name) else {
            return Vec::new();
        };
        let Some(physics) =
            self.world.get_resource::<crate::runtime::PhysicsWorld>()
        else {
            return Vec::new();
        };
        physics
            .contacts()
            .iter()
            .filter_map(|contact| match (contact.a, contact.b) {
                (a, other) | (other, a) if a == entity => Some(other),
                _ => None,
            })
            .filter_map(|other| self.world.get::<Name>(other))
            .map(|other| other.0.clone())
            .collect()
    }

    /// Tries to return a scene object by name.
    pub fn try_object(&mut self, name: &str) -> Option<GameObject<'_>> {
        let entity = find_named_entity(self.world, name)?;
        self.world
            .get_mut::<Transform>(entity)
            .map(|transform| GameObject { transform })
    }

    /// Adds one GPU condition to a named object if it is not already present.
    ///
    /// This method is safe to call from the short update function every frame.
    pub fn watch_gpu_object(&mut self, name: &str, rule: GpuPhysicsRule) {
        let entity = find_named_entity(self.world, name)
            .unwrap_or_else(|| panic!("scene object `{name}` does not exist"));
        if let Some(mut watch) = self.world.get_mut::<GpuPhysicsWatch>(entity) {
            if !watch.rules.contains(&rule) {
                watch.rules.push(rule);
            }
        } else {
            self.world
                .entity_mut(entity)
                .insert(GpuPhysicsWatch { rules: vec![rule] });
        }
    }

    /// Adds one GPU condition to every GPU body in the requested class.
    ///
    /// Objects receive classes in the editor Inspector or through the
    /// [`crate::runtime::ObjectClasses`] component. One object may have several
    /// classes, but the same rule is never added to it twice.
    /// This method is safe to call from the short update function every frame.
    pub fn watch_gpu_class(&mut self, class: &str, rule: GpuPhysicsRule) {
        self.world
            .resource_mut::<GpuPhysicsClassWatches>()
            .add(class, rule);
    }

    /// Returns GPU physics events with the requested registered name.
    #[must_use]
    pub fn gpu_events(&self, name: &str) -> Vec<GpuPhysicsEvent> {
        let Some(event_id) = self.world.resource::<GpuEventRegistry>().id(name)
        else {
            return Vec::new();
        };
        self.world
            .resource::<EventQueue<GpuPhysicsEvent>>()
            .iter()
            .filter(|event| event.event_id == event_id)
            .copied()
            .collect()
    }
}

/// Mutable high-level access to one scene object's transform.
pub struct GameObject<'world> {
    /// Transform borrowed from the real ECS object.
    transform: Mut<'world, Transform>,
}

impl GameObject<'_> {
    /// Returns the current local X, Y, and Z position.
    #[must_use]
    pub fn position(&self) -> [f32; 3] {
        self.transform.position
    }

    /// Replaces the local X, Y, and Z position.
    pub fn set_position(&mut self, position: [f32; 3]) -> &mut Self {
        self.transform.position = position;
        self
    }

    /// Adds an X, Y, and Z offset to the current position.
    pub fn move_by(&mut self, offset: [f32; 3]) -> &mut Self {
        for (position, offset) in self.transform.position.iter_mut().zip(offset)
        {
            *position += offset;
        }
        self
    }

    /// Moves the object along its X axis.
    pub fn move_x(&mut self, distance: f32) -> &mut Self {
        self.transform.position[0] += distance;
        self
    }

    /// Moves the object along its Y axis.
    pub fn move_y(&mut self, distance: f32) -> &mut Self {
        self.transform.position[1] += distance;
        self
    }

    /// Moves the object along its Z axis.
    pub fn move_z(&mut self, distance: f32) -> &mut Self {
        self.transform.position[2] += distance;
        self
    }

    /// Returns the local X, Y, and Z rotation in radians.
    #[must_use]
    pub fn rotation(&self) -> [f32; 3] {
        self.transform.rotation
    }

    /// Returns the local X, Y, and Z scale.
    #[must_use]
    pub fn scale(&self) -> [f32; 3] {
        self.transform.scale
    }

    /// Replaces the local X, Y, and Z rotation in radians.
    pub fn set_rotation(&mut self, rotation: [f32; 3]) -> &mut Self {
        self.transform.rotation = rotation;
        self
    }

    /// Adds rotation in radians to all three axes.
    pub fn rotate_by(&mut self, rotation: [f32; 3]) -> &mut Self {
        for (current, rotation) in
            self.transform.rotation.iter_mut().zip(rotation)
        {
            *current += rotation;
        }
        self
    }

    /// Adds rotation in radians to X axis
    pub fn rotate_x(&mut self, rotation: f32) -> &mut Self {
        self.transform.rotation[0] += rotation;
        self
    }
    /// Adds rotation in radians to Y axis
    pub fn rotate_y(&mut self, rotation: f32) -> &mut Self {
        self.transform.rotation[1] += rotation;
        self
    }
    /// Adds rotation in radians to Y axis
    pub fn rotate_z(&mut self, rotation: f32) -> &mut Self {
        self.transform.rotation[2] += rotation;
        self
    }

    /// Replaces the local size on all three axes
    pub fn set_scale(&mut self, scale: [f32; 3]) -> &mut Self {
        self.transform.scale = scale;
        self
    }
}

/// Connects object names to ECS IDs after the first lookup.
///
/// This avoids searching every object again on later frames.
#[derive(Resource, Clone, Default)]
struct SceneNameIndex {
    /// Object names resolved once instead of searching 10,000 objects again.
    entities: HashMap<String, Entity>,
    /// True after names from the loaded scene were copied into this map.
    initialized: bool,
}

/// Builds the fast name index once after a scene is loaded.
fn ensure_scene_name_index(world: &mut World) {
    if world
        .get_resource::<SceneNameIndex>()
        .is_some_and(|index| index.initialized)
    {
        return;
    }
    let entries = {
        let mut query = world.query::<(Entity, &Name)>();
        query
            .iter(world)
            .map(|(entity, name)| (name.0.clone(), entity))
            .collect::<Vec<_>>()
    };
    let mut entities = HashMap::with_capacity(entries.len());
    for (name, entity) in entries {
        assert!(
            entities.insert(name.clone(), entity).is_none(),
            "scene object name `{name}` is not unique"
        );
    }
    world.insert_resource(SceneNameIndex {
        entities,
        initialized: true,
    });
}

/// Finds a named ECS object and saves the result for later calls.
fn find_named_entity(world: &mut World, name: &str) -> Option<Entity> {
    ensure_scene_name_index(world);
    let entity = world
        .resource::<SceneNameIndex>()
        .entities
        .get(name)
        .copied()?;
    world
        .get::<Name>(entity)
        .is_some_and(|current| current.0 == name)
        .then_some(entity)
}

/// How an object was when the current scene loaded; see
/// [`GameScene::initial`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InitialState {
    /// Local transform, relative to the parent if there is one.
    pub transform: Transform,
    /// Base color and emitted light of an inline material; `None` without
    /// a mesh or with a material file.
    pub color: Option<[f32; 4]>,
    pub emissive: Option<[f32; 3]>,
    pub body_kind: Option<crate::runtime::RigidBodyKind>,
    pub visible: bool,
}

/// What [`GameScene::raycast`] and [`GameScene::aim`] hit.
#[derive(Clone, Debug, PartialEq)]
pub struct RayHit {
    /// Scene name of the object hit; empty for an unnamed one.
    pub name: String,
    pub point: [f32; 3],
    pub normal: [f32; 3],
    pub distance: f32,
}

/// `entity`'s world matrix from the current transforms of it and its
/// parents.
fn world_matrix(world: &World, entity: Entity) -> nalgebra::Matrix4<f32> {
    let mut matrix = nalgebra::Matrix4::identity();
    let mut current = Some(entity);
    while let Some(entity) = current {
        if let Some(transform) = world.get::<Transform>(entity) {
            matrix = nalgebra::Matrix4::from(transform.to_matrix()) * matrix;
        }
        current = world
            .get::<crate::runtime::Parent>(entity)
            .map(|parent| parent.0);
    }
    matrix
}

/// Drops state that belonged to the replaced scene: the name index, the
/// `once` keys, and the solver's contacts, warm starts and sleep counters,
/// so physics after a reload repeats the first load exactly.
fn forget_old_scene(world: &mut World) {
    world.remove_resource::<SceneNameIndex>();
    world.remove_resource::<GameOnceState>();
    if world.contains_resource::<crate::runtime::PhysicsWorld>() {
        world.insert_resource(crate::runtime::PhysicsWorld::default());
    }
}

/// Clones `entity` and its descendants, naming the copy `name` (unnamed
/// when `None`). Each copy gets a new `SceneId`; named children become
/// `"<name>/<child>"`, parented to the copy.
fn copy_tree(world: &mut World, entity: Entity, name: Option<&str>) -> Entity {
    let copy =
        world
            .entity_mut(entity)
            .clone_and_spawn_with_opt_out(|builder| {
                builder.deny::<(
                    crate::runtime::Parent,
                    crate::runtime::Children,
                    crate::runtime::SceneId,
                    crate::runtime::Sleeping,
                    Name,
                )>();
            });
    let order = crate::runtime::next_spawn_order(world);
    world
        .entity_mut(copy)
        .insert((crate::runtime::SceneId::new(), order));
    if let Some(name) = name {
        world.entity_mut(copy).insert(Name(name.to_owned()));
        world
            .resource_mut::<SceneNameIndex>()
            .entities
            .insert(name.to_owned(), copy);
    }
    let children = world
        .get::<crate::runtime::Children>(entity)
        .map(|children| children.0.clone())
        .unwrap_or_default();
    for child in children {
        let child_name = world
            .get::<Name>(child)
            .zip(name)
            .map(|(child, parent)| format!("{parent}/{}", child.0));
        let child = copy_tree(world, child, child_name.as_deref());
        let _ = rusting_core::hierarchy::set_parent(world, child, copy);
    }
    copy
}

fn despawn_tree(world: &mut World, entity: Entity) {
    let children = world
        .get::<crate::runtime::Children>(entity)
        .map(|children| children.0.clone())
        .unwrap_or_default();
    for child in children {
        despawn_tree(world, child);
    }
    world.despawn(entity);
}

/// Signature used by the concise native Rust game update API.
pub type GameUpdate = for<'world> fn(&mut GameScene<'world>, &FrameTime);

/// Update function stored inside the ECS world.
#[derive(Resource, Clone, Copy)]
struct GameUpdateFunction(GameUpdate);

/// Installs the easy game update function into the normal ECS schedule.
#[derive(Clone, Copy)]
struct SimpleGamePlugin {
    /// User function called once per rendered frame.
    update: GameUpdate,
}

impl Plugin for SimpleGamePlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.insert_resource(GameUpdateFunction(self.update));
        app.add_system(ScheduleStage::Update, run_simple_game_update);
        app.register_snapshot_component::<GameUpdateFunction>()
            .register_snapshot_component::<GameOnceState>()
            .register_snapshot_component::<SceneNameIndex>()
            .register_snapshot_component::<SphereMeshCache>();
        Ok(())
    }
}

fn run_simple_game_update(world: &mut World) {
    // Copy these small values before giving the whole world to GameScene.
    let time = *world.resource::<FrameTime>();
    let update = world.resource::<GameUpdateFunction>().0;
    update(&mut GameScene { world }, &time);
}

/// What a code reload carries from the old game process to the new one.
#[derive(serde::Serialize, serde::Deserialize)]
struct CodeReloadState {
    /// Every scene object with its reflected components.
    scene: SceneDocument,
    /// [`GameScene::once`] keys that already ran.
    once: Vec<String>,
}

/// Writes the scene objects and the finished [`GameScene::once`] keys to
/// `path`. Resources and components with no reflection registration are
/// not kept.
fn save_code_reload_state(
    runtime: &mut App,
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let world = runtime.world_mut();
    let scene = scene_document(world, "code reload")?;
    let mut once = world
        .get_resource::<GameOnceState>()
        .map(|state| state.0.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    once.sort();
    write_atomic(path, &serde_json::to_vec(&CodeReloadState { scene, once })?)?;
    Ok(())
}

/// Replaces the freshly loaded scene with the state saved before a code
/// reload. Startup systems and [`GameScene::once`] blocks that already ran
/// do not run again. On error the fresh scene stays, so the game starts
/// clean.
fn restore_code_reload_state(
    runtime: &mut App,
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let state: CodeReloadState = serde_json::from_slice(&std::fs::read(path)?)?;
    let world = runtime.world_mut();
    load_scene_document(world, &state.scene, SceneLoadMode::Replace)?;
    world.insert_resource(GameOnceState(state.once.into_iter().collect()));
    // Names now belong to the restored entities.
    world.remove_resource::<SceneNameIndex>();
    runtime.skip_startup();
    Ok(())
}

/// Builds the same ECS and asset stack for windowed and headless games.
fn load_project_runtime<P: Plugin>(
    scene_path: &Path,
    plugin: P,
) -> Result<App, Box<dyn Error>> {
    let mut runtime = App::new();
    runtime.add_plugin(AssetPlugin)?;
    runtime.add_plugin(HybridPhysicsPlugin)?;
    runtime.add_plugin(RenderExtractPlugin)?;
    runtime.add_plugin(plugin)?;
    load_scene(runtime.world_mut(), scene_path, SceneLoadMode::Replace)?;
    let start = scene_document(runtime.world_mut(), "start")?;
    runtime.insert_resource(StartingScene(start));
    let folder = scene_path
        .ancestors()
        .skip(1)
        .find(|folder| folder.join("project.json").is_file())
        .or_else(|| scene_path.parent())
        .unwrap_or(Path::new("."));
    runtime.insert_resource(ProjectFolder(folder.to_path_buf()));
    crate::runtime::check_determinism(runtime.world_mut())?;
    Ok(runtime)
}

/// egui input translation and painting for [`crate::runtime::RuntimeUi`].
#[cfg(feature = "ui")]
struct RuntimeUiPainter {
    input: egui_winit::State,
    painter: crate::rendering::egui_painter::EguiPainter,
    textures: egui::TexturesDelta,
    primitives: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
}

/// Images the game's swapchain asks for. [`probe_swapchain_images`] lowers it
/// when the surface allows fewer.
static SWAPCHAIN_IMAGES: AtomicU32 = AtomicU32::new(4);

/// Sets [`SWAPCHAIN_IMAGES`] to four, or to the surface's maximum when that is
/// lower. Only a window has a surface, so a hidden one is made and dropped;
/// on any failure the default stays and window creation reports the error.
fn probe_swapchain_images(
    event_loop: &ActiveEventLoop,
    vulkan: &VulkanoContext,
) {
    let Ok(window) = event_loop
        .create_window(Window::default_attributes().with_visible(false))
    else {
        return;
    };
    let Ok(surface) =
        Surface::from_window(vulkan.instance().clone(), Arc::new(window))
    else {
        return;
    };
    let Ok(capabilities) = vulkan
        .device()
        .physical_device()
        .surface_capabilities(&surface, Default::default())
    else {
        return;
    };
    let wanted = capabilities.max_image_count.map_or(4, |max| max.min(4));
    SWAPCHAIN_IMAGES.store(wanted, Ordering::Relaxed);
}

/// Owns the operating-system window and all Vulkan presentation state.
struct WindowRunner {
    /// Text shown in the game window title bar.
    title: String,
    /// Vulkan device, queues, and memory allocators.
    vulkan: VulkanoContext,
    /// Winit windows connected to Vulkan swapchains.
    windows: VulkanoWindows,
    /// Renderer created after the operating system opens the window.
    scene_renderer: Option<SceneRenderer>,
    /// Requests frames immediately or waits when an FPS limit is enabled.
    frame_pacer: FramePacer,
    /// VSync value currently applied to the swapchain.
    applied_vsync: Option<bool>,
    /// Cursor capture currently applied to the window.
    applied_cursor_capture: bool,
    #[cfg(feature = "ui")]
    ui: Option<RuntimeUiPainter>,
}

impl WindowRunner {
    fn new(title: String) -> Self {
        Self {
            title,
            vulkan: VulkanoContext::new(crate::rendering::vulkano_config()),
            windows: VulkanoWindows::default(),
            scene_renderer: None,
            frame_pacer: FramePacer::default(),
            applied_vsync: None,
            applied_cursor_capture: false,
            #[cfg(feature = "ui")]
            ui: None,
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop, runtime: &mut App) {
        // Winit can resume more than once. The renderer must be created once.
        if self.scene_renderer.is_some() {
            return;
        }
        probe_swapchain_images(event_loop, &self.vulkan);
        // Create the operating-system window and its Vulkan swapchain.
        self.windows.create_window(
            event_loop,
            &self.vulkan,
            &WindowDescriptor {
                title: self.title.clone(),
                width: 1440.0,
                height: 900.0,
                ..WindowDescriptor::default()
            },
            |create_info| {
                create_info.image_format = Format::B8G8R8A8_UNORM;
                create_info.min_image_count = create_info
                    .min_image_count
                    .max(SWAPCHAIN_IMAGES.load(Ordering::Relaxed));
            },
        );
        // Apply project render settings before the first presented frame.
        let renderer = self.windows.get_primary_renderer_mut().unwrap();
        let settings = runtime.world().resource::<RenderSettings>();
        let mode = select_present_mode(
            &renderer.graphics_queue(),
            &renderer.surface(),
            settings.vsync,
        );
        if std::env::var_os(PERF_ENV).is_some() {
            eprintln!("[rusting] present mode {mode:?}");
        }
        renderer.set_present_mode(mode);
        self.applied_vsync = Some(settings.vsync);
        let initial_size = renderer.window().inner_size();
        runtime
            .world_mut()
            .resource_mut::<RuntimeInput>()
            .record_viewport_size([
                initial_size.width as f32,
                initial_size.height as f32,
            ]);
        self.scene_renderer = Some(
            SceneRenderer::new(
                renderer.graphics_queue(),
                self.vulkan.memory_allocator().clone(),
                renderer.swapchain_format(),
                renderer.swapchain_image_size(),
            )
            .expect("failed to create game scene renderer"),
        );
        #[cfg(feature = "ui")]
        if let Some(runtime_ui) =
            runtime.world().get_resource::<crate::runtime::RuntimeUi>()
        {
            let painter = crate::rendering::egui_painter::EguiPainter::new(
                renderer.graphics_queue(),
                self.vulkan.memory_allocator().clone(),
                renderer.swapchain_format(),
            )
            .expect("failed to create runtime UI painter");
            let context = runtime_ui.context();
            let input = egui_winit::State::new(
                context.clone(),
                context.viewport_id(),
                event_loop,
                Some(renderer.window().scale_factor() as f32),
                None,
                Some(painter.max_texture_side()),
            );
            self.ui = Some(RuntimeUiPainter {
                input,
                painter,
                textures: egui::TexturesDelta::default(),
                primitives: Vec::new(),
                pixels_per_point: 1.0,
            });
        }
    }

    /// Gives a window event to the runtime UI. Returns true when the UI
    /// used it, so gameplay input should not see it.
    fn ui_event(&mut self, window_id: WindowId, event: &WindowEvent) -> bool {
        #[cfg(feature = "ui")]
        if let (Some(ui), Some(renderer)) =
            (self.ui.as_mut(), self.windows.get_renderer(window_id))
        {
            return ui.input.on_window_event(renderer.window(), event).consumed;
        }
        let _ = (window_id, event);
        false
    }

    /// Hands window input to the UI pass that the next update runs.
    fn begin_ui_frame(&mut self, window_id: WindowId, runtime: &mut App) {
        #[cfg(feature = "ui")]
        if let (Some(ui), Some(renderer), Some(mut runtime_ui)) = (
            self.ui.as_mut(),
            self.windows.get_renderer(window_id),
            runtime
                .world_mut()
                .get_resource_mut::<crate::runtime::RuntimeUi>(),
        ) {
            runtime_ui.set_input(ui.input.take_egui_input(renderer.window()));
        }
        let _ = (window_id, runtime);
    }

    /// Tessellates the finished UI pass for [`Self::render`].
    fn finish_ui_frame(&mut self, window_id: WindowId, runtime: &mut App) {
        #[cfg(feature = "ui")]
        if let (Some(ui), Some(renderer), Some(output)) = (
            self.ui.as_mut(),
            self.windows.get_renderer(window_id),
            runtime
                .world_mut()
                .get_resource_mut::<crate::runtime::RuntimeUi>()
                .and_then(|mut runtime_ui| runtime_ui.take_output()),
        ) {
            ui.input.handle_platform_output(
                renderer.window(),
                output.platform_output,
            );
            ui.textures.append(output.textures_delta);
            ui.pixels_per_point = output.pixels_per_point;
            ui.primitives = ui
                .input
                .egui_ctx()
                .tessellate(output.shapes, output.pixels_per_point);
        }
        let _ = (window_id, runtime);
    }

    fn request_next_frame(
        &mut self,
        event_loop: &ActiveEventLoop,
        runtime: &App,
    ) {
        if let Some(renderer) = self.windows.get_primary_renderer_mut() {
            self.frame_pacer.request_next_frame(
                event_loop,
                renderer.window(),
                runtime.world().resource::<RenderSettings>(),
            );
        }
    }

    /// Hides and locks the cursor while gameplay asks for mouse look.
    fn apply_cursor_capture(&mut self, captured: bool) {
        if self.applied_cursor_capture == captured {
            return;
        }
        let Some(renderer) = self.windows.get_primary_renderer() else {
            return;
        };
        let window = renderer.window();
        let grab = if captured {
            // Some platforms support only one of the two grab modes.
            window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        } else {
            window.set_cursor_grab(CursorGrabMode::None)
        };
        if let Err(error) = grab {
            eprintln!("cursor capture failed: {error}");
        }
        window.set_cursor_visible(!captured);
        self.applied_cursor_capture = captured;
    }

    fn resize(&mut self, window_id: WindowId) {
        self.windows.get_renderer_mut(window_id).unwrap().resize();
    }

    fn render(
        &mut self,
        window_id: WindowId,
        runtime: &App,
    ) -> Result<(), String> {
        let renderer = self.windows.get_renderer_mut(window_id).unwrap();
        let vsync = runtime.world().resource::<RenderSettings>().vsync;
        if self.applied_vsync != Some(vsync) {
            renderer.set_present_mode(select_present_mode(
                &renderer.graphics_queue(),
                &renderer.surface(),
                vsync,
            ));
            self.applied_vsync = Some(vsync);
        }

        let acquired = renderer.acquire(None, |_| {});
        match acquired {
            Ok(future) => {
                let extent = renderer.swapchain_image_size();
                let future = self
                    .scene_renderer
                    .as_mut()
                    .unwrap()
                    .render(
                        future,
                        renderer.swapchain_image_view(),
                        extent,
                        SceneRenderOptions::game(extent),
                        runtime.world().resource::<RenderWorld>(),
                        runtime.world().resource::<AssetServer>(),
                    )
                    .map_err(|error| {
                        format!("scene rendering failed: {error}")
                    })?;
                #[cfg(feature = "ui")]
                let future = match self.ui.as_mut() {
                    Some(ui) => {
                        let future = ui
                            .painter
                            .paint(
                                future,
                                renderer.swapchain_image_view(),
                                ui.pixels_per_point,
                                &ui.primitives,
                                &ui.textures,
                            )
                            .map_err(|error| {
                                format!("runtime UI painting failed: {error}")
                            })?;
                        ui.textures.clear();
                        future
                    }
                    None => future,
                };
                renderer.present(future, false);
                Ok(())
            }
            Err(VulkanError::OutOfDate) => {
                renderer.resize();
                Ok(())
            }
            Err(error) => Err(format!("swapchain acquisition failed: {error}")),
        }
    }
}

struct ProjectApplication {
    window: WindowRunner,
    /// ECS world, schedules, assets, and game plugin.
    runtime: App,
    /// Time of the previous frame, used to calculate delta time.
    previous_frame: Instant,
    /// State file and the flag the standard input reader sets when the
    /// editor asks for a code reload.
    code_reload: Option<(PathBuf, Arc<AtomicBool>)>,
    /// Set by `RUSTING_PERF`: frames counted since the last printed line.
    perf: Option<(Instant, u32)>,
    /// Time spent in `runtime.update` and in `render` since the last line.
    perf_spent: [std::time::Duration; 2],
}

/// Environment variable that makes a running game print, once a second, its
/// frame rate and where the frame time goes.
pub const PERF_ENV: &str = "RUSTING_PERF";

impl ProjectApplication {
    /// Wraps a prepared runtime. The window opens when winit resumes.
    fn new(title: String, mut runtime: App) -> Self {
        runtime
            .world_mut()
            .resource_mut::<PhysicsBackendStatus>()
            .gpu_dynamic_available = true;
        Self {
            window: WindowRunner::new(title),
            runtime,
            previous_frame: Instant::now(),
            code_reload: None,
            perf: std::env::var_os(PERF_ENV).map(|_| (Instant::now(), 0)),
            perf_spent: [std::time::Duration::ZERO; 2],
        }
    }

    /// Prints `[rusting] perf` once a second when `RUSTING_PERF` is set.
    fn report_perf(&mut self) {
        let Some((since, frames)) = &mut self.perf else {
            return;
        };
        *frames += 1;
        let elapsed = since.elapsed();
        if elapsed < std::time::Duration::from_secs(1) {
            return;
        }
        let fps = f64::from(*frames) / elapsed.as_secs_f64();
        let [update, render] =
            std::mem::take(&mut self.perf_spent).map(|time| time / *frames);
        (*since, *frames) = (Instant::now(), 0);
        if let Some(window) = self.window.windows.get_primary_window() {
            window.set_title(&format!(
                "{} - {fps:.0} fps ({:.2} ms)",
                self.window.title,
                1000.0 / fps
            ));
        }
        let ms = |time: std::time::Duration| time.as_secs_f64() * 1000.0;
        let mut cpu = *self
            .runtime
            .world()
            .resource::<crate::runtime::CpuFrameTimings>();
        let Some(renderer) = self.window.scene_renderer.as_mut() else {
            return;
        };
        renderer.write_cpu_timings(&mut cpu);
        let gpu = renderer.gpu_pass_times();
        let counters = renderer.render_counters();
        let mut line = format!(
            "[rusting] perf {fps:.0} fps ({:.2} ms/frame) | update {:.2} render {:.2} ms | CPU physics {:.2} extract {:.2} prepare {:.2} record {:.2} ms | GPU {:.2} ms",
            1000.0 / fps,
            ms(update),
            ms(render),
            ms(cpu.physics),
            ms(cpu.extraction),
            ms(cpu.preparation),
            ms(cpu.recording),
            ms(gpu.total()),
        );
        for (pass, time) in &gpu.0 {
            line += &format!(" | {} {:.2}", pass.label(), ms(*time));
        }
        line += &format!(
            " | draws {} dispatches {} triangles {}",
            counters.draws, counters.dispatches, counters.triangles
        );
        eprintln!("{line}");
    }
}

impl ApplicationHandler for ProjectApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.window.resumed(event_loop, &mut self.runtime);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // The swapchain probe's hidden window reports events too.
        if self.window.windows.get_renderer(window_id).is_none() {
            return;
        }
        // Presses the UI uses stay out of gameplay; releases always pass so
        // no key or button stays held.
        if self.window.ui_event(window_id, &event)
            && match &event {
                WindowEvent::KeyboardInput { event, .. } => {
                    event.state.is_pressed()
                }
                WindowEvent::MouseInput { state, .. } => state.is_pressed(),
                _ => false,
            }
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.window.resize(window_id);
                self.runtime
                    .world_mut()
                    .resource_mut::<RuntimeInput>()
                    .record_viewport_size([
                        size.width as f32,
                        size.height as f32,
                    ]);
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.window.resize(window_id);
            }
            WindowEvent::Focused(false) => {
                self.runtime
                    .world_mut()
                    .resource_mut::<RuntimeInput>()
                    .release_all();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    self.runtime
                        .world_mut()
                        .resource_mut::<RuntimeInput>()
                        .record_key(code, event.state.is_pressed());
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.runtime
                    .world_mut()
                    .resource_mut::<RuntimeInput>()
                    .record_mouse_button(button, state.is_pressed());
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.runtime
                    .world_mut()
                    .resource_mut::<RuntimeInput>()
                    .record_cursor_position([
                        position.x as f32,
                        position.y as f32,
                    ]);
            }
            WindowEvent::RedrawRequested => {
                // Completed GPU events and synchronized state enter ECS before
                // this frame starts, so Rust update systems can read them.
                let scene_renderer =
                    self.window.scene_renderer.as_mut().unwrap();
                let raw_events = scene_renderer.take_completed_physics_events();
                let states = scene_renderer.take_completed_physics_states();
                let hashes =
                    scene_renderer.take_completed_physics_state_hashes();
                let lost = scene_renderer.take_physics_events_lost();
                if lost > 0 {
                    self.runtime
                        .world_mut()
                        .resource_mut::<EventQueue<GpuPhysicsEventsLost>>()
                        .send(GpuPhysicsEventsLost { count: lost });
                }
                if !raw_events.is_empty() {
                    route_gpu_physics_events(
                        self.runtime.world_mut(),
                        &raw_events,
                    );
                }
                if !states.is_empty() {
                    apply_gpu_state_samples(self.runtime.world_mut(), &states);
                }
                record_gpu_state_hashes(self.runtime.world_mut(), &hashes);
                // Delta time tells gameplay how much real time passed.
                let now = Instant::now();
                let delta = now.saturating_duration_since(self.previous_frame);
                self.previous_frame = now;
                self.window.begin_ui_frame(window_id, &mut self.runtime);
                let update_start = Instant::now();
                let updated = self.runtime.update(delta);
                self.perf_spent[0] += update_start.elapsed();
                if let Err(error) = updated {
                    eprintln!("runtime update failed: {error}");
                    event_loop.exit();
                    return;
                }
                self.window.finish_ui_frame(window_id, &mut self.runtime);
                if let Some(mut assets) =
                    self.runtime.world_mut().get_resource_mut::<AssetServer>()
                {
                    for failure in assets.take_reload_failures() {
                        eprintln!("hot reload failed: {failure}");
                    }
                }
                let mut input =
                    self.runtime.world_mut().resource_mut::<RuntimeInput>();
                input.clear_frame_edges();
                let captured = input.cursor_captured();
                self.window.apply_cursor_capture(captured);
                let render_start = Instant::now();
                let rendered = self.window.render(window_id, &self.runtime);
                self.perf_spent[1] += render_start.elapsed();
                match rendered {
                    Ok(()) => {
                        announce_first_frame();
                        self.report_perf();
                    }
                    Err(error) => {
                        eprintln!("{error}");
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.runtime
                .world_mut()
                .resource_mut::<RuntimeInput>()
                .record_mouse_motion([delta.0 as f32, delta.1 as f32]);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // The egui clipboard talks to the Wayland connection, which closes
        // when the event loop returns; dropping it later crashes the exit.
        #[cfg(feature = "ui")]
        {
            self.window.ui = None;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some((path, requested)) = &self.code_reload {
            if requested.load(Ordering::Relaxed) {
                // The editor rebuilds and starts the game again after this
                // process exits.
                if let Err(error) =
                    save_code_reload_state(&mut self.runtime, path)
                {
                    eprintln!(
                        "{} could not save the scene: {error}",
                        crate::project::CODE_RELOAD_CLEAN_MARKER
                    );
                }
                event_loop.exit();
                return;
            }
        }
        self.window.request_next_frame(event_loop, &self.runtime);
    }
}

/// Opens a window and runs a prepared runtime until the window closes.
/// With [`crate::project::REPLAY_OUT_ENV`] set, records the session and
/// writes the replay there on exit.
pub(crate) fn run_windowed(
    title: String,
    mut runtime: App,
) -> Result<(), Box<dyn Error>> {
    let replay_out = std::env::var_os(crate::project::REPLAY_OUT_ENV);
    if replay_out.is_some() {
        runtime.start_recording();
    }
    let event_loop = EventLoop::new()?;
    let mut application = ProjectApplication::new(title, runtime);
    if let Some(path) = std::env::var_os(crate::project::CODE_RELOAD_STATE_ENV)
    {
        let requested = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&requested);
        std::thread::spawn(move || {
            for line in std::io::stdin().lines().map_while(Result::ok) {
                if line.trim() == crate::project::CODE_RELOAD_SAVE_COMMAND {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        });
        application.code_reload = Some((PathBuf::from(path), requested));
    }
    event_loop.run_app(&mut application)?;
    if let (Some(path), Some(replay)) =
        (replay_out, application.runtime.finish_recording())
    {
        std::fs::write(path, serde_json::to_vec(&replay)?)?;
    }
    Ok(())
}

pub use crate::project::HEADLESS_TICKS_ENV;

/// Set when [`run_project`] starts; first-frame timing counts from here.
static GAME_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Prints [`crate::project::FIRST_FRAME_MARKER`] once, so the editor and
/// `rusting run` can measure save-to-first-playable-frame time.
fn announce_first_frame() {
    static ANNOUNCED: std::sync::Once = std::sync::Once::new();
    ANNOUNCED.call_once(|| {
        let start = GAME_START.get_or_init(Instant::now);
        eprintln!(
            "{} {} ms",
            crate::project::FIRST_FRAME_MARKER,
            start.elapsed().as_millis()
        );
    });
}

/// Runs a cooked scene with a native game-defined Rust plugin.
///
/// With [`HEADLESS_TICKS_ENV`] set, runs that many ticks through
/// [`run_project_headless`] instead of opening a window. With
/// [`crate::scenario::TEST_SCENARIO_ENV`] set, runs that scenario file
/// through [`run_project_scenario`] instead. With
/// [`crate::project::REPLAY_PLAY_ENV`] set, plays that replay headless and
/// fails if it diverges.
///
/// # Arguments
/// * `title` - Text shown in the game window title bar.
/// * `scene_path` - Path to cooked `.rscene.bin` data.
/// * `plugin` - Native Rust systems and resources used by this game.
pub fn run_project<P: Plugin>(
    title: impl Into<String>,
    scene_path: impl Into<PathBuf>,
    plugin: P,
) -> Result<(), Box<dyn Error>> {
    GAME_START.get_or_init(Instant::now);
    if let Some(scenario) = std::env::var_os(crate::scenario::TEST_SCENARIO_ENV)
    {
        let report = std::env::var_os(crate::scenario::TEST_REPORT_ENV);
        return run_project_scenario(
            scene_path,
            plugin,
            PathBuf::from(scenario),
            report.map(PathBuf::from),
        );
    }
    if let Some(ticks) = std::env::var_os(HEADLESS_TICKS_ENV) {
        let ticks = ticks
            .to_str()
            .and_then(|ticks| ticks.parse().ok())
            .ok_or(format!("{HEADLESS_TICKS_ENV} must be a tick count"))?;
        return run_project_headless(scene_path, plugin, ticks);
    }
    let mut runtime = load_project_runtime(&scene_path.into(), plugin)?;
    if let Some(path) = std::env::var_os(crate::project::REPLAY_PLAY_ENV) {
        let replay: crate::runtime::Replay =
            serde_json::from_slice(&std::fs::read(path)?)?;
        return match crate::runtime::play_replay(&mut runtime, &replay)? {
            None => Ok(()),
            Some(tick) => Err(format!(
                "replay diverges from the recording at tick {tick}"
            )
            .into()),
        };
    }
    if let Some(path) = std::env::var_os(crate::project::CODE_RELOAD_STATE_ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        match restore_code_reload_state(&mut runtime, &path) {
            Ok(()) => eprintln!("{}", crate::project::CODE_RELOAD_KEPT_MARKER),
            Err(error) => eprintln!(
                "{} the saved scene does not fit the new code: {error}",
                crate::project::CODE_RELOAD_CLEAN_MARKER
            ),
        }
        // A crash on the next run must not restore this state again.
        let _ = std::fs::remove_file(&path);
    }
    run_windowed(title.into(), runtime)
}

/// Runs a cooked scene for a fixed number of ticks with no window, Vulkan
/// device, or renderer, then returns. For CI and machines with no display.
/// With [`crate::project::STATE_HASH_OUT_ENV`] set, writes the run's
/// [`crate::runtime::StateHashReport`] to that file. With
/// [`crate::project::FINAL_SCENE_OUT_ENV`] set, saves the scene as it stands
/// after the last tick to that file.
///
/// # Arguments
/// * `scene_path` - Path to cooked `.rscene.bin` data.
/// * `plugin` - Native Rust systems and resources used by this game.
/// * `ticks` - Number of fixed-step updates to run before returning.
pub fn run_project_headless<P: Plugin>(
    scene_path: impl Into<PathBuf>,
    plugin: P,
    ticks: u32,
) -> Result<(), Box<dyn Error>> {
    let (mut runtime, report, elapsed) =
        simulate_timed(scene_path.into(), plugin, ticks)?;
    eprintln!(
        "{} {:.3}",
        crate::project::TICK_TIME_MARKER,
        elapsed.as_secs_f64() * 1000.0 / f64::from(ticks.max(1))
    );
    if let Some(path) = std::env::var_os(crate::project::FINAL_SCENE_OUT_ENV) {
        crate::runtime::save_scene(
            runtime.world_mut(),
            path,
            format!("after tick {ticks}"),
        )?;
    }
    if let Some(path) = std::env::var_os(crate::project::STATE_HASH_OUT_ENV) {
        std::fs::write(path, serde_json::to_vec(&report)?)?;
    }
    Ok(())
}

/// Loads a cooked scene and runs `ticks` updates of one `fixed_delta` each
/// (one fixed step each at time scale 1) with no window, surface, Vulkan
/// device, or renderer, then returns the runtime and a report of every
/// tick's state hash (`StateHashes` keeps only the recent ones). GPU-class
/// bodies do not move without a renderer; a warning names how many there
/// are.
pub fn simulate_project_headless<P: Plugin>(
    scene_path: impl Into<PathBuf>,
    plugin: P,
    ticks: u32,
) -> Result<(App, crate::runtime::StateHashReport), Box<dyn Error>> {
    let (runtime, report, _) =
        simulate_timed(scene_path.into(), plugin, ticks)?;
    Ok((runtime, report))
}

/// [`simulate_project_headless`] plus the wall time of the tick loop alone,
/// without the scene load.
fn simulate_timed<P: Plugin>(
    scene_path: PathBuf,
    plugin: P,
    ticks: u32,
) -> Result<
    (App, crate::runtime::StateHashReport, std::time::Duration),
    Box<dyn Error>,
> {
    let mut runtime = load_project_runtime(&scene_path, plugin)?;
    let world = runtime.world_mut();
    let gpu_bodies = world
        .query::<&crate::runtime::PhysicsBody>()
        .iter(world)
        .filter(|body| body.uses_gpu())
        .count();
    if gpu_bodies > 0 {
        eprintln!(
            "headless run: {gpu_bodies} GPU physics bodies stay still without a renderer"
        );
    }

    let delta = runtime.world().resource::<FrameTime>().fixed_delta;
    let mut hashes = Vec::with_capacity(ticks as usize);
    let start = Instant::now();
    for _ in 0..ticks {
        runtime.update(delta)?;
        announce_first_frame();
        hashes.extend(
            runtime
                .world()
                .get_resource::<crate::runtime::StateHashes>()
                .and_then(|recorded| recorded.recent.back().copied()),
        );
    }
    let elapsed = start.elapsed();
    let entities =
        crate::runtime::named_entity_state_hashes(runtime.world_mut());
    let report = crate::runtime::StateHashReport {
        ticks: hashes,
        entities,
    };
    Ok((runtime, report, elapsed))
}

/// Runs a [`crate::scenario::Scenario`] file against a cooked scene with no
/// window. Captures render offscreen when Vulkan is available. Writes the
/// [`crate::scenario::ScenarioReport`] as JSON to `report_path`, or to
/// standard output without one, and fails when the scenario fails.
pub fn run_project_scenario<P: Plugin>(
    scene_path: impl Into<PathBuf>,
    plugin: P,
    scenario_path: PathBuf,
    report_path: Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let scenario: crate::scenario::Scenario =
        serde_json::from_str(&std::fs::read_to_string(&scenario_path)?)?;
    let mut runtime = load_project_runtime(&scene_path.into(), plugin)?;
    let base = scenario_path.parent().unwrap_or(Path::new("."));
    let report = crate::scenario::run_scenario(&mut runtime, &scenario, base);
    let text = serde_json::to_string_pretty(&report)?;
    match report_path {
        Some(path) => std::fs::write(path, text)?,
        None => println!("{text}"),
    }
    match report.first_failure {
        None => Ok(()),
        Some(failure) => Err(format!(
            "scenario `{}` failed at tick {} step {}: {}",
            report.name, failure.tick, failure.step, failure.message
        )
        .into()),
    }
}

/// Runs a cooked scene using one short native Rust update function.
///
/// # Arguments
/// * `scene_path` - Path to cooked `.rscene.bin` data.
/// * `update` - Function called once per rendered frame.
pub fn run_game(
    scene_path: impl Into<PathBuf>,
    update: GameUpdate,
) -> GameResult {
    run_project(
        "RustingEngine Game",
        scene_path,
        SimpleGamePlugin { update },
    )
}

/// Finds cooked data beside an exported executable, then falls back to the
/// Cargo project path used during development.
#[must_use]
pub fn resolve_game_scene_path(
    relative: impl AsRef<std::path::Path>,
    project_root: impl AsRef<std::path::Path>,
) -> PathBuf {
    let relative = relative.as_ref();
    if let Some(path) = std::env::var_os("RUSTING_SCENE_PATH") {
        return PathBuf::from(path);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(folder) = executable.parent() {
            let packaged = folder.join(relative);
            if packaged.is_file() {
                return packaged;
            }
        }
    }
    project_root.as_ref().join(relative)
}

/// Generates the native game entry point while keeping gameplay in normal
/// Rust. The scene path is relative to the game project's `Cargo.toml`.
#[macro_export]
macro_rules! rusting_game {
    ($update:path) => {
        $crate::rusting_game!("build/main.rscene.bin", $update);
    };
    ($scene:literal, $update:path) => {
        fn main() -> $crate::project_runner::GameResult {
            let scene = $crate::project_runner::resolve_game_scene_path(
                $scene,
                env!("CARGO_MANIFEST_DIR"),
            );
            $crate::project_runner::run_game(scene, $update)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_simulation_runs_exact_ticks_from_a_cooked_scene() {
        let directory = std::env::temp_dir()
            .join(format!("rusting-headless-{}", uuid::Uuid::new_v4()));
        let source = directory.join("main.rscene");
        let cooked = directory.join("main.rscene.bin");
        let mut editor = App::new();
        editor.add_plugin(AssetPlugin).unwrap();
        editor.spawn((
            Name("Ball".into()),
            Transform::new([0.0, 5.0, 0.0]),
            crate::runtime::PhysicsBody::default(),
            crate::runtime::RigidBody::default(),
            crate::runtime::Collider::default(),
        ));
        crate::runtime::save_scene(editor.world_mut(), &source, "main")
            .unwrap();
        crate::runtime::cook_scene(&source, &cooked).unwrap();
        fn idle(_: &mut GameScene<'_>, _: &FrameTime) {}
        let (mut runtime, report) = simulate_project_headless(
            &cooked,
            SimpleGamePlugin { update: idle },
            30,
        )
        .unwrap();
        std::fs::remove_dir_all(&directory).unwrap();

        assert_eq!(runtime.world().resource::<FrameTime>().fixed_tick, 30);
        let hashes = &runtime
            .world()
            .resource::<crate::runtime::StateHashes>()
            .recent;
        assert_eq!(hashes.len(), 30);
        assert!(hashes.iter().eq(&report.ticks));
        assert_eq!(hashes.back().unwrap().0, 30);
        let mut scene = GameScene {
            world: runtime.world_mut(),
        };
        assert!(scene.object("Ball").position()[1] < 5.0);
    }

    #[derive(
        bevy_ecs::component::Component,
        Clone,
        Default,
        serde::Serialize,
        serde::Deserialize,
    )]
    struct Health {
        hp: i32,
    }

    crate::reflect! {
        struct Health {
            hp: i32,
        }
    }

    /// The same component after a change that old saved values do not fit.
    #[derive(
        bevy_ecs::component::Component,
        Clone,
        Default,
        serde::Serialize,
        serde::Deserialize,
    )]
    struct HealthText {
        hp: String,
    }

    crate::reflect! {
        struct HealthText {
            hp: String,
        }
    }

    /// One build of a test game: its update function and its `Health`.
    struct TestGame {
        update: GameUpdate,
        changed_health: bool,
    }

    impl Plugin for TestGame {
        fn build(&self, app: &mut App) -> Result<(), AppError> {
            let registered = if self.changed_health {
                app.register_scene_component::<HealthText>("test.health")
                    .map(drop)
            } else {
                app.register_scene_component::<Health>("test.health")
                    .map(drop)
            };
            registered.map_err(|error| AppError::PluginSetup {
                plugin: "TestGame",
                message: error.to_string(),
            })?;
            app.add_plugin(SimpleGamePlugin {
                update: self.update,
            })?;
            app.add_system(ScheduleStage::Startup, |world: &mut World| {
                world.spawn((
                    crate::runtime::SceneId::new(),
                    Name("Marker".into()),
                ));
            });
            Ok(())
        }
    }

    fn count_named(world: &mut World, name: &str) -> usize {
        let mut query = world.query::<&Name>();
        query.iter(world).filter(|named| named.0 == name).count()
    }

    #[test]
    fn code_reload_keeps_the_scene_and_runs_the_new_code() {
        let directory = std::env::temp_dir()
            .join(format!("rusting-code-reload-{}", uuid::Uuid::new_v4()));
        let source = directory.join("main.rscene");
        let cooked = directory.join("main.rscene.bin");
        let state = directory.join("state.json");
        let mut editor = App::new();
        editor.add_plugin(AssetPlugin).unwrap();
        editor.spawn((Name("Ball".into()), Transform::new([0.0, 0.0, 0.0])));
        crate::runtime::save_scene(editor.world_mut(), &source, "main")
            .unwrap();
        crate::runtime::cook_scene(&source, &cooked).unwrap();

        // The old code spawns a coin, gives the ball health, then moves the
        // ball right and hurts it every frame.
        fn old_code(scene: &mut GameScene<'_>, _: &FrameTime) {
            scene.once("setup", |scene| {
                scene.spawn_cube(
                    "Coin",
                    Transform::default(),
                    &CubeSpawn::new(),
                );
                let ball = find_named_entity(scene.world, "Ball").unwrap();
                scene.world.entity_mut(ball).insert(Health { hp: 10 });
            });
            scene.object("Ball").move_x(1.0);
            let ball = find_named_entity(scene.world, "Ball").unwrap();
            scene.world.get_mut::<Health>(ball).unwrap().hp -= 1;
        }
        // The changed system moves it up instead. Its setup runs only on a
        // clean start.
        fn new_code(scene: &mut GameScene<'_>, _: &FrameTime) {
            scene.once("setup", |scene| {
                scene.spawn_cube(
                    "Fresh",
                    Transform::default(),
                    &CubeSpawn::new(),
                );
            });
            scene.object("Ball").move_y(1.0);
        }
        let frame = std::time::Duration::from_millis(16);
        let mut old = load_project_runtime(
            &cooked,
            TestGame {
                update: old_code,
                changed_health: false,
            },
        )
        .unwrap();
        for _ in 0..3 {
            old.update(frame).unwrap();
        }
        save_code_reload_state(&mut old, &state).unwrap();

        let mut new = load_project_runtime(
            &cooked,
            TestGame {
                update: new_code,
                changed_health: false,
            },
        )
        .unwrap();
        restore_code_reload_state(&mut new, &state).unwrap();
        for _ in 0..2 {
            new.update(frame).unwrap();
        }
        let world = new.world_mut();
        let ball = find_named_entity(world, "Ball").unwrap();
        assert_eq!(
            world.get::<Transform>(ball).unwrap().position,
            [3.0, 2.0, 0.0]
        );
        assert_eq!(world.get::<Health>(ball).unwrap().hp, 7);
        assert_eq!(count_named(world, "Coin"), 1);
        assert_eq!(count_named(world, "Fresh"), 0);
        // Startup ran in the old process only.
        assert_eq!(count_named(world, "Marker"), 1);

        // Saved values that do not fit the changed component start clean.
        let mut changed = load_project_runtime(
            &cooked,
            TestGame {
                update: new_code,
                changed_health: true,
            },
        )
        .unwrap();
        let error = restore_code_reload_state(&mut changed, &state)
            .unwrap_err()
            .to_string();
        assert!(error.contains("test.health"), "{error}");
        changed.update(frame).unwrap();
        let world = changed.world_mut();
        let ball = find_named_entity(world, "Ball").unwrap();
        assert_eq!(
            world.get::<Transform>(ball).unwrap().position,
            [0.0, 1.0, 0.0]
        );
        assert_eq!(count_named(world, "Coin"), 0);
        assert_eq!(count_named(world, "Fresh"), 1);
        assert_eq!(count_named(world, "Marker"), 1);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn restart_puts_the_starting_scene_back_and_reruns_setup() {
        let directory = std::env::temp_dir()
            .join(format!("rusting-restart-{}", uuid::Uuid::new_v4()));
        let source = directory.join("main.rscene");
        let cooked = directory.join("main.rscene.bin");
        let mut editor = App::new();
        editor.add_plugin(AssetPlugin).unwrap();
        editor.spawn((Name("Ball".into()), Transform::new([0.0, 0.0, 0.0])));
        editor.spawn((Name("Gate".into()), Transform::default()));
        crate::runtime::save_scene(editor.world_mut(), &source, "main")
            .unwrap();
        crate::runtime::cook_scene(&source, &cooked).unwrap();

        // Setup spawns a coin; the ball moves right each frame, the gate is
        // removed, and the third frame restarts.
        fn code(scene: &mut GameScene<'_>, _: &FrameTime) {
            scene.once("setup", |scene| {
                scene.spawn_cube(
                    "Coin",
                    Transform::default(),
                    &CubeSpawn::new(),
                );
            });
            scene.despawn("Gate");
            scene.object("Ball").move_x(1.0);
            if scene.object("Ball").position()[0] >= 3.0 {
                assert!(scene.restart());
            }
        }
        let frame = std::time::Duration::from_millis(16);
        let mut game = load_project_runtime(
            &cooked,
            TestGame {
                update: code,
                changed_health: false,
            },
        )
        .unwrap();
        for _ in 0..3 {
            game.update(frame).unwrap();
        }
        let world = game.world_mut();
        let ball = find_named_entity(world, "Ball").unwrap();
        assert_eq!(world.get::<Transform>(ball).unwrap().position, [0.0; 3]);
        assert_eq!(count_named(world, "Gate"), 1);
        assert_eq!(count_named(world, "Coin"), 0);
        game.update(frame).unwrap();
        let world = game.world_mut();
        assert_eq!(count_named(world, "Coin"), 1, "setup ran again");
        assert_eq!(count_named(world, "Gate"), 0);
        let ball = find_named_entity(world, "Ball").unwrap();
        assert_eq!(world.get::<Transform>(ball).unwrap().position[0], 1.0);

        let mut world = World::new();
        assert!(!GameScene { world: &mut world }.restart());
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn body_calls_stop_spin_and_reset_bodies_and_set_look_turns_the_player() {
        use crate::runtime::{PlayerController, RigidBody, RigidBodyKind};
        let mut world = World::new();
        let crate_entity = world
            .spawn((
                Name("Crate".into()),
                Transform::default(),
                RigidBody {
                    kind: RigidBodyKind::Dynamic,
                    ..RigidBody::default()
                },
                crate::runtime::Sleeping,
            ))
            .id();
        world.spawn((
            Name("Player".into()),
            Transform::default(),
            PlayerController::default(),
        ));
        let mut scene = GameScene { world: &mut world };
        assert!(scene.set_angular_velocity("Crate", [0.0, 2.0, 0.0]));
        assert_eq!(scene.angular_velocity("Crate"), Some([0.0, 2.0, 0.0]));
        assert!(scene.set_linear_velocity("Crate", [1.0, 0.0, 0.0]));
        // Becoming kinematic freezes the body instead of carrying it off.
        assert!(scene.set_body_kind("Crate", RigidBodyKind::Kinematic));
        assert_eq!(scene.linear_velocity("Crate"), Some([0.0; 3]));
        assert_eq!(scene.angular_velocity("Crate"), Some([0.0; 3]));
        assert!(scene.set_body_kind("Crate", RigidBodyKind::Dynamic));
        assert!(scene.set_angular_velocity("Crate", [3.0, 0.0, 0.0]));
        assert!(scene.set_linear_velocity("Crate", [0.0, 5.0, 0.0]));
        assert!(scene.reset_body("Crate"));
        assert_eq!(scene.linear_velocity("Crate"), Some([0.0; 3]));
        assert_eq!(scene.angular_velocity("Crate"), Some([0.0; 3]));
        assert!(!scene.reset_body("Nothing"));
        assert!(scene
            .world()
            .get::<crate::runtime::Sleeping>(crate_entity)
            .is_none());

        assert!(scene.set_look("Player", 1.5, -0.25));
        assert!(!scene.set_look("Crate", 0.0, 0.0), "no controller");
        let mut players = scene.world().query::<&PlayerController>();
        let player = players.single(scene.world()).unwrap();
        assert_eq!((player.yaw, player.pitch), (1.5, -0.25));
    }

    #[test]
    fn restart_repeats_a_physics_pile_exactly() {
        use crate::runtime::{
            Collider, ColliderShape, PhysicsBody, RigidBody, RigidBodyKind,
            SimulationClass,
        };
        let directory = std::env::temp_dir()
            .join(format!("rusting-restart-pile-{}", uuid::Uuid::new_v4()));
        let source = directory.join("main.rscene");
        let cooked = directory.join("main.rscene.bin");
        let mut editor = App::new();
        editor.add_plugin(AssetPlugin).unwrap();
        let body = |kind| {
            (
                PhysicsBody {
                    simulation: SimulationClass::Cpu,
                    ..PhysicsBody::default()
                },
                RigidBody {
                    kind,
                    ..RigidBody::default()
                },
                Collider {
                    shape: ColliderShape::Box {
                        half_extents: [0.5; 3],
                    },
                    ..Collider::default()
                },
            )
        };
        let mut floor = Transform::new([0.0, -0.5, 0.0]);
        floor.scale = [20.0, 1.0, 20.0];
        editor.spawn((Name("Floor".into()), floor, body(RigidBodyKind::Fixed)));
        for index in 0..12 {
            let offset = index as f32;
            editor.spawn((
                Name(format!("Box {index}")),
                crate::runtime::ObjectClasses::new(["box"]),
                Transform::new([
                    (offset * 0.37).sin() * 0.4,
                    0.6 + offset * 1.05,
                    (offset * 0.61).cos() * 0.4,
                ]),
                body(RigidBodyKind::Dynamic),
            ));
        }
        crate::runtime::save_scene(editor.world_mut(), &source, "main")
            .unwrap();
        // Patched files list objects in creation order, not by id, while a
        // restart reloads a capture sorted by id.
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
        json["entities"].as_array_mut().unwrap().reverse();
        std::fs::write(&source, json.to_string()).unwrap();
        crate::runtime::cook_scene(&source, &cooked).unwrap();

        fn code(_: &mut GameScene<'_>, _: &FrameTime) {}
        let mut game = load_project_runtime(
            &cooked,
            TestGame {
                update: code,
                changed_health: false,
            },
        )
        .unwrap();
        let frame = game.world().resource::<FrameTime>().fixed_delta;
        let run = |game: &mut App| {
            for _ in 0..90 {
                game.update(frame).unwrap();
            }
            let world = game.world_mut();
            let hash = GameScene { world: &mut *world }.state_hash("box");
            let poses = (0..12)
                .map(|index| {
                    let entity =
                        find_named_entity(world, &format!("Box {index}"))
                            .unwrap();
                    let transform = world.get::<Transform>(entity).unwrap();
                    (transform.position, transform.rotation)
                })
                .collect::<Vec<_>>();
            (hash, poses)
        };
        let start = GameScene {
            world: game.world_mut(),
        }
        .state_hash("box");
        let first = run(&mut game);
        let mut scene = GameScene {
            world: game.world_mut(),
        };
        let initial = scene.initial("Box 11").unwrap();
        assert_eq!(
            initial.transform.position,
            [
                (11.0f32 * 0.37).sin() * 0.4,
                0.6 + 11.0 * 1.05,
                (11.0f32 * 0.61).cos() * 0.4
            ]
        );
        assert_eq!(initial.body_kind, Some(RigidBodyKind::Dynamic));
        assert!(initial.visible);
        assert_eq!(scene.initial("Nothing"), None);
        assert!(scene.restart());
        let second = run(&mut game);
        assert_ne!(first.0, start, "the pile moved");
        assert_eq!(first, second, "tolerance 0, and equal class hashes");

        // A snapshot taken mid-fall, with warm starts and some sleepers,
        // resumes exactly as the run that went on from it.
        for _ in 0..20 {
            game.update(frame).unwrap();
        }
        let snapshot = GameScene {
            world: game.world_mut(),
        }
        .snapshot()
        .unwrap();
        let went_on = run(&mut game);
        GameScene {
            world: game.world_mut(),
        }
        .restore(&snapshot)
        .unwrap();
        assert_eq!(run(&mut game), went_on, "restored run matches");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn load_scene_switches_to_another_project_scene() {
        let directory = std::env::temp_dir()
            .join(format!("rusting-load-scene-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(directory.join("levels")).unwrap();
        std::fs::write(directory.join("project.json"), "{}").unwrap();
        let source = directory.join("main.rscene");
        let cooked = directory.join("build/main.rscene.bin");
        let mut editor = App::new();
        editor.add_plugin(AssetPlugin).unwrap();
        let ball = editor.spawn((Name("Ball".into()), Transform::default()));
        crate::runtime::save_scene(editor.world_mut(), &source, "main")
            .unwrap();
        crate::runtime::cook_scene(&source, &cooked).unwrap();
        editor.world_mut().despawn(ball);
        editor.spawn((Name("Door".into()), Transform::default()));
        crate::runtime::save_scene(
            editor.world_mut(),
            directory.join("levels/2.rscene"),
            "level 2",
        )
        .unwrap();

        // The first level loads the second at once; the second spawns a coin
        // once per round and restarts when the door has moved twice.
        fn code(scene: &mut GameScene<'_>, _: &FrameTime) {
            if scene.try_object("Ball").is_some() {
                assert!(scene.load_scene("levels/missing.rscene").is_err());
                assert!(scene.try_object("Ball").is_some(), "scene kept");
                scene.load_scene("levels/2.rscene").unwrap();
                return;
            }
            scene.once("setup", |scene| {
                scene.spawn_cube(
                    "Coin",
                    Transform::default(),
                    &CubeSpawn::new(),
                );
            });
            scene.object("Door").move_x(1.0);
            if scene.object("Door").position()[0] >= 2.0 {
                assert!(scene.restart());
            }
        }
        let frame = std::time::Duration::from_millis(16);
        let mut game = load_project_runtime(
            &cooked,
            TestGame {
                update: code,
                changed_health: false,
            },
        )
        .unwrap();
        game.update(frame).unwrap();
        let world = game.world_mut();
        assert_eq!(count_named(world, "Ball"), 0);
        assert_eq!(count_named(world, "Door"), 1);
        game.update(frame).unwrap();
        assert_eq!(count_named(game.world_mut(), "Coin"), 1);
        // Restarting returns to the second level, not the first.
        game.update(frame).unwrap();
        let world = game.world_mut();
        assert_eq!(count_named(world, "Ball"), 0);
        assert_eq!(count_named(world, "Coin"), 0);
        let door = find_named_entity(world, "Door").unwrap();
        assert_eq!(world.get::<Transform>(door).unwrap().position, [0.0; 3]);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn named_game_object_moves_without_an_ecs_query_in_game_code() {
        let mut world = World::new();
        world.spawn((Name("Orange Cube".into()), Transform::default()));

        let mut scene = GameScene { world: &mut world };
        scene
            .object("Orange Cube")
            .move_x(2.0)
            .move_y(3.0)
            .move_z(4.0);

        assert_eq!(scene.object("Orange Cube").position(), [2.0, 3.0, 4.0]);
    }

    #[test]
    fn despawned_name_can_be_spawned_again() {
        let mut world = World::new();
        world.insert_resource(AssetServer::default());
        let mut scene = GameScene { world: &mut world };
        let first =
            scene.spawn_cube("Shot", Transform::default(), &CubeSpawn::new());
        scene.world.despawn(first);
        let second =
            scene.spawn_cube("Shot", Transform::default(), &CubeSpawn::new());
        assert_ne!(first, second);
    }

    #[test]
    fn game_randomness_repeats_for_a_seed_and_varies_by_tick_and_stream() {
        let draw = |seed: u64, tick: u64, stream: u64| {
            let mut world = World::new();
            world.insert_resource(crate::runtime::RandomSeed(seed));
            world.insert_resource(FrameTime {
                fixed_tick: tick,
                ..FrameTime::default()
            });
            GameScene { world: &mut world }.random(stream)
        };
        let value = draw(7, 3, 0);
        assert!((0.0..1.0).contains(&value));
        assert_eq!(value, draw(7, 3, 0));
        assert_ne!(value, draw(8, 3, 0));
        assert_ne!(value, draw(7, 4, 0));
        assert_ne!(value, draw(7, 3, 1));
        // A world without a seed or time still draws.
        let mut world = World::new();
        let _ = GameScene { world: &mut world }.random(0);
    }

    #[test]
    fn game_code_reads_and_sets_counters_by_name() {
        let mut world = World::new();
        world.spawn(crate::runtime::Counter {
            name: "gems".into(),
            value: 2,
            target: Some(5),
        });
        let mut scene = GameScene { world: &mut world };
        scene.counter("gems").unwrap().value += 3;
        assert!(scene.counter("gems").unwrap().complete());
        assert!(scene.counter("coins").is_none());
        // The shorthands read, add and check without a borrow held open.
        assert_eq!(scene.add_to_counter("gems", -1), 4);
        assert_eq!(scene.counter_value("gems"), 4);
        assert!(!scene.counter_complete("gems"));
        assert_eq!(scene.add_to_counter("coins", 1), 0);
        scene.set_counter("gems", 9);
        assert_eq!(scene.counter_value("gems"), 9);
        scene.set_counter("coins", 3);
        assert_eq!(scene.counter_value("coins"), 0);
        assert!(!scene.counter_complete("coins"));
    }

    #[test]
    fn game_code_recolors_one_object_without_touching_shared_materials() {
        let mut app = crate::App::new();
        app.add_plugin(crate::AssetPlugin).unwrap();
        let world = app.world_mut();
        let (mesh, material) = {
            let assets = world.resource::<AssetServer>();
            (assets.fallback_mesh, assets.fallback_material)
        };
        let renderer = MeshRenderer {
            mesh,
            material,
            cast_shadows: true,
            receive_shadows: true,
        };
        world.spawn((Name("A".into()), Transform::default(), renderer));
        world.spawn((Name("B".into()), Transform::default(), renderer));
        let count =
            |world: &World| world.resource::<AssetServer>().materials.len();
        let before = count(world);
        let mut scene = GameScene { world };
        let original = scene.color("A").unwrap();
        scene.set_color("A", [1.0, 0.0, 0.0, 1.0]);
        scene.set_emissive("A", [2.0, 0.0, 0.0]);
        assert_eq!(scene.color("A"), Some([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(scene.color("B"), Some(original));
        assert_eq!(scene.color("Nowhere"), None);
        // Recoloring B the same way shares A's material.
        scene.set_color("B", [1.0, 0.0, 0.0, 1.0]);
        scene.set_emissive("B", [2.0, 0.0, 0.0]);
        let material = |scene: &mut GameScene<'_>, name| {
            let entity = find_named_entity(scene.world, name).unwrap();
            scene.world.get::<MeshRenderer>(entity).unwrap().material
        };
        assert_eq!(material(&mut scene, "A"), material(&mut scene, "B"));
        assert_eq!(count(scene.world), before + 2);
    }

    #[test]
    fn game_code_reads_and_writes_tile_map_cells_by_world_position() {
        let mut world = World::new();
        world.spawn((
            Name("Level".into()),
            Transform::new([-2.0, 1.0, 0.0]),
            crate::runtime::TileMap {
                rows: vec!["#.o".into()],
                ..Default::default()
            },
        ));
        let mut scene = GameScene { world: &mut world };
        assert_eq!(scene.tile("Level", [-1.5, 0.5, 3.0]), Some('#'));
        assert_eq!(scene.tile("Level", [0.2, 0.9, 0.0]), Some('o'));
        assert_eq!(scene.tile("Level", [5.0, -4.0, 0.0]), Some('.'));
        assert_eq!(scene.tile("Level", [-2.5, 0.5, 0.0]), None);
        assert_eq!(scene.tile("Nowhere", [0.0; 3]), None);
        assert!(scene.set_tile("Level", [0.2, 0.9, 0.0], '*'));
        assert!(!scene.set_tile("Level", [0.2, 0.9, 0.0], '*'));
        assert!(scene.set_tile("Level", [-1.5, -0.5, 0.0], '#'));
        assert!(!scene.set_tile("Level", [-2.5, 0.5, 0.0], '#'));
        let map = world
            .query::<&crate::runtime::TileMap>()
            .single(&world)
            .unwrap();
        assert_eq!(map.rows, vec!["#.*", "#"]);
    }

    #[test]
    fn aim_hits_what_the_camera_faces_and_despawn_removes_it() {
        let mut app = App::new();
        let world = app.world_mut();
        let player = world
            .spawn((Name("Player".into()), Transform::new([0.0, 1.0, 0.0])))
            .id();
        let camera = world
            .spawn((
                Name("Camera".into()),
                // Turned 90 degrees left, so it faces -X.
                Transform::new([0.0, 0.5, 0.0]).with_rotation(
                    0.0,
                    std::f32::consts::FRAC_PI_2,
                    0.0,
                ),
                crate::runtime::Camera {
                    active: true,
                    ..Default::default()
                },
            ))
            .id();
        rusting_core::hierarchy::set_parent(world, camera, player).unwrap();
        for (name, x) in [("Left", -5.0), ("Right", 5.0)] {
            world.spawn((
                Name(name.into()),
                Transform::new([x, 1.5, 0.0]),
                crate::runtime::PhysicsBody::default(),
                crate::runtime::RigidBody {
                    kind: crate::runtime::RigidBodyKind::Fixed,
                    ..Default::default()
                },
                crate::runtime::Collider::default(),
            ));
        }
        let step = app.world().resource::<FrameTime>().fixed_delta;
        app.update(step).unwrap();
        let mut scene = GameScene {
            world: app.world_mut(),
        };
        let hit = scene.aim(20.0).expect("the camera faces Left");
        assert_eq!(hit.name, "Left");
        assert!((hit.point[0] + 4.5).abs() < 1e-3, "{hit:?}");
        assert!(scene.despawn("Left"));
        assert!(!scene.despawn("Left"));
        assert!(scene.try_object("Left").is_none());
    }

    #[test]
    fn a_kinematic_template_copy_launches_along_the_camera_ray() {
        let mut app = App::new();
        let world = app.world_mut();
        world.spawn((
            Name("Camera".into()),
            // Turned 90 degrees left, so it faces -X.
            Transform::new([0.0, 2.0, 0.0]).with_rotation(
                0.0,
                std::f32::consts::FRAC_PI_2,
                0.0,
            ),
            crate::runtime::Camera {
                active: true,
                ..Default::default()
            },
        ));
        world.spawn((
            crate::runtime::SceneId::new(),
            Name("Ball".into()),
            Transform::new([0.0, -30.0, 0.0]),
            crate::runtime::PhysicsBody::default(),
            crate::runtime::RigidBody {
                kind: crate::runtime::RigidBodyKind::Kinematic,
                ..Default::default()
            },
            crate::runtime::Collider {
                shape: crate::runtime::ColliderShape::Sphere { radius: 0.2 },
                ..Default::default()
            },
        ));
        let step = app.world().resource::<FrameTime>().fixed_delta;
        let mut scene = GameScene {
            world: app.world_mut(),
        };
        let (origin, forward) = scene.camera_ray().unwrap();
        assert!((origin[1] - 2.0).abs() < 1e-5, "{origin:?}");
        assert!((forward[0] + 1.0).abs() < 1e-5, "{forward:?}");
        scene.spawn_copy("Ball", "Ball 1", origin);
        assert!(scene
            .set_body_kind("Ball 1", crate::runtime::RigidBodyKind::Dynamic));
        assert!(scene.set_linear_velocity("Ball 1", forward.map(|v| v * 10.0)));
        assert_eq!(
            scene.linear_velocity("Ball 1"),
            Some(forward.map(|v| v * 10.0))
        );
        assert_eq!(scene.linear_velocity("Nothing"), None);
        assert!(!scene
            .set_body_kind("Nothing", crate::runtime::RigidBodyKind::Dynamic));
        for _ in 0..30 {
            app.update(step).unwrap();
        }
        let mut scene = GameScene {
            world: app.world_mut(),
        };
        let ball = scene.object("Ball 1").position();
        assert!(ball[0] < -4.0 && ball[1] < 2.0, "flew and fell: {ball:?}");
        assert_eq!(scene.object("Ball").position(), [0.0, -30.0, 0.0]);
    }

    #[test]
    fn pointer_ray_goes_through_the_cursor() {
        let mut app = App::new();
        app.world_mut().spawn((
            Transform::new([0.0, 10.0, 0.0]).with_rotation(
                -std::f32::consts::FRAC_PI_2,
                0.0,
                0.0,
            ),
            crate::runtime::Camera {
                active: true,
                ..Default::default()
            },
        ));
        let mut scene = GameScene {
            world: app.world_mut(),
        };
        assert_eq!(scene.pointer_ray(), None, "no cursor yet");
        let mut input = scene.world.resource_mut::<RuntimeInput>();
        input.record_viewport_size([800.0, 600.0]);
        input.record_cursor_position([400.0, 300.0]);
        let (origin, down) = scene.pointer_ray().unwrap();
        assert!((origin[0].abs() + origin[2].abs()) < 1e-4, "{origin:?}");
        assert!((down[1] + 1.0).abs() < 1e-4, "{down:?}");
        scene
            .world
            .resource_mut::<RuntimeInput>()
            .record_cursor_position([800.0, 300.0]);
        let (_, right) = scene.pointer_ray().unwrap();
        assert!(right[0] > 0.1 && right[1] < 0.0, "{right:?}");
    }

    #[test]
    fn spawn_copy_clones_the_template_tree_and_in_class_lists_it() {
        let mut app = App::new();
        let world = app.world_mut();
        let arena = world
            .spawn((Name("Arena".into()), Transform::default()))
            .id();
        let template = world
            .spawn((
                crate::runtime::SceneId::new(),
                Name("Ember".into()),
                Transform::new([0.0, -50.0, 0.0]),
                crate::runtime::ObjectClasses {
                    names: vec!["ember".into()],
                },
                crate::runtime::PhysicsBody::default(),
                crate::runtime::Collider::default(),
                crate::runtime::Visibility { visible: false },
            ))
            .id();
        let glow = world
            .spawn((Name("Glow".into()), Transform::new([0.0, 1.0, 0.0])))
            .id();
        rusting_core::hierarchy::set_parent(world, glow, template).unwrap();
        rusting_core::hierarchy::set_parent(world, template, arena).unwrap();
        let mut scene = GameScene {
            world: app.world_mut(),
        };
        let copy = scene
            .spawn_copy("Ember", "Ember 1", [3.0, 0.5, 0.0])
            .unwrap();
        scene.set_visible("Ember 1", true);
        assert!(scene.spawn_copy("Missing", "Ember 2", [0.0; 3]).is_none());
        assert_eq!(scene.in_class("ember"), ["Ember", "Ember 1"]);
        assert_eq!(scene.object("Ember 1").position(), [3.0, 0.5, 0.0]);
        assert_eq!(scene.object("Ember").position(), [0.0, -50.0, 0.0]);
        let world = app.world();
        assert!(world.get::<crate::runtime::Collider>(copy).is_some());
        assert!(
            world
                .get::<crate::runtime::Visibility>(copy)
                .unwrap()
                .visible
        );
        assert!(
            !world
                .get::<crate::runtime::Visibility>(template)
                .unwrap()
                .visible
        );
        assert_ne!(
            world.get::<crate::runtime::SceneId>(copy),
            world.get::<crate::runtime::SceneId>(template)
        );
        assert_eq!(world.get::<crate::runtime::Parent>(copy).unwrap().0, arena);
        assert_eq!(
            world
                .get::<crate::runtime::Children>(arena)
                .unwrap()
                .0
                .len(),
            2
        );
        let children = &world.get::<crate::runtime::Children>(copy).unwrap().0;
        assert_eq!(children.len(), 1);
        assert_ne!(children[0], glow);
        assert_eq!(world.get::<Name>(children[0]).unwrap().0, "Ember 1/Glow");
        assert_eq!(
            world.get::<crate::runtime::Parent>(children[0]).unwrap().0,
            copy
        );
        assert_eq!(
            world.get::<crate::runtime::Children>(template).unwrap().0,
            [glow]
        );
        let child = children[0];
        let step = world.resource::<FrameTime>().fixed_delta;
        app.update(step).unwrap();
        let global = app
            .world()
            .get::<crate::runtime::GlobalTransform>(child)
            .expect("the copied child gets a world transform");
        assert_eq!(global.matrix[3][..3], [3.0, 1.5, 0.0]);
    }

    #[test]
    fn scene_input_actions_bind_and_report_presses() {
        let mut app = App::new();
        app.world_mut().spawn(crate::runtime::InputAction {
            action: "fire".into(),
            inputs: vec!["KeyF".into(), "MouseLeft".into()],
        });
        app.update(std::time::Duration::from_millis(16)).unwrap();
        app.world_mut()
            .resource_mut::<RuntimeInput>()
            .record_mouse_button(crate::runtime::MouseButton::Left, true);
        let scene = GameScene {
            world: app.world_mut(),
        };
        assert!(scene.pressed("fire") && scene.held("fire"));
        assert!(!scene.pressed("jump"));
        let bad = serde_json::from_str::<crate::runtime::InputAction>(
            r#"{"action": "fire", "inputs": ["KeyQQ"]}"#,
        );
        assert!(bad
            .unwrap_err()
            .to_string()
            .contains("unknown input `KeyQQ`"));
    }

    #[test]
    fn optional_object_lookup_handles_missing_names() {
        let mut world = World::new();
        let mut scene = GameScene { world: &mut world };
        assert!(scene.try_object("Missing").is_none());
    }

    #[test]
    fn concise_gpu_watch_registration_is_idempotent() {
        let mut app = App::new();
        app.add_plugin(HybridPhysicsPlugin).unwrap();
        let entity = app.spawn((
            Name("Cube".into()),
            Transform::default(),
            crate::runtime::PhysicsBody {
                simulation: crate::runtime::SimulationClass::Gpu,
                ..Default::default()
            },
        ));
        let rule = GpuPhysicsRule::new(
            "cube_fell",
            crate::runtime::GpuCondition::position_y().less_than(-100.0),
        );

        let mut scene = GameScene {
            world: app.world_mut(),
        };
        scene.watch_gpu_object("Cube", rule.clone());
        scene.watch_gpu_object("Cube", rule);

        assert_eq!(
            scene
                .world
                .get::<GpuPhysicsWatch>(entity)
                .unwrap()
                .rules
                .len(),
            1
        );
    }

    #[test]
    fn class_gpu_watch_registration_is_idempotent() {
        let mut app = App::new();
        app.add_plugin(HybridPhysicsPlugin).unwrap();
        let rule = GpuPhysicsRule::new(
            "body_fell",
            crate::runtime::GpuCondition::position_y().less_than(-100.0),
        );

        let mut scene = GameScene {
            world: app.world_mut(),
        };
        scene.watch_gpu_class("falling_cubes", rule.clone());
        scene.watch_gpu_class("falling_cubes", rule);

        assert_eq!(
            scene.world.resource::<GpuPhysicsClassWatches>().classes
                ["falling_cubes"]
                .len(),
            1
        );
    }

    #[test]
    fn concise_api_spawns_ten_thousand_gpu_cubes_only_once() {
        const BODY_COUNT: usize = 10_000;

        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        app.add_plugin(HybridPhysicsPlugin).unwrap();
        let mut scene = GameScene {
            world: app.world_mut(),
        };
        scene.once("spawn_test_cubes", |scene| {
            let cube = CubeSpawn::new().class("gravity").class("falling_cubes");
            for index in 0..BODY_COUNT {
                scene.spawn_cube(
                    format!("Physics Cube {index}"),
                    Transform::new([index as f32, 0.0, 0.0]),
                    &cube,
                );
            }
            assert_eq!(
                scene.apply_gpu_physics_to_class(
                    "gravity",
                    &GpuBodySettings::default(),
                ),
                BODY_COUNT
            );
        });
        scene.once("spawn_test_cubes", |_| {
            panic!("a completed once block ran twice")
        });

        let mut query = scene.world.query::<(
            &crate::runtime::ObjectClasses,
            &crate::runtime::PhysicsBody,
        )>();
        assert_eq!(query.iter(scene.world).count(), BODY_COUNT);
        assert!(query.iter(scene.world).all(|(classes, physics)| {
            classes.contains("falling_cubes") && physics.uses_gpu()
        }));
    }

    #[test]
    fn sphere_spawning_reuses_mesh_for_matching_subdivisions() {
        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        let mut scene = GameScene {
            world: app.world_mut(),
        };
        let sphere = SphereSpawn::new().subdivisions(8).class("gravity");
        let first = scene.spawn_sphere(
            "Sphere A",
            Transform::new([0.0, 1.0, 0.0]),
            &sphere,
        );
        let second = scene.spawn_sphere(
            "Sphere B",
            Transform::new([0.0, 2.0, 0.0]),
            &sphere,
        );
        let first_mesh = scene
            .world
            .get::<crate::runtime::MeshRenderer>(first)
            .unwrap()
            .mesh;
        let second_mesh = scene
            .world
            .get::<crate::runtime::MeshRenderer>(second)
            .unwrap()
            .mesh;
        assert_eq!(first_mesh, second_mesh);
        let mesh = scene
            .world
            .resource::<AssetServer>()
            .meshes
            .get(first_mesh)
            .unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.indices.is_empty());
        assert!(scene
            .world
            .get::<crate::runtime::ObjectClasses>(first)
            .unwrap()
            .contains("gravity"));
    }
}
