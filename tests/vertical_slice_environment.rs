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
