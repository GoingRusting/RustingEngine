//! ECS application runtime and canonical scene components.
//!
//! This module is deliberately independent of Vulkan. Rendering and editor
//! integrations consume the ECS state through the `RenderExtract` schedule.

mod components;
mod events;
mod hierarchy;
mod hybrid_physics;
mod render_world;
mod scene_file;
#[cfg(test)]
mod tests;
mod time;

pub use components::*;
pub use events::EventQueue;
pub use hierarchy::{propagate_transforms, HierarchyDiagnostics};
pub use hybrid_physics::*;
pub use render_world::*;
pub use scene_file::*;
pub use time::{FrameTime, TimeControl};

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::hash::Hasher;
use std::time::{Duration, Instant};

use bevy_ecs::bundle::Bundle;
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{IntoScheduleConfigs, Resource, Schedule, World};
use bevy_ecs::system::ScheduleSystem;

/// Small non-cryptographic hasher for per-frame ECS change fingerprints.
/// Stable handles and float bits do not need the cost of a DOS-resistant map
/// hasher; this value is only used to decide whether cached data needs a check.
pub(super) struct FastHasher(u64);

impl Default for FastHasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write_u64(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 ^= value;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// The ordered stages executed by [`App::update`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScheduleStage {
    Startup,
    FixedUpdate,
    Update,
    PostUpdate,
    RenderExtract,
}

/// Summary of work performed during a single application update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameReport {
    pub fixed_steps: u32,
    pub exit_requested: bool,
}

/// Errors produced while constructing or controlling the runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    InvalidFixedDelta,
    DuplicatePlugin(&'static str),
    HierarchyCycle {
        child: Entity,
        parent: Entity,
    },
    MissingEntity(Entity),
    PluginSetup {
        plugin: &'static str,
        message: String,
    },
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFixedDelta => formatter.write_str("fixed delta must be greater than zero"),
            Self::DuplicatePlugin(name) => write!(formatter, "plugin `{name}` was already added"),
            Self::HierarchyCycle { child, parent } => write!(
                formatter,
                "parenting {child:?} beneath {parent:?} would create a hierarchy cycle"
            ),
            Self::MissingEntity(entity) => write!(formatter, "entity {entity:?} does not exist"),
            Self::PluginSetup { plugin, message } => {
                write!(formatter, "plugin `{plugin}` setup failed: {message}")
            }
        }
    }
}

impl Error for AppError {}

/// A reusable unit of runtime configuration.
pub trait Plugin: Send + Sync + 'static {
    fn build(&self, app: &mut App) -> Result<(), AppError>;

    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[derive(Resource, Default)]
struct ExitState {
    requested: bool,
}

/// Owns the canonical ECS world and all engine schedules.
pub struct App {
    world: World,
    startup: Schedule,
    fixed_update: Schedule,
    update: Schedule,
    post_update: Schedule,
    render_extract: Schedule,
    startup_complete: bool,
    event_maintenance: Vec<fn(&mut World)>,
    plugins: Vec<&'static str>,
}

impl Default for App {
    fn default() -> Self {
        let mut world = World::new();
        world.insert_resource(FrameTime::default());
        world.insert_resource(TimeControl::default());
        world.insert_resource(ExitState::default());
        world.insert_resource(HierarchyDiagnostics::default());
        world.insert_resource(RenderSettings::default());
        world.insert_resource(PhysicsSettings::default());
        world.insert_resource(PhysicsBackendStatus::default());
        world.insert_resource(SceneComponentRegistry::default());

        let mut post_update = Schedule::default();
        post_update.add_systems(propagate_transforms);

        Self {
            world,
            startup: Schedule::default(),
            fixed_update: Schedule::default(),
            update: Schedule::default(),
            post_update,
            render_extract: Schedule::default(),
            startup_complete: false,
            event_maintenance: Vec::new(),
            plugins: Vec::new(),
        }
    }
}

impl App {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn insert_resource<R: Resource>(&mut self, resource: R) -> &mut Self {
        self.world.insert_resource(resource);
        self
    }

    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let mut entity = self.world.spawn(bundle);
        if !entity.contains::<SceneId>() {
            entity.insert(SceneId::new());
        }
        entity.id()
    }

    /// Allows a game plugin to persist one of its compiled Rust components in
    /// scene files. Registration does not add scripting or dynamic dispatch to
    /// normal ECS queries.
    pub fn register_scene_component<T>(
        &mut self,
        name: impl Into<String>,
    ) -> Result<&mut Self, SceneIoError>
    where
        T: bevy_ecs::component::Component
            + serde::Serialize
            + serde::de::DeserializeOwned
            + Default,
    {
        self.world
            .resource_mut::<SceneComponentRegistry>()
            .register::<T>(name)?;
        Ok(self)
    }

    pub fn despawn(&mut self, entity: Entity) -> Result<(), AppError> {
        if self.world.despawn(entity) {
            Ok(())
        } else {
            Err(AppError::MissingEntity(entity))
        }
    }

    pub fn add_systems<M>(
        &mut self,
        stage: ScheduleStage,
        systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.schedule_mut(stage).add_systems(systems);
        self
    }

    pub fn add_system<M>(
        &mut self,
        stage: ScheduleStage,
        system: impl IntoScheduleConfigs<ScheduleSystem, M>,
    ) -> &mut Self {
        self.add_systems(stage, system)
    }

    pub fn add_plugin<P: Plugin>(
        &mut self,
        plugin: P,
    ) -> Result<&mut Self, AppError> {
        let name = plugin.name();
        if self.plugins.contains(&name) {
            return Err(AppError::DuplicatePlugin(name));
        }
        plugin.build(self)?;
        self.plugins.push(name);
        Ok(self)
    }

    pub fn add_event<T: Send + Sync + 'static>(&mut self) -> &mut Self {
        if !self.world.contains_resource::<EventQueue<T>>() {
            self.world.insert_resource(EventQueue::<T>::default());
            self.event_maintenance.push(EventQueue::<T>::begin_frame);
        }
        self
    }

    pub fn send_event<T: Send + Sync + 'static>(&mut self, event: T) {
        let mut events = self
            .world
            .get_resource_mut::<EventQueue<T>>()
            .unwrap_or_else(|| {
                panic!(
                    "event `{}` was not registered",
                    std::any::type_name::<T>()
                )
            });
        events.send(event);
    }

    pub fn request_exit(&mut self) {
        self.world.resource_mut::<ExitState>().requested = true;
    }

    #[must_use]
    pub fn exit_requested(&self) -> bool {
        self.world.resource::<ExitState>().requested
    }

    pub fn set_parent(
        &mut self,
        child: Entity,
        parent: Entity,
    ) -> Result<(), AppError> {
        hierarchy::set_parent(&mut self.world, child, parent)
    }

    pub fn clear_parent(&mut self, child: Entity) -> Result<(), AppError> {
        hierarchy::clear_parent(&mut self.world, child)
    }

    /// Advances every schedule once using a caller-provided real-frame delta.
    pub fn update(
        &mut self,
        real_delta: Duration,
    ) -> Result<FrameReport, AppError> {
        if !self.startup_complete {
            self.startup.run(&mut self.world);
            self.startup_complete = true;
        }

        for maintain in &self.event_maintenance {
            maintain(&mut self.world);
        }

        let fixed_steps = time::advance(&mut self.world, real_delta)?;
        for _ in 0..fixed_steps {
            self.fixed_update.run(&mut self.world);
        }
        self.update.run(&mut self.world);
        self.post_update.run(&mut self.world);
        self.render_extract.run(&mut self.world);

        Ok(FrameReport {
            fixed_steps,
            exit_requested: self.exit_requested(),
        })
    }

    /// Runs a simple main-thread loop until [`App::request_exit`] is called.
    pub fn run(mut self) -> Result<(), AppError> {
        let mut previous = Instant::now();
        while !self.exit_requested() {
            let now = Instant::now();
            self.update(now.saturating_duration_since(previous))?;
            previous = now;
            std::thread::yield_now();
        }
        Ok(())
    }

    fn schedule_mut(&mut self, stage: ScheduleStage) -> &mut Schedule {
        match stage {
            ScheduleStage::Startup => &mut self.startup,
            ScheduleStage::FixedUpdate => &mut self.fixed_update,
            ScheduleStage::Update => &mut self.update,
            ScheduleStage::PostUpdate => &mut self.post_update,
            ScheduleStage::RenderExtract => &mut self.render_extract,
        }
    }
}

/// Fallible builder for the ECS runtime.
pub struct EngineBuilder {
    fixed_delta: Duration,
    max_fixed_steps: u32,
    plugins: Vec<Box<dyn Plugin>>,
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self {
            fixed_delta: Duration::from_secs_f64(1.0 / 60.0),
            max_fixed_steps: 8,
            plugins: Vec::new(),
        }
    }
}

impl EngineBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn fixed_delta(mut self, fixed_delta: Duration) -> Self {
        self.fixed_delta = fixed_delta;
        self
    }

    #[must_use]
    pub fn max_fixed_steps(mut self, max_fixed_steps: u32) -> Self {
        self.max_fixed_steps = max_fixed_steps.max(1);
        self
    }

    #[must_use]
    pub fn plugin<P: Plugin>(mut self, plugin: P) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    pub fn build(self) -> Result<App, AppError> {
        if self.fixed_delta.is_zero() {
            return Err(AppError::InvalidFixedDelta);
        }
        let mut app = App::new();
        {
            let mut control = app.world.resource_mut::<TimeControl>();
            control.fixed_delta = self.fixed_delta;
            control.max_fixed_steps = self.max_fixed_steps;
        }
        for plugin in self.plugins {
            let name = plugin.name();
            if app.plugins.contains(&name) {
                return Err(AppError::DuplicatePlugin(name));
            }
            plugin.build(&mut app)?;
            app.plugins.push(name);
        }
        Ok(app)
    }
}
