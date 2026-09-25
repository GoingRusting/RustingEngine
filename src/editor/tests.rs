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
