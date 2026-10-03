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

use super::render_scale::render_game;
use super::scene_renderer::{resolve_quality, SceneRenderer};
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
    scaled: super::render_scale::ScaledTarget,
    /// Each drawn camera of the last frame with a split view: its work
    /// counters and GPU time. Empty with one full-window camera.
    views: Vec<(
        Option<bevy_ecs::entity::Entity>,
        super::scene_renderer::RenderCounters,
        Duration,
    )>,
    /// Second target for [`Self::view_rgba`], made on first use.
    alt: Option<(Arc<Image>, Arc<ImageView>)>,
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
        let (image, target) = color_target(&allocator, extent)?;
        let scaled = super::render_scale::ScaledTarget::new(allocator.clone());
        Ok(Self {
            base,
            allocator,
            renderer,
            image,
            target,
            extent,
            scaled,
            views: Vec::new(),
            alt: None,
            #[cfg(feature = "ui")]
            ui: None,
        })
    }

    /// Renders the last update again into a second image, without the UI:
    /// through `camera` over the whole frame, or as the game view when
    /// `None`. The frame [`Self::rgba`] reads stays as it was.
    pub fn view_rgba(
        &mut self,
        world: &bevy_ecs::world::World,
        camera: Option<bevy_ecs::entity::Entity>,
    ) -> Result<Vec<u8>, String> {
        if self.alt.is_none() {
            self.alt = Some(color_target(&self.allocator, self.extent)?);
        }
        let (image, view) = self.alt.clone().unwrap();
        let render_world = world.resource::<RenderWorld>();
        let assets = world.resource::<AssetServer>();
        let before = vulkano::sync::now(self.base.device.clone()).boxed();
        let frame = match camera {
            Some(entity) => {
                let camera = world
                    .get::<crate::runtime::Camera>(entity)
                    .ok_or("not a camera")?;
                let transform = world
                    .get::<crate::runtime::GlobalTransform>(entity)
                    .ok_or("camera has no transform")?;
                let options = super::scene_renderer::SceneRenderOptions {
                    camera: Some(crate::runtime::ExtractedCamera {
                        entity,
                        transform: *transform,
                        projection: camera.projection,
                        priority: camera.priority,
                    }),
                    ..super::scene_renderer::SceneRenderOptions::game(
                        self.extent,
                    )
                };
                self.renderer
                    .render(
                        before,
                        view,
                        self.extent,
                        options,
                        render_world,
                        assets,
                    )
                    .map_err(|error| format!("render: {error}"))?
            }
            None => render_game(
                &mut self.renderer,
                Some(&mut self.scaled),
                before,
                view,
                world.resource::<RenderSettings>(),
                render_world,
                assets,
                |_, future, _| Ok(future),
            )?,
        };
        frame
            .then_signal_fence_and_flush()
            .map_err(|error| format!("submit: {error}"))?
            .wait(None)
            .map_err(|error| format!("wait: {error}"))?;
        Ok(self.read(&image))
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
        // The UI pass and pointer picking read the view size; a headless
        // view is the capture.
        let mut input = world.resource_mut::<crate::runtime::RuntimeInput>();
        if input.viewport_size().contains(&0.0) {
            input.record_viewport_size(self.extent.map(|side| side as f32));
        }
        app.update(delta)
            .map_err(|error| format!("update: {error}"))?;
        let world = app.world();
        let render_world = world.resource::<RenderWorld>();
        let split = !render_world.views.is_empty();
        let device = self.base.device.clone();
        self.views.clear();
        let views = &mut self.views;
        let frame = render_game(
            &mut self.renderer,
            Some(&mut self.scaled),
            vulkano::sync::now(device.clone()).boxed(),
            self.target.clone(),
            world.resource::<RenderSettings>(),
            render_world,
            world.resource::<AssetServer>(),
            |renderer, future, camera| {
                if !split {
                    return Ok(future);
                }
                // Each camera's own numbers need its work finished.
                future
                    .then_signal_fence_and_flush()
                    .map_err(|error| format!("submit: {error}"))?
                    .wait(None)
                    .map_err(|error| format!("wait: {error}"))?;
                let counters = renderer.render_counters();
                let gpu = renderer.gpu_pass_times().total();
                views.push((camera, counters, gpu));
                Ok(vulkano::sync::now(device.clone()).boxed())
            },
        )?;
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
        self.read(&self.image)
    }

    fn read(&self, image: &Arc<Image>) -> Vec<u8> {
        let command_allocator = Arc::new(StandardCommandBufferAllocator::new(
            self.base.device.clone(),
            Default::default(),
        ));
        let mut pixels = super::readback::read_back_image(
            &self.base.device,
            &self.base.queue,
            &self.allocator,
            &command_allocator,
            image,
        );
        // The offscreen format is BGRA.
        for pixel in pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        pixels
    }

    /// Device, work counters and GPU time of the last frame, also per
    /// camera when viewport cameras split it.
    #[must_use]
    pub fn metadata(&mut self, app: &App) -> Value {
        let properties = self.base.device.physical_device().properties();
        let counters = self.renderer.render_counters();
        let capacity = self.renderer.capacity_diagnostics();
        let requested = app.world().resource::<RenderSettings>().quality;
        let cameras: Vec<_> = self
            .views
            .iter()
            .map(|(camera, counters, gpu)| {
                let name = camera
                    .and_then(|camera| {
                        app.world().get::<crate::runtime::Name>(camera)
                    })
                    .map(|name| name.0.clone());
                json!({
                    "name": name,
                    "gpu_ms": gpu.as_secs_f64() * 1000.0,
                    "draws": counters.draws,
                    "triangles": counters.triangles,
                })
            })
            .collect();
        let gpu = match self.views.is_empty() {
            true => self.renderer.gpu_pass_times().total(),
            false => self.views.iter().map(|(_, _, gpu)| *gpu).sum(),
        };
        json!({
            "gpu_ms": gpu.as_secs_f64() * 1000.0,
            "cameras": cameras,
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
        save_rgba(path, &self.rgba(), self.extent)
    }

    #[must_use]
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
}

/// Writes RGBA8 rows as a PNG, making its folder.
pub fn save_rgba(
    path: &Path,
    rgba: &[u8],
    extent: [u32; 2],
) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    image::save_buffer(
        path,
        rgba,
        extent[0],
        extent[1],
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn color_target(
    allocator: &Arc<StandardMemoryAllocator>,
    extent: [u32; 2],
) -> Result<(Arc<Image>, Arc<ImageView>), String> {
    let image = Image::new(
        allocator.clone(),
        ImageCreateInfo {
            format: OFFSCREEN_COLOR_FORMAT,
            extent: [extent[0], extent[1], 1],
            usage: ImageUsage::COLOR_ATTACHMENT
                | ImageUsage::TRANSFER_SRC
                | ImageUsage::TRANSFER_DST,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .map_err(|error| format!("capture target: {error}"))?;
    let view = ImageView::new_default(image.clone())
        .map_err(|error| format!("capture target view: {error}"))?;
    Ok((image, view))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::HeadlessCapture;
    use crate::runtime::{
        Camera, MeshRenderer, Projection, RenderExtractPlugin, RenderSettings,
    };
    use crate::{App, AssetPlugin, AssetServer, Transform};

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn a_half_render_scale_still_fills_the_whole_target() {
        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let (mesh, material) = {
            let assets = app.world().resource::<AssetServer>();
            (assets.fallback_mesh, assets.fallback_material)
        };
        app.spawn((
            Transform::new([0.0, 0.0, 3.0]),
            Camera {
                projection: Projection::Perspective {
                    vertical_fov_radians: 1.0,
                    near: 0.1,
                    far: 100.0,
                },
                active: true,
                priority: 0,
                viewport: None,
            },
        ));
        app.spawn((
            Transform::default(),
            MeshRenderer {
                mesh,
                material,
                cast_shadows: false,
                receive_shadows: false,
            },
        ));
        let mut capture = HeadlessCapture::new([64, 64]).unwrap();
        let mut shoot = |app: &mut App, scale: f32| {
            app.world_mut()
                .resource_mut::<RenderSettings>()
                .render_scale = scale;
            capture.frame(app, Duration::from_millis(16)).unwrap();
            capture.rgba()
        };
        let half = shoot(&mut app, 0.5);
        let full = shoot(&mut app, 1.0);
        assert_eq!(half.len(), full.len());
        let pixel = |rgba: &[u8], x: usize, y: usize| {
            let at = (y * 64 + x) * 4;
            rgba[at..at + 3].to_vec()
        };
        // The centre shows the mesh and a corner the background, as at full
        // scale: the blit stretched the half-size frame over the target.
        for (x, y) in [(32, 32), (1, 1), (62, 62)] {
            let (a, b) = (pixel(&full, x, y), pixel(&half, x, y));
            let close = a.iter().zip(&b).all(|(a, b)| a.abs_diff(*b) < 24);
            assert!(close, "({x}, {y}): {a:?} vs {b:?}");
        }
        assert_ne!(pixel(&half, 32, 32), pixel(&half, 1, 1));
        // Pixelated: a quarter-size frame shows 4 x 4 blocks of one color.
        app.world_mut().resource_mut::<RenderSettings>().pixelated = true;
        let blocky = shoot(&mut app, 0.25);
        for y in 0..64 {
            for x in 0..64 {
                let corner = pixel(&blocky, x - x % 4, y - y % 4);
                assert_eq!(pixel(&blocky, x, y), corner, "({x}, {y})");
            }
        }
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn viewport_cameras_split_the_frame() {
        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        app.add_plugin(RenderExtractPlugin).unwrap();
        let (mesh, material) = {
            let assets = app.world().resource::<AssetServer>();
            (assets.fallback_mesh, assets.fallback_material)
        };
        let camera = |priority, viewport| Camera {
            active: true,
            priority,
            viewport: Some(viewport),
            ..Camera::default()
        };
        // The left camera sees the mesh, the right one empty space.
        app.spawn((
            crate::runtime::Name("Left".into()),
            Transform::new([0.0, 0.0, 3.0]),
            camera(0, [0.0, 0.0, 0.5, 1.0]),
        ));
        app.spawn((
            crate::runtime::Name("Right".into()),
            Transform::new([100.0, 0.0, 3.0]),
            camera(1, [0.5, 0.0, 0.5, 1.0]),
        ));
        app.spawn((
            Transform::default(),
            MeshRenderer {
                mesh,
                material,
                cast_shadows: false,
                receive_shadows: false,
            },
        ));
        let mut capture = HeadlessCapture::new([64, 32]).unwrap();
        capture.frame(&mut app, Duration::from_millis(16)).unwrap();
        let rgba = capture.rgba();
        let pixel = |x: usize, y: usize| {
            let at = (y * 64 + x) * 4;
            rgba[at..at + 3].to_vec()
        };
        // The mesh is centred in the left half, and the right half matches
        // the left half's background.
        assert_ne!(pixel(16, 16), pixel(1, 1));
        assert_eq!(pixel(48, 16), pixel(62, 1));
        assert_eq!(pixel(1, 1), pixel(62, 1));
        let metadata = capture.metadata(&app);
        let names: Vec<_> = metadata["cameras"]
            .as_array()
            .unwrap()
            .iter()
            .map(|camera| camera["name"].clone())
            .collect();
        assert_eq!(names, ["Left", "Right"]);
    }
}
