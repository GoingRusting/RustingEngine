//! Data assets: typed values a game defines and saves as `.rdata` files,
//! the engine's version of Godot's `Resource` and `.tres` files.
//!
//! A data asset file is JSON: the registered type name and the value in the
//! same form scenes save components in, with asset references as
//! `{"$asset": path}` relative to the file.
//!
//! ```json
//! {
//!   "type": "my_game.enemy_stats",
//!   "data": { "health": 40, "speed": 3.5 }
//! }
//! ```

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::path::{Path, PathBuf};

use bevy_ecs::prelude::Resource;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{normalize_path, read_asset_file, AssetError, AssetServer, Assets};
use crate::assets::Handle;
use crate::reflect::{
    handle_from_scene, handle_to_scene, map_asset_paths, walk, Located,
    Reflect, ReflectProblem, TypeInfo,
};
use crate::runtime::{normalize_lexical, path_relative_to};

/// File extension of a saved data asset.
pub const DATA_EXTENSION: &str = "rdata";

/// Why an object reference cannot be part of a data asset.
const NO_OBJECTS: &str =
    "no scene object reference, because a data asset lives outside scenes";

/// A game type saved as its own asset file and shared through `Handle<T>`.
///
/// Describe the type with [`reflect!`](crate::reflect!), give it a unique
/// name, and register it with
/// [`App::register_data_asset`](crate::runtime::App::register_data_asset).
/// Components then refer to a file with a `Handle<T>` field, which scenes
/// save as the file's path and the Inspector offers as a drop-down. Fields
/// may hold handles to textures, meshes and other data assets, but not
/// scene objects.
pub trait DataAsset:
    Reflect + Serialize + DeserializeOwned + Default + Send + Sync + 'static
{
    /// Name saved in the file, such as `my_game.enemy_stats`.
    const NAME: &'static str;
}

/// Loaded data assets of every type, one [`Assets`] store per type.
#[derive(Default)]
pub struct DataAssets {
    stores: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    /// Files being read, so a file that refers back to itself fails
    /// instead of recursing forever.
    loading: Vec<PathBuf>,
    /// Hot reloads since the last `AssetServer::take_reloaded`.
    pub(super) reloaded: Vec<PathBuf>,
    /// Failed hot reloads since the last
    /// `AssetServer::take_reload_failures`.
    pub(super) failures: Vec<AssetError>,
}

impl DataAssets {
    #[must_use]
    pub fn get<T: DataAsset>(&self, handle: Handle<T>) -> Option<&T> {
        self.store::<T>()?.get(handle)
    }

    pub fn get_mut<T: DataAsset>(
        &mut self,
        handle: Handle<T>,
    ) -> Option<&mut T> {
        self.store_mut::<T>().get_mut(handle)
    }

    /// The loaded assets of type `T`, or `None` before the first one loads.
    #[must_use]
    pub fn store<T: DataAsset>(&self) -> Option<&Assets<T>> {
        self.stores.get(&TypeId::of::<T>())?.downcast_ref()
    }

    pub fn store_mut<T: DataAsset>(&mut self) -> &mut Assets<T> {
        self.stores
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(Assets::<T>::default()))
            .downcast_mut()
            .expect("stores are keyed by their type")
    }
}

impl AssetServer {
    /// Loads a `.rdata` file as a `T`, or returns the handle it already
    /// has. Asset references inside it load too.
    pub fn load_data<T: DataAsset>(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<Handle<T>, AssetError> {
        let path = normalize_path(path.as_ref())?;
        if let Some(handle) = self
            .data
            .store::<T>()
            .and_then(|store| store.handle_for_path(&path))
        {
            return Ok(handle);
        }
        let value = self.decode_data::<T>(&path)?;
        self.data.store_mut::<T>().insert_with_path(&path, value)
    }

    /// Reads `path` again into the loaded asset, so every handle to it sees
    /// the new value. Returns false when no `T` was loaded from `path`. On
    /// an error the old value stays.
    pub fn reload_data<T: DataAsset>(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<bool, AssetError> {
        let path = normalize_path(path.as_ref())?;
        let Some(handle) = self
            .data
            .store::<T>()
            .and_then(|store| store.handle_for_path(&path))
        else {
            return Ok(false);
        };
        let value = self.decode_data::<T>(&path)?;
        if let Some(slot) = self.data.get_mut(handle) {
            *slot = value;
        }
        Ok(true)
    }

    /// Writes `value` to a `.rdata` file. Handles inside it must point at
    /// assets loaded from files.
    pub fn save_data<T: DataAsset>(
        &self,
        path: impl AsRef<Path>,
        value: &T,
    ) -> Result<(), AssetError> {
        let path = path.as_ref();
        let error = |message: String| AssetError::Load {
            path: path.to_owned(),
            message,
        };
        let mut data = serde_json::to_value(value)
            .map_err(|value| error(value.to_string()))?;
        self.data_to_scene_form(&T::type_info(), &mut data)
            .map_err(|(pointer, problem)| error(located(&pointer, &problem)))?;
        write_data_file(path, T::NAME, data)
    }

    /// Adds a copy of the data asset `handle` points at that no file
    /// backs, like Godot's `Resource.duplicate()`. Scenes save the copy
    /// inside the object that refers to it, so each object keeps its own
    /// values. Handles inside the copy still point at the same assets.
    pub fn duplicate_data<T: DataAsset>(
        &mut self,
        handle: Handle<T>,
    ) -> Option<Handle<T>> {
        let copy = serde_json::to_value(self.data.get(handle)?)
            .ok()
            .and_then(|value| serde_json::from_value::<T>(value).ok())?;
        Some(self.data.store_mut::<T>().insert(copy))
    }

    /// The scene form of a data asset, with handles inside it as
    /// `{"$asset": path}` or `{"$data": value}`.
    pub(crate) fn embedded_data<T: DataAsset>(
        &self,
        handle: Handle<T>,
    ) -> Result<Value, ReflectProblem> {
        let value =
            self.data.get(handle).ok_or(ReflectProblem::UnsavedAsset)?;
        let mut data = serde_json::to_value(value)
            .map_err(|error| ReflectProblem::Asset(error.to_string()))?;
        self.data_to_scene_form(&T::type_info(), &mut data)
            .map_err(|(pointer, problem)| {
                ReflectProblem::Asset(located(&pointer, &problem))
            })?;
        Ok(data)
    }

    /// Adds a data asset saved inside a scene or file as a new copy.
    pub(crate) fn insert_embedded_data<T: DataAsset>(
        &mut self,
        mut data: Value,
    ) -> Result<Handle<T>, ReflectProblem> {
        self.data_from_scene_form(&T::type_info(), &mut data)
            .map_err(|(pointer, problem)| {
                ReflectProblem::Asset(located(&pointer, &problem))
            })?;
        let value = serde_json::from_value(data)
            .map_err(|error| ReflectProblem::Asset(error.to_string()))?;
        // ponytail: every load or Inspector edit of a scene adds a copy and
        // the old one stays; count references if long edit sessions show
        // the memory.
        Ok(self.data.store_mut::<T>().insert(value))
    }

    fn data_to_scene_form(
        &self,
        info: &TypeInfo,
        data: &mut Value,
    ) -> Result<(), Located> {
        walk(info, data, &mut |info, value| match info {
            TypeInfo::Handle(kind) => handle_to_scene(kind, self, value),
            TypeInfo::Entity => Err(ReflectProblem::WrongKind(NO_OBJECTS)),
            _ => Ok(()),
        })
    }

    fn data_from_scene_form(
        &mut self,
        info: &TypeInfo,
        data: &mut Value,
    ) -> Result<(), Located> {
        walk(info, data, &mut |info, value| match info {
            TypeInfo::Handle(kind) => handle_from_scene(kind, self, value),
            TypeInfo::Entity => Err(ReflectProblem::WrongKind(NO_OBJECTS)),
            _ => Ok(()),
        })
    }

    fn decode_data<T: DataAsset>(
        &mut self,
        path: &Path,
    ) -> Result<T, AssetError> {
        let error = |message: String| AssetError::Load {
            path: path.to_owned(),
            message,
        };
        let (name, mut data) = read_data_file(path)?;
        if name != T::NAME {
            return Err(error(format!(
                "holds a `{name}`, not a `{}`",
                T::NAME
            )));
        }
        if self.data.loading.iter().any(|loading| loading == path) {
            return Err(error(
                "refers back to itself through other data assets".into(),
            ));
        }
        self.data.loading.push(path.to_owned());
        let converted = self.data_from_scene_form(&T::type_info(), &mut data);
        self.data.loading.pop();
        converted
            .map_err(|(pointer, problem)| error(located(&pointer, &problem)))?;
        serde_json::from_value(data).map_err(|value| error(value.to_string()))
    }
}

fn located(pointer: &str, problem: &ReflectProblem) -> String {
    let pointer = if pointer.is_empty() { "/" } else { pointer };
    format!("at `{pointer}`: {problem}")
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DataFile {
    #[serde(rename = "type")]
    type_name: String,
    data: Value,
}

/// The folder a file's relative asset paths start from, made absolute.
fn file_folder(path: &Path) -> PathBuf {
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf())
}

/// Reads a `.rdata` file: its type name and its value in scene form, with
/// asset paths made absolute.
pub fn read_data_file(path: &Path) -> Result<(String, Value), AssetError> {
    let file: DataFile = serde_json::from_slice(&read_asset_file(path)?)
        .map_err(|error| AssetError::Load {
            path: path.to_owned(),
            message: error.to_string(),
        })?;
    let folder = file_folder(path);
    let mut data = file.data;
    map_asset_paths(&mut data, &mut |asset| {
        Ok::<_, Infallible>(normalize_lexical(&folder.join(asset)))
    })
    .unwrap_or_else(|never| match never {});
    Ok((file.type_name, data))
}

/// Writes a `.rdata` file holding `data` in scene form. Asset paths are
/// saved relative to the file.
pub fn write_data_file(
    path: &Path,
    type_name: &str,
    mut data: Value,
) -> Result<(), AssetError> {
    let error = |message: String| AssetError::Load {
        path: path.to_owned(),
        message,
    };
    let folder = file_folder(path);
    map_asset_paths(&mut data, &mut |asset| {
        let target = std::fs::canonicalize(asset)
            .unwrap_or_else(|_| normalize_lexical(&folder.join(asset)));
        Ok::<_, Infallible>(
            path_relative_to(&folder, &target).unwrap_or(target),
        )
    })
    .unwrap_or_else(|never| match never {});
    let file = DataFile {
        type_name: type_name.to_owned(),
        data,
    };
    let mut text = serde_json::to_string_pretty(&file)
        .map_err(|value| error(value.to_string()))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|value| error(value.to_string()))
}

/// What the editor needs to know about one registered data asset type.
#[derive(Clone)]
pub struct DataAssetType {
    pub name: &'static str,
    pub info: TypeInfo,
    default: fn() -> Value,
    reload: fn(&mut AssetServer, &Path) -> Result<bool, AssetError>,
    changed: fn(&mut AssetServer) -> Vec<PathBuf>,
}

impl DataAssetType {
    /// The type's `Default` value in the form a file saves.
    #[must_use]
    pub fn default_value(&self) -> Value {
        (self.default)()
    }

    /// [`AssetServer::reload_data`] for this type.
    pub fn reload(
        &self,
        assets: &mut AssetServer,
        path: &Path,
    ) -> Result<bool, AssetError> {
        (self.reload)(assets, path)
    }
}

fn reload<T: DataAsset>(
    assets: &mut AssetServer,
    path: &Path,
) -> Result<bool, AssetError> {
    assets.reload_data::<T>(path)
}

fn changed<T: DataAsset>(assets: &mut AssetServer) -> Vec<PathBuf> {
    if assets.data.store::<T>().is_none() {
        return Vec::new();
    }
    let store = assets.data.store_mut::<T>();
    store.changed().into_iter().map(|(_, path)| path).collect()
}

/// Data asset types the game registered, by name.
#[derive(Resource, Default)]
pub struct DataAssetTypes {
    types: BTreeMap<&'static str, DataAssetType>,
}

impl DataAssetTypes {
    /// Adds `T`. Registering the same name again replaces the entry.
    pub fn register<T: DataAsset>(&mut self) {
        self.types.insert(
            T::NAME,
            DataAssetType {
                name: T::NAME,
                info: T::type_info(),
                default: crate::reflect::default_data::<T>,
                reload: reload::<T>,
                changed: changed::<T>,
            },
        );
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&DataAssetType> {
        self.types.get(name)
    }

    /// Every registered type, sorted by name.
    pub fn iter(&self) -> impl Iterator<Item = &DataAssetType> {
        self.types.values()
    }

    /// Reads every loaded data asset whose file changed on disk again, so
    /// a running game sees the edit through its existing handles. A file
    /// that fails to load keeps its last value. Results reach
    /// `AssetServer::take_reloaded` and `take_reload_failures`. Returns
    /// how many files reloaded.
    // ponytail: decodes on the calling thread; data files are small JSON.
    // Move to workers like meshes if large tables stall a frame.
    pub fn reload_changed(&self, assets: &mut AssetServer) -> usize {
        let mut reloaded = 0;
        for registered in self.types.values() {
            for path in (registered.changed)(assets) {
                match registered.reload(assets, &path) {
                    Ok(true) => {
                        assets.data.reloaded.push(path);
                        reloaded += 1;
                    }
                    Ok(false) => {}
                    Err(error) => assets.data.failures.push(error),
                }
            }
        }
        reloaded
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::component::Component;
    use bevy_ecs::world::Mut;

    use super::*;
    use crate::reflect::{ASSET_KEY, DATA_KEY};
    use crate::runtime::{load_scene, save_scene, App, SceneId, SceneLoadMode};
    use crate::AssetPlugin;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct EnemyStats {
        health: u32,
        speed: f32,
        base: Option<Handle<EnemyStats>>,
    }

    crate::reflect! {
        struct EnemyStats {
            health: u32,
            speed: f32 { unit: "m/s" },
            base: Option<Handle<EnemyStats>>,
        }
    }

    impl DataAsset for EnemyStats {
        const NAME: &'static str = "test.enemy_stats";
    }

    #[derive(Component, Debug, Default, Serialize, Deserialize)]
    struct Spawner {
        stats: Option<Handle<EnemyStats>>,
    }

    crate::reflect! {
        struct Spawner { stats: Option<Handle<EnemyStats>> }
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        app.register_scene_component::<Spawner>("test.spawner")
            .unwrap();
        app.register_data_asset::<EnemyStats>();
        app
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn data_assets_load_share_save_and_reload_through_handles() {
        let folder = std::env::temp_dir()
            .join(format!("rusting-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let monster = folder.join("stats/monster.rdata");
        let goblin = folder.join("stats/goblin.rdata");
        write(
            &monster,
            r#"{"type": "test.enemy_stats", "data": {"health": 10, "speed": 1.0, "base": null}}"#,
        );
        write(
            &goblin,
            r#"{"type": "test.enemy_stats",
                "data": {"health": 40, "speed": 3.5, "base": {"$asset": "monster.rdata"}}}"#,
        );

        // Loading follows references and shares one copy per file.
        let mut app = app();
        let mut assets = app.world_mut().resource_mut::<AssetServer>();
        let handle = assets.load_data::<EnemyStats>(&goblin).unwrap();
        assert_eq!(assets.load_data::<EnemyStats>(&goblin).unwrap(), handle);
        let stats = assets.data.get(handle).unwrap();
        assert_eq!((stats.health, stats.speed), (40, 3.5));
        let base = stats.base.unwrap();
        assert_eq!(assets.load_data::<EnemyStats>(&monster).unwrap(), base);
        assert_eq!(assets.data.get(base).unwrap().health, 10);

        // Saving writes references relative to the file.
        let copy = folder.join("copy.rdata");
        let value = EnemyStats {
            health: 7,
            speed: 2.0,
            base: Some(base),
        };
        assets.save_data(&copy, &value).unwrap();
        let (name, saved) = read_data_file(&copy).unwrap();
        assert_eq!(name, EnemyStats::NAME);
        let text = std::fs::read_to_string(&copy).unwrap();
        assert!(text.contains("\"stats/monster.rdata\""), "{text}");
        assert_eq!(
            saved["base"][ASSET_KEY].as_str().map(PathBuf::from),
            Some(std::fs::canonicalize(&monster).unwrap())
        );

        // A component refers to the file by path in the scene.
        app.spawn((
            SceneId::new(),
            Spawner {
                stats: Some(handle),
            },
        ));
        let scene = folder.join("level.rscene");
        save_scene(app.world_mut(), &scene, "level").unwrap();
        let text = std::fs::read_to_string(&scene).unwrap();
        assert!(text.contains("stats/goblin.rdata"), "{text}");
        let mut loaded = self::app();
        load_scene(loaded.world_mut(), &scene, SceneLoadMode::Replace).unwrap();
        let world = loaded.world_mut();
        let stats = world
            .query::<&Spawner>()
            .iter(world)
            .next()
            .unwrap()
            .stats
            .unwrap();
        let assets = world.resource::<AssetServer>();
        assert_eq!(assets.data.get(stats).unwrap().health, 40);

        // Rewriting the file and reloading changes what every handle sees.
        write(
            &goblin,
            r#"{"type": "test.enemy_stats", "data": {"health": 55, "speed": 3.5, "base": null}}"#,
        );
        let types = loaded.world().resource::<DataAssetTypes>();
        let registered = types.get(EnemyStats::NAME).unwrap().clone();
        assert_eq!(
            registered.default_value(),
            serde_json::to_value(EnemyStats::default()).unwrap()
        );
        let mut assets = loaded.world_mut().resource_mut::<AssetServer>();
        assert!(registered.reload(&mut assets, &goblin).unwrap());
        assert_eq!(assets.data.get(stats).unwrap().health, 55);
        assert!(!registered.reload(&mut assets, &copy).unwrap());

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn unique_data_assets_are_private_copies_saved_in_place() {
        let folder = std::env::temp_dir()
            .join(format!("rusting-data-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let goblin = folder.join("goblin.rdata");
        write(
            &goblin,
            r#"{"type": "test.enemy_stats", "data": {"health": 40, "speed": 3.5, "base": null}}"#,
        );
        let mut app = app();
        let mut assets = app.world_mut().resource_mut::<AssetServer>();
        let shared = assets.load_data::<EnemyStats>(&goblin).unwrap();
        let unique = assets.duplicate_data(shared).unwrap();
        assert_ne!(unique, shared);
        assets.data.get_mut(unique).unwrap().health = 99;
        assert_eq!(assets.data.get(shared).unwrap().health, 40);

        // A data asset file can hold a unique copy too.
        let nested = folder.join("boss.rdata");
        let boss = EnemyStats {
            health: 500,
            speed: 1.0,
            base: Some(unique),
        };
        assets.save_data(&nested, &boss).unwrap();
        let text = std::fs::read_to_string(&nested).unwrap();
        assert!(text.contains(DATA_KEY), "{text}");
        let loaded = assets.load_data::<EnemyStats>(&nested).unwrap();
        let base = assets.data.get(loaded).unwrap().base.unwrap();
        assert_ne!(base, unique);
        assert_eq!(assets.data.get(base).unwrap().health, 99);

        // Scenes save the shared one by path and each unique one in place.
        for stats in [shared, unique, unique] {
            app.spawn((SceneId::new(), Spawner { stats: Some(stats) }));
        }
        let scene = folder.join("level.rscene");
        save_scene(app.world_mut(), &scene, "level").unwrap();
        let text = std::fs::read_to_string(&scene).unwrap();
        assert!(text.contains("goblin.rdata"), "{text}");
        assert_eq!(text.matches(DATA_KEY).count(), 2, "{text}");

        // Every unique reference loads as its own copy.
        let mut loaded = self::app();
        load_scene(loaded.world_mut(), &scene, SceneLoadMode::Replace).unwrap();
        let world = loaded.world_mut();
        let handles: Vec<_> = world
            .query::<&Spawner>()
            .iter(world)
            .map(|spawner| spawner.stats.unwrap())
            .collect();
        let assets = world.resource::<AssetServer>();
        let path = |handle| {
            assets
                .data
                .store::<EnemyStats>()
                .unwrap()
                .path(handle)
                .is_some()
        };
        let (files, copies): (Vec<_>, Vec<_>) =
            handles.into_iter().partition(|&handle| path(handle));
        assert_eq!(files.len(), 1);
        assert_eq!(copies.len(), 2);
        assert_ne!(copies[0], copies[1]);
        for copy in copies {
            assert_eq!(assets.data.get(copy).unwrap().health, 99);
        }

        // Reloading the file leaves the copies alone.
        write(
            &goblin,
            r#"{"type": "test.enemy_stats", "data": {"health": 1, "speed": 3.5, "base": null}}"#,
        );
        let mut assets = world.resource_mut::<AssetServer>();
        assert!(assets.reload_data::<EnemyStats>(&goblin).unwrap());
        assert_eq!(assets.data.get(files[0]).unwrap().health, 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn changed_data_files_reload_into_a_running_game() {
        use std::time::{Duration, SystemTime};

        let folder = std::env::temp_dir()
            .join(format!("rusting-data-hot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let goblin = folder.join("goblin.rdata");
        // Explicit timestamps: a coarse file clock could hide a rewrite.
        let rewrite = |health: &str, seconds: u64| {
            write(
                &goblin,
                &format!(
                    r#"{{"type": "test.enemy_stats", "data": {{"health": {health}, "speed": 1.0, "base": null}}}}"#
                ),
            );
            std::fs::File::options()
                .write(true)
                .open(&goblin)
                .unwrap()
                .set_modified(
                    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
                )
                .unwrap();
        };
        rewrite("10", 1_000);
        let mut app = app();
        let mut assets = app.world_mut().resource_mut::<AssetServer>();
        let handle = assets.load_data::<EnemyStats>(&goblin).unwrap();
        let unique = assets.duplicate_data(handle).unwrap();
        let store = assets.data.store::<EnemyStats>().unwrap();
        let revision = store.revision(handle).unwrap();

        // The frame's asset scan picks up the new file.
        rewrite("25", 2_000);
        app.update(Duration::from_millis(16)).unwrap();
        let mut assets = app.world_mut().resource_mut::<AssetServer>();
        assert_eq!(assets.data.get(handle).unwrap().health, 25);
        assert_eq!(assets.data.get(unique).unwrap().health, 10);
        let store = assets.data.store::<EnemyStats>().unwrap();
        assert!(store.revision(handle).unwrap() > revision);
        assert_eq!(
            assets.take_reloaded(),
            vec![std::fs::canonicalize(&goblin).unwrap()]
        );

        // A broken rewrite keeps the last value and reports why.
        rewrite("\"many\"", 3_000);
        let world = app.world_mut();
        world.resource_scope(|world, types: Mut<DataAssetTypes>| {
            let mut assets = world.resource_mut::<AssetServer>();
            assert_eq!(types.reload_changed(&mut assets), 0);
        });
        let mut assets = world.resource_mut::<AssetServer>();
        assert_eq!(assets.data.get(handle).unwrap().health, 25);
        let failures = assets.take_reload_failures();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].to_string().contains("goblin.rdata"));
        assert!(assets.take_reloaded().is_empty());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn bad_data_files_fail_with_the_reason() {
        let folder = std::env::temp_dir()
            .join(format!("rusting-data-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut assets = AssetServer::default();
        let mut failure = |name: &str, text: &str| {
            let path = folder.join(name);
            write(&path, text);
            assets
                .load_data::<EnemyStats>(&path)
                .unwrap_err()
                .to_string()
        };

        let other =
            failure("other.rdata", r#"{"type": "test.weapon", "data": {}}"#);
        assert!(other.contains("holds a `test.weapon`"), "{other}");
        let unknown = failure(
            "unknown.rdata",
            r#"{"type": "test.enemy_stats", "data": {"health": 1, "armor": 2}}"#,
        );
        assert!(unknown.contains("/armor"), "{unknown}");
        write(
            &folder.join("a.rdata"),
            r#"{"type": "test.enemy_stats", "data": {"health": 1, "speed": 1.0, "base": {"$asset": "b.rdata"}}}"#,
        );
        let cycle = failure(
            "b.rdata",
            r#"{"type": "test.enemy_stats", "data": {"health": 1, "speed": 1.0, "base": {"$asset": "a.rdata"}}}"#,
        );
        assert!(cycle.contains("refers back to itself"), "{cycle}");
        assert!(assets
            .data
            .store::<EnemyStats>()
            .is_none_or(Assets::is_empty));

        let _ = std::fs::remove_dir_all(&folder);
    }
}
