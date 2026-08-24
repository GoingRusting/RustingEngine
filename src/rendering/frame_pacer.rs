use std::sync::Arc;
use std::time::{Duration, Instant};

use vulkano::device::Queue;
use vulkano::swapchain::{PresentMode, Surface};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::Window;

use crate::runtime::RenderSettings;

/// Selects the fastest available non-VSync mode, while retaining Vulkan's
/// guaranteed FIFO fallback on devices that expose no optional present mode.
#[must_use]
pub fn select_present_mode(
    queue: &Arc<Queue>,
    surface: &Arc<Surface>,
    vsync: bool,
) -> PresentMode {
    let Ok(modes) = queue
        .device()
        .physical_device()
        .surface_present_modes(surface, Default::default())
    else {
        return PresentMode::Fifo;
    };
    let preferences = if vsync {
        [
            PresentMode::Fifo,
            PresentMode::Mailbox,
            PresentMode::Immediate,
        ]
    } else {
        [
            PresentMode::Immediate,
            PresentMode::Mailbox,
            PresentMode::Fifo,
        ]
    };
    preferences
        .into_iter()
        .find(|mode| modes.contains(mode))
        .unwrap_or(PresentMode::Fifo)
}

/// Winit-side optional FPS limiter. Unlimited mode continuously polls; limited
/// mode sleeps through `ControlFlow::WaitUntil` without blocking render work.
pub struct FramePacer {
    next_frame: Instant,
}

impl Default for FramePacer {
    fn default() -> Self {
        Self {
            next_frame: Instant::now(),
        }
    }
}

impl FramePacer {
    pub fn request_next_frame(
        &mut self,
        event_loop: &ActiveEventLoop,
        window: &Window,
        settings: &RenderSettings,
    ) {
        if !settings.limit_fps {
            event_loop.set_control_flow(ControlFlow::Poll);
            self.next_frame = Instant::now();
            window.request_redraw();
            return;
        }

        let interval =
            Duration::from_secs_f64(1.0 / f64::from(settings.max_fps.max(1)));
        let now = Instant::now();
        if now >= self.next_frame {
            self.next_frame = now + interval;
            window.request_redraw();
        } else {
            event_loop
                .set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        }
    }
}
