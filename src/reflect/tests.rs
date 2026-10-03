use std::path::Path;

use bevy_ecs::component::Component;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;
use crate::runtime::{
    load_scene, registered_component_field, registered_component_values,
    save_scene, set_registered_component, set_registered_component_field, App,
    Name, SceneDocument, SceneIoError, SceneLoadMode,
};
use crate::AssetPlugin;

#[derive(Component, Default, Serialize, Deserialize, Debug, PartialEq)]
struct Turret {
    range: f32,
    #[serde(skip)]
    cooldown: f32,
}

crate::reflect! {
    struct Turret {
        range: f32 { unit: "m", min: 0.0, max: 50.0, doc: "firing reach" },
        #[skip] cooldown: f32,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
enum Aim {
    #[default]
    Idle,
    At(Entity),
    Point {
        position: [f32; 3],
    },
}

crate::reflect! {
    enum Aim {
        Idle,
        At(Entity),
        Point { position: [f32; 3] { unit: "m" } },
    }
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Ammo(u32);

crate::reflect! {
    struct Ammo(u32)
}

#[derive(Component, Default, Serialize, Deserialize, Debug, PartialEq)]
struct Guard {
    aim: Aim,
    friends: Vec<Entity>,
    leader: Option<Entity>,
    decal: Option<Handle<TextureAsset>>,
}

crate::reflect! {
    struct Guard {
        aim: Aim,
        friends: Vec<Entity>,
        leader: Option<Entity>,
        decal: Option<Handle<TextureAsset>>,
    }
}

fn test_app() -> App {
    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    app.register_scene_component::<Turret>("test.turret")
        .unwrap();
    app.register_scene_component::<Guard>("test.guard").unwrap();
    app
}

fn reflection(error: SceneIoError) -> ReflectError {
    match error {
        SceneIoError::Reflection(error) => *error,
        other => panic!("expected a reflection error, got {other}"),
    }
}

#[test]
fn the_macro_describes_structs_newtypes_and_enums() {
    let TypeInfo::Struct(turret) = Turret::type_info() else {
        panic!("a struct");
    };
    assert_eq!(turret.name, "Turret");
    assert_eq!(turret.fields.len(), 1, "skipped fields are left out");
    assert_eq!(turret.fields[0].name, "range");
    assert_eq!(turret.fields[0].ty, TypeInfo::Float);
    assert_eq!(
        turret.fields[0].hints,
        Hints {
            unit: "m",
            min: Some(0.0),
            max: Some(50.0),
            doc: "firing reach",
            color: false,
        }
    );
    assert_eq!(Ammo::type_info(), u32::type_info());

    let TypeInfo::Enum(aim) = Aim::type_info() else {
        panic!("an enum");
    };
    assert!(!aim.is_unit_only());
    assert_eq!(aim.variants[0].fields, VariantFields::Unit);
    assert_eq!(
        aim.variants[1].fields,
        VariantFields::Tuple(vec![TypeInfo::Entity])
    );
    let VariantFields::Struct(point) = &aim.variants[2].fields else {
        panic!("a struct variant");
    };
    assert_eq!(point[0].ty, TypeInfo::Array(Box::new(TypeInfo::Float), 3));

    let guard = Guard::type_info();
    assert!(guard.has_references());
    assert!(!Turret::type_info().has_references());
    assert_eq!(guard.at("/aim/Point/position/2"), Some(&TypeInfo::Float));
    assert_eq!(guard.at("/aim/At"), Some(&TypeInfo::Entity));
    assert_eq!(guard.at("/friends/7"), Some(&TypeInfo::Entity));
    assert_eq!(guard.at("/aim/Nowhere"), None);
    assert_eq!(
        guard.zero_value(),
        json!({"aim": "Idle", "friends": [], "leader": null, "decal": null})
    );
}

#[test]
fn unknown_fields_and_variants_are_rejected_with_their_path() {
    let mut app = test_app();
    let world = app.world_mut();
    let entity = world.spawn_empty().id();
    let error = reflection(
        set_registered_component(
            world,
            entity,
            "test.turret",
            r#"{"range": 5.0, "reach": 9.0}"#,
        )
        .unwrap_err(),
    );
    assert_eq!(error.component, "test.turret");
    assert_eq!(error.path, "/reach");
    assert_eq!(error.problem, ReflectProblem::UnknownField);
    assert_eq!((error.saved_version, error.current_version), (0, 0));
    assert!(error.to_string().contains("rename or remove migration"));

    let error = reflection(
        set_registered_component(
            world,
            entity,
            "test.guard",
            r#"{"aim": "Sleeping", "friends": [], "decal": null}"#,
        )
        .unwrap_err(),
    );
    assert_eq!(error.path, "/aim");
    assert_eq!(
        error.problem,
        ReflectProblem::UnknownVariant(
            "Sleeping".into(),
            vec!["Idle", "At", "Point"]
        )
    );
}

#[test]
fn migrations_rename_and_remove_fields_and_newer_versions_fail() {
    #[derive(Component, Default, Serialize, Deserialize)]
    struct Door {
        open_speed: f32,
    }
    crate::reflect! {
        struct Door { open_speed: f32 }
    }

    let mut app = App::new();
    app.register_scene_component::<Door>("test.door")
        .unwrap()
        .migrate_scene_component(
            "test.door",
            FieldMigration::rename("speed", "open_speed"),
        )
        .unwrap()
        .migrate_scene_component("test.door", FieldMigration::remove("locked"))
        .unwrap();
    let world = app.world_mut();
    let entity = world.spawn_empty().id();
    // Saved before either change: no version, old name, removed field.
    set_registered_component(
        world,
        entity,
        "test.door",
        r#"{"speed": 2.0, "locked": true}"#,
    )
    .unwrap();
    assert_eq!(world.get::<Door>(entity).unwrap().open_speed, 2.0);
    // Saved after the rename only: "locked" still goes.
    set_registered_component(
        world,
        entity,
        "test.door",
        r#"{"$version": 1, "open_speed": 3.0, "locked": false}"#,
    )
    .unwrap();
    assert_eq!(world.get::<Door>(entity).unwrap().open_speed, 3.0);
    // Saving writes the current version.
    let values = registered_component_values(world, entity).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&values[0].1).unwrap();
    assert_eq!(saved, json!({"$version": 2, "open_speed": 3.0}));

    let error = reflection(
        set_registered_component(
            world,
            entity,
            "test.door",
            r#"{"$version": 3, "open_speed": 1.0}"#,
        )
        .unwrap_err(),
    );
    assert_eq!(error.problem, ReflectProblem::NewerVersion);
    assert_eq!((error.saved_version, error.current_version), (3, 2));

    // A field unknown after migrating reports the saved version.
    let error = reflection(
        set_registered_component(
            world,
            entity,
            "test.door",
            r#"{"$version": 2, "speed": 1.0}"#,
        )
        .unwrap_err(),
    );
    assert_eq!(error.path, "/speed");
    assert_eq!((error.saved_version, error.current_version), (2, 2));

    let error = app
        .migrate_scene_component(
            "rusting.physics_sync",
            FieldMigration::remove("x"),
        )
        .err()
        .map(reflection)
        .unwrap();
    assert_eq!(error.problem, ReflectProblem::NotAStruct);
}

#[test]
fn entity_references_and_handles_save_as_ids_and_asset_paths() {
    let folder = std::env::temp_dir()
        .join(format!("rusting-reflect-{}", Uuid::new_v4()));
    std::fs::create_dir_all(folder.join("textures")).unwrap();
    let texture_path = folder.join("textures/decal.png");
    image::RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255])
        .unwrap()
        .save(&texture_path)
        .unwrap();
    let scene = folder.join("scenes/guards.rscene");

    let mut app = test_app();
    let target = app.spawn(Name("Target".into()));
    let friend = app.spawn(Name("Friend".into()));
    let texture = app
        .world_mut()
        .resource_mut::<AssetServer>()
        .load_texture(&texture_path)
        .unwrap();
    app.spawn((
        Name("Guard".into()),
        Guard {
            aim: Aim::At(target),
            friends: vec![friend, target],
            leader: Some(target),
            decal: Some(texture),
        },
    ));
    save_scene(app.world_mut(), &scene, "Guards").unwrap();

    let document: SceneDocument =
        serde_json::from_slice(&std::fs::read(&scene).unwrap()).unwrap();
    let id = |name: &str| {
        document
            .entities
            .iter()
            .find(|entity| entity.name.as_deref() == Some(name))
            .unwrap()
            .id
            .to_string()
    };
    let guard = document
        .entities
        .iter()
        .find(|entity| entity.name.as_deref() == Some("Guard"))
        .unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&guard.components["test.guard"]).unwrap();
    assert_eq!(
        saved,
        json!({
            "aim": {"At": id("Target")},
            "friends": [id("Friend"), id("Target")],
            "leader": id("Target"),
            "decal": {"$asset": Path::new("../textures/decal.png")},
        })
    );

    let mut loaded = test_app();
    load_scene(loaded.world_mut(), &scene, SceneLoadMode::Replace).unwrap();
    let world = loaded.world_mut();
    let find = |world: &mut World, wanted: &str| {
        let mut query = world.query::<(Entity, &Name)>();
        query
            .iter(world)
            .find(|(_, name)| name.0 == wanted)
            .map(|(entity, _)| entity)
            .unwrap()
    };
    let (target, friend, guard) = (
        find(world, "Target"),
        find(world, "Friend"),
        find(world, "Guard"),
    );
    let loaded_guard = world.get::<Guard>(guard).unwrap();
    assert_eq!(loaded_guard.aim, Aim::At(target));
    assert_eq!(loaded_guard.friends, vec![friend, target]);
    assert_eq!(loaded_guard.leader, Some(target));
    let decal = loaded_guard.decal.unwrap();
    let assets = world.resource::<AssetServer>();
    assert_eq!(
        assets.textures.path(decal),
        Some(texture_path.canonicalize().unwrap().as_path())
    );

    let guard_after_load = |scene: &Path| {
        let mut app = test_app();
        load_scene(app.world_mut(), scene, SceneLoadMode::Replace).unwrap();
        let world = app.world_mut();
        let mut query = world.query::<&Guard>();
        let guard = query.single(world).unwrap();
        (guard.aim.clone(), guard.friends.clone(), guard.leader)
    };

    // References to an entity outside the scene save as null, so a
    // despawned target never blocks saving or undo, and load as dangling.
    let mut app = test_app();
    let loose = app.world_mut().spawn_empty().id();
    let kept = app.spawn(Name("Kept".into()));
    app.spawn(Guard {
        aim: Aim::At(loose),
        friends: vec![loose, kept],
        leader: Some(loose),
        decal: None,
    });
    save_scene(app.world_mut(), &scene, "Loose").unwrap();
    let (aim, friends, leader) = guard_after_load(&scene);
    assert_eq!(aim, Aim::At(Entity::PLACEHOLDER));
    assert_eq!(friends[0], Entity::PLACEHOLDER);
    assert_ne!(friends[1], Entity::PLACEHOLDER);
    assert_eq!(leader, None);

    // Deleting a referenced object from the file still loads: references
    // to it dangle, and an optional one becomes None.
    let mut broken = document.clone();
    broken
        .entities
        .retain(|entity| entity.name.as_deref() != Some("Target"));
    std::fs::write(&scene, serde_json::to_vec(&broken).unwrap()).unwrap();
    let (aim, friends, leader) = guard_after_load(&scene);
    assert_eq!(aim, Aim::At(Entity::PLACEHOLDER));
    assert_eq!(friends[1], Entity::PLACEHOLDER);
    assert_eq!(leader, None);

    // A missing asset fails before the open scene is replaced.
    let mut open = test_app();
    open.spawn((Name("Open".into()), SceneId(Uuid::new_v4())));
    let mut missing = document.clone();
    for entity in &mut missing.entities {
        if let Some(guard) = entity.components.get_mut("test.guard") {
            *guard = guard.replace("decal.png", "gone.png");
        }
    }
    std::fs::write(&scene, serde_json::to_vec(&missing).unwrap()).unwrap();
    let error = reflection(
        load_scene(open.world_mut(), &scene, SceneLoadMode::Replace)
            .unwrap_err(),
    );
    assert!(matches!(error.problem, ReflectProblem::Asset(_)));
    assert_eq!(
        error.object.map(|id| id.to_string()),
        Some(id("Guard")),
        "names the scene object"
    );
    let world = open.world_mut();
    let mut names = world.query::<&Name>();
    let names: Vec<_> = names.iter(world).map(|name| name.0.clone()).collect();
    assert_eq!(names, ["Open"]);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn field_paths_read_and_write_checked_values() {
    let mut app = test_app();
    let target = app.spawn(Name("Target".into()));
    let world = app.world_mut();
    let entity = world
        .spawn((
            Turret {
                range: 5.0,
                cooldown: 1.0,
            },
            Guard::default(),
        ))
        .id();
    assert_eq!(
        registered_component_field(world, entity, "test.turret", "/range")
            .unwrap(),
        Some(json!(5.0))
    );
    set_registered_component_field(
        world,
        entity,
        "test.turret",
        "/range",
        json!(7.5),
    )
    .unwrap();
    assert_eq!(world.get::<Turret>(entity).unwrap().range, 7.5);

    let error = reflection(
        set_registered_component_field(
            world,
            entity,
            "test.turret",
            "/range",
            json!("far"),
        )
        .unwrap_err(),
    );
    assert_eq!(error.problem, ReflectProblem::WrongKind("a number"));
    let error = reflection(
        registered_component_field(world, entity, "test.turret", "/cooldown")
            .unwrap_err(),
    );
    assert_eq!(error.problem, ReflectProblem::NoSuchPath);

    // Entity fields take object IDs, resolved against the world.
    let id = world.get::<SceneId>(target).unwrap().0;
    set_registered_component_field(
        world,
        entity,
        "test.guard",
        "/aim",
        json!({"At": id.to_string()}),
    )
    .unwrap();
    assert_eq!(world.get::<Guard>(entity).unwrap().aim, Aim::At(target));
    assert_eq!(
        registered_component_field(world, entity, "test.guard", "/aim/At")
            .unwrap(),
        Some(json!(id.to_string()))
    );
}

#[test]
fn builtin_types_are_described_and_listed() {
    let registry = TypeRegistry::default();
    let resources: Vec<_> =
        registry.resources().map(|(name, _)| name).collect();
    assert_eq!(
        resources,
        [
            "rusting.determinism",
            "rusting.physics_settings",
            "rusting.random_seed",
            "rusting.render_settings",
        ]
    );
    let (_, material) = registry.assets().next().unwrap();
    assert_eq!(
        material.at("/base_color_texture"),
        Some(&TypeInfo::Option(Box::new(TypeInfo::Handle(
            TextureAsset::KIND
        ))))
    );
}
