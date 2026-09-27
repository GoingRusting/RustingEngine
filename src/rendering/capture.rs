//! Offscreen frames for tools: `rusting capture` and scenario tests.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, StandardMemoryAllocator,
};
use vulkano::sync::GpuFuture;

use super::scene_renderer::{
    resolve_quality, SceneRenderOptions, SceneRenderer,
};
use super::swapchain::OFFSCREEN_COLOR_FORMAT;
use super::HeadlessVulkanBase;
use crate::runtime::{
    apply_gpu_state_samples, record_gpu_state_hashes, route_gpu_physics_events,
    EventQueue, GpuPhysicsEventsLost, RenderSettings, RenderWorld,
};
use crate::{App, AssetServer};

/// Renders every app update into one offscreen image, as the game window
/// would, so GPU physics and its readback advance the same way. With the
/// `ui` feature the runtime UI (HUD) is painted over the frame at one pixel
/// per point.
pub struct HeadlessCapture {
    base: HeadlessVulkanBase,
    allocator: Arc<StandardMemoryAllocator>,
    renderer: SceneRenderer,
    image: Arc<Image>,
    target: Arc<ImageView>,
    extent: [u32; 2],
    #[cfg(feature = "ui")]
    ui: Option<super::egui_painter::EguiPainter>,
}

impl HeadlessCapture {
    /// Opens a headless Vulkan device. Fails without printing when there is
    /// no usable device.
    pub fn new(extent: [u32; 2]) -> Result<Self, String> {
        let base = super::try_init_vulkan_headless()?;
        let allocator =
            Arc::new(StandardMemoryAllocator::new_default(base.device.clone()));
        let renderer = SceneRenderer::new(
            base.queue.clone(),
            allocator.clone(),
            OFFSCREEN_COLOR_FORMAT,
            extent,
        )
        .map_err(|error| format!("scene renderer: {error}"))?;
        let image = Image::new(
            allocator.clone(),
            ImageCreateInfo {
                format: OFFSCREEN_COLOR_FORMAT,
                extent: [extent[0], extent[1], 1],
                usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo::default(),
        )
        .map_err(|error| format!("capture target: {error}"))?;
        let target = ImageView::new_default(image.clone())
            .map_err(|error| format!("capture target view: {error}"))?;
        Ok(Self {
            base,
            allocator,
            renderer,
            image,
            target,
            extent,
            #[cfg(feature = "ui")]
            ui: None,
        })
    }

    /// Delivers completed GPU physics readback, updates `app` by `delta`,
    /// and renders the frame, waiting for it to finish.
    pub fn frame(
        &mut self,
        app: &mut App,
        delta: Duration,
    ) -> Result<(), String> {
        let events = self.renderer.take_completed_physics_events();
        let states = self.renderer.take_completed_physics_states();
        let hashes = self.renderer.take_completed_physics_state_hashes();
        let lost = self.renderer.take_physics_events_lost();
        let world = app.world_mut();
        if lost > 0 {
            world
                .resource_mut::<EventQueue<GpuPhysicsEventsLost>>()
                .send(GpuPhysicsEventsLost { count: lost });
        }
        if !events.is_empty() {
            route_gpu_physics_events(world, &events);
        }
        if !states.is_empty() {
            apply_gpu_state_samples(world, &states);
        }
        record_gpu_state_hashes(world, &hashes);
        #[cfg(feature = "ui")]
        if let Some(mut ui) =
            world.get_resource_mut::<crate::runtime::RuntimeUi>()
        {
            let size = egui::vec2(self.extent[0] as f32, self.extent[1] as f32);
            ui.set_input(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    size,
                )),
                ..egui::RawInput::default()
            });
        }
        app.update(delta)
            .map_err(|error| format!("update: {error}"))?;
        let world = app.world();
        let frame = self
            .renderer
            .render(
                vulkano::sync::now(self.base.device.clone()).boxed(),
                self.target.clone(),
                self.extent,
                SceneRenderOptions::game(self.extent),
                world.resource::<RenderWorld>(),
                world.resource::<AssetServer>(),
            )
            .map_err(|error| format!("render: {error}"))?
            .boxed();
        #[cfg(feature = "ui")]
        let frame = self.paint_ui(app, frame)?;
        frame
            .then_signal_fence_and_flush()
            .map_err(|error| format!("submit: {error}"))?
            .wait(None)
            .map_err(|error| format!("wait: {error}"))
    }

    /// Paints the runtime UI output of the last update over the frame.
    #[cfg(feature = "ui")]
    fn paint_ui(
        &mut self,
        app: &mut App,
        frame: Box<dyn GpuFuture>,
    ) -> Result<Box<dyn GpuFuture>, String> {
        let Some(output) = app
            .world_mut()
            .get_resource_mut::<crate::runtime::RuntimeUi>()
            .and_then(|mut ui| ui.take_output())
        else {
            return Ok(frame);
        };
        let primitives = app
            .world()
            .resource::<crate::runtime::RuntimeUi>()
            .context()
            .tessellate(output.shapes, output.pixels_per_point);
        let painter = match &mut self.ui {
            Some(painter) => painter,
            None => self.ui.insert(
                super::egui_painter::EguiPainter::new(
                    self.base.queue.clone(),
                    self.allocator.clone(),
                    OFFSCREEN_COLOR_FORMAT,
                )
                .map_err(|error| format!("UI painter: {error}"))?,
            ),
        };
        painter
            .paint(
                frame,
                self.target.clone(),
                output.pixels_per_point,
                &primitives,
                &output.textures_delta,
            )
            .map_err(|error| format!("UI paint: {error}"))
    }

    /// The last rendered frame as tightly packed RGBA8 sRGB rows.
    #[must_use]
    pub fn rgba(&self) -> Vec<u8> {
        let command_allocator = Arc::new(StandardCommandBufferAllocator::new(
            self.base.device.clone(),
            Default::default(),
        ));
        let mut pixels = super::readback::read_back_image(
            &self.base.device,
            &self.base.queue,
            &self.allocator,
            &command_allocator,
            &self.image,
        );
        // The offscreen format is BGRA.
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        pixels
    }

    /// Device and work counters of the last frame.
    #[must_use]
    pub fn metadata(&mut self, app: &App) -> Value {
        let properties = self.base.device.physical_device().properties();
        let counters = self.renderer.render_counters();
        let capacity = self.renderer.capacity_diagnostics();
        let requested = app.world().resource::<RenderSettings>().quality;
        json!({
            "device": properties.device_name,
            "driver": properties.driver_info,
            "quality": resolve_quality(requested, self.renderer.capabilities()),
            "requested_quality": requested,
            "draws": counters.draws,
            "triangles": counters.triangles,
            "visible_instances": counters.visible_instances,
            "dropped_lights": capacity.dropped_lights,
        })
    }

    /// Writes the last rendered frame as an image; the extension picks the
    /// format, normally `.png`.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| error.to_string())?;
        }
        image::save_buffer(
            path,
            &self.rgba(),
            self.extent[0],
            self.extent[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("could not write {}: {error}", path.display()))
    }
}
