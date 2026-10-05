//! Checks that the shipped Milestone 7 glTF uses the normal asset importer.

#[cfg(feature = "gltf")]
#[test]
fn pbr_environment_imports_geometry_materials_and_maps() {
    use rusting_engine::assets::{AssetServer, MaterialModel};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples/vertical_slice/environment.gltf");
    let folder = std::env::temp_dir()
        .join(format!("rusting-environment-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let source = folder.join("environment.gltf");
    std::fs::copy(fixture, &source).unwrap();

    let mut assets = AssetServer::default();
    let nodes = assets.import_gltf_scene(&source).unwrap();
    assert_eq!(nodes.len(), 11);
    assert!(nodes.iter().any(|node| node.name == "Floor"));
    assert!(nodes.iter().any(|node| node.name == "Signal block"));
    let floor = &nodes[0].primitives[0];
    let material = assets.materials.get(floor.material).unwrap();
    assert_eq!(material.model, MaterialModel::Pbr);
    assert!(material.base_color_texture.is_some());
    assert!(material.metallic_roughness_texture.is_some());
    assert!(assets.meshes.path(floor.mesh).unwrap().is_file());
    assert_eq!(assets.mesh_bounds(floor.mesh), Some(([-1.0; 3], [1.0; 3])));

    std::fs::remove_dir_all(folder).unwrap();
}

/// The imported courtyard plus a player and a sun survive save and reload in
/// a fresh `App`, and a rewritten mesh file reloads while that app runs.
#[cfg(feature = "gltf")]
#[test]
fn vertical_slice_scene_persists_and_reloads_assets_live() {
    use std::collections::BTreeSet;
    use std::time::{Duration, Instant, SystemTime};

    use rusting_engine::assets::{
        spawn_gltf_nodes_in_world, AssetServer, MeshAsset, MeshVertex,
    };
    use rusting_engine::runtime::{
        load_scene, save_scene, Camera, Children, DirectionalLight,
        MeshRenderer, Name, PlayerController, SceneId, SceneLoadMode,
    };
    use rusting_engine::{App, AssetPlugin, Transform};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples/vertical_slice/environment.gltf");
    let folder = std::env::temp_dir()
        .join(format!("rusting-slice-scene-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let source = folder.join("environment.gltf");
    std::fs::copy(fixture, &source).unwrap();
    let scene_path = folder.join("slice.rscene");

    let names = |app: &mut App| {
        let world = app.world_mut();
        let mut query = world.query::<&Name>();
        query
            .iter(world)
            .map(|name| name.0.clone())
            .collect::<BTreeSet<_>>()
    };

    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    let nodes = app
        .world_mut()
        .resource_mut::<AssetServer>()
        .import_gltf_scene(&source)
        .unwrap();
    spawn_gltf_nodes_in_world(app.world_mut(), &nodes, None).unwrap();
    let player = app.spawn((
        SceneId::new(),
        Name("Player".into()),
        Transform::new([0.0, 1.0, 4.0]),
        PlayerController {
            walk_speed: 5.5,
            yaw: 0.25,
            ..PlayerController::default()
        },
    ));
    let eye = app.spawn((
        SceneId::new(),
        Name("Eye".into()),
        Transform::new([0.0, 0.7, 0.0]),
        Camera::default(),
    ));
    app.set_parent(eye, player).unwrap();
    app.spawn((
        SceneId::new(),
        Name("Sun".into()),
        Transform::default(),
        DirectionalLight {
            illuminance: 80_000.0,
            ..DirectionalLight::default()
        },
    ));
    let saved_names = names(&mut app);
    save_scene(app.world_mut(), &scene_path, "slice").unwrap();

    let mut loaded = App::new();
    loaded.add_plugin(AssetPlugin).unwrap();
    load_scene(loaded.world_mut(), &scene_path, SceneLoadMode::Replace)
        .unwrap();
    assert_eq!(names(&mut loaded), saved_names);
    let world = loaded.world_mut();
    let mut players =
        world.query::<(&PlayerController, &Transform, &Children)>();
    let (controller, transform, children) = players.single(world).unwrap();
    assert_eq!((controller.walk_speed, controller.yaw), (5.5, 0.25));
    assert_eq!(transform.position, [0.0, 1.0, 4.0]);
    let eye = children.0[0];
    assert!(world.get::<Camera>(eye).is_some());
    let mut suns = world.query::<&DirectionalLight>();
    assert_eq!(suns.single(world).unwrap().illuminance, 80_000.0);
    let mut renderers = world.query::<(&Name, &MeshRenderer)>();
    let floor = renderers
        .iter(world)
        .find(|(name, _)| name.0 == "Floor")
        .map(|(_, renderer)| renderer.mesh)
        .unwrap();
    let assets = world.resource::<AssetServer>();
    let mesh_path = assets.meshes.path(floor).unwrap().to_path_buf();
    let revision = assets.meshes.revision(floor).unwrap();
    assert_eq!(assets.meshes.get(floor).unwrap().indices.len(), 36);

    // Rewrite the cooked floor mesh as one triangle; the running app picks
    // it up on a later frame without a restart.
    let triangle = MeshAsset {
        vertices: vec![MeshVertex::default(); 3],
        indices: vec![0, 1, 2],
    };
    std::fs::write(&mesh_path, bincode::serialize(&triangle).unwrap()).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&mesh_path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(3600))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        loaded.update(Duration::from_millis(16)).unwrap();
        let assets = loaded.world().resource::<AssetServer>();
        if assets.meshes.get(floor).unwrap().indices.len() == 3 {
            assert!(assets.meshes.revision(floor).unwrap() > revision);
            break;
        }
        assert!(Instant::now() < deadline, "live reload timed out");
        std::thread::sleep(Duration::from_millis(20));
    }

    std::fs::remove_dir_all(folder).unwrap();
}

/// `scene add-model` keeps the windmill's glTF node animation as a
/// `rusting.animation` clip that autoplays and turns the rotor.
#[cfg(feature = "gltf")]
#[test]
fn add_model_keeps_gltf_node_animation_as_a_clip() {
    use rusting_engine::runtime::{
        load_scene, save_scene, Animation, AnimationProperty, Interpolation,
        Name, SceneLoadMode,
    };
    use rusting_engine::{App, AssetPlugin, Transform};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples/vertical_slice/windmill.gltf");
    let folder = std::env::temp_dir()
        .join(format!("rusting-windmill-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("windmill.gltf");
    std::fs::copy(fixture, &model).unwrap();
    let scene = folder.join("mill.rscene");
    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    save_scene(app.world_mut(), &scene, "mill").unwrap();

    let result =
        rusting_engine::cli::add_model(&scene, &model, "Windmill", false);
    assert!(result.ok, "{:?}", result.diagnostics);

    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    load_scene(app.world_mut(), &scene, SceneLoadMode::Replace).unwrap();
    let find = |app: &mut App, wanted: &str| {
        let world = app.world_mut();
        let mut query = world.query::<(bevy_ecs::entity::Entity, &Name)>();
        query
            .iter(world)
            .find(|(_, name)| name.0 == wanted)
            .map(|(entity, _)| entity)
            .unwrap()
    };
    let windmill = find(&mut app, "Windmill");
    let rotor = find(&mut app, "Rotor");
    let animation = app.world().get::<Animation>(windmill).unwrap();
    assert_eq!(animation.autoplay, "spin");
    let clip = &animation.clips[0];
    assert_eq!(clip.length(), 4.0);
    let tracks = clip
        .tracks
        .iter()
        .map(|track| {
            (
                track.target.as_str(),
                track.property.clone(),
                track.keys.len(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tracks,
        [
            ("Tower/Rotor", AnimationProperty::Orientation, 5),
            ("Tower/Flag", AnimationProperty::Position, 3),
        ]
    );
    // Cubic spline keys keep their values, not the tangents.
    assert_eq!(clip.tracks[1].interpolation, Interpolation::Smooth);
    assert_eq!(clip.tracks[1].keys[1].value, [0.5, 2.0, 0.0]);

    for _ in 0..61 {
        app.update(std::time::Duration::from_secs_f64(1.0 / 60.0))
            .unwrap();
    }
    let turned = app.world().get::<Transform>(rotor).unwrap().rotation[2];
    assert!(
        (turned - std::f32::consts::FRAC_PI_2).abs() < 0.05,
        "{turned}"
    );

    std::fs::remove_dir_all(folder).unwrap();
}

/// `scene add-model` keeps the bar's glTF skin as `rusting.skin`; when the
/// "bend" clip turns the Tip joint a quarter turn, the drawn copy of the mesh
/// folds over while the cooked mesh stays straight.
#[cfg(feature = "gltf")]
#[test]
fn add_model_keeps_gltf_skin_and_bends_the_mesh() {
    use rusting_engine::assets::AssetServer;
    use rusting_engine::runtime::{
        load_scene, save_scene, update_skins, MeshRenderer, Name,
        SceneLoadMode, Skin, SkinnedMesh,
    };
    use rusting_engine::{App, AssetPlugin};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples/vertical_slice/bending_bar.gltf");
    let folder = std::env::temp_dir()
        .join(format!("rusting-bar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("bending_bar.gltf");
    std::fs::copy(fixture, &model).unwrap();
    let scene = folder.join("bar.rscene");
    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    save_scene(app.world_mut(), &scene, "bar").unwrap();

    let result = rusting_engine::cli::add_model(&scene, &model, "Bar", false);
    assert!(result.ok, "{:?}", result.diagnostics);

    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    load_scene(app.world_mut(), &scene, SceneLoadMode::Replace).unwrap();
    let world = app.world_mut();
    let mut bodies = world.query::<(bevy_ecs::entity::Entity, &Name, &Skin)>();
    let (body, _, skin) = bodies
        .iter(world)
        .find(|(_, name, _)| name.0 == "Body")
        .unwrap();
    assert_eq!(skin.joints, ["../Root", "../Root/Tip"]);

    let min_x = |app: &mut App| {
        app.update(std::time::Duration::from_secs_f64(1.0 / 60.0))
            .unwrap();
        update_skins(app.world_mut());
        let world = app.world();
        let mesh = world.get::<SkinnedMesh>(body).unwrap().mesh;
        let assets = world.resource::<AssetServer>();
        let cooked = world.get::<MeshRenderer>(body).unwrap().mesh;
        assert!(assets
            .meshes
            .get(cooked)
            .unwrap()
            .vertices
            .iter()
            .all(|vertex| vertex.position[0].abs() < 0.11));
        assets
            .meshes
            .get(mesh)
            .unwrap()
            .vertices
            .iter()
            .fold(f32::INFINITY, |min, vertex| min.min(vertex.position[0]))
    };
    assert!((min_x(&mut app) + 0.1).abs() < 0.05);
    // The clip loops each second; stop just before it wraps.
    for _ in 0..55 {
        app.update(std::time::Duration::from_secs_f64(1.0 / 60.0))
            .unwrap();
    }
    // The top ring sits about one meter left of the Tip joint.
    let folded = min_x(&mut app);
    assert!((folded + 1.0).abs() < 0.05, "{folded}");

    std::fs::remove_dir_all(folder).unwrap();
}

/// `scene add-model` keeps the box's blend shapes, its clip keys the
/// weights, and the hat primitive reshapes with the box's weights.
#[cfg(feature = "gltf")]
#[test]
fn add_model_keeps_gltf_morph_weights_and_reshapes_every_part() {
    use rusting_engine::assets::AssetServer;
    use rusting_engine::runtime::{
        load_scene, save_scene, update_skins, Animation, AnimationProperty,
        Children, Morph, Name, SceneLoadMode, SkinnedMesh,
    };
    use rusting_engine::{App, AssetPlugin};

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("samples/vertical_slice/squash_box.gltf");
    let folder = std::env::temp_dir()
        .join(format!("rusting-morph-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let model = folder.join("squash_box.gltf");
    std::fs::copy(fixture, &model).unwrap();
    let scene = folder.join("box.rscene");
    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    save_scene(app.world_mut(), &scene, "box").unwrap();

    let result = rusting_engine::cli::add_model(&scene, &model, "Toy", false);
    assert!(result.ok, "{:?}", result.diagnostics);

    let mut app = App::new();
    app.add_plugin(AssetPlugin).unwrap();
    load_scene(app.world_mut(), &scene, SceneLoadMode::Replace).unwrap();
    let world = app.world_mut();
    let mut boxes = world.query::<(bevy_ecs::entity::Entity, &Name, &Morph)>();
    let (body, _, morph) = boxes
        .iter(world)
        .find(|(_, name, _)| name.0 == "Box")
        .unwrap();
    assert_eq!(morph.weights, [0.0, 0.0]);
    let hat = world.get::<Children>(body).unwrap().0[0];
    assert!(world.get::<Morph>(hat).is_none());
    let mut clips = world.query::<&Animation>();
    let track = &clips.single(world).unwrap().clips[0].tracks[0];
    assert!(matches!(
        &track.property,
        AnimationProperty::Field { component, path }
            if component == "rusting.morph" && path == "/weights"
    ));
    assert_eq!(track.keys[1].value, [1.0, 0.0]);

    let top = |app: &App, entity| {
        let world = app.world();
        let mesh = world.get::<SkinnedMesh>(entity).unwrap().mesh;
        world
            .resource::<AssetServer>()
            .meshes
            .get(mesh)
            .unwrap()
            .vertices
            .iter()
            .fold(f32::NEG_INFINITY, |max, vertex| max.max(vertex.position[1]))
    };
    // One second in, "squash" is fully on: the box and its hat halve.
    for _ in 0..60 {
        app.update(std::time::Duration::from_secs_f64(1.0 / 60.0))
            .unwrap();
    }
    update_skins(app.world_mut());
    let (body_top, hat_top) = (top(&app, body), top(&app, hat));
    assert!((body_top - 0.5).abs() < 0.05, "{body_top}");
    assert!((hat_top - 0.55).abs() < 0.05, "{hat_top}");

    std::fs::remove_dir_all(folder).unwrap();
}
