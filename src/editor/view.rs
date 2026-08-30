//! Main editor frame and panel contents.
//!
//! Keeping the complete frame in this file makes the smaller editor modules
//! easier to read while this view is gradually divided into panel modules.

use egui::Pos2;
use nalgebra::{Matrix4, Rotation3, Vector3, Vector4};

use super::picking::{pick_entity, project_world_to_screen, scene_ray};
use super::*;
use crate::editor::overlay::{add_axis, add_bound_box};

/// Draws the interactive editor into an already-open egui frame.
///
/// Window runners use this entry point after forwarding winit input to egui.
///
/// # Arguments
/// * `world` - ECS world containing scene objects and editor resources.
/// * `context` - Current egui frame used to draw all controls.
pub fn draw_editor_view(world: &mut World, context: &Context) {
    // Copy editor values that egui will change during this frame.
    let mut state = world.resource::<EditorState>().clone();
    let mut project_manager = world.resource::<ProjectManagerState>().clone();
    let mut history = world.resource::<EditorHistory>().clone();
    let mut pending_action =
        world.resource::<PendingDestructiveAction>().clone();
    let mut editor_assets = world.resource::<EditorAssetState>().clone();
    let (build_running, build_finished, build_console_output) = {
        let mut build = world.resource_mut::<EditorBuildState>();
        let finished = build.poll();
        let console_output = build.take_console_output();
        (build.running, finished, console_output)
    };
    if !build_console_output.is_empty() {
        world.resource_mut::<EditorConsole>().push(
            ConsoleLevel::Info,
            format!("Cargo Output\n{build_console_output}"),
        );
    }
    if let Some(success) = build_finished {
        world.resource_mut::<EditorConsole>().push(
            if success {
                ConsoleLevel::Info
            } else {
                ConsoleLevel::Error
            },
            if success {
                "Build or game task finished successfully"
            } else {
                "Build or game failed; open Code Editor for output"
            },
        );
    }
    let entities = collect_entities(world);
    let asset_files = project_asset_files(&state.project_root);
    let loaded_textures = world
        .get_resource::<AssetServer>()
        .map(|assets| {
            assets
                .textures
                .paths()
                .map(|(handle, path)| (handle, path.to_path_buf()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let frame_time = *world.resource::<FrameTime>();
    let mut render_settings = world.resource::<RenderSettings>().clone();
    let mut gizmo_settings = *world.resource::<EditorGizmoSettings>();
    let mut gizmo_drag = world.resource::<EditorGizmoDrag>().clone();
    let mut transform_mode = *world.resource::<EditorTransformMode>();
    let editor_shortcuts = world.resource::<EditorShortcuts>().clone();
    let fly_camera_active = world.resource::<EditorFlyCamera>().active;
    let physics_backends = *world.resource::<PhysicsBackendStatus>();
    let asset_counts = world.get_resource::<AssetServer>().map(|assets| {
        (
            assets.meshes.len(),
            assets.materials.len(),
            assets.textures.len(),
            assets.scenes.len(),
        )
    });
    let render_report = world
        .get_resource::<RenderWorld>()
        .map(|render_world| render_world.report);
    let mut edited_transform = state
        .selected
        .and_then(|entity| world.get::<Transform>(entity).copied());
    let mut edited_camera = state
        .selected
        .and_then(|entity| world.get::<Camera>(entity).copied());
    let mut edited_classes = state.selected.map(|entity| {
        world
            .get::<ObjectClasses>(entity)
            .cloned()
            .unwrap_or_default()
    });
    let mut edited_physics = state
        .selected
        .and_then(|entity| world.get::<PhysicsBody>(entity).cloned());
    let mut edited_rigid_body = state
        .selected
        .and_then(|entity| world.get::<RigidBody>(entity).copied());
    let mut edited_collider = state
        .selected
        .and_then(|entity| world.get::<Collider>(entity).copied());
    let original_selected = state.selected;
    let original_transform = edited_transform;
    let original_camera = edited_camera;
    let original_classes = edited_classes.clone();
    let original_physics = edited_physics.clone();
    let original_rigid_body = edited_rigid_body;
    let original_collider = edited_collider;
    let registered_names = registered_component_names(world);
    let custom_values = state
        .selected
        .map(|entity| registered_component_values(world, entity))
        .transpose()
        .unwrap_or_else(|error| {
            state.scene_message = Some(format!("Inspector failed: {error}"));
            None
        })
        .unwrap_or_default();
    if let Some(entity) = state.selected {
        for (name, value) in &custom_values {
            state
                .component_drafts
                .entry((entity, name.clone()))
                .or_insert_with(|| value.clone());
        }
    }
    // Buttons set these small requests while drawing. We apply them later,
    // after egui no longer borrows temporary values.
    let mut viewport_rect = None;
    let mut scene_click_position: Option<Pos2> = None;
    let mut scene_drag_right_started: Option<Pos2> = None;
    let mut scene_drag_right_position: Option<Pos2> = None;
    let mut scene_drag_right_stopped = false;
    let mut scene_drag_left_started: Option<Pos2> = None;
    let mut scene_drag_left_position: Option<Pos2> = None;
    let mut scene_drag_left_stopped = false;
    let mut scene_hover_position: Option<Pos2> = None;
    let mut scene_right_clicked = false;
    let left_pressed = context.input(|input| {
        input.pointer.button_pressed(egui::PointerButton::Primary)
    });
    let right_pressed = context.input(|input| {
        input.pointer.button_pressed(egui::PointerButton::Secondary)
    });
    let escape_pressed =
        context.input(|input| input.key_pressed(egui::Key::Escape));
    let mut save_clicked = false;
    let mut load_clicked = false;
    let mut component_edits = Vec::new();
    let mut add_physics = false;
    let mut remove_physics = false;
    let mut edit_custom_shader = false;
    let mut load_code = false;
    let mut save_code = false;
    let mut validate_code = false;
    let mut run_project = false;
    let mut add_area = false;
    let mut reset_layout = false;
    let mut save_layout = false;
    let mut load_layout = false;
    let mut open_projects = false;
    let mut new_project = false;
    let mut save_project = false;
    let mut undo_clicked = false;
    let mut redo_clicked = false;
    let mut new_scene = false;
    let mut open_scene = false;
    let mut save_scene_as = false;
    let mut entity_request = None;
    let mut asset_request = None;
    let mut build_request = None;
    let mut export_destination = None;
    let has_open_project = !state.project_root.is_empty()
        && std::path::Path::new(&state.project_root)
            .join("project.json")
            .is_file();

    // The top toolbar stays visible even when every area below it changes.
    TopBottomPanel::top("visible_editor_toolbar")
        .frame(
            egui::Frame::NONE
                .fill(gui_elements::EditorTheme::PANEL)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    gui_elements::EditorTheme::BORDER_SOFT,
                )),
        )
        .show(context, |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("editor_toolbar_scroll")
                .scroll_bar_visibility(
                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("RustingEngine");
                        if state.scene_dirty {
                            ui.colored_label(
                                egui::Color32::YELLOW,
                                "Unsaved scene",
                            );
                        }
                        ui.separator();
                        gui_elements::EditorTheme::toolbar_menu(
                            ui,
                            "toolbar_file_menu",
                            "File",
                            66.0,
                            280.0,
                            |ui| {
                                gui_elements::EditorTheme::menu_section(
                                    ui, "PROJECT",
                                );
                                open_projects |= gui_elements::EditorTheme::menu_action(
                                    ui,
                                    "Open Project...",
                                    true,
                                )
                                .clicked();
                                new_project |= gui_elements::EditorTheme::menu_action(
                                    ui,
                                    "New Project...",
                                    true,
                                )
                                .clicked();
                                save_project |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Save Project",
                                        has_open_project,
                                    )
                                    .on_hover_text(
                                        "Save the open source file and scene",
                                    )
                                    .clicked();
                                gui_elements::EditorTheme::menu_section(
                                    ui, "SCENE",
                                );
                                new_scene |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "New Scene",
                                        has_open_project,
                                    )
                                    .clicked();
                                open_scene |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Open Scene...",
                                        has_open_project,
                                    )
                                    .clicked();
                                load_clicked |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Reload Scene",
                                        has_open_project,
                                    )
                                    .clicked();
                                save_clicked |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Save Scene",
                                        has_open_project,
                                    )
                                    .clicked();
                                save_scene_as |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Save Scene As...",
                                        has_open_project,
                                    )
                                    .clicked();
                            },
                        );
                        gui_elements::EditorTheme::toolbar_menu(
                            ui,
                            "toolbar_edit_menu",
                            "Edit",
                            66.0,
                            220.0,
                            |ui| {
                                gui_elements::EditorTheme::menu_section(
                                    ui, "HISTORY",
                                );
                                undo_clicked |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Undo",
                                        !history.undo.is_empty()
                                            || history
                                                .pending_inspector
                                                .is_some(),
                                    )
                                    .clicked();
                                redo_clicked |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Redo",
                                        !history.redo.is_empty(),
                                    )
                                    .clicked();
                            },
                        );
                        gui_elements::EditorTheme::toolbar_menu(
                            ui,
                            "toolbar_view_menu",
                            "View",
                            70.0,
                            250.0,
                            |ui| {
                                gui_elements::EditorTheme::menu_section(
                                    ui, "WORKSPACE",
                                );
                                add_area |= gui_elements::EditorTheme::menu_action(
                                    ui,
                                    "Add Area",
                                    true,
                                )
                                .on_hover_text(
                                    "Split the selected area left and right",
                                )
                                .clicked();
                                reset_layout |= gui_elements::EditorTheme::menu_action(
                                    ui,
                                    "Reset Layout",
                                    true,
                                )
                                .clicked();
                                gui_elements::EditorTheme::menu_section(
                                    ui, "LAYOUT FILE",
                                );
                                save_layout |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Save Layout",
                                        has_open_project,
                                    )
                                    .clicked();
                                load_layout |=
                                    gui_elements::EditorTheme::menu_action(
                                        ui,
                                        "Load Layout",
                                        has_open_project,
                                    )
                                    .clicked();
                            },
                        );
                        ui.separator();
                        run_project =
                            gui_elements::EditorTheme::toolbar_button(
                                ui,
                                if build_running {
                                    "Building..."
                                } else {
                                    "Play"
                                },
                                false,
                                !build_running && has_open_project,
                            )
                            .on_hover_text(
                                "Save and cook the scene, compile the Rust game, then run it",
                            )
                            .clicked();
                        gui_elements::EditorTheme::toolbar_combo_box_with_popup(
                            ui,
                            "toolbar_game_build_profile",
                            state.game_build_profile.label(),
                            96.0,
                            250.0,
                            |ui| {
                                gui_elements::EditorTheme::menu_section(
                                    ui,
                                    "BUILD PROFILE",
                                );
                                if gui_elements::EditorTheme::menu_choice(
                                    ui,
                                    "Debug | Fast compile",
                                    state.game_build_profile
                                        == GameBuildProfile::Debug,
                                    true,
                                )
                                .on_hover_text(
                                    "Fast compile for normal game development",
                                )
                                .clicked()
                                {
                                    state.game_build_profile =
                                        GameBuildProfile::Debug;
                                }
                                if gui_elements::EditorTheme::menu_choice(
                                    ui,
                                    "Release | Full optimization",
                                    state.game_build_profile
                                        == GameBuildProfile::Release,
                                    true,
                                )
                                .on_hover_text(
                                    "Slow compile with full optimizations",
                                )
                                .clicked()
                                {
                                    state.game_build_profile =
                                        GameBuildProfile::Release;
                                }
                            },
                        );
                        ui.separator();
                        ui.label(format!(
                            "Frame {} | {:.2} ms",
                            frame_time.frame,
                            frame_time.real_delta.as_secs_f64() * 1_000.0
                        ));
                        ui.separator();
                        ui.label(if build_running {
                            "GAME TASK RUNNING"
                        } else {
                            "EDITING"
                        });
                    });
                });
        });

    if open_projects {
        project_manager.open = true;
    }
    if new_project {
        project_manager.open = true;
        project_manager.parent_directory = PathBuf::new();
        project_manager.message = Some(
            "Choose the parent folder where the new Cargo project will be created"
                .into(),
        );
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Choose Where to Create the New Project")
            .pick_folder()
        {
            project_manager.parent_directory = path;
        }
    }
    if save_clicked && state.scene_path.is_empty() {
        save_clicked = false;
        save_scene_as = true;
    }
    let requested_project = draw_project_manager(context, &mut project_manager);
    let selected_scene = open_scene.then(|| {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open RustingEngine Scene")
            .add_filter("RustingEngine Scene", &["rscene"]);
        if has_open_project {
            dialog = dialog.set_directory(
                std::path::Path::new(&state.project_root).join("scenes"),
            );
        }
        dialog.pick_file()
    });
    let requested_action = requested_project
        .map(DestructiveRequest::OpenProject)
        .or_else(|| selected_scene.flatten().map(DestructiveRequest::OpenScene))
        .or_else(|| new_scene.then_some(DestructiveRequest::NewScene))
        .or_else(|| load_clicked.then_some(DestructiveRequest::ReloadScene));
    let mut action_to_run = None;
    if let Some(request) = requested_action {
        if state.scene_dirty {
            pending_action.request = Some(request);
        } else {
            action_to_run = Some(request);
        }
    }
    if pending_action.request.is_some() {
        let mut save_and_continue = false;
        let mut discard_and_continue = false;
        let mut cancel = false;
        egui::Window::new("Unsaved Scene")
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label("The current scene has unsaved changes.");
                ui.label("Save it before continuing?");
                ui.horizontal(|ui| {
                    save_and_continue =
                        ui.button("Save and Continue").clicked();
                    discard_and_continue =
                        ui.button("Discard Changes").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if save_and_continue {
            let chosen_path = if state.scene_path.is_empty() {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Save RustingEngine Scene")
                    .set_file_name("main.rscene")
                    .add_filter("RustingEngine Scene", &["rscene"]);
                if has_open_project {
                    dialog = dialog.set_directory(
                        std::path::Path::new(&state.project_root)
                            .join("scenes"),
                    );
                }
                dialog.save_file()
            } else {
                project_content_path(&state.project_root, &state.scene_path)
                    .ok()
            };
            let result = chosen_path
                .ok_or_else(|| "Save was cancelled".to_owned())
                .and_then(|path| {
                    let relative =
                        project_relative_path(&state.project_root, &path)?;
                    save_scene(world, &path, "Main Scene")
                        .map(|()| relative)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(relative) => {
                    state.scene_path = relative;
                    state.scene_dirty = false;
                    action_to_run = pending_action.request.take();
                }
                Err(error) => {
                    state.scene_message = Some(format!(
                        "Could not save before continuing: {error}"
                    ));
                }
            }
        } else if discard_and_continue {
            action_to_run = pending_action.request.take();
        } else if cancel {
            pending_action.request = None;
        }
    }
    // Convert the confirmed request back into the small flags used below.
    new_scene = matches!(action_to_run, Some(DestructiveRequest::NewScene));
    load_clicked =
        matches!(action_to_run, Some(DestructiveRequest::ReloadScene));
    let open_scene_path = match &action_to_run {
        Some(DestructiveRequest::OpenScene(path)) => Some(path.clone()),
        _ => None,
    };
    let project_request = match action_to_run {
        Some(DestructiveRequest::OpenProject(request)) => Some(request),
        _ => None,
    };

    if reset_layout {
        state.dock_layout = EditorDockNode::default_layout();
        state.active_area = 2;
        state.next_area_id = 5;
    }
    if add_area {
        let new_id = state.next_area_id;
        state.next_area_id = state.next_area_id.saturating_add(1);
        if state.dock_layout.split(
            state.active_area,
            EditorSplitAxis::Columns,
            new_id,
        ) {
            state.active_area = new_id;
        }
    }

    // Move the layout out of EditorState for a moment. Panel code needs to edit
    // the rest of EditorState while the layout tree is also borrowed.
    let mut dock_layout = std::mem::replace(
        &mut state.dock_layout,
        EditorDockNode::Area {
            id: 0,
            panel: EditorPanel::Scene,
        },
    );
    let mut dock_actions = Vec::new();
    let mut rendered_workspace = None;
    let mut active_area = state.active_area;
    // Every movable area lives inside this one transparent central panel.
    CentralPanel::default()
        .frame(egui::Frame::NONE.fill(egui::Color32::TRANSPARENT))
        .show(context, |ui| {
            let rect = ui.available_rect_before_wrap();
            show_dock_node(
                ui,
                &mut dock_layout,
                rect,
                &mut active_area,
                &mut dock_actions,
                &mut |ui, panel| match panel {
                    EditorPanel::Hierarchy => {
                        draw_hierarchy_area(
                            ui,
                            world,
                            &entities,
                            &mut state,
                            &mut entity_request,
                            &mut edited_transform,
                            &mut edited_camera,
                            &mut edited_physics,
                            &mut edited_rigid_body,
                            &mut edited_collider,
                        );
                    }
                    EditorPanel::Inspector => draw_inspector_area(
                        ui,
                        world,
                        &mut state,
                        physics_backends,
                        &mut edited_transform,
                        &mut edited_camera,
                        &mut edited_classes,
                        &mut edited_physics,
                        &mut edited_rigid_body,
                        &mut edited_collider,
                        &registered_names,
                        &custom_values,
                        &mut component_edits,
                        &mut add_physics,
                        &mut remove_physics,
                        &mut edit_custom_shader,
                    ),
                    EditorPanel::Scene | EditorPanel::Game => {
                        let workspace = if panel == EditorPanel::Scene {
                            EditorWorkspace::Scene
                        } else {
                            EditorWorkspace::Game
                        };
                        ui.small(if workspace == EditorWorkspace::Scene {
                            "Editor camera | selection and authoring"
                        } else {
                            "Active game camera | runtime preview"
                        });
                        if workspace == EditorWorkspace::Scene {
                            ui.small(if fly_camera_active {
                                "Fly camera active · release right mouse or press Numpad 0 to exit"
                            } else {
                                "Hold right mouse for FPS fly camera · Numpad 0 toggles"
                            });
                            draw_transform_controls(
                                ui,
                                &mut transform_mode,
                                &editor_shortcuts,
                                gizmo_drag.is_active(),
                                state.selected.is_some(),
                            );
                            if transform_mode.active_mode
                                != TransformModes::Combo
                            {
                                ui.small(format!(
                                    "{} active on {}",
                                    mode_name(transform_mode.active_mode),
                                    transform_axis_label(
                                        transform_mode.axis_mask
                                    )
                                ));
                            }
                            ui.horizontal(|ui| {
                                ui.checkbox(
                                    &mut gizmo_settings.show_grid,
                                    "Grid",
                                )
                                .on_hover_text(
                                    "Show the editor-only XZ ground grid",
                                );
                                ui.checkbox(
                                    &mut gizmo_settings.show_selected_axes,
                                    "Axes",
                                )
                                .on_hover_text(
                                    "Show X/Y/Z arrows on the selected object",
                                );
                                ui.checkbox(
                                    &mut gizmo_settings.show_selected_bounds,
                                    "Bounds",
                                )
                                .on_hover_text(
                                    "Show a yellow box around the selected mesh",
                                );
                            });
                        }
                        ui.separator();
                        if viewport_rect.is_none() {
                            let rect = ui.available_rect_before_wrap();
                            if workspace == EditorWorkspace::Scene {
                                let response = ui.interact(
                                    rect,
                                    ui.id().with("scene_view_pick_area"),
                                    egui::Sense::click_and_drag(),
                                );
                                scene_hover_position = response.hover_pos();
                                if response.clicked() {
                                    scene_click_position =
                                        response.interact_pointer_pos();
                                }
                                ui.input(|input| {
                                    if input.pointer.button_pressed(
                                        egui::PointerButton::Primary,
                                    ) {
                                        scene_drag_left_started = input
                                            .pointer
                                            .press_origin()
                                            .or_else(|| input.pointer.latest_pos());
                                    }
                                    if input.pointer.button_down(
                                        egui::PointerButton::Primary,
                                    ) {
                                        scene_drag_left_position =
                                            input.pointer.latest_pos();
                                    }
                                    if input.pointer.button_released(
                                        egui::PointerButton::Primary,
                                    ) {
                                        scene_drag_left_stopped = true;
                                    }
                                });
                                if response.drag_started_by(egui::PointerButton::Secondary) {
                                    scene_drag_right_started =
                                        response.interact_pointer_pos();
                                }
                                if response.dragged_by(egui::PointerButton::Secondary) {
                                    scene_drag_right_position =
                                        response.interact_pointer_pos();
                                }
                                if response.drag_stopped_by(egui::PointerButton::Secondary) {
                                    scene_drag_right_stopped = true;
                                }
                                scene_right_clicked = response.clicked_by(egui::PointerButton::Secondary);
                            }
                            viewport_rect = Some(rect);
                            rendered_workspace = Some(workspace);
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label("Only one live 3D viewport is currently supported.");
                            });
                        }
                    }
                    EditorPanel::Code => {
                        egui::ScrollArea::both()
                            .id_salt("code_panel_scroll")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            load_code = ui
                                .add_enabled(
                                    has_open_project,
                                    egui::Button::new("Open"),
                                )
                                .clicked();
                            save_code = ui
                                .add_enabled(
                                    has_open_project,
                                    egui::Button::new("Save"),
                                )
                                .clicked();
                            validate_code = ui
                                .add_enabled(
                                    !build_running && has_open_project,
                                    egui::Button::new("Check"),
                                )
                                .clicked();
                            run_project |= ui
                                .add_enabled(
                                    !build_running && has_open_project,
                                    egui::Button::new(format!(
                                        "Build & Run ({})",
                                        state.game_build_profile.label()
                                    )),
                                )
                                .on_hover_text("Save, cook, and run native Rust")
                                .clicked();
                            if state.code_dirty {
                                ui.colored_label(
                                    egui::Color32::YELLOW,
                                    "Unsaved",
                                );
                            }
                        });
                        ui.small("Project-relative source path");
                        ui.text_edit_singleline(&mut state.code_path);
                        if let Some(message) = &state.code_message {
                            ui.small(message);
                        }
                        if build_running {
                            ui.colored_label(
                                egui::Color32::YELLOW,
                                "Build or native game is running...",
                            );
                        }
                        // Build logs live in Console, so the source editor
                        // always receives the complete remaining panel height.
                        let source_height = ui.available_height().max(160.0);
                        let editor = egui::TextEdit::multiline(&mut state.code_source)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .lock_focus(true);
                        if ui
                            .add_sized(
                                egui::vec2(ui.available_width(), source_height),
                                editor,
                            )
                            .changed()
                        {
                            state.code_dirty = true;
                        }
                            });
                    }
                    EditorPanel::Project => {
                        egui::ScrollArea::both()
                            .id_salt("project_panel_scroll")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Project folder");
                            if state.project_root.is_empty() {
                                ui.colored_label(
                                    gui_elements::EditorTheme::TEXT_MUTED,
                                    "No project open",
                                );
                            } else {
                                ui.monospace(&state.project_root);
                            }
                            ui.label("Scene (relative)");
                            ui.text_edit_singleline(&mut state.scene_path);
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut render_settings.vsync, "VSync");
                            ui.checkbox(
                                &mut render_settings.limit_fps,
                                "Limit FPS",
                            );
                            ui.add_enabled(
                                render_settings.limit_fps,
                                DragValue::new(&mut render_settings.max_fps)
                                    .range(1..=1_000),
                            );
                        });
                        if ui
                            .add_enabled(
                                !build_running,
                                egui::Button::new("Export Game..."),
                            )
                            .on_hover_text(
                                "Build a native release and copy runtime files",
                            )
                            .clicked()
                        {
                            export_destination = rfd::FileDialog::new()
                                .set_title("Choose Export Parent Folder")
                                .pick_folder();
                        }
                        if let Some(message) = &state.scene_message {
                            ui.label(message);
                        }
                        if let Some((meshes, materials, textures, scenes)) =
                            asset_counts
                        {
                            ui.label(format!(
                                "Meshes {meshes} | Materials {materials} | Textures {textures} | Scenes {scenes}"
                            ));
                        }
                        if let Some(report) = render_report {
                            ui.label(format!(
                                "Renderables {} | +{} ~{} -{}",
                                report.total,
                                report.added,
                                report.changed,
                                report.removed
                            ));
                        }
                            });
                    }
                    EditorPanel::Console => {
                        egui::ScrollArea::vertical()
                            .id_salt("console_panel_scroll")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show(ui, |ui| {
                            if let Some(console) =
                                world.get_resource::<EditorConsole>()
                            {
                                for entry in console.entries() {
                                    let color = match entry.level {
                                        ConsoleLevel::Info => {
                                            ui.visuals().text_color()
                                        }
                                        ConsoleLevel::Warning => {
                                            egui::Color32::YELLOW
                                        }
                                        ConsoleLevel::Error => {
                                            egui::Color32::LIGHT_RED
                                        }
                                    };
                                    ui.colored_label(color, &entry.message);
                                }
                                if console.entries().is_empty() {
                                    ui.label("No console messages.");
                                }
                            } else {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    "EditorConsole is unavailable. Install EditorPlugin.",
                                );
                            }
                        });
                    }
                    EditorPanel::Assets => {
                        egui::ScrollArea::both()
                            .id_salt("assets_panel_scroll")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show(ui, |ui| {
                        if let Some((meshes, materials, textures, scenes)) =
                            asset_counts
                        {
                            ui.label(format!(
                                "Meshes {} | Materials {} | Textures {} | Scenes {}",
                                meshes, materials, textures, scenes
                            ));
                        }
                        if ui.button("Import Files...").clicked() {
                            if let Some(paths) = rfd::FileDialog::new()
                                .set_title("Import Project Assets")
                                .pick_files()
                            {
                                asset_request =
                                    Some(AssetRequest::ImportFiles(paths));
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label("Filter");
                            ui.text_edit_singleline(&mut editor_assets.filter);
                        });
                        if let Some(message) = &editor_assets.message {
                            ui.small(message);
                        }
                        let filter = editor_assets.filter.to_lowercase();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.strong("Project files");
                            for path in &asset_files {
                                let relative = path
                                    .strip_prefix(&state.project_root)
                                    .unwrap_or(path);
                                let label = editor_relative_path(relative);
                                if !filter.is_empty()
                                    && !label.to_lowercase().contains(&filter)
                                {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    ui.monospace(&label);
                                    match path
                                        .extension()
                                        .and_then(|value| value.to_str())
                                        .map(str::to_ascii_lowercase)
                                        .as_deref()
                                    {
                                        Some("png" | "jpg" | "jpeg" | "bmp" | "tga")
                                            if ui.button("Load").clicked() =>
                                        {
                                            asset_request = Some(
                                                AssetRequest::LoadTexture(
                                                    path.clone(),
                                                ),
                                            );
                                        }
                                        Some("gltf" | "glb")
                                            if ui.button("Import").clicked() =>
                                        {
                                            asset_request = Some(
                                                AssetRequest::ImportGltf(
                                                    path.clone(),
                                                ),
                                            );
                                        }
                                        _ => {}
                                    }
                                });
                            }
                            ui.separator();
                            ui.strong("Loaded textures");
                            for (handle, path) in &loaded_textures {
                                ui.horizontal(|ui| {
                                    ui.monospace(path.display().to_string());
                                    if ui
                                        .add_enabled(
                                            state.selected.is_some(),
                                            egui::Button::new("Use on Selected"),
                                        )
                                        .clicked()
                                    {
                                        asset_request = Some(
                                            AssetRequest::AssignTexture(*handle),
                                        );
                                    }
                                });
                            }
                            ui.separator();
                            ui.strong("Imported glTF primitives");
                            for primitive in &editor_assets.gltf_primitives {
                                ui.horizontal(|ui| {
                                    ui.label(&primitive.name);
                                    if ui
                                        .add_enabled(
                                            state.selected.is_some(),
                                            egui::Button::new("Use on Selected"),
                                        )
                                        .clicked()
                                    {
                                        asset_request = Some(
                                            AssetRequest::AssignPrimitive(
                                                primitive.mesh,
                                                primitive.material,
                                            ),
                                        );
                                    }
                                });
                            }
                        });
                            });
                    }
                },
            );
        });
    state.dock_layout = dock_layout;
    state.active_area = active_area;
    apply_dock_actions(&mut state, dock_actions);
    let layout_path =
        std::path::Path::new(&state.project_root).join("editor_layout.json");
    if save_layout {
        let file = EditorLayoutFile {
            layout: state.dock_layout.clone(),
            active_area: state.active_area,
            next_area_id: state.next_area_id,
        };
        state.scene_message = Some(
            serde_json::to_vec_pretty(&file)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    std::fs::write(&layout_path, bytes)
                        .map_err(|error| error.to_string())
                })
                .map_or_else(
                    |error| format!("Could not save layout: {error}"),
                    |()| format!("Saved layout to {}", layout_path.display()),
                ),
        );
    }
    if load_layout {
        state.scene_message = Some(
            std::fs::read(&layout_path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<EditorLayoutFile>(&bytes)
                        .map_err(|error| error.to_string())
                })
                .map_or_else(
                    |error| format!("Could not load layout: {error}"),
                    |file| {
                        state.dock_layout = file.layout;
                        state.active_area = file.active_area;
                        state.next_area_id = file.next_area_id;
                        format!("Loaded layout from {}", layout_path.display())
                    },
                ),
        );
    }
    if let Some(workspace) = rendered_workspace {
        state.workspace = workspace;
    }

    let (hovered_gizmo, gizmo_consumed) =
        if state.workspace == EditorWorkspace::Scene {
            update_transform_gizmo(
                world,
                &mut state,
                viewport_rect,
                scene_hover_position,
                scene_drag_left_started,
                scene_drag_left_position,
                scene_drag_left_stopped,
                &mut gizmo_drag,
                &mut edited_transform,
                &mut history,
                scene_right_clicked || right_pressed,
                left_pressed,
                escape_pressed,
                &mut transform_mode,
            )
        } else {
            (None, false)
        };

    // A Scene View click selects the closest renderable mesh under the editor
    // camera ray. Game View clicks never change editor selection.
    if let (Some(click), Some(viewport), Some(camera)) =
        (scene_click_position, viewport_rect, state.editor_camera)
    {
        if !gizmo_consumed {
            state.selected = pick_entity(world, camera, click, viewport);
            state.rename_draft = state
                .selected
                .and_then(|entity| world.get::<Name>(entity))
                .map_or_else(String::new, |name| name.0.clone());
        }
    }

    // Convert egui points into physical pixels used by the Vulkan viewport.
    if let Some(rect) = viewport_rect {
        let pixels_per_point = context.pixels_per_point();
        let min = rect.min * pixels_per_point;
        let max = rect.max * pixels_per_point;
        let offset =
            [min.x.max(0.0).floor() as u32, min.y.max(0.0).floor() as u32];
        let end = [
            max.x.max(min.x).ceil() as u32,
            max.y.max(min.y).ceil() as u32,
        ];
        let viewport = EditorViewport {
            offset,
            extent: [
                end[0].saturating_sub(offset[0]),
                end[1].saturating_sub(offset[1]),
            ],
            valid: true,
        };
        if let Some(mut current) = world.get_resource_mut::<EditorViewport>() {
            *current = viewport;
        } else {
            world.insert_resource(viewport);
        }
    } else if let Some(mut current) = world.get_resource_mut::<EditorViewport>()
    {
        current.valid = false;
    }

    // Scene file buttons use the operating system picker, so game projects
    // can keep every editable file inside their selected project folder.
    if let Some(path) = open_scene_path {
        let result = project_relative_path(&state.project_root, &path)
            .and_then(|relative| {
                load_scene(world, &path, SceneLoadMode::Replace)
                    .map(|count| (count, relative))
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok((entity_count, relative)) => {
                state.selected = None;
                state.scene_path = relative;
                state.scene_dirty = false;
                history.undo.clear();
                history.redo.clear();
                history.pending_inspector = None;
                state.scene_message = Some(format!(
                    "Opened {entity_count} objects from {}",
                    path.display()
                ));
            }
            Err(error) => {
                state.scene_message =
                    Some(format!("Could not open scene: {error}"));
            }
        }
    }
    if save_scene_as {
        let suggested_name = std::path::Path::new(&state.scene_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("main.rscene");
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save RustingEngine Scene")
            .set_file_name(suggested_name)
            .add_filter("RustingEngine Scene", &["rscene"]);
        if has_open_project {
            dialog = dialog.set_directory(
                std::path::Path::new(&state.project_root).join("scenes"),
            );
        }
        if let Some(path) = dialog.save_file() {
            let result = project_relative_path(&state.project_root, &path)
                .and_then(|relative| {
                    save_scene(world, &path, "Main Scene")
                        .map(|()| relative)
                        .map_err(|error| error.to_string())
                });
            match result {
                Ok(relative) => {
                    state.scene_path = relative;
                    state.scene_dirty = false;
                    state.scene_message =
                        Some(format!("Saved {}", state.scene_path));
                }
                Err(error) => {
                    state.scene_message =
                        Some(format!("Save As failed: {error}"));
                }
            }
        }
    }
    if new_scene {
        let empty = SceneDocument {
            format_version: crate::runtime::SCENE_FORMAT_VERSION,
            name: "New Scene".into(),
            entities: Vec::new(),
        };
        match crate::runtime::load_scene_document(
            world,
            &empty,
            SceneLoadMode::Replace,
        ) {
            Ok(_) => {
                state.selected = None;
                state.scene_path.clear();
                state.scene_dirty = true;
                history.undo.clear();
                history.redo.clear();
                history.pending_inspector = None;
                state.scene_message = Some(
                    "Created a new scene. Use Save As to choose its file."
                        .into(),
                );
            }
            Err(error) => {
                state.scene_message =
                    Some(format!("Could not create scene: {error}"));
            }
        }
    }
    if undo_clicked {
        if let Some(document) = history.pending_inspector.take() {
            history.push_undo(document);
        }
        if let Some(previous) = history.undo.pop() {
            match scene_document(world, "Redo Snapshot") {
                Ok(current) => match crate::runtime::load_scene_document(
                    world,
                    &previous,
                    SceneLoadMode::Replace,
                ) {
                    Ok(_) => {
                        history.redo.push(current);
                        state.selected = None;
                        state.scene_dirty = true;
                        state.scene_message = Some("Undo applied".into());
                    }
                    Err(error) => {
                        history.undo.push(previous);
                        state.scene_message =
                            Some(format!("Undo failed: {error}"));
                    }
                },
                Err(error) => {
                    history.undo.push(previous);
                    state.scene_message = Some(format!("Undo failed: {error}"));
                }
            }
        }
    }
    if redo_clicked {
        if let Some(next) = history.redo.pop() {
            match scene_document(world, "Undo Snapshot") {
                Ok(current) => match crate::runtime::load_scene_document(
                    world,
                    &next,
                    SceneLoadMode::Replace,
                ) {
                    Ok(_) => {
                        history.undo.push(current);
                        state.selected = None;
                        state.scene_dirty = true;
                        state.scene_message = Some("Redo applied".into());
                    }
                    Err(error) => {
                        history.redo.push(next);
                        state.scene_message =
                            Some(format!("Redo failed: {error}"));
                    }
                },
                Err(error) => {
                    history.redo.push(next);
                    state.scene_message = Some(format!("Redo failed: {error}"));
                }
            }
        }
    }

    let inspector_changed = !gizmo_consumed
        && state.selected == original_selected
        && (edited_transform != original_transform
            || edited_camera != original_camera
            || edited_classes != original_classes
            || edited_physics != original_physics
            || edited_rigid_body != original_rigid_body
            || edited_collider != original_collider
            || add_physics
            || remove_physics);
    if inspector_changed {
        if history.pending_inspector.is_none() {
            match scene_document(world, "Inspector Undo Snapshot") {
                Ok(document) => history.pending_inspector = Some(document),
                Err(error) => {
                    state.scene_message = Some(format!(
                        "Could not create undo snapshot: {error}"
                    ));
                }
            }
        }
        state.scene_dirty = true;
    } else if let Some(document) = history.pending_inspector.take() {
        // The field stopped changing, so all drag frames become one Undo step.
        history.push_undo(document);
    }

    // A Hierarchy click changes selection after these temporary values were
    // copied. Never write the previous object's values into the new object.
    let unchanged_selection = state.selected == original_selected;

    // Write temporary Inspector values back to the selected ECS object.
    if let (true, Some(entity), Some(transform)) =
        (unchanged_selection, state.selected, edited_transform)
    {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.insert(transform);
        }
    }
    if let (true, Some(entity), Some(camera)) =
        (unchanged_selection, state.selected, edited_camera)
    {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            entity.insert(camera);
        }
    }
    if let (true, Some(entity), Some(classes)) =
        (unchanged_selection, state.selected, edited_classes)
    {
        if let Ok(mut entity) = world.get_entity_mut(entity) {
            if classes.names.is_empty() {
                entity.remove::<ObjectClasses>();
            } else {
                entity.insert(classes);
            }
        }
    }
    if let (true, Some(entity)) = (unchanged_selection, state.selected) {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            if remove_physics {
                entity_mut.remove::<(
                    PhysicsBody,
                    RigidBody,
                    Collider,
                    CollisionLayers,
                    crate::runtime::GpuEffectBody,
                )>();
            } else if let Some(physics) = edited_physics {
                let uses_gpu = physics.uses_gpu();
                entity_mut.insert(physics);
                if let Some(rigid_body) = edited_rigid_body {
                    entity_mut.insert(rigid_body);
                }
                if let Some(collider) = edited_collider {
                    entity_mut.insert(collider);
                }
                if add_physics && !entity_mut.contains::<CollisionLayers>() {
                    entity_mut.insert(CollisionLayers::default());
                }
                if uses_gpu {
                    entity_mut.insert(crate::runtime::GpuEffectBody);
                } else {
                    entity_mut.remove::<crate::runtime::GpuEffectBody>();
                }
            }
        }
    }
    if edit_custom_shader {
        if let Some(path) = state
            .selected
            .and_then(|entity| world.get::<PhysicsBody>(entity))
            .and_then(|physics| physics.custom_shader.clone())
        {
            state.code_path = path;
            state.workspace = EditorWorkspace::Code;
            state
                .dock_layout
                .set_panel(state.active_area, EditorPanel::Code);
            load_code = true;
        }
    }
    if load_code {
        let result = project_source_path(&state.project_root, &state.code_path)
            .and_then(|path| {
                let source =
                    std::fs::read_to_string(&path).map_err(|error| {
                        format!("Could not open {}: {error}", path.display())
                    })?;
                Ok((path, source))
            });
        match result {
            Ok((path, source)) => {
                state.code_path =
                    project_relative_path(&state.project_root, &path)
                        .unwrap_or_else(|_| state.code_path.clone());
                state.code_source = source;
                state.code_dirty = false;
                state.code_message =
                    Some(format!("Opened {}", state.code_path));
            }
            Err(error) => state.code_message = Some(error),
        }
    }
    if validate_code {
        let is_rust = std::path::Path::new(&state.code_path)
            .extension()
            .is_some_and(|extension| extension == "rs");
        let result = if is_rust {
            save_project_source(
                &state.project_root,
                &state.code_path,
                &state.code_source,
            )
        } else {
            validate_project_source(
                &state.project_root,
                &state.code_path,
                &state.code_source,
            )
        };
        if result.is_ok() && is_rust {
            state.code_dirty = false;
            build_request = Some(BuildRequest::Check);
        }
        state.code_message = Some(result.map_or_else(
            |error| error,
            |()| {
                if is_rust {
                    "Saved Rust source and started cargo check".into()
                } else {
                    "Shader validation passed".into()
                }
            },
        ));
    }
    if save_project {
        let result = (|| -> Result<(), String> {
            // Opening validates Cargo.toml, project.json, and the standard
            // project files before any editable document is written.
            open_project(std::path::Path::new(&state.project_root))
                .map_err(|error| error.to_string())?;
            let scene_path =
                project_content_path(&state.project_root, &state.scene_path)?;
            save_project_source(
                &state.project_root,
                &state.code_path,
                &state.code_source,
            )?;
            save_scene(world, &scene_path, "Main Scene")
                .map_err(|error| error.to_string())?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                state.code_dirty = false;
                state.scene_dirty = false;
                state.scene_message =
                    Some(format!("Saved project {}", state.project_root));
                state.code_message = Some(format!("Saved {}", state.code_path));
            }
            Err(error) => {
                state.scene_message =
                    Some(format!("Could not save project: {error}"));
            }
        }
    }
    if save_code {
        state.code_message = Some(
            match save_project_source(
                &state.project_root,
                &state.code_path,
                &state.code_source,
            ) {
                Ok(()) => {
                    state.code_dirty = false;
                    if let Ok(path) = project_relative_path(
                        &state.project_root,
                        std::path::Path::new(&state.code_path),
                    ) {
                        state.code_path = path;
                    }
                    format!("Saved {}", state.code_path)
                }
                Err(error) => error,
            },
        );
    }
    if run_project {
        let cooked = std::path::Path::new(&state.project_root)
            .join("build/main.rscene.bin");
        let result =
            project_content_path(&state.project_root, &state.scene_path)
                .and_then(|scene_path| {
                    if state.code_dirty {
                        save_project_source(
                            &state.project_root,
                            &state.code_path,
                            &state.code_source,
                        )
                    } else {
                        Ok(())
                    }
                    .and_then(|()| {
                        save_scene(world, &scene_path, "Main Scene")
                            .map_err(|error| error.to_string())
                    })
                    .and_then(|()| {
                        cook_scene(&scene_path, &cooked)
                            .map_err(|error| error.to_string())
                    })
                });
        if result.is_ok() {
            state.code_dirty = false;
            state.scene_dirty = false;
            build_request = Some(BuildRequest::BuildAndRun {
                profile: state.game_build_profile,
            });
        }
        state.code_message = Some(result.map_or_else(
            |error| format!("Could not run project: {error}"),
            |()| {
                format!(
                    "Saved and cooked; {} build started",
                    state.game_build_profile.label()
                )
            },
        ));
    }
    if let Some(parent) = export_destination {
        let result = open_project(std::path::Path::new(&state.project_root))
            .map_err(|error| error.to_string())
            .and_then(|project| {
                if state.code_dirty {
                    save_project_source(
                        &state.project_root,
                        &state.code_path,
                        &state.code_source,
                    )?;
                }
                let scene_path = project_content_path(
                    &state.project_root,
                    &state.scene_path,
                )?;
                save_scene(world, &scene_path, "Main Scene")
                    .map_err(|error| error.to_string())?;
                let cooked = project.root.join(&project.manifest.cooked_scene);
                cook_scene(&scene_path, &cooked)
                    .map_err(|error| error.to_string())?;
                Ok(BuildRequest::Export {
                    parent,
                    project_name: project.manifest.name,
                    binary_name: project.manifest.binary_name,
                    cooked_scene: project.manifest.cooked_scene,
                })
            });
        match result {
            Ok(request) => {
                state.code_dirty = false;
                state.scene_dirty = false;
                build_request = Some(request);
                state.code_message =
                    Some("Saved and cooked; export build started".into());
            }
            Err(error) => {
                state.code_message =
                    Some(format!("Could not export game: {error}"));
            }
        }
    }
    if save_clicked {
        state.scene_message = Some(
            match project_content_path(&state.project_root, &state.scene_path)
                .and_then(|path| {
                    save_scene(world, &path, "Main Scene")
                        .map_err(|error| error.to_string())
                }) {
                Ok(()) => {
                    state.scene_dirty = false;
                    format!("Saved {}", state.scene_path)
                }
                Err(error) => format!("Save failed: {error}"),
            },
        );
    }
    if load_clicked {
        state.scene_message = Some(
            match project_content_path(&state.project_root, &state.scene_path)
                .and_then(|path| {
                    load_scene(world, &path, SceneLoadMode::Replace)
                        .map_err(|error| error.to_string())
                }) {
                Ok(entity_count) => {
                    state.selected = None;
                    state.scene_dirty = false;
                    history.undo.clear();
                    history.redo.clear();
                    history.pending_inspector = None;
                    format!(
                        "Loaded {entity_count} entities from {}",
                        state.scene_path
                    )
                }
                Err(error) => format!("Load failed: {error}"),
            },
        );
    }
    if let Some(request) = entity_request {
        if let Err(error) = remember_scene_before_edit(world, &mut history) {
            state.scene_message = Some(error);
        } else {
            let result = (|| -> Result<Option<Entity>, String> {
                match request {
                    EntityRequest::CreateEmpty => {
                        let name = unique_object_name(world, "Empty");
                        let entity = world
                            .spawn((
                                SceneId::new(),
                                Name(name),
                                Transform::default(),
                            ))
                            .id();
                        Ok(Some(entity))
                    }
                    EntityRequest::CreateCube => {
                        let name = unique_object_name(world, "Cube");
                        let (mesh, material) = {
                            let assets = world.resource::<AssetServer>();
                            (assets.fallback_mesh, assets.fallback_material)
                        };
                        let entity = world
                            .spawn((
                                SceneId::new(),
                                Name(name),
                                Transform::default(),
                                MeshRenderer {
                                    mesh,
                                    material,
                                    cast_shadows: true,
                                    receive_shadows: true,
                                },
                                Visibility::default(),
                            ))
                            .id();
                        Ok(Some(entity))
                    }
                    EntityRequest::CreateSphere => {
                        let name = unique_object_name(world, "Sphere");
                        let (mesh, material) = {
                            let assets = world.resource::<AssetServer>();
                            (assets.builtin_sphere, assets.fallback_material)
                        };
                        let entity = world
                            .spawn((
                                SceneId::new(),
                                Name(name),
                                Transform::default(),
                                MeshRenderer {
                                    mesh,
                                    material,
                                    cast_shadows: true,
                                    receive_shadows: true,
                                },
                                Visibility::default(),
                            ))
                            .id();
                        Ok(Some(entity))
                    }
                    EntityRequest::CreateCamera => {
                        let name = unique_object_name(world, "Camera");
                        let entity = world
                            .spawn((
                                SceneId::new(),
                                Name(name),
                                Transform::default(),
                                Camera {
                                    active: false,
                                    ..Camera::default()
                                },
                            ))
                            .id();
                        Ok(Some(entity))
                    }
                    EntityRequest::Rename(entity, name) => {
                        let duplicate = {
                            let mut names = world.query::<(Entity, &Name)>();
                            names.iter(world).any(|(other, current)| {
                                other != entity && current.0 == name
                            })
                        };
                        if duplicate {
                            return Err(format!(
                                "Another object is already named `{name}`"
                            ));
                        }
                        let mut entity =
                            world.get_entity_mut(entity).map_err(|_| {
                                "Selected object no longer exists".to_owned()
                            })?;
                        entity.insert(Name(name));
                        Ok(Some(entity.id()))
                    }
                    EntityRequest::Reparent(entity, parent) => {
                        let child_id = world
                            .get::<SceneId>(entity)
                            .copied()
                            .ok_or_else(|| {
                                "Selected object is not part of the saved scene"
                                    .to_owned()
                            })?;
                        let parent_id = parent
                            .map(|parent| {
                                world
                                    .get::<SceneId>(parent)
                                    .copied()
                                    .map(|id| id.0)
                                    .ok_or_else(|| {
                                        "Parent is not part of the saved scene"
                                            .to_owned()
                                    })
                            })
                            .transpose()?;
                        let mut document = scene_document(world, "Main Scene")
                            .map_err(|error| error.to_string())?;
                        let parents = document
                            .entities
                            .iter()
                            .map(|item| (item.id, item.parent))
                            .collect::<std::collections::HashMap<_, _>>();
                        let mut ancestor = parent_id;
                        let mut visited = std::collections::HashSet::new();
                        while let Some(id) = ancestor {
                            if id == child_id.0 || !visited.insert(id) {
                                return Err(
                                    "That parent would create a hierarchy cycle"
                                        .into(),
                                );
                            }
                            ancestor = parents.get(&id).copied().flatten();
                        }
                        let child = document
                            .entities
                            .iter_mut()
                            .find(|item| item.id == child_id.0)
                            .ok_or_else(|| {
                                "Selected object was not found in the scene"
                                    .to_owned()
                            })?;
                        child.parent = parent_id;
                        crate::runtime::load_scene_document(
                            world,
                            &document,
                            SceneLoadMode::Replace,
                        )
                        .map_err(|error| error.to_string())?;
                        let mut query = world.query::<(Entity, &SceneId)>();
                        Ok(query.iter(world).find_map(|(entity, id)| {
                            (id.0 == child_id.0).then_some(entity)
                        }))
                    }
                    EntityRequest::Duplicate(entity) => {
                        let source_id = world
                            .get::<SceneId>(entity)
                            .copied()
                            .ok_or_else(|| {
                            "Selected object is not part of the saved scene"
                                .to_owned()
                        })?;
                        let mut document = scene_document(world, "Main Scene")
                            .map_err(|error| error.to_string())?;
                        let mut copy = document
                            .entities
                            .iter()
                            .find(|item| item.id == source_id.0)
                            .cloned()
                            .ok_or_else(|| {
                                "Selected object was not found in the scene"
                                    .to_owned()
                            })?;
                        copy.id = uuid::Uuid::new_v4();
                        copy.name = Some(format!(
                            "{} Copy",
                            copy.name.as_deref().unwrap_or("Object")
                        ));
                        let new_id = copy.id;
                        document.entities.push(copy);
                        crate::runtime::load_scene_document(
                            world,
                            &document,
                            SceneLoadMode::Replace,
                        )
                        .map_err(|error| error.to_string())?;
                        let mut query = world.query::<(Entity, &SceneId)>();
                        Ok(query.iter(world).find_map(|(entity, id)| {
                            (id.0 == new_id).then_some(entity)
                        }))
                    }
                    EntityRequest::Delete(entity) => {
                        let source_id = world
                            .get::<SceneId>(entity)
                            .copied()
                            .ok_or_else(|| {
                            "Selected object is not part of the saved scene"
                                .to_owned()
                        })?;
                        let mut document = scene_document(world, "Main Scene")
                            .map_err(|error| error.to_string())?;
                        let mut removed =
                            std::collections::HashSet::from([source_id.0]);
                        loop {
                            let old_count = removed.len();
                            for item in &document.entities {
                                if item
                                    .parent
                                    .is_some_and(|id| removed.contains(&id))
                                {
                                    removed.insert(item.id);
                                }
                            }
                            if removed.len() == old_count {
                                break;
                            }
                        }
                        document
                            .entities
                            .retain(|item| !removed.contains(&item.id));
                        crate::runtime::load_scene_document(
                            world,
                            &document,
                            SceneLoadMode::Replace,
                        )
                        .map_err(|error| error.to_string())?;
                        Ok(None)
                    }
                }
            })();
            match result {
                Ok(selected) => {
                    state.selected = selected;
                    state.rename_draft = selected
                        .and_then(|entity| world.get::<Name>(entity))
                        .map(|name| name.0.clone())
                        .unwrap_or_default();
                    state.scene_dirty = true;
                    state.scene_message = Some("Scene object changed".into());
                }
                Err(error) => {
                    // The edit did not happen, so its unused snapshot is removed.
                    history.undo.pop();
                    state.scene_message =
                        Some(format!("Could not change scene object: {error}"));
                }
            }
        }
    }
    if let Some(request) = asset_request {
        let result: Result<String, String> = match request {
            AssetRequest::ImportFiles(paths) => {
                (|| -> Result<String, String> {
                    let mut imported_paths = Vec::new();
                    let mut prepared_count = 0;
                    for source in paths {
                        let path = copy_into_project_assets(
                            &state.project_root,
                            &source,
                        )?;
                        imported_paths.push(path);
                    }
                    // Prepare only after every selected dependency was copied.
                    for path in &imported_paths {
                        match path
                            .extension()
                            .and_then(|value| value.to_str())
                            .map(str::to_ascii_lowercase)
                            .as_deref()
                        {
                            Some("png" | "jpg" | "jpeg" | "bmp" | "tga") => {
                                world
                                    .resource_mut::<AssetServer>()
                                    .load_texture(path)
                                    .map_err(|error| error.to_string())?;
                                prepared_count += 1;
                            }
                            Some("gltf" | "glb") => {
                                let primitives = world
                                    .resource_mut::<AssetServer>()
                                    .import_gltf(path)
                                    .map_err(|error| error.to_string())?;
                                prepared_count += primitives.len();
                                editor_assets
                                    .gltf_primitives
                                    .extend(primitives);
                            }
                            _ => {}
                        }
                    }
                    let imported_count = imported_paths.len();
                    Ok(format!(
                        "Imported {imported_count} file(s) and prepared {prepared_count} asset(s)"
                    ))
                })()
            }
            AssetRequest::LoadTexture(path) => world
                .resource_mut::<AssetServer>()
                .load_texture(&path)
                .map(|_| format!("Loaded texture {}", path.display()))
                .map_err(|error| error.to_string()),
            AssetRequest::ImportGltf(path) => world
                .resource_mut::<AssetServer>()
                .import_gltf(&path)
                .map(|primitives| {
                    let count = primitives.len();
                    editor_assets.gltf_primitives.retain(|old| {
                        !primitives.iter().any(|new| new.mesh == old.mesh)
                    });
                    editor_assets.gltf_primitives.extend(primitives);
                    format!(
                        "Imported {count} glTF primitive(s) from {}",
                        path.display()
                    )
                })
                .map_err(|error| error.to_string()),
            AssetRequest::AssignTexture(texture) => {
                let selected = state
                    .selected
                    .ok_or_else(|| "Select a mesh object first".to_owned());
                selected.and_then(|entity| {
                    let mut renderer = world
                        .get::<MeshRenderer>(entity)
                        .copied()
                        .ok_or_else(|| {
                            "Selected object has no Mesh Renderer".to_owned()
                        })?;
                    remember_scene_before_edit(world, &mut history)?;
                    let material = {
                        let mut assets = world.resource_mut::<AssetServer>();
                        if !assets.textures.contains(texture) {
                            return Err("Texture is no longer loaded".into());
                        }
                        let mut material = assets
                            .materials
                            .get(renderer.material)
                            .cloned()
                            .unwrap_or_default();
                        material.base_color_texture = Some(texture);
                        assets.materials.insert(material)
                    };
                    renderer.material = material;
                    world.entity_mut(entity).insert(renderer);
                    state.scene_dirty = true;
                    Ok("Assigned texture to selected object".into())
                })
            }
            AssetRequest::AssignPrimitive(mesh, material) => {
                let selected = state
                    .selected
                    .ok_or_else(|| "Select a scene object first".to_owned());
                selected.and_then(|entity| {
                    {
                        let assets = world.resource::<AssetServer>();
                        if !assets.meshes.contains(mesh)
                            || !assets.materials.contains(material)
                        {
                            return Err(
                                "Imported glTF asset is no longer loaded"
                                    .into(),
                            );
                        }
                    }
                    remember_scene_before_edit(world, &mut history)?;
                    world.entity_mut(entity).insert((
                        MeshRenderer {
                            mesh,
                            material,
                            cast_shadows: true,
                            receive_shadows: true,
                        },
                        Visibility::default(),
                    ));
                    state.scene_dirty = true;
                    Ok("Assigned glTF primitive to selected object".into())
                })
            }
        };
        editor_assets.message = Some(result.unwrap_or_else(|error| error));
    }
    if !component_edits.is_empty() {
        match remember_scene_before_edit(world, &mut history) {
            Ok(()) => state.scene_dirty = true,
            Err(error) => state.scene_message = Some(error),
        }
    }
    for edit in component_edits {
        let result = match edit {
            ComponentEdit::Set {
                entity,
                name,
                value,
            } => set_registered_component(world, entity, &name, &value),
            ComponentEdit::Add { entity, name } => {
                add_registered_component(world, entity, &name)
            }
            ComponentEdit::Remove { entity, name } => {
                state.component_drafts.remove(&(entity, name.clone()));
                remove_registered_component(world, entity, &name)
            }
        };
        if let Err(error) = result {
            state.scene_message =
                Some(format!("Component edit failed: {error}"));
        }
    }
    if let Some(request) = project_request {
        let opened = match request {
            ProjectRequest::Create { parent, name } => {
                create_project(&parent, &name)
            }
            ProjectRequest::Open(path) => open_project(&path),
        };
        match opened {
            Ok(project) => {
                match load_scene(
                    world,
                    &project.scene_path,
                    SceneLoadMode::Replace,
                ) {
                    Ok(entity_count) => {
                        state.selected = None;
                        set_open_project_paths(&mut state, &project);
                        match std::fs::read_to_string(&project.code_path) {
                            Ok(source) => {
                                state.code_source = source;
                                state.code_dirty = false;
                                state.code_message = Some(format!(
                                    "Opened {} from {}",
                                    state.code_path, project.manifest.name
                                ));
                            }
                            Err(error) => {
                                state.code_message = Some(format!(
                                    "Could not open Rust code: {error}"
                                ));
                            }
                        }
                        state.scene_message = Some(format!(
                            "Opened {} with {entity_count} scene objects",
                            project.manifest.name
                        ));
                        state.scene_dirty = false;
                        history.undo.clear();
                        history.redo.clear();
                        history.pending_inspector = None;
                        project_manager.open = false;
                        if let Err(error) = project::remember_project(
                            &mut project_manager.recent_projects,
                            &project,
                        ) {
                            project_manager.message = Some(format!(
                                "Project opened, but recent projects could not be saved: {error}"
                            ));
                        } else {
                            project_manager.message = None;
                        }
                    }
                    Err(error) => {
                        project_manager.message = Some(format!(
                            "Could not load project scene: {error}"
                        ));
                    }
                }
            }
            Err(error) => {
                project_manager.message =
                    Some(format!("Could not open project: {error}"));
            }
        }
    }
    world.resource_mut::<RenderCameraOverride>().entity = (state.workspace
        == EditorWorkspace::Scene)
        .then_some(state.editor_camera)
        .flatten();
    render_settings.max_fps = render_settings.max_fps.max(1);
    *world.resource_mut::<RenderSettings>() = render_settings;
    if let Some(request) = build_request {
        if let Err(error) = world
            .resource_mut::<EditorBuildState>()
            .start(PathBuf::from(&state.project_root), request)
        {
            state.code_message =
                Some(format!("Could not start Cargo: {error}"));
        }
    }
    crate::runtime::propagate_transforms(world);
    let overlay = if state.workspace == EditorWorkspace::Scene {
        build_scene_debug_overlay(
            world,
            state.selected,
            gizmo_settings,
            hovered_gizmo,
            &gizmo_drag,
            transform_mode.active_mode,
        )
    } else {
        RenderDebugOverlay::default()
    };
    world.resource_mut::<EditorDebugOverlay>().0 = overlay;
    *world.resource_mut::<EditorGizmoSettings>() = gizmo_settings;
    *world.resource_mut::<EditorGizmoDrag>() = gizmo_drag;
    *world.resource_mut::<EditorTransformMode>() = transform_mode;
    *world.resource_mut::<EditorState>() = state;
    *world.resource_mut::<EditorHistory>() = history;
    *world.resource_mut::<PendingDestructiveAction>() = pending_action;
    *world.resource_mut::<EditorAssetState>() = editor_assets;
    *world.resource_mut::<ProjectManagerState>() = project_manager;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GizmoHandle {
    mode: TransformModes,
    axis: GizmoAxis,
}

#[allow(clippy::too_many_arguments)]
fn update_transform_gizmo(
    world: &mut World,
    state: &mut EditorState,
    viewport: Option<egui::Rect>,
    hover: Option<Pos2>,
    drag_started: Option<Pos2>,
    drag_position: Option<Pos2>,
    drag_stopped: bool,
    drag: &mut EditorGizmoDrag,
    edited_transform: &mut Option<Transform>,
    history: &mut EditorHistory,
    right_clicked: bool,
    left_pressed: bool,
    escape_pressed: bool,
    transform_mode: &mut EditorTransformMode,
) -> (Option<GizmoHandle>, bool) {
    let (Some(entity), Some(camera), Some(viewport)) =
        (state.selected, state.editor_camera, viewport)
    else {
        *drag = EditorGizmoDrag::default();
        transform_mode.start_requested = false;
        transform_mode.active_mode = TransformModes::Combo;
        return (None, false);
    };
    let Some(geometry) = gizmo_geometry(world, entity) else {
        return (None, false);
    };
    // Pressing G/S/R during an unfinished operation changes operation type by
    // cancelling the preview and beginning again from the authored transform.
    if transform_mode.start_requested && drag.is_active() {
        *edited_transform = drag.original_transform;
        *drag = EditorGizmoDrag::default();
    }
    if transform_mode.start_requested {
        if let Some(pointer) = hover {
            begin_gizmo_drag(
                world,
                entity,
                pointer,
                transform_mode.active_mode,
                true,
                transform_mode.axis_mask,
                &geometry,
                camera,
                viewport,
                drag,
                *edited_transform,
            );
            transform_mode.start_requested = false;
        }
    }

    let hovered =
        if let (Some(mode), Some(axis)) = (drag.mode, drag.active_axis) {
            Some(GizmoHandle { mode, axis })
        } else {
            hover
                .and_then(|pointer| {
                    hit_test_gizmo(world, camera, viewport, pointer, &geometry)
                })
                .map(|hit| hit.1)
        };
    let mut consumed = drag.is_active();
    if !drag.is_active() {
        if let Some(pointer) = drag_started {
            if let Some((_, handle)) =
                hit_test_gizmo(world, camera, viewport, pointer, &geometry)
            {
                let mut mask = [false; 3];
                mask[gizmo_axis_index(handle.axis)] = true;
                begin_gizmo_drag(
                    world,
                    entity,
                    pointer,
                    handle.mode,
                    false,
                    mask,
                    &geometry,
                    camera,
                    viewport,
                    drag,
                    *edited_transform,
                );
                transform_mode.active_mode = handle.mode;
                transform_mode.axis_mask = mask;
                consumed = true;
            }
        }
    }

    if drag.is_active() {
        drag.axis_mask = transform_mode.axis_mask;
        drag.active_axis = single_enabled_axis(drag.axis_mask);
        let pointer = if drag.modal { hover } else { drag_position };
        if let Some(pointer) = pointer {
            let move_parameter = if drag.mode == Some(TransformModes::Move)
                && single_enabled_axis(drag.axis_mask).is_some()
            {
                world
                    .get::<Camera>(camera)
                    .copied()
                    .zip(world.get::<GlobalTransform>(camera).copied())
                    .and_then(|(camera_component, camera_transform)| {
                        scene_ray(
                            pointer,
                            viewport,
                            camera_component,
                            camera_transform,
                        )
                    })
                    .and_then(|ray| {
                        move_axis_parameter(
                            ray,
                            drag.move_axis_origin,
                            drag.move_axis_direction,
                            drag.move_drag_plane_normal,
                        )
                    })
            } else {
                None
            };
            if let Some(transform) =
                transformed_from_pointer(drag, pointer, move_parameter)
            {
                *edited_transform = Some(transform);
                state.scene_dirty = true;
            }
        }
    }

    if (right_clicked || escape_pressed) && drag.is_active() {
        *edited_transform = drag.original_transform;
        *drag = EditorGizmoDrag::default();
        finish_transform_mode(transform_mode);
        return (hovered, true);
    }

    let confirmed = drag.is_active()
        && if drag.modal {
            left_pressed && hover.is_some()
        } else {
            drag_stopped
        };
    if confirmed {
        if let Some(snapshot) = drag.undo_snapshot.take() {
            if drag.original_transform != *edited_transform {
                history.push_undo(snapshot);
            }
        }

        *drag = EditorGizmoDrag::default();
        finish_transform_mode(transform_mode);
        return (hovered, true);
    }
    (hovered, consumed)
}

fn finish_transform_mode(transform_mode: &mut EditorTransformMode) {
    transform_mode.active_mode = TransformModes::Combo;
    transform_mode.axis_mask = [true; 3];
    transform_mode.start_requested = false;
}

const fn transform_axis_label(mask: [bool; 3]) -> &'static str {
    match mask {
        [true, false, false] => "X",
        [false, true, false] => "Y",
        [false, false, true] => "Z",
        [true, true, false] => "XY",
        [true, false, true] => "XZ",
        [false, true, true] => "YZ",
        _ => "XYZ",
    }
}

fn draw_transform_controls(
    ui: &mut egui::Ui,
    transform: &mut EditorTransformMode,
    shortcuts: &EditorShortcuts,
    drag_active: bool,
    has_selection: bool,
) {
    ui.horizontal(|ui| {
        ui.small("Transform");
        for mode in [
            TransformModes::Move,
            TransformModes::Rotate,
            TransformModes::Scale,
        ] {
            let shortcut = transform_shortcut_label(shortcuts, mode);
            let label = format!("{}  {shortcut}", mode_name(mode));
            if gui_elements::EditorTheme::toolbar_button(
                ui,
                &label,
                transform.active_mode == mode,
                has_selection,
            )
            .on_hover_text(format!(
                "Start {} with the mouse or press {}",
                mode_name(mode),
                shortcut
            ))
            .clicked()
            {
                transform.active_mode = mode;
                transform.axis_mask = [true; 3];
                transform.start_requested = true;
            }
        }
    });

    if drag_active || transform.start_requested {
        ui.horizontal(|ui| {
            ui.small("Direction");
            for (axis, label) in [
                (GizmoAxis::X, "X"),
                (GizmoAxis::Y, "Y"),
                (GizmoAxis::Z, "Z"),
            ] {
                let active = transform.axis_mask[gizmo_axis_index(axis)];
                let text = egui::RichText::new(label)
                    .strong()
                    .color(egui::Color32::WHITE);
                let mut button =
                    egui::Button::new(text).min_size(egui::vec2(28.0, 24.0));
                button = if active {
                    button.fill(gui_elements::EditorTheme::ACCENT_HOVER).stroke(
                        egui::Stroke::new(
                            1.0_f32,
                            egui::Color32::from_rgb(130, 190, 255),
                        ),
                    )
                } else {
                    button.frame(false)
                };
                if ui
                    .add(button)
                    .on_hover_text(format!(
                        "Toggle {label} direction (shortcut: {label})"
                    ))
                    .clicked()
                {
                    transform.select_only_or_toggle(axis);
                }
            }
            ui.small("Click Scene View to confirm · Right-click/Esc cancels");
        });
    }
}

const fn mode_name(mode: TransformModes) -> &'static str {
    match mode {
        TransformModes::Move => "Move",
        TransformModes::Rotate => "Rotate",
        TransformModes::Scale => "Scale",
        TransformModes::Combo => "Combined Transform",
    }
}

fn transform_shortcut_label(
    shortcuts: &EditorShortcuts,
    mode: TransformModes,
) -> String {
    shortcuts
        .get(ShortcutAction::SceneView(SceneViewAction::TransformModes(
            mode,
        )))
        .map_or_else(
            || "Unbound".into(),
            |binding| {
                let debug = format!("{:?}", binding.key);
                debug.strip_prefix("Key").unwrap_or(&debug).to_owned()
            },
        )
}

#[allow(clippy::too_many_arguments)]
fn begin_gizmo_drag(
    world: &mut World,
    entity: Entity,
    pointer: Pos2,
    mode: TransformModes,
    modal: bool,
    axis_mask: [bool; 3],
    geometry: &GizmoGeometry,
    camera: Entity,
    viewport: egui::Rect,
    drag: &mut EditorGizmoDrag,
    original_transform: Option<Transform>,
) {
    let Some(origin_screen) =
        project_world_to_screen(world, camera, geometry.origin, viewport)
    else {
        return;
    };
    let world_axes = geometry.axes.map(|(_, direction)| direction);
    let parent_inverse = world
        .get::<Parent>(entity)
        .and_then(|parent| world.get::<GlobalTransform>(parent.0))
        .and_then(|parent| Matrix4::from(parent.matrix).try_inverse());
    let local_delta_axes = world_axes.map(|axis| {
        parent_inverse.map_or(axis, |inverse| {
            let local = inverse * Vector4::new(axis[0], axis[1], axis[2], 0.0);
            [local.x, local.y, local.z]
        })
    });
    let screen_vectors = world_axes.map(|axis| {
        let point = [
            geometry.origin[0] + axis[0],
            geometry.origin[1] + axis[1],
            geometry.origin[2] + axis[2],
        ];
        project_world_to_screen(world, camera, point, viewport)
            .map_or([0.0; 2], |screen| {
                [screen.x - origin_screen.x, screen.y - origin_screen.y]
            })
    });
    let rotation_screen_signs = std::array::from_fn(|normal_axis| {
        let first = screen_vectors[(normal_axis + 1) % 3];
        let second = screen_vectors[(normal_axis + 2) % 3];
        let determinant = first[0] * second[1] - first[1] * second[0];
        if determinant.abs() > 0.001 {
            determinant.signum()
        } else {
            1.0
        }
    });
    let camera_forward = world.get::<GlobalTransform>(camera).map_or(
        [0.0, 0.0, -1.0],
        |camera_transform| {
            let matrix = camera_transform.matrix;
            normalized_axis([-matrix[2][0], -matrix[2][1], -matrix[2][2]])
        },
    );
    let view_rotation_axis = parent_inverse.map_or(camera_forward, |inverse| {
        let local = inverse
            * Vector4::new(
                camera_forward[0],
                camera_forward[1],
                camera_forward[2],
                0.0,
            );
        normalized_axis([local.x, local.y, local.z])
    });
    let move_axis = single_enabled_axis(axis_mask)
        .map(gizmo_axis_index)
        .unwrap_or(0);
    let move_axis_direction = world_axes[move_axis];
    let camera_position =
        world
            .get::<GlobalTransform>(camera)
            .map_or([0.0; 3], |transform| {
                [
                    transform.matrix[3][0],
                    transform.matrix[3][1],
                    transform.matrix[3][2],
                ]
            });
    let to_camera =
        Vector3::from(camera_position) - Vector3::from(geometry.origin);
    let axis_vector = Vector3::from(move_axis_direction);
    let plane_normal = to_camera - axis_vector * to_camera.dot(&axis_vector);
    let move_drag_plane_normal = plane_normal
        .try_normalize(0.0001)
        .map_or(camera_forward, Into::into);
    let move_start_axis_parameter = if mode == TransformModes::Move
        && single_enabled_axis(axis_mask).is_some()
    {
        let camera_component = world.get::<Camera>(camera).copied();
        let camera_transform = world.get::<GlobalTransform>(camera).copied();
        camera_component
            .zip(camera_transform)
            .and_then(|(camera_component, camera_transform)| {
                scene_ray(pointer, viewport, camera_component, camera_transform)
            })
            .and_then(|ray| {
                move_axis_parameter(
                    ray,
                    geometry.origin,
                    move_axis_direction,
                    move_drag_plane_normal,
                )
            })
    } else {
        None
    };
    let active_axis = single_enabled_axis(axis_mask);
    *drag = EditorGizmoDrag {
        mode: Some(mode),
        modal,
        axis_mask,
        active_axis,
        entity: Some(entity),
        start_pointer: Some(pointer),
        original_transform,
        origin_screen: Some(origin_screen),
        world_axes,
        local_delta_axes,
        screen_vectors,
        rotation_screen_signs,
        view_rotation_axis,
        move_axis_origin: geometry.origin,
        move_axis_direction,
        move_drag_plane_normal,
        move_start_axis_parameter,
        gizmo_axis_length: geometry.axis_length,
        undo_snapshot: scene_document(world, "Gizmo Undo Snapshot").ok(),
        ..EditorGizmoDrag::default()
    };
}

fn transformed_from_pointer(
    drag: &EditorGizmoDrag,
    pointer: Pos2,
    move_axis_parameter: Option<f32>,
) -> Option<Transform> {
    let start = drag.start_pointer?;
    let original = drag.original_transform?;
    let delta = pointer - start;
    let mut transform = original;
    match drag.mode? {
        TransformModes::Move => {
            if let (
                Some(start_parameter),
                Some(current_parameter),
                Some(axis),
            ) = (
                drag.move_start_axis_parameter,
                move_axis_parameter,
                single_enabled_axis(drag.axis_mask),
            ) {
                let distance = current_parameter - start_parameter;
                let axis = gizmo_axis_index(axis);
                for component in 0..3 {
                    transform.position[component] +=
                        drag.local_delta_axes[axis][component] * distance;
                }
                return Some(transform);
            }
            let coefficients = screen_axis_coefficients(
                drag.screen_vectors,
                drag.axis_mask,
                [delta.x, delta.y],
            );
            for (axis, coefficient) in coefficients.into_iter().enumerate() {
                for component in 0..3 {
                    transform.position[component] +=
                        drag.local_delta_axes[axis][component] * coefficient;
                }
            }
        }
        TransformModes::Scale => {
            let factor = if drag.modal {
                ((delta.x - delta.y) * 0.01).exp()
            } else {
                let axis = drag.active_axis?;
                let vector = drag.screen_vectors[gizmo_axis_index(axis)];
                let handle = [
                    vector[0] * drag.gizmo_axis_length * 0.68,
                    vector[1] * drag.gizmo_axis_length * 0.68,
                ];
                let length_squared =
                    (handle[0] * handle[0] + handle[1] * handle[1]).max(16.0);
                (1.0 + (delta.x * handle[0] + delta.y * handle[1])
                    / length_squared)
                    .max(0.01)
            };
            for axis in 0..3 {
                if drag.axis_mask[axis] {
                    transform.scale[axis] =
                        (original.scale[axis] * factor).max(0.001);
                }
            }
        }
        TransformModes::Rotate => {
            if drag.modal && drag.axis_mask == [true; 3] {
                return Some(rotate_transform_in_parent_space(
                    original,
                    drag.view_rotation_axis,
                    raw_screen_rotation_angle(drag, pointer),
                ));
            }
            let mut angles = [0.0; 3];
            if let Some(axis) = single_enabled_axis(drag.axis_mask) {
                let index = gizmo_axis_index(axis);
                angles[index] = screen_rotation_angle(drag, pointer, index);
            } else if drag.modal {
                // Free rotation behaves like a small virtual trackball. Mouse
                // vertical rotates around local X, horizontal around local Y,
                // and diagonal motion contributes local Z when it is enabled.
                angles = [
                    -delta.y * 0.01,
                    delta.x * 0.01,
                    (delta.x - delta.y) * 0.005,
                ];
            } else {
                let index = gizmo_axis_index(drag.active_axis?);
                angles[index] = screen_rotation_angle(drag, pointer, index);
            }
            transform =
                rotate_transform_locally(original, angles, drag.axis_mask);
        }
        TransformModes::Combo => return None,
    }
    Some(transform)
}

fn move_axis_parameter(
    ray: super::picking::Ray,
    axis_origin: [f32; 3],
    axis_direction: [f32; 3],
    plane_normal: [f32; 3],
) -> Option<f32> {
    let origin = Vector3::from(axis_origin);
    let axis = Vector3::from(axis_direction);
    let normal = Vector3::from(plane_normal);
    let denominator = ray.direction.dot(&normal);
    if denominator.abs() <= 0.0001 {
        return None;
    }
    let ray_distance = (origin - ray.origin).dot(&normal) / denominator;
    let point = ray.origin + ray.direction * ray_distance;
    Some((point - origin).dot(&axis))
}

fn screen_rotation_angle(
    drag: &EditorGizmoDrag,
    pointer: Pos2,
    axis: usize,
) -> f32 {
    raw_screen_rotation_angle(drag, pointer) * drag.rotation_screen_signs[axis]
}

fn raw_screen_rotation_angle(drag: &EditorGizmoDrag, pointer: Pos2) -> f32 {
    let Some(origin) = drag.origin_screen else {
        return 0.0;
    };
    let Some(start) = drag.start_pointer else {
        return 0.0;
    };
    let from = start - origin;
    let to = pointer - origin;
    if from.length_sq() <= 4.0 || to.length_sq() <= 4.0 {
        let delta = pointer - start;
        return delta.x * 0.01;
    }
    (from.x * to.y - from.y * to.x).atan2(from.dot(to))
}

fn rotate_transform_locally(
    original: Transform,
    angles: [f32; 3],
    mask: [bool; 3],
) -> Transform {
    let current = Rotation3::from_euler_angles(
        original.rotation[0],
        original.rotation[1],
        original.rotation[2],
    );
    let mut local_delta = Rotation3::identity();
    if mask[0] {
        local_delta *=
            Rotation3::from_axis_angle(&Vector3::x_axis(), angles[0]);
    }
    if mask[1] {
        local_delta *=
            Rotation3::from_axis_angle(&Vector3::y_axis(), angles[1]);
    }
    if mask[2] {
        local_delta *=
            Rotation3::from_axis_angle(&Vector3::z_axis(), angles[2]);
    }
    let rotation = (current * local_delta).euler_angles();
    Transform {
        rotation: [rotation.0, rotation.1, rotation.2],
        ..original
    }
}

fn rotate_transform_in_parent_space(
    original: Transform,
    axis: [f32; 3],
    angle: f32,
) -> Transform {
    let current = Rotation3::from_euler_angles(
        original.rotation[0],
        original.rotation[1],
        original.rotation[2],
    );
    let axis = nalgebra::Unit::new_normalize(Vector3::from(axis));
    let rotation =
        (Rotation3::from_axis_angle(&axis, angle) * current).euler_angles();
    Transform {
        rotation: [rotation.0, rotation.1, rotation.2],
        ..original
    }
}

fn screen_axis_coefficients(
    vectors: [[f32; 2]; 3],
    mask: [bool; 3],
    delta: [f32; 2],
) -> [f32; 3] {
    // Minimum-norm solution of A*c=delta. It works for one, two, or all three
    // projected axes and naturally gives free movement in the camera plane.
    let mut xx = 0.0;
    let mut xy = 0.0;
    let mut yy = 0.0;
    for axis in 0..3 {
        if mask[axis] {
            xx += vectors[axis][0] * vectors[axis][0];
            xy += vectors[axis][0] * vectors[axis][1];
            yy += vectors[axis][1] * vectors[axis][1];
        }
    }
    let determinant = xx * yy - xy * xy;
    let mut result = [0.0; 3];
    if determinant.abs() > 0.001 {
        let solved = [
            (yy * delta[0] - xy * delta[1]) / determinant,
            (-xy * delta[0] + xx * delta[1]) / determinant,
        ];
        for axis in 0..3 {
            if mask[axis] {
                result[axis] =
                    vectors[axis][0] * solved[0] + vectors[axis][1] * solved[1];
            }
        }
    } else {
        for axis in 0..3 {
            if mask[axis] {
                let length_squared = vectors[axis][0] * vectors[axis][0]
                    + vectors[axis][1] * vectors[axis][1];
                if length_squared > 0.001 {
                    result[axis] = (vectors[axis][0] * delta[0]
                        + vectors[axis][1] * delta[1])
                        / length_squared;
                }
            }
        }
    }
    result
}

fn single_enabled_axis(mask: [bool; 3]) -> Option<GizmoAxis> {
    if mask.iter().filter(|enabled| **enabled).count() != 1 {
        return None;
    }
    mask.into_iter()
        .position(|enabled| enabled)
        .map(gizmo_axis_from_index)
}

const fn gizmo_axis_index(axis: GizmoAxis) -> usize {
    match axis {
        GizmoAxis::X => 0,
        GizmoAxis::Y => 1,
        GizmoAxis::Z => 2,
    }
}

const fn gizmo_axis_from_index(index: usize) -> GizmoAxis {
    match index {
        0 => GizmoAxis::X,
        1 => GizmoAxis::Y,
        _ => GizmoAxis::Z,
    }
}

struct GizmoGeometry {
    origin: [f32; 3],
    axes: [(GizmoAxis, [f32; 3]); 3],
    axis_length: f32,
}

fn gizmo_geometry(world: &World, entity: Entity) -> Option<GizmoGeometry> {
    let matrix = world.get::<GlobalTransform>(entity)?.matrix;
    let origin = [matrix[3][0], matrix[3][1], matrix[3][2]];
    let axes = [
        (
            GizmoAxis::X,
            normalized_axis([matrix[0][0], matrix[0][1], matrix[0][2]]),
        ),
        (
            GizmoAxis::Y,
            normalized_axis([matrix[1][0], matrix[1][1], matrix[1][2]]),
        ),
        (
            GizmoAxis::Z,
            normalized_axis([matrix[2][0], matrix[2][1], matrix[2][2]]),
        ),
    ];
    let axis_length = world
        .get::<MeshRenderer>(entity)
        .and_then(|renderer| {
            world.resource::<AssetServer>().meshes.get(renderer.mesh)
        })
        .and_then(|mesh| {
            crate::editor::overlay::mesh_world_radius_from_origin(mesh, matrix)
        })
        .map_or(1.0, |radius| (radius * 1.2).max(1.0));
    Some(GizmoGeometry {
        origin,
        axes,
        axis_length,
    })
}

fn hit_test_gizmo(
    world: &World,
    camera: Entity,
    viewport: egui::Rect,
    pointer: Pos2,
    geometry: &GizmoGeometry,
) -> Option<(f32, GizmoHandle)> {
    let origin_screen =
        project_world_to_screen(world, camera, geometry.origin, viewport)?;
    let mut hits = Vec::new();
    for (index, (axis, direction)) in geometry.axes.iter().copied().enumerate()
    {
        let project_at = |distance: f32| {
            project_world_to_screen(
                world,
                camera,
                [
                    geometry.origin[0] + direction[0] * distance,
                    geometry.origin[1] + direction[1] * distance,
                    geometry.origin[2] + direction[2] * distance,
                ],
                viewport,
            )
        };

        if let (Some(start), Some(end)) = (
            project_at(geometry.axis_length * 0.82),
            project_at(geometry.axis_length * 1.25),
        ) {
            let distance = point_segment_distance(pointer, start, end);
            if (end - start).length() >= 12.0 && distance <= 8.0 {
                hits.push((
                    distance,
                    GizmoHandle {
                        mode: TransformModes::Move,
                        axis,
                    },
                ));
            }
        }

        if let Some(handle) = project_at(geometry.axis_length * 0.68) {
            let distance = pointer.distance(handle);
            if handle.distance(origin_screen) >= 14.0 && distance <= 10.0 {
                hits.push((
                    distance,
                    GizmoHandle {
                        mode: TransformModes::Scale,
                        axis,
                    },
                ));
            }
        }

        let radius = geometry.axis_length * 0.52;
        let mut previous = None;
        let mut ring_distance = f32::INFINITY;
        for step in 0..=48 {
            let angle = std::f32::consts::TAU * step as f32 / 48.0;
            let world_point = gizmo_ring_point(geometry, index, radius, angle);
            let Some(screen) =
                project_world_to_screen(world, camera, world_point, viewport)
            else {
                previous = None;
                continue;
            };
            if let Some(start) = previous {
                ring_distance = ring_distance
                    .min(point_segment_distance(pointer, start, screen));
            }
            previous = Some(screen);
        }
        if ring_distance <= 7.0 {
            hits.push((
                ring_distance,
                GizmoHandle {
                    mode: TransformModes::Rotate,
                    axis,
                },
            ));
        }
    }
    hits.into_iter()
        .min_by(|left, right| left.0.total_cmp(&right.0))
}

fn point_segment_distance(point: Pos2, start: Pos2, end: Pos2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let along = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * along)
}

fn gizmo_ring_point(
    geometry: &GizmoGeometry,
    normal_axis: usize,
    radius: f32,
    angle: f32,
) -> [f32; 3] {
    let first = geometry.axes[(normal_axis + 1) % 3].1;
    let second = geometry.axes[(normal_axis + 2) % 3].1;
    let cosine = angle.cos() * radius;
    let sine = angle.sin() * radius;
    [
        geometry.origin[0] + first[0] * cosine + second[0] * sine,
        geometry.origin[1] + first[1] * cosine + second[1] * sine,
        geometry.origin[2] + first[2] * cosine + second[2] * sine,
    ]
}

fn gizmo_axis_color(
    mode: TransformModes,
    axis: GizmoAxis,
    hovered: Option<GizmoHandle>,
    drag: &EditorGizmoDrag,
    normal: [f32; 4],
) -> [f32; 4] {
    let handle = GizmoHandle { mode, axis };
    if drag.mode == Some(mode) && drag.axis_mask[gizmo_axis_index(axis)] {
        [1.0, 0.9, 0.2, 1.0]
    } else if hovered == Some(handle) {
        [1.0, 1.0, 0.65, 1.0]
    } else {
        normal
    }
}

/// Creates the first editor helpers: a quiet XZ grid and selected-object axes.
/// More helpers such as bounds and collider shapes can append lines here later.
fn build_scene_debug_overlay(
    world: &World,
    selected: Option<Entity>,
    settings: EditorGizmoSettings,
    hovered: Option<GizmoHandle>,
    drag: &EditorGizmoDrag,
    selected_mode: TransformModes,
) -> RenderDebugOverlay {
    let mut overlay = RenderDebugOverlay::default();
    if settings.show_grid {
        // A future procedural grid can follow the editor camera
        // without building CPU vertices every frame.
        for index in -20..=20 {
            let value = index as f32;
            let color = if index == 0 {
                [0.34, 0.38, 0.46, 0.9]
            } else {
                [0.16, 0.18, 0.23, 0.72]
            };
            overlay.line([value, 0.0, -20.0], [value, 0.0, 20.0], color);
            overlay.line([-20.0, 0.0, value], [20.0, 0.0, value], color);
        }
    }
    if settings.show_selected_axes {
        if let Some(entity) = selected {
            if let Some(transform) = world.get::<GlobalTransform>(entity) {
                let matrix = transform.matrix;
                let origin = [matrix[3][0], matrix[3][1], matrix[3][2]];
                let axis_length = world
                    .get::<MeshRenderer>(entity)
                    .and_then(|renderer| {
                        world
                            .resource::<AssetServer>()
                            .meshes
                            .get(renderer.mesh)
                    })
                    .and_then(|mesh| {
                        crate::editor::overlay::mesh_world_radius_from_origin(
                            mesh, matrix,
                        )
                    })
                    .map_or(1.0, |radius| (radius * 1.2).max(1.0));
                let geometry = GizmoGeometry {
                    origin,
                    axes: [
                        (
                            GizmoAxis::X,
                            normalized_axis([
                                matrix[0][0],
                                matrix[0][1],
                                matrix[0][2],
                            ]),
                        ),
                        (
                            GizmoAxis::Y,
                            normalized_axis([
                                matrix[1][0],
                                matrix[1][1],
                                matrix[1][2],
                            ]),
                        ),
                        (
                            GizmoAxis::Z,
                            normalized_axis([
                                matrix[2][0],
                                matrix[2][1],
                                matrix[2][2],
                            ]),
                        ),
                    ],
                    axis_length,
                };
                let display_mode = drag.mode.unwrap_or(selected_mode);
                if matches!(
                    display_mode,
                    TransformModes::Combo | TransformModes::Move
                ) {
                    for (axis, direction) in geometry.axes {
                        add_axis(
                            &mut overlay,
                            origin,
                            direction,
                            axis_length * 1.25,
                            gizmo_axis_color(
                                TransformModes::Move,
                                axis,
                                hovered,
                                drag,
                                gizmo_base_color(axis),
                            ),
                        );
                    }
                }
                if matches!(
                    display_mode,
                    TransformModes::Combo | TransformModes::Scale
                ) {
                    for (axis, direction) in geometry.axes {
                        add_scale_gizmo_handle(
                            &mut overlay,
                            origin,
                            direction,
                            axis_length * 0.68,
                            gizmo_axis_color(
                                TransformModes::Scale,
                                axis,
                                hovered,
                                drag,
                                gizmo_base_color(axis),
                            ),
                        );
                    }
                }
                if matches!(
                    display_mode,
                    TransformModes::Combo | TransformModes::Rotate
                ) {
                    for index in 0..3 {
                        let axis = geometry.axes[index].0;
                        add_rotation_gizmo_ring(
                            &mut overlay,
                            &geometry,
                            index,
                            axis_length * 0.52,
                            gizmo_axis_color(
                                TransformModes::Rotate,
                                axis,
                                hovered,
                                drag,
                                gizmo_base_color(axis),
                            ),
                        );
                    }
                }
                if settings.show_selected_bounds {
                    add_selected_bounds(&mut overlay, world, entity, matrix);
                }
            }
        }
    }
    overlay
}

const fn gizmo_base_color(axis: GizmoAxis) -> [f32; 4] {
    match axis {
        GizmoAxis::X => [0.95, 0.24, 0.24, 1.0],
        GizmoAxis::Y => [0.25, 0.90, 0.35, 1.0],
        GizmoAxis::Z => [0.25, 0.52, 1.0, 1.0],
    }
}

fn add_scale_gizmo_handle(
    overlay: &mut RenderDebugOverlay,
    origin: [f32; 3],
    direction: [f32; 3],
    length: f32,
    color: [f32; 4],
) {
    let end = add3(origin, scale3(direction, length));
    overlay.line_on_top(origin, end, color, 2.0);
    let reference = if direction[1].abs() < 0.9 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let side = normalized_axis(cross3(direction, reference));
    let other = normalized_axis(cross3(direction, side));
    let size = length * 0.075;
    overlay.line_on_top(
        add3(end, scale3(side, -size)),
        add3(end, scale3(side, size)),
        color,
        5.0,
    );
    overlay.line_on_top(
        add3(end, scale3(other, -size)),
        add3(end, scale3(other, size)),
        color,
        5.0,
    );
}

fn add_rotation_gizmo_ring(
    overlay: &mut RenderDebugOverlay,
    geometry: &GizmoGeometry,
    normal_axis: usize,
    radius: f32,
    color: [f32; 4],
) {
    let mut previous = gizmo_ring_point(geometry, normal_axis, radius, 0.0);
    for step in 1..=48 {
        let angle = std::f32::consts::TAU * step as f32 / 48.0;
        let current = gizmo_ring_point(geometry, normal_axis, radius, angle);
        overlay.line_on_top(previous, current, color, 2.0);
        previous = current;
    }
}

fn add3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale3(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn cross3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

/// Finds the selected mesh's local bounds and asks the overlay helper to draw
/// them in world space. Physics colliders are intentionally not involved.
fn add_selected_bounds(
    overlay: &mut RenderDebugOverlay,
    world: &World,
    entity: Entity,
    matrix: [[f32; 4]; 4],
) {
    let Some(renderer) = world.get::<MeshRenderer>(entity).copied() else {
        return;
    };
    let Some(mesh) = world.resource::<AssetServer>().meshes.get(renderer.mesh)
    else {
        return;
    };
    let Some((minimum, maximum)) = crate::editor::overlay::mesh_bounds(mesh)
    else {
        return;
    };
    add_bound_box(overlay, matrix, minimum, maximum, [1.0, 0.78, 0.12, 1.0]);
}

/// Removes object scale from a transform column so gizmos stay a useful size.
fn normalized_axis(axis: [f32; 3]) -> [f32; 3] {
    let length =
        (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if length > f32::EPSILON {
        [axis[0] / length, axis[1] / length, axis[2] / length]
    } else {
        [1.0, 0.0, 0.0]
    }
}

#[cfg(test)]
mod gizmo_tests {
    use super::*;

    #[test]
    fn projected_two_axis_move_recovers_both_axis_distances() {
        let coefficients = screen_axis_coefficients(
            [[10.0, 0.0], [0.0, 20.0], [5.0, 5.0]],
            [true, true, false],
            [30.0, 40.0],
        );

        assert!((coefficients[0] - 3.0).abs() < 0.001);
        assert!((coefficients[1] - 2.0).abs() < 0.001);
        assert_eq!(coefficients[2], 0.0);
    }

    #[test]
    fn move_line_uses_world_axis_parameter_without_screen_projection_drift() {
        let ray = super::super::picking::Ray {
            origin: Vector3::new(5.0, 0.0, 10.0),
            direction: Vector3::new(0.0, 0.0, -1.0),
        };
        let parameter = move_axis_parameter(
            ray,
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        )
        .unwrap();
        let drag = EditorGizmoDrag {
            mode: Some(TransformModes::Move),
            axis_mask: [true, false, false],
            start_pointer: Some(Pos2::ZERO),
            original_transform: Some(Transform::default()),
            local_delta_axes: [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            move_start_axis_parameter: Some(2.0),
            ..EditorGizmoDrag::default()
        };

        let transformed =
            transformed_from_pointer(&drag, Pos2::ZERO, Some(parameter))
                .unwrap();

        assert!((transformed.position[0] - 3.0).abs() < 0.001);
        assert_eq!(transformed.position[1], 0.0);
        assert_eq!(transformed.position[2], 0.0);
    }

    #[test]
    fn modal_scale_only_changes_enabled_axes() {
        let drag = EditorGizmoDrag {
            mode: Some(TransformModes::Scale),
            modal: true,
            axis_mask: [true, false, true],
            start_pointer: Some(Pos2::ZERO),
            original_transform: Some(
                Transform::default().with_scale(1.0, 2.0, 3.0),
            ),
            ..EditorGizmoDrag::default()
        };
        let pointer = Pos2::new(std::f32::consts::LN_2 / 0.01, 0.0);

        let transformed =
            transformed_from_pointer(&drag, pointer, None).unwrap();

        assert!((transformed.scale[0] - 2.0).abs() < 0.001);
        assert_eq!(transformed.scale[1], 2.0);
        assert!((transformed.scale[2] - 6.0).abs() < 0.001);
    }

    #[test]
    fn direct_scale_sensitivity_uses_visible_handle_length() {
        let make_drag = |axis_length| EditorGizmoDrag {
            mode: Some(TransformModes::Scale),
            modal: false,
            axis_mask: [true, false, false],
            active_axis: Some(GizmoAxis::X),
            start_pointer: Some(Pos2::ZERO),
            original_transform: Some(Transform::default()),
            screen_vectors: [[5.0, 0.0], [0.0; 2], [0.0; 2]],
            gizmo_axis_length: axis_length,
            ..EditorGizmoDrag::default()
        };
        let short = transformed_from_pointer(
            &make_drag(10.0),
            Pos2::new(34.0, 0.0),
            None,
        )
        .unwrap();
        let long = transformed_from_pointer(
            &make_drag(20.0),
            Pos2::new(68.0, 0.0),
            None,
        )
        .unwrap();

        assert!((short.scale[0] - 2.0).abs() < 0.001);
        assert!((long.scale[0] - 2.0).abs() < 0.001);
    }

    #[test]
    fn projected_ring_direction_is_converted_to_positive_local_rotation() {
        let drag = EditorGizmoDrag {
            start_pointer: Some(Pos2::new(10.0, 0.0)),
            origin_screen: Some(Pos2::ZERO),
            rotation_screen_signs: [1.0, 1.0, -1.0],
            ..EditorGizmoDrag::default()
        };

        let angle = screen_rotation_angle(&drag, Pos2::new(0.0, -10.0), 2);

        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
    }

    #[test]
    fn rotation_composes_around_the_objects_local_axis() {
        let original = Transform::default().with_rotation(0.4, 0.7, -0.2);
        let angle = 0.3;
        let transformed = rotate_transform_locally(
            original,
            [0.0, 0.0, angle],
            [false, false, true],
        );
        let actual = Matrix4::from(transformed.to_matrix());
        let expected = Matrix4::from(original.to_matrix())
            * Rotation3::from_axis_angle(&Vector3::z_axis(), angle)
                .to_homogeneous();

        for row in 0..3 {
            for column in 0..3 {
                assert!(
                    (actual[(row, column)] - expected[(row, column)]).abs()
                        < 0.001
                );
            }
        }
    }

    #[test]
    fn free_rotation_composes_around_the_camera_facing_axis() {
        let original = Transform::default().with_rotation(0.4, 0.7, -0.2);
        let angle = 0.3;
        let transformed =
            rotate_transform_in_parent_space(original, [0.0, 0.0, -1.0], angle);
        let actual = Matrix4::from(transformed.to_matrix());
        let expected = Rotation3::from_axis_angle(&Vector3::z_axis(), -angle)
            .to_homogeneous()
            * Matrix4::from(original.to_matrix());

        for row in 0..3 {
            for column in 0..3 {
                assert!(
                    (actual[(row, column)] - expected[(row, column)]).abs()
                        < 0.001
                );
            }
        }
    }
}
