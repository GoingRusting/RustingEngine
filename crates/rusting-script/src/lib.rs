//! Optional sandboxed WebAssembly scripting for RustingEngine games.
//!
//! Native Rust plugins stay the main way to write gameplay. Scripts are for
//! modding and small designer-level logic: a `.wasm` (or `.wat`) module on
//! an object, run by the [Wasmi](https://docs.rs/wasmi) interpreter with no
//! access to files, the network, or the clock. A script reads and writes the
//! same reflected component fields that scene files, the Inspector and
//! animation tracks use.
//!
//! Add [`ScriptPlugin`] to a game, then give an object the `rusting.script`
//! component ([`Script`]) with the module path, relative to the project
//! folder like other asset paths. See `docs/scripting.md` for the host API.

use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use bevy_ecs::prelude::*;
use rusting_engine::runtime::{
    registered_component_field, set_registered_component_field, ActionMap, App,
    AppError, FrameTime, Name, Plugin, RuntimeInput, SceneTransform,
    ScheduleStage,
};
use rusting_engine::Transform;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasmi::{
    Caller, Engine, Error, Extern, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

/// Registry name of [`Script`] in scene files.
pub const SCRIPT_COMPONENT: &str = "rusting.script";

/// Name a script uses in `get` and `set` for the object's transform, which
/// is not a registered component.
pub const TRANSFORM_FIELD_OWNER: &str = "transform";

/// Wasm instructions one call into a script may run (roughly). A script
/// that runs out stops, and the game goes on without it.
pub const FUEL_PER_CALL: u64 = 10_000_000;

/// Linear memory one script may use.
pub const MEMORY_LIMIT_BYTES: usize = 16 << 20;

/// `get` and `set` results below zero.
pub mod status {
    /// The entity does not exist.
    pub const NO_ENTITY: i32 = -1;
    /// The entity lacks the component, or the field is an empty option.
    pub const NO_VALUE: i32 = -2;
    /// Unknown component, bad field path, bad JSON, or a value of the wrong
    /// kind. The game's log says which.
    pub const INVALID: i32 = -3;
}

/// Runs a WebAssembly module on this object. Saved in scenes as
/// `rusting.script`.
#[derive(
    Component, Clone, Debug, Default, Serialize, Deserialize, PartialEq,
)]
pub struct Script {
    pub module: PathBuf,
}

rusting_engine::reflect! {
    struct Script {
        module: PathBuf { doc: "`.wasm` or `.wat` file, relative to the project" },
    }
}

/// Registers [`Script`] and runs every script once per frame in
/// [`ScheduleStage::Update`].
pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.register_scene_component::<Script>(SCRIPT_COMPONENT)
            .map_err(|error| AppError::PluginSetup {
                plugin: self.name(),
                message: error.to_string(),
            })?;
        app.insert_resource(ScriptHost::new());
        app.add_systems(ScheduleStage::Update, run_scripts);
        Ok(())
    }
}

/// What host functions see during one call into a script.
struct HostState {
    /// The world while a call runs; null otherwise.
    world: *mut World,
    entity: Entity,
    module: PathBuf,
    limits: StoreLimits,
}

// SAFETY: `world` is only set by `LoadedScript::call`, which holds the
// `&mut World` of the exclusive `run_scripts` system for the whole call and
// clears it before returning. No other code reads it.
unsafe impl Send for HostState {}
// SAFETY: as above; a shared `HostState` never touches `world`.
unsafe impl Sync for HostState {}

struct LoadedScript {
    module: PathBuf,
    store: Store<HostState>,
    update: Option<TypedFunc<f32, ()>>,
    /// Stopped after an error; runs again only when its module changes.
    failed: bool,
}

impl LoadedScript {
    fn call(
        &mut self,
        world: &mut World,
        call: impl FnOnce(&mut Store<HostState>) -> Result<(), Error>,
    ) {
        self.store.data_mut().world = world;
        let result = self
            .store
            .set_fuel(FUEL_PER_CALL)
            .and_then(|()| call(&mut self.store));
        self.store.data_mut().world = null_mut();
        if let Err(error) = result {
            eprintln!(
                "[rusting] script {} stopped: {error}",
                self.module.display()
            );
            self.failed = true;
        }
    }
}

/// Loaded scripts by entity, run in entity order so a replay runs them in
/// the same order.
#[derive(Resource)]
struct ScriptHost {
    engine: Engine,
    linker: Linker<HostState>,
    scripts: std::collections::BTreeMap<Entity, LoadedScript>,
}

impl ScriptHost {
    fn new() -> Self {
        let mut config = wasmi::Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let mut linker = Linker::new(&engine);
        define_host_functions(&mut linker);
        Self {
            engine,
            linker,
            scripts: std::collections::BTreeMap::new(),
        }
    }

    /// Instantiates `module` for `entity` and runs its `start` export.
    fn load(
        &self,
        world: &mut World,
        entity: Entity,
        module: &Path,
    ) -> LoadedScript {
        let state = HostState {
            world: null_mut(),
            entity,
            module: module.to_owned(),
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_LIMIT_BYTES)
                .instances(1)
                .build(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        let mut script = LoadedScript {
            module: module.to_owned(),
            store,
            update: None,
            failed: false,
        };
        let binary = module.extension().is_some_and(|ext| ext == "wasm");
        let bytes = std::fs::read(module)
            .map_err(|error| Error::new(error.to_string()))
            .and_then(|bytes| {
                if binary && !bytes.starts_with(b"\0asm") {
                    return Err(Error::new("not a WebAssembly binary"));
                }
                Ok(bytes)
            });
        let (engine, linker) = (&self.engine, &self.linker);
        let mut update = None;
        script.call(world, |store| {
            let module = Module::new(engine, bytes?)?;
            let instance =
                linker.instantiate_and_start(&mut *store, &module)?;
            update = instance
                .get_func(&*store, "update")
                .map(|update| update.typed::<f32, ()>(&*store))
                .transpose()?;
            match instance.get_func(&*store, "start") {
                Some(start) => start.typed::<(), ()>(&*store)?.call(store, ()),
                None => Ok(()),
            }
        });
        script.update = update;
        script
    }
}

/// Loads new and changed scripts, drops removed ones, and calls each
/// script's `update(delta_seconds)`.
fn run_scripts(world: &mut World) {
    let Some(mut host) = world.remove_resource::<ScriptHost>() else {
        return;
    };
    let delta = world.resource::<FrameTime>().delta_seconds();
    let mut scripts = world.query::<(Entity, &Script)>();
    let mut wanted: Vec<(Entity, PathBuf)> = scripts
        .iter(world)
        .map(|(entity, script)| (entity, script.module.clone()))
        .collect();
    wanted.sort_by_key(|&(entity, _)| entity);
    host.scripts.retain(|entity, _| {
        wanted.binary_search_by_key(entity, |&(e, _)| e).is_ok()
    });
    for (entity, module) in wanted {
        let current = host
            .scripts
            .get(&entity)
            .is_some_and(|script| script.module == module);
        if !current {
            let script = host.load(world, entity, &module);
            host.scripts.insert(entity, script);
        }
        let script = host.scripts.get_mut(&entity).expect("inserted above");
        if let (false, Some(update)) = (script.failed, script.update) {
            script.call(world, |store| update.call(store, delta));
        }
    }
    world.insert_resource(host);
}

fn define_host_functions(linker: &mut Linker<HostState>) {
    const MODULE: &str = "rusting";
    linker
        .func_wrap(
            MODULE,
            "log",
            |caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let text = read_text(&caller, ptr, len)?;
                eprintln!("[script {}] {text}", caller.data().module.display());
                Ok(())
            },
        )
        .expect("new linker")
        .func_wrap(MODULE, "entity", |caller: Caller<'_, HostState>| {
            caller.data().entity.to_bits() as i64
        })
        .expect("new linker")
        .func_wrap(
            MODULE,
            "find",
            |caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let name = read_text(&caller, ptr, len)?;
                let world = world(&caller);
                let mut named = world.query::<(Entity, &Name)>();
                let found = named
                    .iter(world)
                    .filter(|(_, named)| named.0 == name)
                    .map(|(entity, _)| entity)
                    .min();
                Ok(found.map_or(-1, |entity| entity.to_bits() as i64))
            },
        )
        .expect("new linker")
        .func_wrap(
            MODULE,
            "action",
            |caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let action = read_text(&caller, ptr, len)?;
                let world = world(&caller);
                let (Some(actions), Some(input)) = (
                    world.get_resource::<ActionMap>(),
                    world.get_resource::<RuntimeInput>(),
                ) else {
                    return Ok(0);
                };
                Ok(i32::from(actions.held(input, &action))
                    | i32::from(actions.just_pressed(input, &action)) << 1
                    | i32::from(actions.just_released(input, &action)) << 2)
            },
        )
        .expect("new linker")
        .func_wrap(
            MODULE,
            "get",
            |mut caller: Caller<'_, HostState>,
             entity: i64,
             component_ptr: i32,
             component_len: i32,
             path_ptr: i32,
             path_len: i32,
             out_ptr: i32,
             out_capacity: i32| {
                let component =
                    read_text(&caller, component_ptr, component_len)?;
                let path = read_text(&caller, path_ptr, path_len)?;
                let Some(entity) = live_entity(&caller, entity) else {
                    return Ok(status::NO_ENTITY);
                };
                let value = match get_field(
                    world(&caller),
                    entity,
                    &component,
                    &path,
                ) {
                    Ok(Some(value)) => value.to_string(),
                    Ok(None) => return Ok(status::NO_VALUE),
                    Err(error) => return Ok(invalid(&caller, &error)),
                };
                let (Ok(len), Ok(capacity)) =
                    (i32::try_from(value.len()), usize::try_from(out_capacity))
                else {
                    return Err(Error::new("value too large"));
                };
                if value.len() <= capacity {
                    guest_bytes(&mut caller, out_ptr, len)?
                        .copy_from_slice(value.as_bytes());
                }
                Ok(len)
            },
        )
        .expect("new linker")
        .func_wrap(
            MODULE,
            "set",
            |caller: Caller<'_, HostState>,
             entity: i64,
             component_ptr: i32,
             component_len: i32,
             path_ptr: i32,
             path_len: i32,
             value_ptr: i32,
             value_len: i32| {
                let component =
                    read_text(&caller, component_ptr, component_len)?;
                let path = read_text(&caller, path_ptr, path_len)?;
                let value = read_text(&caller, value_ptr, value_len)?;
                let Some(entity) = live_entity(&caller, entity) else {
                    return Ok(status::NO_ENTITY);
                };
                let result = serde_json::from_str(&value)
                    .map_err(|error| format!("bad JSON `{value}`: {error}"))
                    .and_then(|value| {
                        set_field(
                            world(&caller),
                            entity,
                            &component,
                            &path,
                            value,
                        )
                    });
                Ok(match result {
                    Ok(()) => 0,
                    Err(error) => invalid(&caller, &error),
                })
            },
        )
        .expect("new linker");
}

/// The world of the running call.
#[allow(clippy::mut_from_ref)]
fn world<'a>(caller: &Caller<'_, HostState>) -> &'a mut World {
    // SAFETY: host functions only run inside `LoadedScript::call`, which
    // set this pointer from a live `&mut World` it holds for the call.
    unsafe { &mut *caller.data().world }
}

fn live_entity(caller: &Caller<'_, HostState>, bits: i64) -> Option<Entity> {
    let entity = Entity::try_from_bits(bits as u64)?;
    world(caller).get_entity(entity).is_ok().then_some(entity)
}

fn invalid(caller: &Caller<'_, HostState>, error: &str) -> i32 {
    eprintln!("[script {}] {error}", caller.data().module.display());
    status::INVALID
}

fn memory(caller: &Caller<'_, HostState>) -> Result<Memory, Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| Error::new("the script exports no `memory`"))
}

fn read_text(
    caller: &Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> Result<String, Error> {
    let bytes = memory(caller)?
        .data(caller)
        .get(range(ptr, len)?)
        .ok_or_else(|| Error::new("text outside the script's memory"))?;
    String::from_utf8(bytes.to_vec())
        .map_err(|_| Error::new("text is not UTF-8"))
}

fn guest_bytes<'a>(
    caller: &'a mut Caller<'_, HostState>,
    ptr: i32,
    len: i32,
) -> Result<&'a mut [u8], Error> {
    let range = range(ptr, len)?;
    memory(caller)?
        .data_mut(caller)
        .get_mut(range)
        .ok_or_else(|| Error::new("buffer outside the script's memory"))
}

fn range(ptr: i32, len: i32) -> Result<std::ops::Range<usize>, Error> {
    let (Ok(start), Ok(len)) = (usize::try_from(ptr), usize::try_from(len))
    else {
        return Err(Error::new("negative pointer or length"));
    };
    Ok(start..start + len)
}

fn get_field(
    world: &World,
    entity: Entity,
    component: &str,
    path: &str,
) -> Result<Option<Value>, String> {
    if component == TRANSFORM_FIELD_OWNER {
        let Some(&transform) = world.get::<Transform>(entity) else {
            return Ok(None);
        };
        let value = serde_json::to_value(SceneTransform::from(transform))
            .expect("transforms serialize");
        return value
            .pointer(path)
            .cloned()
            .map(Some)
            .ok_or_else(|| format!("transform has no field `{path}`"));
    }
    registered_component_field(world, entity, component, path)
        .map_err(|error| error.to_string())
}

fn set_field(
    world: &mut World,
    entity: Entity,
    component: &str,
    path: &str,
    value: Value,
) -> Result<(), String> {
    if component == TRANSFORM_FIELD_OWNER {
        let mut transform = world
            .get_mut::<Transform>(entity)
            .ok_or("the entity has no transform")?;
        let mut whole = serde_json::to_value(SceneTransform::from(*transform))
            .expect("transforms serialize");
        *whole
            .pointer_mut(path)
            .ok_or_else(|| format!("transform has no field `{path}`"))? = value;
        let changed: SceneTransform = serde_json::from_value(whole)
            .map_err(|error| format!("transform `{path}`: {error}"))?;
        *transform = changed.into();
        return Ok(());
    }
    set_registered_component_field(world, entity, component, path, value)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
