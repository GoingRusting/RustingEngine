//! Headless render benchmark runner and regression baselines.
//!
//! [`run_render_benchmark`] renders the fixed benchmark scene along its
//! camera path into an offscreen image and summarises every frame into a
//! [`RenderBenchmarkReport`]. Reports serialize to JSON, so a report stored
//! from an earlier run is a baseline: [`RenderBenchmarkReport::regressions`]
//! lists every material regression of a new run against it.
//!
//! The runner waits for each frame before starting the next, so frame time
//! is CPU plus GPU time without overlap: an upper bound on a presented
//! frame, never a lower one.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use vulkano::device::Queue;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageUsage};
use vulkano::memory::allocator::{
    AllocationCreateInfo, StandardMemoryAllocator,
};
use vulkano::sync::GpuFuture;

use super::scene_renderer::{SceneRenderOptions, SceneRenderer};
use super::swapchain::OFFSCREEN_COLOR_FORMAT;
use crate::assets::{AssetPlugin, AssetServer};
use crate::runtime::{
    render_benchmark_camera, spawn_render_benchmark,
    spawn_render_benchmark_extras, update_render_benchmark_bears, App,
    CpuFrameTimings, HybridPhysicsPlugin, PhysicsBody, QualityProfile,
    RenderBenchmarkExtras, RenderExtractPlugin, RenderSettings, RenderWorld,
};
use crate::Transform;

/// Simulated time per benchmark frame, so physics advances one 60 Hz tick
/// per frame regardless of how fast the machine renders.
pub const RENDER_BENCHMARK_FRAME_TIME: Duration = Duration::from_micros(16_667);

/// Relative growth of a time or work counter that counts as a regression.
pub const REGRESSION_TOLERANCE: f64 = 0.10;
/// Absolute time growth below which timing noise is ignored.
pub const REGRESSION_NOISE_MS: f64 = 0.5;

/// Mean, 95th percentile, and worst value of a per-frame time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TimeStats {
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

impl TimeStats {
    fn from_samples(samples: &[Duration]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut ms: Vec<f64> = samples
            .iter()
            .map(|time| time.as_secs_f64() * 1e3)
            .collect();
        ms.sort_by(f64::total_cmp);
        let p95 = ms[((ms.len() * 95).div_ceil(100)).max(1) - 1];
        Self {
            mean_ms: ms.iter().sum::<f64>() / ms.len() as f64,
            p95_ms: p95,
            max_ms: ms[ms.len() - 1],
        }
    }
}

/// Summary of one benchmark run. Counters are per-frame maxima unless they
/// say they are totals.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderBenchmarkReport {
    pub device: String,
    pub driver: String,
    pub extent: [u32; 2],
    pub quality: QualityProfile,
    pub frames: u32,
    /// Wall time of update, extraction, recording, and the GPU wait.
    pub frame_time: TimeStats,
    /// CPU time measured inside the frame: physics, extraction,
    /// preparation, and recording.
    pub cpu_time: TimeStats,
    /// Sum of timed GPU passes. Empty when the queue has no timestamps.
    pub gpu_time: TimeStats,
    /// Mean GPU time per pass, by pass label.
    pub gpu_passes_ms: Vec<(String, f64)>,
    pub draws: u32,
    pub dispatches: u32,
    pub triangles: u64,
    pub visible_instances: usize,
    pub gpu_memory_bytes: u64,
    /// Total bytes written to GPU buffers over the run.
    pub upload_bytes: u64,
    pub physics_bodies: usize,
    /// Totals over the run.
    pub physics_command_bytes: u64,
    pub physics_event_bytes: u64,
    pub physics_state_bytes: u64,
    /// Most frames between submitting and collecting a physics readback.
    pub readback_latency_frames: u32,
    /// Overflow and fallbacks at the end of the run.
    pub dropped_lights: usize,
    pub physics_events_dropped: u64,
    pub physics_commands_rejected: u64,
    /// Scene additions on top of the fixed scene; runs with different
    /// extras do not compare.
    #[serde(default)]
    pub screens: usize,
    #[serde(default)]
    pub screen_size: [u32; 2],
    #[serde(default)]
    pub bears: usize,
    #[serde(default)]
    pub bear_updates: usize,
    #[serde(default)]
    pub screen_every: u32,
    #[serde(default)]
    pub without_bodies: bool,
}

impl RenderBenchmarkReport {
    fn extras(&self) -> RenderBenchmarkExtras {
        RenderBenchmarkExtras {
            screens: self.screens,
            screen_size: self.screen_size,
            bears: self.bears,
            bear_updates: self.bear_updates,
            screen_every: self.screen_every,
            without_bodies: self.without_bodies,
        }
    }

    /// Material regressions of `self` against `baseline`, one line each.
    ///
    /// Work counters are compared when both runs used the same extent and
    /// quality. Times are compared only on the same device and driver too;
    /// other machines' times say nothing about this one. Any overflow that
    /// the baseline did not have is a regression.
    #[must_use]
    pub fn regressions(&self, baseline: &Self) -> Vec<String> {
        let mut found = Vec::new();
        if self.extent != baseline.extent
            || self.quality != baseline.quality
            || self.frames != baseline.frames
            || self.extras() != baseline.extras()
        {
            found.push(format!(
                "run {:?} {:?} {} frames does not match baseline {:?} {:?} {} frames",
                self.extent,
                self.quality,
                self.frames,
                baseline.extent,
                baseline.quality,
                baseline.frames,
            ));
            return found;
        }
        let mut counter = |name: &str, now: f64, then: f64| {
            if now > then * (1.0 + REGRESSION_TOLERANCE) {
                found.push(format!("{name} rose from {then} to {now}"));
            }
        };
        counter("draws", self.draws.into(), baseline.draws.into());
        counter(
            "dispatches",
            self.dispatches.into(),
            baseline.dispatches.into(),
        );
        counter(
            "triangles",
            self.triangles as f64,
            baseline.triangles as f64,
        );
        counter(
            "visible instances",
            self.visible_instances as f64,
            baseline.visible_instances as f64,
        );
        counter(
            "GPU memory bytes",
            self.gpu_memory_bytes as f64,
            baseline.gpu_memory_bytes as f64,
        );
        counter(
            "upload bytes",
            self.upload_bytes as f64,
            baseline.upload_bytes as f64,
        );
        counter(
            "physics event bytes",
            self.physics_event_bytes as f64,
            baseline.physics_event_bytes as f64,
        );
        counter(
            "physics state bytes",
            self.physics_state_bytes as f64,
            baseline.physics_state_bytes as f64,
        );
        for (name, now, then) in [
            (
                "dropped lights",
                self.dropped_lights as u64,
                baseline.dropped_lights as u64,
            ),
            (
                "dropped physics events",
                self.physics_events_dropped,
                baseline.physics_events_dropped,
            ),
            (
                "rejected physics commands",
                self.physics_commands_rejected,
                baseline.physics_commands_rejected,
            ),
            (
                "readback latency frames",
                self.readback_latency_frames.into(),
                baseline.readback_latency_frames.into(),
            ),
        ] {
            if now > then {
                found.push(format!("{name} rose from {then} to {now}"));
            }
        }
        if self.device == baseline.device && self.driver == baseline.driver {
            for (name, now, then) in [
                ("frame time p95", self.frame_time, baseline.frame_time),
                ("CPU time p95", self.cpu_time, baseline.cpu_time),
                ("GPU time p95", self.gpu_time, baseline.gpu_time),
            ] {
                let limit = (then.p95_ms * (1.0 + REGRESSION_TOLERANCE))
                    .max(then.p95_ms + REGRESSION_NOISE_MS);
                if now.p95_ms > limit {
                    found.push(format!(
                        "{name} rose from {:.2} ms to {:.2} ms",
                        then.p95_ms, now.p95_ms
                    ));
                }
            }
        }
        found
    }
}

/// Renders `frames` frames of the fixed benchmark scene and camera path at
/// `extent` and `quality` into an offscreen image.
pub fn run_render_benchmark(
    queue: Arc<Queue>,
    extent: [u32; 2],
    quality: QualityProfile,
    frames: u32,
) -> Result<RenderBenchmarkReport, String> {
    run_render_benchmark_with(
        queue,
        extent,
        quality,
        frames,
        RenderBenchmarkExtras::default(),
    )
}

/// [`run_render_benchmark`] with camera screens and bears added to the
/// scene; screens render before each frame, as in the game window.
pub fn run_render_benchmark_with(
    queue: Arc<Queue>,
    extent: [u32; 2],
    quality: QualityProfile,
    frames: u32,
    extras: RenderBenchmarkExtras,
) -> Result<RenderBenchmarkReport, String> {
    let device = queue.device().clone();
    let memory_allocator =
        Arc::new(StandardMemoryAllocator::new_default(device.clone()));
    let mut renderer = SceneRenderer::new(
        queue.clone(),
        memory_allocator.clone(),
        OFFSCREEN_COLOR_FORMAT,
        extent,
    )
    .map_err(|error| format!("scene renderer: {error}"))?;
    let image = Image::new(
        memory_allocator,
        ImageCreateInfo {
            format: OFFSCREEN_COLOR_FORMAT,
            extent: [extent[0], extent[1], 1],
            usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo::default(),
    )
    .map_err(|error| format!("benchmark target: {error}"))?;
    let target = ImageView::new_default(image)
        .map_err(|error| format!("benchmark target view: {error}"))?;

    let mut app = App::new();
    app.add_plugin(AssetPlugin)
        .and_then(|app| app.add_plugin(HybridPhysicsPlugin))
        .and_then(|app| app.add_plugin(RenderExtractPlugin))
        .map_err(|error| format!("benchmark plugins: {error}"))?;
    app.world_mut().resource_mut::<RenderSettings>().quality = quality;
    let camera = spawn_render_benchmark(app.world_mut());
    let bears = spawn_render_benchmark_extras(app.world_mut(), extras);
    let physics_bodies = app
        .world_mut()
        .query::<&PhysicsBody>()
        .iter(app.world())
        .count();

    let properties = device.physical_device().properties();
    let mut report = RenderBenchmarkReport {
        device: properties.device_name.clone(),
        driver: properties.driver_info.clone().unwrap_or_default(),
        extent,
        quality,
        frames,
        physics_bodies,
        screens: extras.screens,
        screen_size: extras.screen_size,
        bears: extras.bears,
        bear_updates: extras.bear_updates,
        screen_every: extras.screen_every,
        without_bodies: extras.without_bodies,
        ..RenderBenchmarkReport::default()
    };
    let mut frame_times = Vec::with_capacity(frames as usize);
    let mut cpu_times = Vec::with_capacity(frames as usize);
    let mut gpu_times = Vec::with_capacity(frames as usize);
    let mut passes: Vec<(String, Duration, u32)> = Vec::new();
    for frame in 0..frames {
        let start = Instant::now();
        // Only traffic is measured; gameplay never reads these.
        renderer.take_completed_physics_events();
        renderer.take_completed_physics_states();
        renderer.take_physics_events_lost();
        *app.world_mut().get_mut::<Transform>(camera).unwrap() =
            render_benchmark_camera(frame);
        update_render_benchmark_bears(app.world_mut(), &bears, extras, frame);
        app.update(RENDER_BENCHMARK_FRAME_TIME)
            .map_err(|error| format!("frame {frame} update: {error}"))?;
        let world = app.world();
        let screens = renderer
            .render_screens(
                vulkano::sync::now(device.clone()).boxed(),
                world.resource::<RenderWorld>(),
                world.resource::<AssetServer>(),
                extent,
            )
            .map_err(|error| format!("frame {frame} screens: {error}"))?;
        renderer
            .render(
                screens,
                target.clone(),
                extent,
                SceneRenderOptions::game(extent),
                world.resource::<RenderWorld>(),
                world.resource::<AssetServer>(),
            )
            .map_err(|error| format!("frame {frame} render: {error}"))?
            .then_signal_fence_and_flush()
            .map_err(|error| format!("frame {frame} submit: {error}"))?
            .wait(None)
            .map_err(|error| format!("frame {frame} wait: {error}"))?;
        frame_times.push(start.elapsed());

        let mut cpu = *app.world().resource::<CpuFrameTimings>();
        renderer.write_cpu_timings(&mut cpu);
        cpu_times.push(
            cpu.physics + cpu.extraction + cpu.preparation + cpu.recording,
        );
        // Pass times lag a frame or two; each completed frame counts once
        // it is read, so the samples cover all but the last frames.
        let gpu = renderer.gpu_pass_times();
        if !gpu.0.is_empty() {
            gpu_times.push(gpu.total());
            for (pass, time) in gpu.0 {
                match passes
                    .iter_mut()
                    .find(|(label, ..)| label == pass.label())
                {
                    Some((_, total, count)) => {
                        *total += time;
                        *count += 1;
                    }
                    None => passes.push((pass.label().into(), time, 1)),
                }
            }
        }
        let counters = renderer.render_counters();
        report.draws = report.draws.max(counters.draws);
        report.dispatches = report.dispatches.max(counters.dispatches);
        report.triangles = report.triangles.max(counters.triangles);
        report.visible_instances =
            report.visible_instances.max(counters.visible_instances);
        report.gpu_memory_bytes =
            report.gpu_memory_bytes.max(counters.gpu_memory_bytes);
        report.upload_bytes += counters.upload_bytes;
        report.physics_command_bytes += counters.physics_command_bytes;
        report.physics_event_bytes += counters.physics_event_bytes;
        report.physics_state_bytes += counters.physics_state_bytes;
        report.readback_latency_frames = report
            .readback_latency_frames
            .max(counters.physics_readback_latency_frames);
    }
    let capacity = renderer.capacity_diagnostics();
    report.dropped_lights = capacity.dropped_lights;
    report.physics_events_dropped = capacity.physics_events_dropped;
    report.physics_commands_rejected = capacity.physics_commands_rejected;
    report.frame_time = TimeStats::from_samples(&frame_times);
    report.cpu_time = TimeStats::from_samples(&cpu_times);
    report.gpu_time = TimeStats::from_samples(&gpu_times);
    report.gpu_passes_ms = passes
        .into_iter()
        .map(|(label, total, count)| {
            (label, total.as_secs_f64() * 1e3 / f64::from(count))
        })
        .collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> RenderBenchmarkReport {
        let time = |p95_ms| TimeStats {
            mean_ms: p95_ms,
            p95_ms,
            max_ms: p95_ms,
        };
        RenderBenchmarkReport {
            device: "gpu".into(),
            driver: "1".into(),
            extent: [1920, 1080],
            quality: QualityProfile::Eco,
            frames: 600,
            frame_time: time(10.0),
            cpu_time: time(4.0),
            gpu_time: time(6.0),
            draws: 100,
            triangles: 1_000_000,
            upload_bytes: 1 << 20,
            ..RenderBenchmarkReport::default()
        }
    }

    #[test]
    fn time_stats_take_mean_p95_and_worst() {
        let samples: Vec<_> = (1..=100).map(Duration::from_millis).collect();
        let stats = TimeStats::from_samples(&samples);
        assert_eq!(stats.p95_ms, 95.0);
        assert_eq!(stats.max_ms, 100.0);
        assert!((stats.mean_ms - 50.5).abs() < 1e-9);
        assert_eq!(TimeStats::from_samples(&[]), TimeStats::default());
    }

    #[test]
    fn regressions_flag_material_growth_only() {
        let baseline = report();
        assert!(baseline.regressions(&baseline).is_empty());

        // Within tolerance and noise: not a regression.
        let mut run = report();
        run.draws = 110;
        run.frame_time.p95_ms = 10.4;
        run.gpu_time.p95_ms = 6.4;
        assert!(
            run.regressions(&baseline).is_empty(),
            "{:?}",
            run.regressions(&baseline)
        );

        let mut run = report();
        run.draws = 111;
        run.triangles = 2_000_000;
        run.cpu_time.p95_ms = 5.0;
        run.physics_events_dropped = 1;
        let found = run.regressions(&baseline);
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found[0].starts_with("draws"), "{found:?}");
        assert!(found.iter().any(|line| line.starts_with("CPU time")));

        // Another machine's times are not compared; its counters are.
        run.device = "other gpu".into();
        assert_eq!(run.regressions(&baseline).len(), 3);

        run.extent = [1280, 720];
        let found = run.regressions(&baseline);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("does not match baseline"), "{found:?}");
    }

    #[test]
    fn reports_round_trip_through_json() {
        let mut report = report();
        report.gpu_passes_ms = vec![("scene".into(), 3.5)];
        let json = serde_json::to_string_pretty(&report).unwrap();
        let back: RenderBenchmarkReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
    }

    #[test]
    #[cfg_attr(
        not(feature = "gpu-tests"),
        ignore = "run with `--features gpu-tests` on a machine with a Vulkan driver"
    )]
    fn benchmark_records_every_metric_along_the_camera_path() {
        let base = crate::rendering::test_support::headless_device();
        let report = run_render_benchmark(
            base.queue.clone(),
            [320, 180],
            QualityProfile::Eco,
            30,
        )
        .unwrap();
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        assert_eq!(report.frames, 30);
        assert!(report.frame_time.mean_ms > 0.0);
        assert!(report.cpu_time.mean_ms > 0.0);
        assert!(report.draws > 0 && report.dispatches > 0);
        assert!(report.triangles > 0 && report.visible_instances > 0);
        assert!(report.gpu_memory_bytes > 0 && report.upload_bytes > 0);
        assert_eq!(
            report.physics_bodies,
            1 + crate::runtime::RENDER_BENCHMARK_BODIES
        );
        // 1 sun + 24 point lights fit Eco's 256.
        assert_eq!(report.dropped_lights, 0);
        assert!(report.regressions(&report).is_empty());
    }
}
