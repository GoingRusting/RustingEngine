//! Runtime-only scene player. This target contains no egui/editor code.

use std::error::Error;
use std::path::PathBuf;
use std::time::Instant;

use rusting_engine::demo::DemoPlugin;
use rusting_engine::rendering::frame_pacer::{select_present_mode, FramePacer};
use rusting_engine::rendering::scene_renderer::{SceneRenderer, SceneViewport};
use rusting_engine::runtime::{
    load_scene, RenderExtractPlugin, RenderSettings, RenderWorld, SceneLoadMode,
};
use rusting_engine::{App, AssetPlugin, AssetServer};
use vulkano::format::Format;
use vulkano::VulkanError;
use vulkano_util::context::{VulkanoConfig, VulkanoContext};
use vulkano_util::window::{VulkanoWindows, WindowDescriptor};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

struct GameApplication {
    vulkan: VulkanoContext,
    windows: VulkanoWindows,
    scene_renderer: Option<SceneRenderer>,
    runtime: App,
    previous_frame: Instant,
    frame_pacer: FramePacer,
    applied_vsync: Option<bool>,
}

impl GameApplication {
    fn load(scene_path: PathBuf) -> Result<Self, Box<dyn Error>> {
        let mut runtime = App::new();
        runtime.add_plugin(AssetPlugin)?;
        runtime.add_plugin(RenderExtractPlugin)?;
        runtime.add_plugin(DemoPlugin)?;
        load_scene(runtime.world_mut(), &scene_path, SceneLoadMode::Replace)?;
        Ok(Self {
            vulkan: VulkanoContext::new(VulkanoConfig::default()),
            windows: VulkanoWindows::default(),
            scene_renderer: None,
            runtime,
            previous_frame: Instant::now(),
            frame_pacer: FramePacer::default(),
            applied_vsync: None,
        })
    }
}

impl ApplicationHandler for GameApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.scene_renderer.is_some() {
            return;
        }
        self.windows.create_window(
            event_loop,
            &self.vulkan,
            &WindowDescriptor {
                title: "RustingEngine Game".into(),
                width: 1440.0,
                height: 900.0,
                ..WindowDescriptor::default()
            },
            |create_info| {
                create_info.image_format = Format::B8G8R8A8_UNORM;
                create_info.min_image_count =
                    create_info.min_image_count.max(2);
            },
        );
        let renderer = self.windows.get_primary_renderer_mut().unwrap();
        let settings = self.runtime.world().resource::<RenderSettings>();
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
            .expect("failed to create game scene renderer"),
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
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
                    eprintln!("runtime update failed: {error}");
                    event_loop.exit();
                    return;
                }
                let vsync =
                    self.runtime.world().resource::<RenderSettings>().vsync;
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
                        let extent = renderer.swapchain_image_size();
                        let future =
                            match self.scene_renderer.as_mut().unwrap().render(
                                future,
                                renderer.swapchain_image_view(),
                                extent,
                                SceneViewport::full(extent),
                                self.runtime.world().resource::<RenderWorld>(),
                                self.runtime.world().resource::<AssetServer>(),
                            ) {
                                Ok(future) => future,
                                Err(error) => {
                                    eprintln!(
                                        "scene rendering failed: {error}"
                                    );
                                    event_loop.exit();
                                    return;
                                }
                            };
                        renderer.present(future, false);
                    }
                    Err(VulkanError::OutOfDate) => renderer.resize(),
                    Err(error) => {
                        eprintln!("swapchain acquisition failed: {error}");
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
                self.runtime.world().resource::<RenderSettings>(),
            );
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let scene_path = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from("assets/scenes/main.rscene"),
        PathBuf::from,
    );
    let event_loop = EventLoop::new()?;
    event_loop.run_app(&mut GameApplication::load(scene_path)?)?;
    Ok(())
}
