//! Windowed runtime runner for native Rust game projects.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Mut, Resource, World};
use vulkano::format::Format;
use vulkano::VulkanError;
use vulkano_util::context::{VulkanoConfig, VulkanoContext};
use vulkano_util::window::{VulkanoWindows, WindowDescriptor};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use crate::rendering::frame_pacer::{select_present_mode, FramePacer};
use crate::rendering::scene_renderer::{SceneRenderer, SceneViewport};
use crate::runtime::{
    load_scene, route_gpu_physics_events, AppError, EventQueue, FrameTime,
    GpuEventRegistry, GpuPhysicsClassWatches, GpuPhysicsEvent, GpuPhysicsRule,
    GpuPhysicsWatch, HybridPhysicsPlugin, Name, PhysicsBackendStatus, Plugin,
    RenderExtractPlugin, RenderSettings, RenderWorld, SceneLoadMode,
    ScheduleStage,
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
#[derive(Resource, Default)]
struct SphereMeshCache(
    HashMap<u32, crate::assets::Handle<crate::assets::MeshAsset>>,
);

/// Keys already executed through [`GameScene::once`].
#[derive(Resource, Default)]
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
        ensure_scene_name_index(self.world);
        if self
            .world
            .resource::<SceneNameIndex>()
            .entities
            .contains_key(&name)
        {
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
                    simulation: crate::runtime::SimulationClass::GpuDynamic,
                    solver: settings.solver,
                    custom_shader: settings.custom_shader.clone(),
                },
                settings.rigid_body,
                settings.collider,
                settings.collision_layers,
                crate::runtime::GpuEffectBody,
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
#[derive(Resource, Default)]
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
        Ok(())
    }
}

fn run_simple_game_update(world: &mut World) {
    // Copy these small values before giving the whole world to GameScene.
    let time = *world.resource::<FrameTime>();
    let update = world.resource::<GameUpdateFunction>().0;
    update(&mut GameScene { world }, &time);
}

struct ProjectApplication {
    /// Text shown in the game window title bar.
    title: String,
    /// Vulkan device, queues, and memory allocators.
    vulkan: VulkanoContext,
    /// Winit windows connected to Vulkan swapchains.
    windows: VulkanoWindows,
    /// Renderer created after the operating system opens the window.
    scene_renderer: Option<SceneRenderer>,
    /// ECS world, schedules, assets, and game plugin.
    runtime: App,
    /// Time of the previous frame, used to calculate delta time.
    previous_frame: Instant,
    /// Requests frames immediately or waits when an FPS limit is enabled.
    frame_pacer: FramePacer,
    /// VSync value currently applied to the swapchain.
    applied_vsync: Option<bool>,
}

impl ProjectApplication {
    /// Creates the ECS runtime and loads cooked scene data before opening a window.
    ///
    /// # Arguments
    /// * `title` - Text shown in the window title bar.
    /// * `scene_path` - Cooked scene file loaded into the ECS world.
    /// * `plugin` - Native Rust gameplay systems supplied by the game.
    fn load<P: Plugin>(
        title: String,
        scene_path: PathBuf,
        plugin: P,
    ) -> Result<Self, Box<dyn Error>> {
        // Install common engine systems before game code and scene objects.
        let mut runtime = App::new();
        runtime.add_plugin(AssetPlugin)?;
        runtime.add_plugin(HybridPhysicsPlugin)?;
        runtime.add_plugin(RenderExtractPlugin)?;
        runtime.add_plugin(plugin)?;
        load_scene(runtime.world_mut(), &scene_path, SceneLoadMode::Replace)?;
        runtime
            .world_mut()
            .resource_mut::<PhysicsBackendStatus>()
            .gpu_dynamic_available = true;
        Ok(Self {
            title,
            vulkan: VulkanoContext::new(VulkanoConfig::default()),
            windows: VulkanoWindows::default(),
            scene_renderer: None,
            runtime,
            previous_frame: Instant::now(),
            frame_pacer: FramePacer::default(),
            applied_vsync: None,
        })
    }
}

impl ApplicationHandler for ProjectApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
        let settings = self.runtime.world().resource::<RenderSettings>();
        renderer.set_present_mode(select_present_mode(
            &renderer.graphics_queue(),
            &renderer.surface(),
            settings.vsync,
        ));
        self.applied_vsync = Some(settings.vsync);
        self.scene_renderer = Some(
            SceneRenderer::new(
                renderer.graphics_queue(),
                self.vulkan.memory_allocator().clone(),
                renderer.swapchain_format(),
                renderer.swapchain_image_size(),
            )
            .expect("failed to create game scene renderer"),
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let renderer = self.windows.get_renderer_mut(window_id).unwrap();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => renderer.resize(),
            WindowEvent::RedrawRequested => {
                // Completed GPU events enter ECS before this frame starts, so
                // Rust update systems can read them from the normal event API.
                let raw_events = self
                    .scene_renderer
                    .as_mut()
                    .unwrap()
                    .take_completed_physics_events();
                if !raw_events.is_empty() {
                    route_gpu_physics_events(
                        self.runtime.world_mut(),
                        &raw_events,
                    );
                }
                // Delta time tells gameplay how much real time passed.
                let now = Instant::now();
                let delta = now.saturating_duration_since(self.previous_frame);
                self.previous_frame = now;
                if let Err(error) = self.runtime.update(delta) {
                    eprintln!("runtime update failed: {error}");
                    event_loop.exit();
                    return;
                }
                let vsync =
                    self.runtime.world().resource::<RenderSettings>().vsync;
                if self.applied_vsync != Some(vsync) {
                    renderer.set_present_mode(select_present_mode(
                        &renderer.graphics_queue(),
                        &renderer.surface(),
                        vsync,
                    ));
                    self.applied_vsync = Some(vsync);
                }
                // Get the next swapchain image, draw the scene, then present it.
                match renderer.acquire(None, |_| {}) {
                    Ok(future) => {
                        let extent = renderer.swapchain_image_size();
                        let future =
                            match self.scene_renderer.as_mut().unwrap().render(
                                future,
                                renderer.swapchain_image_view(),
                                extent,
                                SceneViewport::full(extent),
                                self.runtime.world().resource::<RenderWorld>(),
                                self.runtime.world().resource::<AssetServer>(),
                            ) {
                                Ok(future) => future,
                                Err(error) => {
                                    eprintln!(
                                        "scene rendering failed: {error}"
                                    );
                                    event_loop.exit();
                                    return;
                                }
                            };
                        renderer.present(future, false);
                    }
                    Err(VulkanError::OutOfDate) => renderer.resize(),
                    Err(error) => {
                        eprintln!("swapchain acquisition failed: {error}");
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(renderer) = self.windows.get_primary_renderer_mut() {
            // This requests another frame and applies the optional FPS limit.
            self.frame_pacer.request_next_frame(
                event_loop,
                renderer.window(),
                self.runtime.world().resource::<RenderSettings>(),
            );
        }
    }
}

/// Runs a cooked scene with a native game-defined Rust plugin.
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
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut ProjectApplication::load(
        title.into(),
        scene_path.into(),
        plugin,
    )?)?;
    Ok(())
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
                simulation: crate::runtime::SimulationClass::GpuDynamic,
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
