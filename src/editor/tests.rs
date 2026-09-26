//! Tests for the editor state, dock layout, project tools, and GUI elements.

use std::time::Duration;

use super::*;

#[test]
fn editor_play_uses_debug_builds_by_default() {
    let state = EditorState::default();

    assert_eq!(state.game_build_profile, GameBuildProfile::Debug);
    assert!(state.project_root.is_empty());
    assert!(state.scene_path.is_empty());
    assert_eq!(state.code_path, "src/main.rs");
    assert_eq!(GameBuildProfile::Debug.cargo_flag(), None);
    assert_eq!(GameBuildProfile::Release.cargo_flag(), Some("--release"));
}

#[test]
fn editor_plugin_builds_panels_and_preserves_selection() {
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    let entity = app.spawn((Name("Cube".into()), Transform::default()));
    app.world_mut().resource_mut::<EditorState>().selected = Some(entity);

    app.update(Duration::from_millis(16)).unwrap();
    let context = Context::default();
    let output = context.run(egui::RawInput::default(), |context| {
        draw_editor_view(app.world_mut(), context);
    });

    assert_eq!(app.world().resource::<EditorState>().selected, Some(entity));
    assert!(!output.shapes.is_empty());
}

#[test]
fn material_edits_copy_shared_materials_and_edit_owned_ones_in_place() {
    let mut app = App::new();
    app.add_plugin(crate::AssetPlugin).unwrap();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    let world = app.world_mut();
    let (mesh, fallback) = {
        let assets = world.resource::<AssetServer>();
        (assets.fallback_mesh, assets.fallback_material)
    };
    let renderer = MeshRenderer {
        mesh,
        material: fallback,
        cast_shadows: true,
        receive_shadows: true,
    };
    let edited = world.spawn(renderer).id();
    let other = world.spawn(renderer).id();
    let material_of = |world: &World, entity| {
        world.get::<MeshRenderer>(entity).unwrap().material
    };
    let red = MaterialAsset {
        base_color: [1.0, 0.0, 0.0, 1.0],
        ..MaterialAsset::default()
    };

    // The fallback is shared, so the edit gets its own copy.
    super::view::set_material(world, edited, red.clone());
    let owned = material_of(world, edited);
    assert_ne!(owned, fallback);
    assert_eq!(material_of(world, other), fallback);
    assert_ne!(
        world.resource::<AssetServer>().materials.get(fallback),
        Some(&red)
    );

    // An owned material is edited in place instead of leaking a copy per
    // drag frame.
    let blue = MaterialAsset {
        base_color: [0.0, 0.0, 1.0, 1.0],
        ..red
    };
    super::view::set_material(world, edited, blue.clone());
    assert_eq!(material_of(world, edited), owned);
    assert_eq!(
        world.resource::<AssetServer>().materials.get(owned),
        Some(&blue)
    );

    // Sharing the owned material again makes the next edit copy it.
    world.get_mut::<MeshRenderer>(other).unwrap().material = owned;
    super::view::set_material(world, edited, red.clone());
    assert_ne!(material_of(world, edited), owned);
    assert_eq!(
        world.resource::<AssetServer>().materials.get(owned),
        Some(&blue)
    );
}

#[test]
fn scene_render_settings_edits_are_undoable_scene_changes() {
    use crate::runtime::{CullingMode, QualityProfile};
    let mut app = App::new();
    app.add_plugin(crate::AssetPlugin).unwrap();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    let world = app.world_mut();
    let mut history = EditorHistory::default();
    let mut state = EditorState::default();
    let defaults = (QualityProfile::Auto, CullingMode::Auto);
    let current = |world: &World| {
        let settings = world.resource::<RenderSettings>();
        (settings.quality, settings.culling)
    };

    let mut edited = world.resource::<RenderSettings>().clone();
    edited.quality = QualityProfile::High;
    edited.culling = CullingMode::FrustumAndOcclusion;
    super::view::apply_render_settings(
        world,
        &mut history,
        &mut state,
        edited.clone(),
        defaults,
    );
    let chosen = (QualityProfile::High, CullingMode::FrustumAndOcclusion);
    assert_eq!(current(world), chosen);
    assert!(state.scene_dirty);
    assert_eq!(history.undo.len(), 1);

    // Undo reloads the snapshot taken before the edit.
    let before = history.undo.pop_back().unwrap();
    crate::runtime::load_scene_document(world, &before, SceneLoadMode::Replace)
        .unwrap();
    assert_eq!(current(world), defaults);

    // A panel copy taken before that Undo must not write the edit back.
    super::view::apply_render_settings(
        world,
        &mut history,
        &mut state,
        edited,
        chosen,
    );
    assert_eq!(current(world), defaults);
    assert!(history.undo.is_empty());
}

#[test]
fn scene_view_shading_applies_only_in_the_scene_workspace() {
    let mut app = App::new();
    app.add_plugin(EditorPlugin).unwrap();
    assert_eq!(editor_debug_view(app.world()), SceneDebugView::Lit);

    app.world_mut()
        .resource_mut::<EditorGizmoSettings>()
        .shading = SceneDebugView::Normals;
    assert_eq!(editor_debug_view(app.world()), SceneDebugView::Normals);

    app.world_mut().resource_mut::<EditorState>().workspace =
        EditorWorkspace::Game;
    assert_eq!(editor_debug_view(app.world()), SceneDebugView::Lit);
}

#[test]
fn code_editor_keeps_relative_and_absolute_paths_inside_project() {
    assert!(project_source_path("testGame", "../outside.comp").is_err());
    assert!(project_source_path("testGame", "/tmp/outside.comp").is_err());
    assert!(project_source_path("testGame", "shaders/custom.comp").is_ok());
    assert!(
        project_source_path("testGame", "src/shaders/compute/full.comp")
            .is_ok()
    );
    let project = std::path::Path::new("testGame").canonicalize().unwrap();
    assert!(project_source_path(
        project.to_str().unwrap(),
        project.join("src/main.rs").to_str().unwrap()
    )
    .is_ok());
}

#[test]
fn editor_document_paths_always_use_forward_slashes() {
    assert_eq!(
        editor_relative_path(std::path::Path::new(r"src\main.rs")),
        "src/main.rs"
    );
}

#[test]
fn opening_project_replaces_stale_code_with_relative_document_paths() {
    let folder = std::env::temp_dir()
        .join(format!("rusting-open-path-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let project = create_project(&folder, "Fresh Game").unwrap();
    let mut state = EditorState {
        project_root: "/old/project".into(),
        scene_path: "/old/project/scenes/old.rscene".into(),
        code_path: "/old/project/src/main.rs".into(),
        code_source: "old project source".into(),
        code_dirty: true,
        ..EditorState::default()
    };

    set_open_project_paths(&mut state, &project);

    assert_eq!(state.project_root, project.root.display().to_string());
    assert_eq!(state.scene_path, "scenes/main.rscene");
    assert_eq!(state.code_path, "src/main.rs");
    assert!(state.code_source.is_empty());
    assert!(!state.code_dirty);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn project_manager_requires_an_explicit_creation_folder() {
    assert!(ProjectManagerState::default()
        .parent_directory
        .as_os_str()
        .is_empty());
}

#[test]
fn compute_shader_validation_checks_entry_point_and_workgroup() {
    assert!(validate_project_source(
        "testGame",
        "testGame/shaders/custom.comp",
        "#version 450\nlayout(local_size_x = 64) in;\nvoid main() {}",
    )
    .is_ok());
    assert!(validate_project_source(
        "testGame",
        "testGame/shaders/custom.comp",
        "#version 450\nvoid main() {}",
    )
    .is_err());
}

#[test]
fn dock_areas_can_split_change_type_and_close() {
    let mut layout = EditorDockNode::Area {
        id: 1,
        panel: EditorPanel::Scene,
    };
    assert!(layout.split(1, EditorSplitAxis::Columns, 2));
    assert!(layout.set_panel(2, EditorPanel::Code));
    assert!(layout.close(1));
    assert_eq!(
        layout,
        EditorDockNode::Area {
            id: 2,
            panel: EditorPanel::Code,
        }
    );
}

#[test]
fn duplicate_panels_get_different_widget_id_spaces() {
    let mut layout = EditorDockNode::Split {
        axis: EditorSplitAxis::Columns,
        ratio: 0.5,
        first: Box::new(EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Console,
        }),
        second: Box::new(EditorDockNode::Area {
            id: 2,
            panel: EditorPanel::Console,
        }),
    };
    let context = Context::default();
    let mut panel_ids = Vec::new();
    let mut active_area = 1;
    let mut actions = Vec::new();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };

    let _ = context.run(input, |context| {
        CentralPanel::default().show(context, |ui| {
            let rect = ui.available_rect_before_wrap();
            show_dock_node(
                ui,
                &mut layout,
                rect,
                &mut active_area,
                &mut actions,
                &mut |ui, _panel| panel_ids.push(ui.next_auto_id()),
            );
        });
    });

    assert_eq!(panel_ids.len(), 2);
    assert_ne!(panel_ids[0], panel_ids[1]);
}

#[test]
fn dock_header_buttons_have_the_same_size_and_height() {
    let context = Context::default();
    let mut rects = Vec::new();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(300.0, 100.0),
        )),
        ..Default::default()
    };

    let _ = context.run(input, |context| {
        CentralPanel::default().show(context, |ui| {
            ui.horizontal(|ui| {
                for icon in [
                    EditorIcon::SplitColumns,
                    EditorIcon::SplitRows,
                    EditorIcon::Close,
                ] {
                    rects.push(dock_header_button(ui, icon, "test").rect);
                }
            });
        });
    });

    assert_eq!(rects.len(), 3);
    assert!(rects
        .iter()
        .all(|rect| rect.size() == egui::vec2(22.0, 20.0)));
    assert!(rects.iter().all(|rect| rect.top() == rects[0].top()));
}

#[test]
fn dock_area_clips_panel_content_to_its_own_rectangle() {
    let context = Context::default();
    let mut layout = EditorDockNode::Area {
        id: 1,
        panel: EditorPanel::Hierarchy,
    };
    let mut active_area = 1;
    let mut actions = Vec::new();
    let mut panel_clip = None;
    let mut expected_clip = None;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        ..Default::default()
    };

    let _ = context.run(input, |context| {
        CentralPanel::default().show(context, |ui| {
            let area_rect = egui::Rect::from_min_size(
                ui.min_rect().min,
                egui::vec2(120.0, 100.0),
            );
            expected_clip =
                Some(ui.clip_rect().intersect(area_rect.shrink(1.0)));
            show_dock_node(
                ui,
                &mut layout,
                area_rect,
                &mut active_area,
                &mut actions,
                &mut |ui, _panel| panel_clip = Some(ui.clip_rect()),
            );
        });
    });

    assert_eq!(panel_clip, expected_clip);
}

#[test]
fn clicking_anywhere_inside_a_dock_selects_it() {
    let mut layout = EditorDockNode::Split {
        axis: EditorSplitAxis::Columns,
        ratio: 0.5,
        first: Box::new(EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Hierarchy,
        }),
        second: Box::new(EditorDockNode::Area {
            id: 2,
            panel: EditorPanel::Inspector,
        }),
    };
    let context = Context::default();
    let mut active_area = 1;
    let mut actions = Vec::new();
    let pointer = egui::pos2(300.0, 80.0);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 200.0),
        )),
        events: vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ],
        ..Default::default()
    };

    let _ = context.run(input, |context| {
        CentralPanel::default().show(context, |ui| {
            let rect = ui.available_rect_before_wrap();
            show_dock_node(
                ui,
                &mut layout,
                rect,
                &mut active_area,
                &mut actions,
                &mut |_ui, _panel| {},
            );
        });
    });

    assert_eq!(active_area, 2);
}

#[test]
fn dock_layout_round_trips_through_project_settings() {
    let file = EditorLayoutFile {
        layout: EditorDockNode::default_layout(),
        active_area: 2,
        next_area_id: 5,
    };
    let json = serde_json::to_string(&file).unwrap();
    let restored: EditorLayoutFile = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.layout, file.layout);
    assert_eq!(restored.active_area, 2);
    assert_eq!(restored.next_area_id, 5);
}

#[test]
fn imported_project_files_never_overwrite_existing_assets() {
    let folder = std::env::temp_dir()
        .join(format!("rusting-editor-assets-{}", uuid::Uuid::new_v4()));
    let project = folder.join("project");
    let source_folder = folder.join("source");
    std::fs::create_dir_all(project.join("assets")).unwrap();
    std::fs::create_dir_all(&source_folder).unwrap();
    std::fs::write(project.join("assets/cube.glb"), "old").unwrap();
    std::fs::write(source_folder.join("cube.glb"), "new").unwrap();

    let imported = copy_into_project_assets(
        project.to_str().unwrap(),
        &source_folder.join("cube.glb"),
    )
    .unwrap();

    assert_eq!(
        std::fs::read(project.join("assets/cube.glb")).unwrap(),
        b"old"
    );
    assert_eq!(imported.file_name().unwrap(), "cube_2.glb");
    assert_eq!(std::fs::read(imported).unwrap(), b"new");
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn cargo_worker_completion_is_received_without_blocking() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut build = EditorBuildState {
        running: true,
        output: "running".into(),
        console_cursor: 0,
        receiver: std::sync::Mutex::new(Some(receiver)),
        ..Default::default()
    };
    sender
        .send(BuildWorkerMessage::Output(
            "\nruntime panic details\n".into(),
        ))
        .unwrap();

    assert_eq!(build.poll(), None);
    assert!(build.running);
    assert!(build.output.contains("runtime panic details"));

    sender
        .send(BuildWorkerMessage::Finished(BuildFinished {
            success: true,
            output: "task finished".into(),
        }))
        .unwrap();

    assert_eq!(build.poll(), Some(true));
    assert!(!build.running);
    assert!(build.output.contains("runtime panic details"));
    assert!(build.output.contains("task finished"));
    let console_output = build.take_console_output();
    assert!(console_output.contains("runtime panic details"));
    assert!(console_output.contains("task finished"));
    assert!(build.take_console_output().is_empty());
}

#[test]
fn console_cursor_survives_output_truncation() {
    let mut build = EditorBuildState::default();
    // Multi-byte text makes a stale byte offset land inside a character.
    build.append_output(&"é".repeat(200_000));
    build.take_console_output();
    build.append_output(&"ü".repeat(100_000));
    build.append_output("tail");
    let text = build.take_console_output();
    assert_eq!(text, format!("{}tail", "ü".repeat(100_000)));
    assert!(build.take_console_output().is_empty());
}

#[test]
fn export_package_contains_executable_scene_assets_and_readme() {
    let folder = std::env::temp_dir()
        .join(format!("rusting-export-test-{}", uuid::Uuid::new_v4()));
    let project = folder.join("project");
    let exports = folder.join("exports");
    std::fs::create_dir_all(project.join("build")).unwrap();
    std::fs::create_dir_all(project.join("assets")).unwrap();
    std::fs::create_dir_all(&exports).unwrap();
    let executable = project.join("fake_game");
    std::fs::write(&executable, "binary").unwrap();
    std::fs::write(project.join("build/main.rscene.bin"), "scene").unwrap();
    std::fs::write(project.join("assets/texture.png"), "texture").unwrap();

    let exported = package_game_files(
        &project,
        &executable,
        &exports,
        "Fake Game",
        "fake_game",
        std::path::Path::new("build/main.rscene.bin"),
        None,
    )
    .unwrap();

    let executable_name = if cfg!(target_os = "windows") {
        "fake_game.exe"
    } else {
        "fake_game"
    };
    assert!(exported.join(executable_name).is_file());
    assert!(exported.join("build/main.rscene.bin").is_file());
    assert!(exported.join("assets/texture.png").is_file());
    assert!(exported.join("README.txt").is_file());

    // A Windows export from any system gets an .exe in its own folder.
    let windows = package_game_files(
        &project,
        &executable,
        &exports,
        "Fake Game",
        "fake_game",
        std::path::Path::new("build/main.rscene.bin"),
        Some(super::WINDOWS_TARGET),
    )
    .unwrap();
    assert!(windows.ends_with("fake_game_windows_export"));
    assert!(windows.join("fake_game.exe").is_file());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn every_editor_panel_draws_even_when_optional_resources_are_missing() {
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    app.world_mut().remove_resource::<EditorConsole>();
    let context = Context::default();

    for (index, panel) in EditorPanel::ALL.into_iter().enumerate() {
        app.world_mut().resource_mut::<EditorState>().dock_layout =
            EditorDockNode::Area {
                id: index as u64 + 1,
                panel,
            };
        let _ = context.run(egui::RawInput::default(), |context| {
            draw_editor_view(app.world_mut(), context);
        });
    }
}

#[test]
fn idle_editor_stops_continuous_redraw() {
    let mut app = App::new();
    app.add_plugin(EditorPlugin).unwrap();
    assert!(!editor_needs_continuous_redraw(app.world()));

    app.world_mut().resource_mut::<EditorState>().mode = EditorMode::Play;
    assert!(editor_needs_continuous_redraw(app.world()));

    app.world_mut().resource_mut::<EditorState>().mode = EditorMode::Paused;
    app.world_mut().resource_mut::<EditorFlyCamera>().active = true;
    assert!(editor_needs_continuous_redraw(app.world()));
}

#[cfg(unix)]
#[test]
fn streamed_command_sends_lines_early_and_stops_on_request() {
    let (sender, receiver) = std::sync::mpsc::channel();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_stop = std::sync::Arc::clone(&stop);
    let worker = std::thread::spawn(move || {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "echo first; exec sleep 30"]);
        run_streamed(&mut command, &sender, &worker_stop)
    });

    // The first line arrives while the process still runs.
    match receiver.recv_timeout(Duration::from_secs(10)).unwrap() {
        BuildWorkerMessage::Output(text) => assert_eq!(text, "first\n"),
        BuildWorkerMessage::Finished(_) => panic!("finished too early"),
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(worker.join().unwrap().unwrap().is_none());
}

#[test]
fn editor_scene_restores_camera_pose_and_selection_after_reload() {
    let folder = std::env::temp_dir()
        .join(format!("rusting-editor-meta-{}", uuid::Uuid::new_v4()));
    let path = folder.join("main.rscene");
    let mut app = App::new();
    app.insert_resource(AssetServer::default());
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    let cube = app.spawn((
        SceneId(uuid::Uuid::new_v4()),
        Name("Cube".into()),
        Transform::default(),
    ));
    let lamp = app.spawn((
        SceneId(uuid::Uuid::new_v4()),
        Name("Lamp".into()),
        Transform::default(),
    ));
    let world = app.world_mut();
    // Spawned like the editor binary does: no `SceneId`, so it is not saved.
    let camera = world
        .spawn((
            Name("Editor Camera".into()),
            Transform::new([1.0, 2.0, 3.0]),
        ))
        .id();
    let mut state = EditorState {
        editor_camera: Some(camera),
        selected: Some(lamp),
        selection: vec![cube, lamp],
        ..Default::default()
    };
    world.resource_mut::<EditorFlyCamera>().orbit_distance = 7.0;
    save_editor_scene(world, &state, &path).unwrap();

    world.get_mut::<Transform>(camera).unwrap().position = [0.0; 3];
    world.resource_mut::<EditorFlyCamera>().orbit_distance = 1.0;
    assert_eq!(load_editor_scene(world, &mut state, &path).unwrap(), 2);

    assert_eq!(
        world.get::<Transform>(camera).unwrap().position,
        [1.0, 2.0, 3.0]
    );
    assert_eq!(world.resource::<EditorFlyCamera>().orbit_distance, 7.0);
    let names: Vec<_> = state
        .selection
        .iter()
        .map(|&entity| world.get::<Name>(entity).unwrap().0.clone())
        .collect();
    assert_eq!(names, ["Lamp", "Cube"], "active object stays first");
    assert_eq!(state.selected, state.selection.first().copied());
    assert!(world.get_entity(cube).is_err(), "reload respawns entities");

    std::fs::remove_file(folder.join("main.rscene.editor.json")).unwrap();
    load_editor_scene(world, &mut state, &path).unwrap();
    assert!(state.selection.is_empty() && state.selected.is_none());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn added_models_keep_their_node_tree_under_one_saved_root() {
    let folder = std::env::temp_dir()
        .join(format!("rusting-editor-model-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    // One triangle without normals, so the import has to make flat ones.
    let positions: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    std::fs::write(folder.join("tri.bin"), &positions).unwrap();
    let path = folder.join("crate.gltf");
    std::fs::write(
        &path,
        r#"{
          "asset": {"version": "2.0"},
          "buffers": [{"uri": "tri.bin", "byteLength": 36}],
          "bufferViews": [{"buffer": 0, "byteLength": 36}],
          "accessors": [{"bufferView": 0, "componentType": 5126,
            "count": 3, "type": "VEC3",
            "min": [0, 0, 0], "max": [1, 1, 0]}],
          "cameras": [{"type": "perspective",
            "perspective": {"yfov": 1.0, "znear": 0.1}}],
          "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
          "nodes": [
            {"name": "Body", "mesh": 0, "children": [1],
             "translation": [0, 2, 0]},
            {"name": "Body", "camera": 0}],
          "scenes": [{"nodes": [0]}]
        }"#,
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugin(crate::AssetPlugin).unwrap();
    let world = app.world_mut();

    let first = add_model_to_scene(world, &path).unwrap();
    let second = add_model_to_scene(world, &path).unwrap();

    assert_eq!(world.get::<Name>(first).unwrap().0, "crate");
    assert_eq!(world.get::<Name>(second).unwrap().0, "crate 2");
    let mut objects = world.query::<(Entity, &Name, Option<&Parent>)>();
    let objects = objects
        .iter(world)
        .map(|(entity, name, parent)| {
            (entity, name.0.clone(), parent.map(|p| p.0))
        })
        .collect::<Vec<_>>();
    // Two roots, and a body and a camera under each; every name is unique.
    assert_eq!(objects.len(), 6);
    let mut names = objects.iter().map(|(_, name, _)| name).collect::<Vec<_>>();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 6);
    for root in [first, second] {
        let body = objects
            .iter()
            .find(|(_, _, parent)| *parent == Some(root))
            .unwrap()
            .0;
        assert_eq!(
            world.get::<Transform>(body).unwrap().position,
            [0.0, 2.0, 0.0]
        );
        let camera = objects
            .iter()
            .find(|(_, _, parent)| *parent == Some(body))
            .unwrap()
            .0;
        assert!(world.get::<Camera>(camera).is_some());
        let mesh = world.get::<MeshRenderer>(body).unwrap().mesh;
        let assets = world.resource::<AssetServer>();
        for vertex in &assets.meshes.get(mesh).unwrap().vertices {
            assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
        }
    }
    // Every object saves with the scene.
    let mut ids = world.query::<&SceneId>();
    assert_eq!(ids.iter(world).count(), 6);
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn inspector_shows_gpu_sync_mode_and_readback_age() {
    use crate::runtime::{
        FrameTime, GpuStateMirror, PhysicsBody, PhysicsSyncMode,
        SimulationClass,
    };
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    let entity = app.spawn((
        Name("Crate".into()),
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::Gpu,
            ..PhysicsBody::default()
        },
        PhysicsSyncMode::SelectedState,
        GpuStateMirror {
            tick: 3,
            transform: Transform::default(),
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            custom_values: None,
        },
    ));
    app.world_mut().resource_mut::<FrameTime>().fixed_tick = 5;
    {
        let mut state = app.world_mut().resource_mut::<EditorState>();
        state.selected = Some(entity);
        state.dock_layout = EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Inspector,
        };
    }
    let context = Context::default();
    let output = context.run(egui::RawInput::default(), |context| {
        draw_editor_view(app.world_mut(), context);
    });

    let mut texts = String::new();
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            texts.push_str(text.galley.text());
            texts.push('\n');
        }
    }
    assert!(texts.contains("Selected State"), "{texts}");
    assert!(texts.contains("tick 3 (2 ticks old)"), "{texts}");
    // The dedicated row replaces the raw registered-component section.
    assert!(!texts.contains("rusting.physics_sync"), "{texts}");
}

#[test]
fn inspector_shows_the_auto_simulation_decision() {
    use crate::runtime::{
        AllocationDecision, AllocationReason, AutoSimulation, PhysicsBody,
        SimulationClass,
    };
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    let entity = app.spawn((
        Name("Crate".into()),
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::Gpu,
            ..PhysicsBody::default()
        },
        AutoSimulation {
            decision: Some(AllocationDecision {
                class: SimulationClass::Gpu,
                reason: AllocationReason::ManyBodies,
            }),
        },
    ));
    {
        let mut state = app.world_mut().resource_mut::<EditorState>();
        state.selected = Some(entity);
        state.dock_layout = EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Inspector,
        };
    }
    let context = Context::default();
    let output = context.run(egui::RawInput::default(), |context| {
        draw_editor_view(app.world_mut(), context);
    });

    let mut texts = String::new();
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            texts.push_str(text.galley.text());
            texts.push('\n');
        }
    }
    assert!(texts.contains("Auto CPU/GPU"), "{texts}");
    assert!(texts.contains("Gpu (ManyBodies)"), "{texts}");
    assert!(!texts.contains("rusting.auto_simulation"), "{texts}");
}

#[test]
fn queued_shortcuts_delete_and_undo_like_the_menu() {
    let mut app = App::new();
    app.add_plugin(crate::AssetPlugin).unwrap();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    let entity = app.spawn((
        Name("Crate".into()),
        Transform::default(),
        crate::runtime::SceneId(uuid::Uuid::new_v4()),
    ));
    {
        let mut state = app.world_mut().resource_mut::<EditorState>();
        state.selected = Some(entity);
        state.selection = vec![entity];
    }
    let count = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Name>().iter(world).count()
    };
    let frame = |app: &mut App, command| {
        app.world_mut()
            .resource_mut::<EditorCommandQueue>()
            .0
            .push(command);
        let context = Context::default();
        let _ = context.run(egui::RawInput::default(), |context| {
            draw_editor_view(app.world_mut(), context);
        });
    };
    frame(&mut app, EditorAction::DeleteSelection);
    assert_eq!(
        count(&mut app),
        0,
        "{:?}",
        app.world().resource::<EditorState>().scene_message
    );
    assert!(app.world().resource::<EditorCommandQueue>().0.is_empty());
    frame(&mut app, EditorAction::Undo);
    assert_eq!(count(&mut app), 1);

    let entity = {
        let world = app.world_mut();
        world
            .query::<(bevy_ecs::entity::Entity, &Name)>()
            .iter(world)
            .next()
            .unwrap()
            .0
    };
    app.world_mut().resource_mut::<EditorState>().selected = Some(entity);
    frame(&mut app, EditorAction::RenameSelection);
    let state = app.world().resource::<EditorState>();
    assert_eq!(state.rename_target, Some(entity));
    assert_eq!(state.rename_draft, "Crate");
}

#[test]
fn shortcuts_area_lists_every_action_with_its_key() {
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    app.world_mut().resource_mut::<EditorState>().dock_layout =
        EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Shortcuts,
        };
    app.world_mut().resource_mut::<EditorShortcuts>().capturing =
        Some(ShortcutAction::Editor(EditorAction::Redo));
    let context = Context::default();
    let output = context.run(egui::RawInput::default(), |context| {
        draw_editor_view(app.world_mut(), context);
    });
    let mut texts = String::new();
    for clipped in &output.shapes {
        if let egui::Shape::Text(text) = &clipped.shape {
            texts.push_str(text.galley.text());
            texts.push('\n');
        }
    }
    for expected in [
        "Keyboard Shortcuts",
        "Undo",
        "Ctrl+Z",
        "Press a key...",
        "Fly Camera",
        "Numpad0",
    ] {
        assert!(texts.contains(expected), "{expected} missing from {texts}");
    }
}

#[test]
fn font_scale_multiplies_every_default_text_size() {
    let context = egui::Context::default();
    super::configure_editor_style(&context);
    super::apply_font_scale(&context, 1.5);
    super::apply_font_scale(&context, 1.3);
    let defaults = egui::Style::default().text_styles;
    context
        .style()
        .text_styles
        .iter()
        .for_each(|(style, font)| {
            assert!((font.size - defaults[style].size * 1.3).abs() < 1e-4);
        });
}

#[test]
fn scene_area_draws_the_offscreen_view_image_over_its_viewport() {
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    app.world_mut().resource_mut::<EditorState>().dock_layout =
        EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Scene,
        };
    let context = Context::default();
    let output = context.run(egui::RawInput::default(), |context| {
        draw_editor_view(app.world_mut(), context);
    });
    let image = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect)
                if rect.fill_texture_id() == super::SCENE_VIEW_TEXTURE =>
            {
                Some(rect.rect)
            }
            egui::Shape::Mesh(mesh)
                if mesh.texture_id == super::SCENE_VIEW_TEXTURE =>
            {
                Some(egui::Rect::from_points(
                    &mesh.vertices.iter().map(|v| v.pos).collect::<Vec<_>>(),
                ))
            }
            _ => None,
        })
        .expect("scene view image is drawn");
    let viewport = *app.world().resource::<EditorViewport>();
    assert!(viewport.valid);
    // One point is one pixel here; the viewport rounds outward.
    assert_eq!(
        viewport.offset,
        [image.min.x.floor() as u32, image.min.y.floor() as u32]
    );
    assert_eq!(viewport.extent[0], image.width().ceil() as u32);
}

#[test]
fn reparenting_a_selection_keeps_world_positions_and_nested_children() {
    let mut app = App::new();
    app.add_plugin(crate::AssetPlugin).unwrap();
    let scene_object = |app: &mut App, name: &str, position| {
        app.spawn((
            Name(name.into()),
            Transform::new(position),
            crate::runtime::SceneId(uuid::Uuid::new_v4()),
        ))
    };
    let target = scene_object(&mut app, "Target", [5.0, 0.0, 0.0]);
    let a = scene_object(&mut app, "A", [1.0, 2.0, 0.0]);
    let b = scene_object(&mut app, "B", [0.0, 0.0, 3.0]);
    let b_child = scene_object(&mut app, "B Child", [0.0, 1.0, 0.0]);
    app.world_mut()
        .entity_mut(b_child)
        .insert(crate::runtime::Parent(b));
    crate::runtime::propagate_transforms(app.world_mut());

    let moved = super::hierarchy::reparent_entities(
        app.world_mut(),
        &[a, b, b_child],
        Some(target),
    )
    .unwrap()
    .unwrap();
    crate::runtime::propagate_transforms(app.world_mut());
    let world = app.world_mut();
    let by_name = |world: &mut World, name: &str| {
        let mut query = world.query::<(Entity, &Name)>();
        query
            .iter(world)
            .find_map(|(entity, found)| (found.0 == name).then_some(entity))
            .unwrap()
    };
    let target = by_name(world, "Target");
    assert_eq!(world.get::<Name>(moved).unwrap().0, "A");
    for (name, parent, position) in [
        ("A", target, [1.0, 2.0, 0.0]),
        ("B", target, [0.0, 0.0, 3.0]),
        ("B Child", by_name(world, "B"), [0.0, 1.0, 3.0]),
    ] {
        let entity = by_name(world, name);
        assert_eq!(
            world.get::<crate::runtime::Parent>(entity).unwrap().0,
            parent
        );
        let global = world
            .get::<crate::runtime::GlobalTransform>(entity)
            .unwrap();
        for (actual, expected) in global.matrix[3].iter().zip(position) {
            assert!((actual - expected).abs() < 1e-5, "{name}");
        }
    }
    let target_child = by_name(world, "B");
    assert!(super::hierarchy::reparent_entities(
        world,
        &[target],
        Some(target_child)
    )
    .is_err());
}

#[test]
fn registered_inspectors_draw_and_edit_their_component() {
    #[derive(
        bevy_ecs::component::Component,
        serde::Serialize,
        serde::Deserialize,
        Default,
        Debug,
        PartialEq,
    )]
    struct Health {
        points: u32,
    }
    fn draw_health(ui: &mut egui::Ui, health: &mut Health) -> bool {
        ui.label(format!("Typed health {}", health.points));
        health.points += 1;
        true
    }
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    app.register_scene_component::<Health>("game.health")
        .unwrap();
    app.world_mut()
        .resource_mut::<super::InspectorRegistry>()
        .register("game.health", draw_health);
    let entity = app.spawn((
        Name("Hero".into()),
        Transform::default(),
        Health { points: 7 },
    ));
    {
        let mut state = app.world_mut().resource_mut::<EditorState>();
        state.selected = Some(entity);
        state.dock_layout = EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Inspector,
        };
    }
    let context = Context::default();
    let output = context.run(egui::RawInput::default(), |context| {
        draw_editor_view(app.world_mut(), context);
    });
    let texts = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(texts.contains("Typed health 7"), "{texts}");
    // The generic JSON editor would show the field name as a row label.
    assert!(!texts.contains("points"), "{texts}");
    assert_eq!(
        app.world().get::<Health>(entity),
        Some(&Health { points: 8 })
    );
}

#[test]
fn dropping_an_image_on_hierarchy_rows_and_texture_slots_assigns_it() {
    let folder = std::env::temp_dir()
        .join(format!("rusting-editor-drop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&folder).unwrap();
    let image_path = folder.join("bricks.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 20, 20, 255]))
        .save(&image_path)
        .unwrap();

    let mut app = App::new();
    app.add_plugin(crate::AssetPlugin).unwrap();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    let (mesh, material) = {
        let assets = app.world().resource::<AssetServer>();
        (assets.fallback_mesh, assets.fallback_material)
    };
    let entity = app.spawn((
        Name("Crate".into()),
        Transform::default(),
        SceneId(uuid::Uuid::new_v4()),
        MeshRenderer {
            mesh,
            material,
            cast_shadows: true,
            receive_shadows: true,
        },
    ));
    let texture_of = |world: &World, slot: usize| {
        let material = world.get::<MeshRenderer>(entity).unwrap().material;
        let mut material = world
            .resource::<AssetServer>()
            .materials
            .get(material)
            .cloned()
            .unwrap();
        *super::inspector::texture_slots(&mut material)[slot]
    };

    // Releases an image drag over the first text shape reading `target`.
    let drop_on = |app: &mut App, panel: EditorPanel, target: &str| {
        {
            let mut state = app.world_mut().resource_mut::<EditorState>();
            state.selected = Some(entity);
            state.dock_layout = EditorDockNode::Area { id: 1, panel };
        }
        let context = Context::default();
        let output = context.run(egui::RawInput::default(), |context| {
            draw_editor_view(app.world_mut(), context);
        });
        let position = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == target => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {target} text"));
        egui::DragAndDrop::set_payload(
            &context,
            super::assets_panel::ImageDrag(image_path.clone()),
        );
        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };
        let _ = context.run(input, |context| {
            draw_editor_view(app.world_mut(), context);
        });
    };

    drop_on(&mut app, EditorPanel::Hierarchy, "Crate");
    assert!(texture_of(app.world(), 0).is_some());
    assert!(texture_of(app.world(), 1).is_none());
    drop_on(&mut app, EditorPanel::Inspector, "Normal Map");
    assert!(texture_of(app.world(), 1).is_some());
    // Each drop is its own Undo step.
    assert_eq!(app.world().resource::<EditorHistory>().undo.len(), 2);
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn console_groups_repeats_filters_and_records_status_lines() {
    let mut app = App::new();
    app.add_plugin(crate::runtime::RenderExtractPlugin).unwrap();
    app.add_plugin(EditorPlugin).unwrap();
    {
        let mut console = app.world_mut().resource_mut::<EditorConsole>();
        console.push_from(ConsoleLevel::Warning, "Assets", "Texture missing");
        console.push_from(ConsoleLevel::Warning, "Assets", "Texture missing");
        console.push_from(ConsoleLevel::Info, "Build", "Build finished");
        assert_eq!(console.entries().len(), 2);
        assert_eq!(console.entries()[0].count, 2);
    }
    {
        let mut state = app.world_mut().resource_mut::<EditorState>();
        state.dock_layout = EditorDockNode::Area {
            id: 1,
            panel: EditorPanel::Console,
        };
        state.scene_message = Some("Save As failed: disk full".into());
    }
    let texts = |app: &mut App| {
        let output = Context::default().run(egui::RawInput::default(), |c| {
            draw_editor_view(app.world_mut(), c);
        });
        let mut texts = String::new();
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                texts.push_str(text.galley.text());
                texts.push('\n');
            }
        }
        texts
    };
    // A status line is copied into the Console once, with a guessed level.
    let _ = texts(&mut app);
    let first = texts(&mut app);
    assert!(first.contains("Warnings 2"), "{first}");
    assert!(first.contains("Errors 1"), "{first}");
    assert!(first.contains("x2"), "{first}");
    assert!(first.contains("disk full"), "{first}");

    app.world_mut().resource_mut::<EditorState>().scene_message =
        Some("Undo failed: no snapshot".into());
    let _ = texts(&mut app);
    let last = app
        .world()
        .resource::<EditorConsole>()
        .entries()
        .last()
        .cloned();
    assert_eq!(
        last.map(|entry| (entry.level, entry.source)),
        Some((ConsoleLevel::Error, "Scene"))
    );

    app.world_mut().resource_mut::<EditorConsole>().filter = "build".into();
    let filtered = texts(&mut app);
    assert!(filtered.contains("Build finished"), "{filtered}");
    assert!(!filtered.contains("Texture missing"), "{filtered}");
    let mut console = app.world_mut().resource_mut::<EditorConsole>();
    console.filter.clear();
    console.hidden[ConsoleLevel::Info as usize] = true;
    let hidden = texts(&mut app);
    assert!(!hidden.contains("Build finished"), "{hidden}");
    assert!(hidden.contains("Texture missing"), "{hidden}");
}
