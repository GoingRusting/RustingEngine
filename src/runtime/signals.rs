//! Signals: entity-targeted reactions to events and component changes.
//!
//! This is the ECS form of Godot's signals. An entity that emits a signal
//! carries a [`Connections`] component that lists named handlers and the
//! entity each one acts on. Handlers are Rust systems registered once with
//! [`App::add_signal_handler`](super::App::add_signal_handler); their input
//! type says which signal they answer:
//!
//! - `In<Signal<E>>` for a typed [`EntityEvent`] `E`, sent with
//!   `commands.trigger(..)` and aimed at the emitting entity;
//! - `In<Signal<Added<T>>>` or `In<Signal<Removed<T>>>` for component `T`
//!   being added to or removed from the emitting entity (despawns count as
//!   removals).
//!
//! Connections are plain data, so snapshots, replays and scene files keep
//! them (scene key `rusting.connections`, targets saved by object ID). A
//! game fails to load a scene whose connections name an unregistered
//! handler or a missing object. Bevy's per-entity `observe` is not used: its observer entities
//! hold boxed systems that a snapshot cannot copy.

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::event::EntityEvent;
use bevy_ecs::lifecycle::{Add, Remove};
use bevy_ecs::observer::On;
use bevy_ecs::prelude::{Commands, Query, Res, Resource, World};
use bevy_ecs::system::{In, IntoSystem, System};

/// One call of a signal handler.
#[derive(Clone, Debug)]
pub struct Signal<E> {
    /// The entity that emitted the signal (the one holding the connection).
    pub source: Entity,
    /// The entity the connection names as the handler's receiver.
    pub target: Entity,
    /// The event, or an [`Added`] / [`Removed`] marker.
    pub event: E,
}

/// Signal payload: component `T` was added to the source entity.
pub struct Added<T>(PhantomData<fn() -> T>);

/// Signal payload: component `T` was removed from the source entity, or the
/// entity was despawned. Handlers run after the removal, so the value and a
/// despawned source are gone by then.
pub struct Removed<T>(PhantomData<fn() -> T>);

impl<T> Clone for Added<T> {
    fn clone(&self) -> Self {
        Self(PhantomData)
    }
}

impl<T> Clone for Removed<T> {
    fn clone(&self) -> Self {
        Self(PhantomData)
    }
}

/// One entry of [`Connections`]: run `handler` with `target` as receiver.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Connection {
    /// Name given to [`App::add_signal_handler`](super::App::add_signal_handler).
    pub handler: String,
    /// Receiver passed to the handler as [`Signal::target`].
    pub target: Entity,
}

/// The signal connections of the entity that emits them. Handlers run in
/// list order; a handler whose signal type does not match the event is
/// skipped for that event.
#[derive(
    Component,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Connections {
    /// Handlers to run, in order.
    pub list: Vec<Connection>,
}

impl Connections {
    /// Adds a connection and returns the list, for building in a bundle.
    #[must_use]
    pub fn with(mut self, handler: impl Into<String>, target: Entity) -> Self {
        self.list.push(Connection {
            handler: handler.into(),
            target,
        });
        self
    }
}

/// Error from [`App::connect`](super::App::connect) and handler registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignalError {
    /// No handler is registered under this name.
    UnknownHandler(String),
    /// A handler is already registered under this name.
    DuplicateHandler(String),
    /// The source or target entity does not exist.
    MissingEntity(Entity),
}

impl std::fmt::Display for SignalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownHandler(name) => {
                write!(formatter, "no signal handler is named `{name}`")
            }
            Self::DuplicateHandler(name) => write!(
                formatter,
                "a signal handler named `{name}` is already registered"
            ),
            Self::MissingEntity(entity) => {
                write!(formatter, "entity {entity:?} does not exist")
            }
        }
    }
}

impl std::error::Error for SignalError {}

/// Lets `Plugin::build` register handlers with `?`.
impl From<SignalError> for super::AppError {
    fn from(error: SignalError) -> Self {
        Self::PluginSetup {
            plugin: "signals",
            message: error.to_string(),
        }
    }
}

type BoxedHandler<E> = Box<dyn System<In = In<Signal<E>>, Out = ()>>;

/// A handler system, taken out while it runs so that a handler which
/// re-emits its own signal does not run inside itself.
type Slot = Arc<Mutex<Option<Box<dyn Any + Send>>>>;

struct Handler {
    signal: TypeId,
    slot: Slot,
}

/// Registered handlers. Fixed after setup, so snapshots leave it out.
#[derive(Resource, Default)]
pub(super) struct SignalHandlers {
    handlers: HashMap<String, Handler>,
    dispatchers: HashSet<TypeId>,
    /// Set while a snapshot is restored, so writing components back does
    /// not fire `Added` and `Removed` signals.
    pub(super) muted: bool,
}

/// A signal type: something a dispatcher observer can watch.
pub trait SignalEvent: Clone + Send + Sync + 'static {
    /// Adds the one observer that routes this signal to connections.
    #[doc(hidden)]
    fn add_dispatcher(world: &mut World);
}

impl<E: EntityEvent + Clone> SignalEvent for E {
    fn add_dispatcher(world: &mut World) {
        world.add_observer(
            |on: On<E>,
             connections: Query<&Connections>,
             handlers: Res<SignalHandlers>,
             mut commands: Commands| {
                let event = on.event();
                queue_signals(
                    event.event_target(),
                    event,
                    &connections,
                    &handlers,
                    &mut commands,
                );
            },
        );
    }
}

impl<T: Component> SignalEvent for Added<T> {
    fn add_dispatcher(world: &mut World) {
        world.add_observer(
            |on: On<Add, T>,
             connections: Query<&Connections>,
             handlers: Res<SignalHandlers>,
             mut commands: Commands| {
                queue_signals(
                    on.entity,
                    &Self(PhantomData),
                    &connections,
                    &handlers,
                    &mut commands,
                );
            },
        );
    }
}

impl<T: Component> SignalEvent for Removed<T> {
    fn add_dispatcher(world: &mut World) {
        world.add_observer(
            |on: On<Remove, T>,
             connections: Query<&Connections>,
             handlers: Res<SignalHandlers>,
             mut commands: Commands| {
                queue_signals(
                    on.entity,
                    &Self(PhantomData),
                    &connections,
                    &handlers,
                    &mut commands,
                );
            },
        );
    }
}

/// Queues the handlers connected on `source` that answer `E`. Connections
/// are read now, because a despawned source is gone when commands run.
fn queue_signals<E: SignalEvent>(
    source: Entity,
    event: &E,
    connections: &Query<&Connections>,
    handlers: &SignalHandlers,
    commands: &mut Commands,
) {
    if handlers.muted {
        return;
    }
    let Ok(connections) = connections.get(source) else {
        return;
    };
    for connection in &connections.list {
        let Some(handler) = handlers.handlers.get(&connection.handler) else {
            continue;
        };
        if handler.signal != TypeId::of::<E>() {
            continue;
        }
        let slot = Arc::clone(&handler.slot);
        let signal = Signal {
            source,
            target: connection.target,
            event: event.clone(),
        };
        commands
            .queue(move |world: &mut World| run_handler(&slot, signal, world));
    }
}

fn run_handler<E: SignalEvent>(
    slot: &Slot,
    signal: Signal<E>,
    world: &mut World,
) {
    let taken = slot.lock().expect("handler slot").take();
    // A handler that emits its own signal is still running.
    let Some(mut system) = taken else { return };
    let handler = system
        .downcast_mut::<BoxedHandler<E>>()
        .expect("signal type checked at dispatch");
    // Parameters that fail validation (a missing resource) skip the call,
    // as they do for scheduled systems.
    let _ = handler.run(signal, world);
    *slot.lock().expect("handler slot") = Some(system);
}

/// Checks every connection of the objects a scene load just spawned: each
/// must name a registered handler and an object that exists.
pub(super) fn validate_connections(
    world: &World,
    spawned: &HashMap<uuid::Uuid, Entity>,
) -> Result<(), super::SceneIoError> {
    let mut objects: Vec<_> = spawned.iter().collect();
    objects.sort_unstable();
    for (&object, &entity) in objects {
        let Some(connections) = world.get::<Connections>(entity) else {
            continue;
        };
        for connection in &connections.list {
            let problem =
                if !SignalHandlers::contains(world, &connection.handler) {
                    SignalError::UnknownHandler(connection.handler.clone())
                } else if world.get_entity(connection.target).is_err() {
                    SignalError::MissingEntity(connection.target)
                } else {
                    continue;
                };
            return Err(super::SceneIoError::Connection { object, problem });
        }
    }
    Ok(())
}

impl SignalHandlers {
    pub(super) fn register<E: SignalEvent, M>(
        world: &mut World,
        name: String,
        handler: impl IntoSystem<In<Signal<E>>, (), M>,
    ) -> Result<(), SignalError> {
        let handlers = world.get_resource_or_init::<Self>().into_inner();
        if handlers.handlers.contains_key(&name) {
            return Err(SignalError::DuplicateHandler(name));
        }
        let add_dispatcher = handlers.dispatchers.insert(TypeId::of::<E>());
        let mut system: BoxedHandler<E> =
            Box::new(IntoSystem::into_system(handler));
        system.initialize(world);
        world.resource_mut::<Self>().handlers.insert(
            name,
            Handler {
                signal: TypeId::of::<E>(),
                slot: Arc::new(Mutex::new(Some(Box::new(system)))),
            },
        );
        if add_dispatcher {
            E::add_dispatcher(world);
        }
        Ok(())
    }

    pub(super) fn contains(world: &World, name: &str) -> bool {
        world
            .get_resource::<Self>()
            .is_some_and(|handlers| handlers.handlers.contains_key(name))
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::ResMut;

    use super::super::App;
    use super::*;

    #[derive(EntityEvent, Clone)]
    struct Hit {
        entity: Entity,
        amount: u32,
    }

    #[derive(Component, Clone)]
    struct Health;

    #[derive(Resource, Clone, Default)]
    struct Log(Vec<(&'static str, Entity, Entity, u32)>);

    fn app() -> App {
        let mut app = App::new();
        app.insert_resource(Log::default())
            // Left out, so a handler run by a restore would show.
            .ignore_in_snapshots::<Log>()
            .register_snapshot_component::<Health>();
        app.add_signal_handler(
            "hit",
            |In(signal): In<Signal<Hit>>, mut log: ResMut<Log>| {
                log.0.push((
                    "hit",
                    signal.source,
                    signal.target,
                    signal.event.amount,
                ));
            },
        )
        .unwrap()
        .add_signal_handler(
            "hit_again",
            |In(signal): In<Signal<Hit>>, mut log: ResMut<Log>| {
                log.0.push(("again", signal.source, signal.target, 0));
            },
        )
        .unwrap()
        .add_signal_handler(
            "armed",
            |In(signal): In<Signal<Added<Health>>>, mut log: ResMut<Log>| {
                log.0.push(("armed", signal.source, signal.target, 0));
            },
        )
        .unwrap()
        .add_signal_handler(
            "disarmed",
            |In(signal): In<Signal<Removed<Health>>>, mut log: ResMut<Log>| {
                log.0.push(("disarmed", signal.source, signal.target, 0));
            },
        )
        .unwrap();
        app
    }

    fn take_log(app: &mut App) -> Vec<(&'static str, Entity, Entity, u32)> {
        app.world_mut().flush();
        std::mem::take(&mut app.world_mut().resource_mut::<Log>().0)
    }

    #[test]
    fn connected_handlers_answer_events_and_component_changes() {
        let mut app = app();
        let coin = app.spawn(());
        let player = app.spawn(());
        let other = app.spawn(());
        for (handler, target) in [
            ("hit", player),
            ("hit_again", coin),
            ("armed", coin),
            ("disarmed", player),
        ] {
            app.connect(coin, handler, target).unwrap();
        }

        // Through commands, as a system sends them; the other entity has no
        // connections.
        let mut commands = app.world_mut().commands();
        commands.trigger(Hit {
            entity: coin,
            amount: 3,
        });
        commands.trigger(Hit {
            entity: other,
            amount: 9,
        });
        assert_eq!(
            take_log(&mut app),
            [("hit", coin, player, 3), ("again", coin, coin, 0)]
        );

        app.world_mut().entity_mut(coin).insert(Health);
        app.world_mut().entity_mut(other).insert(Health);
        assert_eq!(take_log(&mut app), [("armed", coin, coin, 0)]);
        app.world_mut().despawn(coin);
        assert_eq!(take_log(&mut app), [("disarmed", coin, player, 0)]);
    }

    #[test]
    fn snapshots_keep_connections_and_restores_fire_nothing() {
        let mut app = app();
        let coin = app.spawn(Health);
        app.connect(coin, "armed", coin)
            .unwrap()
            .connect(coin, "disarmed", coin)
            .unwrap()
            .connect(coin, "hit", coin)
            .unwrap();
        take_log(&mut app);
        let snapshot = app.snapshot().unwrap();

        app.world_mut().entity_mut(coin).remove::<Health>();
        assert_eq!(take_log(&mut app), [("disarmed", coin, coin, 0)]);
        app.restore(&snapshot).unwrap();
        // Writing Health back fired no `armed`.
        assert!(take_log(&mut app).is_empty());

        app.world_mut().entity_mut(coin).remove::<Connections>();
        app.restore(&snapshot).unwrap();

        app.world_mut().trigger(Hit {
            entity: coin,
            amount: 1,
        });
        assert_eq!(take_log(&mut app), [("hit", coin, coin, 1)]);
    }

    #[test]
    fn connections_name_registered_handlers_and_live_entities() {
        let mut app = app();
        let coin = app.spawn(());
        let gone = app.spawn(());
        app.despawn(gone).unwrap();
        assert_eq!(
            app.connect(coin, "missing", coin).err(),
            Some(SignalError::UnknownHandler("missing".into()))
        );
        assert_eq!(
            app.connect(coin, "hit", gone).err(),
            Some(SignalError::MissingEntity(gone))
        );
        assert_eq!(
            app.connect(gone, "hit", coin).err(),
            Some(SignalError::MissingEntity(gone))
        );
        assert_eq!(
            app.add_signal_handler("hit", |_: In<Signal<Hit>>| {}).err(),
            Some(SignalError::DuplicateHandler("hit".into()))
        );
    }

    #[test]
    fn scenes_save_connections_by_object_id_and_check_them_on_load() {
        use super::super::{
            load_scene_document, scene_document, Name, SceneComponentRegistry,
            SceneId, SceneIoError, SceneLoadMode, CONNECTIONS_COMPONENT,
        };

        let game = || {
            let mut app = app();
            app.add_plugin(crate::AssetPlugin).unwrap();
            app
        };
        let mut editor = game();
        let coin = editor.spawn(Name("Coin".into()));
        let player = editor.spawn(Name("Player".into()));
        editor.connect(coin, "hit", player).unwrap();
        let document = scene_document(editor.world_mut(), "level").unwrap();
        let id =
            |app: &App, entity| app.world().get::<SceneId>(entity).unwrap().0;
        let (coin_id, player_id) = (id(&editor, coin), id(&editor, player));
        let saved = document
            .entities
            .iter()
            .find(|entity| entity.id == coin_id)
            .unwrap()
            .components[CONNECTIONS_COMPONENT]
            .clone();
        assert!(saved.contains(&player_id.to_string()), "{saved}");

        let mut app = game();
        load_scene_document(app.world_mut(), &document, SceneLoadMode::Replace)
            .unwrap();
        let entity = |app: &mut App, wanted| {
            let mut query = app.world_mut().query::<(Entity, &SceneId)>();
            query
                .iter(app.world())
                .find(|(_, id)| id.0 == wanted)
                .unwrap()
                .0
        };
        let (coin, player) =
            (entity(&mut app, coin_id), entity(&mut app, player_id));
        app.world_mut().trigger(Hit {
            entity: coin,
            amount: 2,
        });
        assert_eq!(take_log(&mut app), [("hit", coin, player, 2)]);

        let with_connections = |connections: String| {
            let mut document = document.clone();
            for entity in &mut document.entities {
                if entity.id == coin_id {
                    entity.components.insert(
                        CONNECTIONS_COMPONENT.into(),
                        connections.clone(),
                    );
                }
            }
            document
        };
        let unknown = with_connections(saved.replace("\"hit\"", "\"gone\""));
        match load_scene_document(
            app.world_mut(),
            &unknown,
            SceneLoadMode::Replace,
        ) {
            Err(SceneIoError::Connection {
                object,
                problem: SignalError::UnknownHandler(name),
            }) if object == coin_id && name == "gone" => {}
            other => panic!("{other:?}"),
        }
        let mut dangling = document.clone();
        dangling.entities.retain(|entity| entity.id != player_id);
        match load_scene_document(
            app.world_mut(),
            &dangling,
            SceneLoadMode::Replace,
        ) {
            Err(SceneIoError::Connection {
                object,
                problem: SignalError::MissingEntity(_),
            }) if object == coin_id => {}
            other => panic!("{other:?}"),
        }
        // The failed loads kept the open scene.
        assert_eq!(app.world().get::<SceneId>(coin).unwrap().0, coin_id);

        // Tools such as the editor load scenes without the game's handlers.
        app.world_mut()
            .resource_mut::<SceneComponentRegistry>()
            .keep_unregistered();
        load_scene_document(app.world_mut(), &unknown, SceneLoadMode::Replace)
            .unwrap();
    }
}
