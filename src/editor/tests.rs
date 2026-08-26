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
                for symbol in ["<>", "||", "x"] {
                    rects.push(dock_header_button(ui, symbol, "test").rect);
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
                Some(ui.clip_rect().intersect(area_rect.shrink(2.0)));
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
