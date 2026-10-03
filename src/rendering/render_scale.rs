//! `RenderSettings::render_scale`: the scene renders into a smaller (or
//! larger) offscreen image, then a linear blit in the same command buffer
//! stretches it over the real target. UI is painted afterwards at full
//! resolution. [`render_game`] also draws cameras with a viewport this way,
//! each blitted onto its part of the target.

use std::sync::Arc;

use bevy_ecs::entity::Entity;
use vulkano::device::DeviceOwned;
use vulkano::format::{Format, FormatFeatures};
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::sync::GpuFuture;

use super::scene_renderer::{
    SceneRenderError, SceneRenderOptions, SceneRenderer, SceneViewport,
};
use crate::runtime::RenderWorld;
use crate::AssetServer;
use vulkano::memory::allocator::{
    AllocationCreateInfo, StandardMemoryAllocator,
};

/// Smallest and largest accepted render scale.
pub const RENDER_SCALE_RANGE: (f32, f32) = (0.25, 2.0);

/// Size the scene renders at for `scale` of `extent`. Never zero.
#[must_use]
pub fn scaled_extent(extent: [u32; 2], scale: f32) -> [u32; 2] {
    let scale = scale.clamp(RENDER_SCALE_RANGE.0, RENDER_SCALE_RANGE.1);
    extent.map(|side| ((side as f32 * scale).round() as u32).max(1))
}

/// An offscreen view and its size.
pub type ScaledView = (Arc<ImageView>, [u32; 2]);

/// The offscreen image used while the render scale is not 1. The scene
/// renders into it, then [`super::scene_renderer::SceneRenderer`] blits it
/// over the real target (`upscale_next_frame`).
pub struct ScaledTarget {
    memory: Arc<StandardMemoryAllocator>,
    view: Option<Arc<ImageView>>,
}

impl ScaledTarget {
    pub fn new(memory: Arc<StandardMemoryAllocator>) -> Self {
        Self { memory, view: None }
    }

    /// The image the scene should render into for `scale` of a target of
    /// `extent`, or `None` to render straight into the target: the scale is
    /// 1, or the device cannot blit `format` with linear filtering.
    pub fn target(
        &mut self,
        format: Format,
        extent: [u32; 2],
        scale: f32,
    ) -> Result<Option<ScaledView>, String> {
        self.image(format, extent, scale, false)
    }

    /// As [`Self::target`], but with `always` an image of the scaled size
    /// even at scale 1, unless the device cannot blit `format`.
    fn image(
        &mut self,
        format: Format,
        extent: [u32; 2],
        scale: f32,
        always: bool,
    ) -> Result<Option<ScaledView>, String> {
        let size = scaled_extent(extent, scale);
        // The image is kept: viewport cameras reuse it every frame.
        if size == extent && !always || !self.can_blit(format) {
            return Ok(None);
        }
        let reuse = self.view.as_ref().is_some_and(|view| {
            let image = view.image();
            image.format() == format && image.extent()[..2] == size
        });
        if !reuse {
            let image = Image::new(
                self.memory.clone(),
                ImageCreateInfo {
                    format,
                    extent: [size[0], size[1], 1],
                    usage: ImageUsage::COLOR_ATTACHMENT
                        | ImageUsage::TRANSFER_SRC,
                    ..Default::default()
                },
                AllocationCreateInfo::default(),
            )
            .map_err(|error| format!("render scale image: {error}"))?;
            self.view = Some(
                ImageView::new_default(image)
                    .map_err(|error| format!("render scale view: {error}"))?,
            );
        }
        Ok(self.view.clone().map(|view| (view, size)))
    }

    fn can_blit(&self, format: Format) -> bool {
        self.memory
            .device()
            .physical_device()
            .format_properties(format)
            .is_ok_and(|properties| {
                properties.optimal_tiling_features.contains(
                    FormatFeatures::BLIT_SRC
                        | FormatFeatures::BLIT_DST
                        | FormatFeatures::SAMPLED_IMAGE_FILTER_LINEAR,
                )
            })
    }
}

/// `viewport` (`[x, y, width, height]` fractions) in pixels of `extent`,
/// rounded to whole pixels and kept inside it.
#[must_use]
pub fn viewport_pixels(viewport: [f32; 4], extent: [u32; 2]) -> SceneViewport {
    let edge = |fraction: f32, side: u32| {
        ((fraction.clamp(0.0, 1.0) * side as f32).round() as u32).min(side)
    };
    let [x, y, width, height] = viewport;
    let left = edge(x, extent[0]);
    let top = edge(y, extent[1]);
    SceneViewport {
        offset: [left, top],
        extent: [
            edge(x + width, extent[0]).saturating_sub(left),
            edge(y + height, extent[1]).saturating_sub(top),
        ],
    }
}

/// Renders the game onto `target`: the active camera over all of it at the
/// render scale (or black when every active camera has a viewport), then
/// each viewport camera over its part, lowest priority first.
/// `after_view` runs after each camera's render with that camera's entity.
/// Without `scratch` (a target that cannot be blitted to) everything draws
/// straight into the target at scale 1.
#[allow(clippy::too_many_arguments)]
pub fn render_game(
    renderer: &mut SceneRenderer,
    mut scratch: Option<&mut ScaledTarget>,
    before: Box<dyn GpuFuture>,
    target: Arc<ImageView>,
    settings: &crate::runtime::RenderSettings,
    world: &RenderWorld,
    assets: &AssetServer,
    mut after_view: impl FnMut(
        &mut SceneRenderer,
        Box<dyn GpuFuture>,
        Option<Entity>,
    ) -> Result<Box<dyn GpuFuture>, String>,
) -> Result<Box<dyn GpuFuture>, String> {
    let failed = |error: SceneRenderError| format!("render: {error}");
    let scale = settings.render_scale;
    renderer.set_upscale_nearest(settings.pixelated);
    let format = target.format();
    let [width, height, _] = target.image().extent();
    let extent = [width, height];
    let active = world.active_camera.map(|camera| camera.entity);
    let mut future = before;
    let can_blit = scratch.is_some();
    let mut scaled = |always| match scratch.as_deref_mut() {
        Some(scratch) => scratch.image(format, extent, scale, always),
        None => Ok(None),
    };
    if world
        .views
        .iter()
        .any(|(view, _)| Some(view.entity) == active)
    {
        if can_blit {
            future = renderer.clear(future, target.clone()).map_err(failed)?;
        }
    } else {
        let (view, size) = match scaled(false)? {
            Some(scaled) => {
                renderer.upscale_next_frame(target.clone());
                scaled
            }
            None => (target.clone(), extent),
        };
        let options = SceneRenderOptions::game(size);
        future = renderer
            .render(future, view, size, options, world, assets)
            .map_err(failed)?;
        future = after_view(renderer, future, active)?;
    }
    for (camera, viewport) in &world.views {
        let region = viewport_pixels(*viewport, extent);
        if region.extent.contains(&0) {
            continue;
        }
        let (view, size, area) = match scaled(true)? {
            Some((view, size)) => {
                renderer.blit_next_frame(target.clone(), region);
                (view, size, viewport_pixels(*viewport, size))
            }
            // ponytail: without blit support the view draws straight
            // into the target, whose other parts the pass may not keep.
            None => (target.clone(), extent, region),
        };
        let options = SceneRenderOptions {
            viewport: area,
            camera: Some(*camera),
            ..SceneRenderOptions::game(size)
        };
        future = renderer
            .render(future, view, size, options, world, assets)
            .map_err(failed)?;
        future = after_view(renderer, future, Some(camera.entity))?;
    }
    Ok(future)
}

#[cfg(test)]
mod tests {
    use super::{scaled_extent, viewport_pixels};

    #[test]
    fn viewport_pixels_round_and_stay_inside_the_target() {
        let half = viewport_pixels([0.5, 0.0, 0.5, 1.0], [1281, 720]);
        assert_eq!((half.offset, half.extent), ([641, 0], [640, 720]));
        let past = viewport_pixels([0.75, 0.5, 0.5, 2.0], [100, 100]);
        assert_eq!((past.offset, past.extent), ([75, 50], [25, 50]));
    }

    #[test]
    fn scaled_extent_rounds_clamps_and_never_reaches_zero() {
        assert_eq!(scaled_extent([1920, 1080], 0.5), [960, 540]);
        assert_eq!(scaled_extent([1920, 1080], 1.0), [1920, 1080]);
        assert_eq!(scaled_extent([100, 100], 0.01), [25, 25]);
        assert_eq!(scaled_extent([100, 100], 9.0), [200, 200]);
        assert_eq!(scaled_extent([1, 1], 0.25), [1, 1]);
    }
}
