use std::time::Instant;

use egui_winit_vulkano::{Gui, GuiConfig};
use rusting_engine::demo::{DemoPlugin, Spin};
use rusting_engine::editor::{
    configure_editor_style, draw_editor_view, EditorDebugOverlay, EditorPlugin,
    EditorState, EditorViewport, EditorWorkspace,
};
use rusting_engine::rendering::frame_pacer::{select_present_mode, FramePacer};
use rusting_engine::rendering::scene_renderer::{
    SceneRenderOptions, SceneRenderer, SceneViewport,
};
use rusting_engine::runtime::{
    load_scene, Camera, MeshRenderer, Name, RenderExtractPlugin, RenderWorld,
    SceneLoadMode, TimeControl,
};
use rusting_engine::{
    App as RuntimeApp, AssetPlugin, AssetServer, MaterialAsset, Transform,
};
use vulkano::format::Format;
use vulkano::VulkanError;
use vulkano_util::context::{VulkanoConfig, VulkanoContext};
use vulkano_util::window::{VulkanoWindows, WindowDescriptor};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

struct EditorApplication {
    /// Vulkan device, queues, and shared memory allocators.
    vulkan: VulkanoContext,
    /// Winit windows and their Vulkan swapchains.
    windows: VulkanoWindows,
    /// Egui renderer created after the window opens.
    gui: Option<Gui>,
    /// 3D renderer created after the swapchain format is known.
    scene_renderer: Option<SceneRenderer>,
    /// ECS world containing scene objects and editor state.
    runtime: RuntimeApp,
    /// Time of the previous frame, used to calculate delta time.
    previous_frame: Instant,
    /// Handles unlimited and limited frame-rate modes.
    frame_pacer: FramePacer,
    /// VSync value currently used by the swapchain.
    applied_vsync: Option<bool>,
}

impl EditorApplication {
    /// Creates editor data that does not need an open operating-system window.
    fn new() -> Self {
        // Plugins add assets, render extraction, demo behaviour, and GUI state.
        let mut runtime = RuntimeApp::new();
        runtime.add_plugin(AssetPlugin).unwrap();
        runtime.add_plugin(RenderExtractPlugin).unwrap();
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
        // Replace the demo objects when a saved editor scene already exists.
        let scene_path =
            runtime.world().resource::<EditorState>().scene_path.clone();
        if std::path::Path::new(&scene_path).is_file() {
            if let Err(error) = load_scene(
                runtime.world_mut(),
                &scene_path,
                SceneLoadMode::Replace,
            ) {
                eprintln!("failed to restore editor scene: {error}");
            }
        }
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
            .resource_mut::<rusting_engine::runtime::RenderCameraOverride>()
            .entity = Some(editor_camera);

        Self {
            vulkan: VulkanoContext::new(VulkanoConfig::default()),
            windows: VulkanoWindows::default(),
            gui: None,
            scene_renderer: None,
            runtime,
            previous_frame: Instant::now(),
            frame_pacer: FramePacer::default(),
            applied_vsync: None,
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
        let gui = Gui::new(
            event_loop,
            renderer.surface(),
            renderer.graphics_queue(),
            renderer.swapchain_format(),
            GuiConfig {
                is_overlay: true,
                ..GuiConfig::default()
            },
        );
        configure_editor_style(&gui.context());
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
        // Give keyboard, mouse, and clipboard events to egui first.
        gui.update(&event);
        let renderer = self.windows.get_renderer_mut(window_id).unwrap();

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                renderer.resize();
            }
            WindowEvent::RedrawRequested => {
                // Update game time and all ECS schedules before drawing.
                let now = Instant::now();
                let delta = now.saturating_duration_since(self.previous_frame);
                self.previous_frame = now;
                if let Err(error) = self.runtime.update(delta) {
                    eprintln!("editor runtime update failed: {error}");
                    event_loop.exit();
                    return;
                }

                // GUI changes ECS values and reports the live 3D rectangle.
                gui.immediate_ui(|gui| {
                    draw_editor_view(self.runtime.world_mut(), &gui.context());
                });
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
                        let target_extent = renderer.swapchain_image_size();
                        let editor_viewport =
                            *self.runtime.world().resource::<EditorViewport>();
                        let viewport = if editor_viewport.valid {
                            SceneViewport {
                                offset: editor_viewport.offset,
                                extent: editor_viewport.extent,
                            }
                        } else {
                            SceneViewport::full(target_extent)
                        };
                        let future =
                            match self.scene_renderer.as_mut().unwrap().render(
                                future,
                                renderer.swapchain_image_view(),
                                target_extent,
                                SceneRenderOptions {
                                    viewport,
                                    debug_overlay: (self
                                        .runtime
                                        .world()
                                        .resource::<EditorState>()
                                        .workspace
                                        == EditorWorkspace::Scene)
                                        .then(|| {
                                            &self
                                                .runtime
                                                .world()
                                                .resource::<EditorDebugOverlay>(
                                                )
                                                .0
                                        }),
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
                        let future = gui.draw_on_image(
                            future,
                            renderer.swapchain_image_view(),
                        );
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(renderer) = self.windows.get_primary_renderer_mut() {
            self.frame_pacer.request_next_frame(
                event_loop,
                renderer.window(),
                self.runtime
                    .world()
                    .resource::<rusting_engine::runtime::RenderSettings>(),
            );
        }
    }
}

fn main() -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut EditorApplication::new())
}
