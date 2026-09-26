use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusting_engine::demo::{DemoPlugin, Spin};
use rusting_engine::editor::{
    add_mouse_delta, configure_editor_style, draw_editor_view,
    editor_debug_view, editor_navigation_active,
    editor_needs_continuous_redraw, handle_keyboard_input,
    handle_mouse_button_input, handle_mouse_wheel, load_editor_scene,
    release_editor_navigation, ui_receives_during_navigation,
    update_fly_camera, EditorDebugOverlay, EditorPlugin, EditorShortcuts,
    EditorState, EditorViewport, EditorWorkspace, SCENE_VIEW_TEXTURE,
};
use rusting_engine::rendering::egui_painter::EguiPainter;
use rusting_engine::rendering::frame_pacer::{select_present_mode, FramePacer};
use rusting_engine::rendering::scene_renderer::{
    SceneRenderOptions, SceneRenderer, SceneViewport,
};
use rusting_engine::runtime::{
    extract_render_world, Camera, CpuFrameTimings, MeshRenderer, Name,
    RenderCameraOverride, RenderWorld, TimeControl,
};
use rusting_engine::{
    App as RuntimeApp, AssetPlugin, AssetServer, MaterialAsset, Transform,
};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::AllocationCreateInfo;
use vulkano::VulkanError;
use vulkano_util::context::VulkanoContext;
use vulkano_util::window::{VulkanoWindows, WindowDescriptor};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

/// egui context, winit input translation, and the engine painter.
struct Gui {
    context: egui::Context,
    input: egui_winit::State,
    painter: EguiPainter,
    /// Texture changes not painted yet. A frame that fails to acquire an
    /// image keeps them for the next one, so the font atlas is never lost.
    textures: egui::TexturesDelta,
    primitives: Vec<egui::ClippedPrimitive>,
    pixels_per_point: f32,
}

struct EditorApplication {
    /// Vulkan device, queues, and shared memory allocators.
    vulkan: VulkanoContext,
    /// Winit windows and their Vulkan swapchains.
    windows: VulkanoWindows,
    /// Egui renderer created after the window opens.
    gui: Option<Gui>,
    /// 3D renderer created after the swapchain format is known.
    scene_renderer: Option<SceneRenderer>,
    /// Offscreen image the live 3D view renders into; egui shows it as
    /// [`SCENE_VIEW_TEXTURE`]. Recreated when the view changes size.
    scene_target: Option<Arc<ImageView>>,
    /// ECS world containing scene objects and editor state.
    runtime: RuntimeApp,
    /// Time of the previous frame, used to calculate delta time.
    previous_frame: Instant,
    /// Handles unlimited and limited frame-rate modes.
    frame_pacer: FramePacer,
    /// VSync value currently used by the swapchain.
    applied_vsync: Option<bool>,
    /// Latest physical cursor coordinates used to enter fly mode from Scene View.
    cursor_position: [f64; 2],
    /// Modifier keys held now, for Ctrl/Shift editor shortcuts.
    modifiers: winit::keyboard::ModifiersState,
    /// True after a window event that the next frame must show.
    input_pending: bool,
    /// Earliest time egui asked to repaint, set by its repaint callback.
    egui_repaint_at: Arc<Mutex<Option<Instant>>>,
    /// Time of the next idle redraw, which shows build output, finished asset
    /// loads, and hot reloads that arrive without input.
    idle_redraw_at: Instant,
}

/// Longest time an idle editor waits before it draws a frame.
const IDLE_REDRAW_INTERVAL: Duration = Duration::from_millis(500);

impl EditorApplication {
    /// Creates editor data that does not need an open operating-system window.
    fn new() -> Self {
        // Plugins add assets, render extraction, demo behaviour, and GUI state.
        let mut runtime = RuntimeApp::new();
        runtime.add_plugin(AssetPlugin).unwrap();
        // The editor extracts once, after the GUI edits the world, so it adds
        // RenderExtractPlugin's resources without its scheduled system.
        runtime
            .insert_resource(RenderWorld::default())
            .insert_resource(RenderCameraOverride::default());
        runtime.add_plugin(DemoPlugin).unwrap();
        runtime.add_plugin(EditorPlugin).unwrap();
        runtime.world_mut().resource_mut::<TimeControl>().pause();

        // Create the mesh and materials used by the small default scene.
        let (mesh, blue_material, orange_material) = {
            let mut assets = runtime.world_mut().resource_mut::<AssetServer>();
            let blue_material = assets.materials.insert(MaterialAsset {
                base_color: [0.08, 0.35, 0.95, 1.0],
                ..MaterialAsset::default()
            });
            let orange_material = assets.materials.insert(MaterialAsset {
                base_color: [1.0, 0.28, 0.04, 1.0],
                ..MaterialAsset::default()
            });
            (assets.fallback_mesh, blue_material, orange_material)
        };
        let blue_renderer = MeshRenderer {
            mesh,
            material: blue_material,
            cast_shadows: true,
            receive_shadows: true,
        };
        // Spawn a scene that is visible before the user saves a project scene.
        let scene_root =
            runtime.spawn((Name("Demo Scene".into()), Transform::default()));
        let blue = runtime.spawn((
            Name("Blue Cube".into()),
            Transform::default(),
            blue_renderer,
        ));
        let orange_renderer = MeshRenderer {
            mesh,
            material: orange_material,
            cast_shadows: true,
            receive_shadows: true,
        };
        let orange = runtime.spawn((
            Name("Orange Cube".into()),
            Transform::new([2.0, 1.0, 0.0]),
            orange_renderer,
            Spin::default(),
        ));
        runtime.set_parent(blue, scene_root).unwrap();
        runtime.set_parent(orange, scene_root).unwrap();
        runtime.spawn((
            Name("Game Camera".into()),
            Transform::new([0.0, 3.0, 8.0]),
            Camera {
                active: true,
                priority: 10,
                ..Camera::default()
            },
        ));
        // The editor camera is separate from the camera shipped with the game.
        let editor_camera = runtime
            .world_mut()
            .spawn((
                Name("Editor Camera".into()),
                Transform::new([0.0, 3.0, 8.0]),
                Camera {
                    active: false,
                    priority: 0,
                    ..Camera::default()
                },
            ))
            .id();
        runtime
            .world_mut()
            .resource_mut::<EditorState>()
            .editor_camera = Some(editor_camera);
        runtime
            .world_mut()
            .resource_mut::<RenderCameraOverride>()
            .entity = Some(editor_camera);
        // Replace the demo objects when a saved editor scene already exists.
        // This runs after the editor camera exists so its saved pose applies.
        let scene_path =
            runtime.world().resource::<EditorState>().scene_path.clone();
        if std::path::Path::new(&scene_path).is_file() {
            let result = runtime.world_mut().resource_scope(
                |world, mut state: bevy_ecs::prelude::Mut<EditorState>| {
                    load_editor_scene(
                        world,
                        &mut state,
                        std::path::Path::new(&scene_path),
                    )
                },
            );
            if let Err(error) = result {
                eprintln!("failed to restore editor scene: {error}");
            }
        }

        Self {
            vulkan: VulkanoContext::new(
                rusting_engine::rendering::vulkano_config(),
            ),
            windows: VulkanoWindows::default(),
            gui: None,
            scene_renderer: None,
            scene_target: None,
            runtime,
            previous_frame: Instant::now(),
            frame_pacer: FramePacer::default(),
            applied_vsync: None,
            cursor_position: [0.0; 2],
            modifiers: winit::keyboard::ModifiersState::empty(),
            input_pending: true,
            egui_repaint_at: Arc::default(),
            idle_redraw_at: Instant::now(),
        }
    }
}

impl ApplicationHandler for EditorApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Winit can resume more than once, but the GUI must be created once.
        if self.gui.is_some() {
            return;
        }
        // Set the initial size and title of the editor window.
        let descriptor = WindowDescriptor {
            title: "RustingEngine Editor".into(),
            width: 1440.0,
            height: 900.0,
            ..WindowDescriptor::default()
        };
        self.windows.create_window(
            event_loop,
            &self.vulkan,
            &descriptor,
            |create_info| {
                create_info.image_format = Format::B8G8R8A8_UNORM;
                create_info.min_image_count =
                    create_info.min_image_count.max(2);
            },
        );

        // The window now exists, so swapchain-dependent renderers can be made.
        let renderer = self.windows.get_primary_renderer_mut().unwrap();
        let settings = self
            .runtime
            .world()
            .resource::<rusting_engine::runtime::RenderSettings>();
        renderer.set_present_mode(select_present_mode(
            &renderer.graphics_queue(),
            &renderer.surface(),
            settings.vsync,
        ));
        self.applied_vsync = Some(settings.vsync);
        self.scene_renderer = Some(
            SceneRenderer::new(
                renderer.graphics_queue(),
                self.vulkan.memory_allocator().clone(),
                renderer.swapchain_format(),
                renderer.swapchain_image_size(),
            )
            .expect("failed to create editor scene renderer"),
        );
        // Egui draws last, which places controls over the 3D scene.
        let painter = EguiPainter::new(
            renderer.graphics_queue(),
            self.vulkan.memory_allocator().clone(),
            renderer.swapchain_format(),
        )
        .expect("failed to create editor egui painter");
        let context = egui::Context::default();
        let input = egui_winit::State::new(
            context.clone(),
            context.viewport_id(),
            event_loop,
            Some(renderer.window().scale_factor() as f32),
            Some(match context.theme() {
                egui::Theme::Dark => winit::window::Theme::Dark,
                egui::Theme::Light => winit::window::Theme::Light,
            }),
            Some(painter.max_texture_side()),
        );
        let gui = Gui {
            context,
            input,
            painter,
            textures: egui::TexturesDelta::default(),
            primitives: Vec::new(),
            pixels_per_point: 1.0,
        };
        configure_editor_style(&gui.context);
        self.runtime
            .world_mut()
            .insert_resource(EditorShortcuts::load());
        let repaint_at = Arc::clone(&self.egui_repaint_at);
        gui.context.set_request_repaint_callback(move |info| {
            let at = Instant::now() + info.delay;
            if let Ok(mut slot) = repaint_at.lock() {
                *slot = Some(slot.map_or(at, |old| old.min(at)));
            }
        });
        self.gui = Some(gui);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(gui) = self.gui.as_mut() else {
            return;
        };
        let renderer = self.windows.get_renderer_mut(window_id).unwrap();
        // Give keyboard, mouse, and clipboard events to egui first, except
        // the ones fly or orbit navigation owns.
        let was_navigating = editor_navigation_active(self.runtime.world());
        if !was_navigating || ui_receives_during_navigation(&event) {
            let _ = gui.input.on_window_event(renderer.window(), &event);
        }
        if !matches!(event, WindowEvent::RedrawRequested) {
            self.input_pending = true;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(false) => {
                release_editor_navigation(
                    self.runtime.world_mut(),
                    renderer.window(),
                );
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let ui_wants_keyboard = gui.context.wants_keyboard_input();
                handle_keyboard_input(
                    self.runtime.world_mut(),
                    renderer.window(),
                    &event,
                    self.modifiers,
                    ui_wants_keyboard,
                );
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = [position.x, position.y];
            }
            WindowEvent::MouseInput { state, button, .. } => {
                handle_mouse_button_input(
                    self.runtime.world_mut(),
                    renderer.window(),
                    state,
                    button,
                    self.cursor_position,
                    gui.context.input(|input| input.modifiers.shift),
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    // About 20 logical pixels per wheel line, so trackpad
                    // dolly speed does not depend on display DPI.
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.y / (renderer.window().scale_factor() * 20.0))
                            as f32
                    }
                };
                handle_mouse_wheel(
                    self.runtime.world_mut(),
                    lines,
                    self.cursor_position,
                );
            }
            WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                renderer.resize();
            }
            WindowEvent::RedrawRequested => {
                // Update game time and all ECS schedules before drawing.
                let now = Instant::now();
                self.input_pending = false;
                self.idle_redraw_at = now + IDLE_REDRAW_INTERVAL;
                // The callback refills this with requests made by this frame.
                if let Ok(mut slot) = self.egui_repaint_at.lock() {
                    *slot = None;
                }
                let delta = now.saturating_duration_since(self.previous_frame);
                self.previous_frame = now;
                // Navigation runs before ECS extraction, so the renderer uses
                // the new editor-camera transform in this same frame.
                update_fly_camera(self.runtime.world_mut(), delta);
                if let Err(error) = self.runtime.update(delta) {
                    eprintln!("editor runtime update failed: {error}");
                    event_loop.exit();
                    return;
                }

                // GUI changes ECS values and reports the live 3D rectangle.
                let editor_start = Instant::now();
                let raw_input = gui.input.take_egui_input(renderer.window());
                let output = gui.context.run(raw_input, |context| {
                    draw_editor_view(self.runtime.world_mut(), context);
                });
                gui.input.handle_platform_output(
                    renderer.window(),
                    output.platform_output,
                );
                gui.textures.append(output.textures_delta);
                gui.pixels_per_point = output.pixels_per_point;
                gui.primitives = gui
                    .context
                    .tessellate(output.shapes, output.pixels_per_point);
                let extraction_start = Instant::now();
                extract_render_world(self.runtime.world_mut());
                {
                    let world = self.runtime.world_mut();
                    let mut timings = world.resource_mut::<CpuFrameTimings>();
                    timings.editor =
                        extraction_start.duration_since(editor_start);
                    // The GUI edits the scene, so extraction runs again here.
                    timings.extraction += extraction_start.elapsed();
                }
                let vsync = self
                    .runtime
                    .world()
                    .resource::<rusting_engine::runtime::RenderSettings>()
                    .vsync;
                if self.applied_vsync != Some(vsync) {
                    renderer.set_present_mode(select_present_mode(
                        &renderer.graphics_queue(),
                        &renderer.surface(),
                        vsync,
                    ));
                    self.applied_vsync = Some(vsync);
                }
                // Draw Vulkan scene first, egui second, and then present.
                match renderer.acquire(None, |_| {}) {
                    Ok(future) => {
                        let editor_viewport =
                            *self.runtime.world().resource::<EditorViewport>();
                        // The live view renders offscreen and egui draws the
                        // image in its area. Without one, the scene still
                        // renders (GPU physics runs in that pass) into the
                        // window, under the opaque editor panels.
                        let (scene_target, target_extent) = if editor_viewport
                            .valid
                            && editor_viewport
                                .extent
                                .iter()
                                .all(|side| *side > 0)
                        {
                            let target = match self.scene_target.take() {
                                Some(target)
                                    if target.image().extent()[..2]
                                        == editor_viewport.extent =>
                                {
                                    target
                                }
                                _ => match scene_view_target(
                                    &self.vulkan,
                                    renderer.swapchain_format(),
                                    editor_viewport.extent,
                                ) {
                                    Ok(target) => target,
                                    Err(error) => {
                                        eprintln!("failed to create the scene view image: {error}");
                                        event_loop.exit();
                                        return;
                                    }
                                },
                            };
                            if let Err(error) = gui.painter.set_native_texture(
                                SCENE_VIEW_TEXTURE,
                                target.clone(),
                            ) {
                                eprintln!(
                                    "failed to show the scene view: {error}"
                                );
                                event_loop.exit();
                                return;
                            }
                            self.scene_target = Some(target.clone());
                            (target, editor_viewport.extent)
                        } else {
                            (
                                renderer.swapchain_image_view(),
                                renderer.swapchain_image_size(),
                            )
                        };
                        let viewport = SceneViewport::full(target_extent);
                        let world = self.runtime.world();
                        let scene_view =
                            world.resource::<EditorState>().workspace
                                == EditorWorkspace::Scene;
                        let future =
                            match self.scene_renderer.as_mut().unwrap().render(
                                future,
                                scene_target,
                                target_extent,
                                SceneRenderOptions {
                                    viewport,
                                    debug_overlay: scene_view.then(|| {
                                        &world
                                            .resource::<EditorDebugOverlay>()
                                            .0
                                    }),
                                    debug_view: editor_debug_view(world),
                                },
                                self.runtime.world().resource::<RenderWorld>(),
                                self.runtime.world().resource::<AssetServer>(),
                            ) {
                                Ok(future) => future,
                                Err(error) => {
                                    eprintln!(
                                    "failed to render editor scene: {error}"
                                );
                                    event_loop.exit();
                                    return;
                                }
                            };
                        // The UI shows these on the next frame.
                        let scene_renderer =
                            self.scene_renderer.as_mut().unwrap();
                        let culling = scene_renderer.culling_stats();
                        let pass_times = scene_renderer.gpu_pass_times();
                        let counters = scene_renderer.render_counters();
                        let capacity = scene_renderer.capacity_diagnostics();
                        self.runtime.world_mut().insert_resource(counters);
                        self.runtime.world_mut().insert_resource(capacity);
                        self.runtime.world_mut().insert_resource(culling);
                        self.runtime.world_mut().insert_resource(pass_times);
                        scene_renderer.write_cpu_timings(
                            &mut self
                                .runtime
                                .world_mut()
                                .resource_mut::<CpuFrameTimings>(),
                        );
                        let future = match gui.painter.paint(
                            future,
                            renderer.swapchain_image_view(),
                            gui.pixels_per_point,
                            &gui.primitives,
                            &gui.textures,
                        ) {
                            Ok(future) => future,
                            Err(error) => {
                                eprintln!("failed to paint editor UI: {error}");
                                event_loop.exit();
                                return;
                            }
                        };
                        gui.textures.clear();
                        renderer.present(future, false);
                    }
                    Err(VulkanError::OutOfDate) => renderer.resize(),
                    Err(error) => {
                        eprintln!(
                            "failed to acquire editor swapchain image: {error}"
                        );
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            add_mouse_delta(self.runtime.world_mut(), delta);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(renderer) = self.windows.get_primary_renderer_mut() else {
            return;
        };
        let now = Instant::now();
        let egui_repaint_at =
            self.egui_repaint_at.lock().ok().and_then(|at| *at);
        let wake = egui_repaint_at
            .map_or(self.idle_redraw_at, |at| at.min(self.idle_redraw_at));
        if self.input_pending
            || now >= wake
            || editor_needs_continuous_redraw(self.runtime.world())
        {
            self.frame_pacer.request_next_frame(
                event_loop,
                renderer.window(),
                self.runtime
                    .world()
                    .resource::<rusting_engine::runtime::RenderSettings>(),
            );
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        }
    }
}

/// Creates the offscreen image the live 3D view renders into. It uses the
/// swapchain format because the scene renderer's pipelines are built for it.
fn scene_view_target(
    vulkan: &VulkanoContext,
    format: Format,
    extent: [u32; 2],
) -> Result<Arc<ImageView>, Box<dyn std::error::Error>> {
    let image = Image::new(
        vulkan.memory_allocator().clone(),
        ImageCreateInfo {
            format,
            extent: [extent[0], extent[1], 1],
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::SAMPLED,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )?;
    Ok(ImageView::new_default(image)?)
}

fn main() -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut EditorApplication::new())
}
