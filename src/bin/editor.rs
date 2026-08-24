use std::time::Instant;

use egui_winit_vulkano::{Gui, GuiConfig};
use rusting_engine::demo::{DemoPlugin, Spin};
use rusting_engine::editor::{
    configure_editor_style, draw_editor_view, EditorState, EditorViewport,
};
use rusting_engine::rendering::frame_pacer::{select_present_mode, FramePacer};
use rusting_engine::rendering::scene_renderer::{SceneRenderer, SceneViewport};
use rusting_engine::runtime::{
    load_scene, Camera, MeshRenderer, Name, RenderExtractPlugin, RenderWorld,
    SceneLoadMode, ScriptPlugin, ScriptSettings, TimeControl,
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
    vulkan: VulkanoContext,
    windows: VulkanoWindows,
    gui: Option<Gui>,
    scene_renderer: Option<SceneRenderer>,
    runtime: RuntimeApp,
    previous_frame: Instant,
    frame_pacer: FramePacer,
    applied_vsync: Option<bool>,
}

impl EditorApplication {
    fn new() -> Self {
        let mut runtime = RuntimeApp::new();
        runtime.add_plugin(AssetPlugin).unwrap();
        runtime.add_plugin(RenderExtractPlugin).unwrap();
        runtime.add_plugin(DemoPlugin).unwrap();
        runtime.add_plugin(ScriptPlugin).unwrap();
        runtime.insert_resource(EditorState::default());
        runtime.insert_resource(EditorViewport::default());
        runtime.world_mut().resource_mut::<TimeControl>().pause();
        runtime.world_mut().resource_mut::<ScriptSettings>().enabled = false;

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
        if self.gui.is_some() {
            return;
        }
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
        gui.update(&event);
        let renderer = self.windows.get_renderer_mut(window_id).unwrap();

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                renderer.resize();
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let delta = now.saturating_duration_since(self.previous_frame);
                self.previous_frame = now;
                if let Err(error) = self.runtime.update(delta) {
                    eprintln!("editor runtime update failed: {error}");
                    event_loop.exit();
                    return;
                }

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
                                viewport,
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
