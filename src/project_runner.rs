//! Windowed runtime runner for native Rust game projects.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Mut, Resource, World};
use vulkano::format::Format;
use vulkano::VulkanError;
use vulkano_util::context::VulkanoContext;
use vulkano_util::window::{VulkanoWindows, WindowDescriptor};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, WindowId};

use crate::rendering::frame_pacer::{select_present_mode, FramePacer};
use crate::rendering::scene_renderer::{SceneRenderOptions, SceneRenderer};
use crate::runtime::{
    apply_gpu_state_samples, load_scene, load_scene_document,
    record_gpu_state_hashes, route_gpu_physics_events, scene_document,
    write_atomic, AppError, EventQueue, FrameTime, GpuEventRegistry,
    GpuPhysicsClassWatches, GpuPhysicsEvent, GpuPhysicsEventsLost,
    GpuPhysicsRule, GpuPhysicsWatch, HybridPhysicsPlugin, Name,
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
    /// Runs setup code once during this game process.
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
        let entity = self
            .world
            .spawn((
                crate::runtime::SceneId::new(),
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

    /// Sets one GPU body's starting linear velocity.
    ///
    /// Call this during setup after assigning GPU physics. The value is read
    /// by the next GPU extraction and then owned by the compute shader.
    pub fn set_linear_velocity(
        &mut self,
        name: &str,
        velocity: [f32; 3],
    ) -> bool {
        let Some(entity) = find_named_entity(self.world, name) else {
            return false;
        };
        let Some(mut rigid_body) =
            self.world.get_mut::<crate::runtime::RigidBody>(entity)
        else {
            return false;
        };
        rigid_body.linear_velocity = velocity;
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
                create_info.min_image_count =
                    create_info.min_image_count.max(2);
            },
        );
        // Apply project render settings before the first presented frame.
        let renderer = self.windows.get_primary_renderer_mut().unwrap();
        let settings = runtime.world().resource::<RenderSettings>();
        renderer.set_present_mode(select_present_mode(
            &renderer.graphics_queue(),
            &renderer.surface(),
            settings.vsync,
        ));
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

        match renderer.acquire(None, |_| {}) {
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
}

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
        }
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
                if let Err(error) = self.runtime.update(delta) {
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
                match self.window.render(window_id, &self.runtime) {
                    Ok(()) => announce_first_frame(),
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
/// [`crate::runtime::StateHashReport`] to that file.
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
    let (_, report) = simulate_project_headless(scene_path, plugin, ticks)?;
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
    let scene_path: PathBuf = scene_path.into();
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
    let entities =
        crate::runtime::named_entity_state_hashes(runtime.world_mut());
    let report = crate::runtime::StateHashReport {
        ticks: hashes,
        entities,
    };
    Ok((runtime, report))
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
