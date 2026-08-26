use std::time::Duration;

use bevy_ecs::prelude::{Resource, World};

use super::AppError;

/// Frame and simulation timing visible to systems.
#[derive(Resource, Clone, Copy, Debug)]
pub struct FrameTime {
    /// Number of rendered updates since the app started.
    pub frame: u64,
    /// Real time since the previous frame, even while paused.
    pub real_delta: Duration,
    /// Game time since the previous frame after pause and time scale.
    pub delta: Duration,
    /// Total game time, without time spent paused.
    pub elapsed: Duration,
    /// Time simulated by one fixed physics update.
    pub fixed_delta: Duration,
    /// Number of completed fixed physics updates.
    pub fixed_tick: u64,
}

impl Default for FrameTime {
    fn default() -> Self {
        Self {
            frame: 0,
            real_delta: Duration::ZERO,
            delta: Duration::ZERO,
            elapsed: Duration::ZERO,
            fixed_delta: Duration::from_secs_f64(1.0 / 60.0),
            fixed_tick: 0,
        }
    }
}

impl FrameTime {
    /// Returns the current game-frame time as an easy-to-use `f32` value.
    #[must_use]
    pub fn delta_seconds(&self) -> f32 {
        self.delta.as_secs_f32()
    }

    /// Returns total running game time as an easy-to-use `f32` value.
    #[must_use]
    pub fn elapsed_seconds(&self) -> f32 {
        self.elapsed.as_secs_f32()
    }
}

/// Pause, stepping, scale, and fixed-update catch-up controls.
#[derive(Resource, Clone, Debug)]
pub struct TimeControl {
    pub paused: bool,
    pub time_scale: f64,
    pub fixed_delta: Duration,
    pub max_fixed_steps: u32,
    accumulator: Duration,
    pending_steps: u32,
}

impl Default for TimeControl {
    fn default() -> Self {
        Self {
            paused: false,
            time_scale: 1.0,
            fixed_delta: Duration::from_secs_f64(1.0 / 60.0),
            max_fixed_steps: 8,
            accumulator: Duration::ZERO,
            pending_steps: 0,
        }
    }
}

impl TimeControl {
    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn step(&mut self) {
        self.pending_steps = self.pending_steps.saturating_add(1);
    }
}

pub(super) fn advance(
    world: &mut World,
    real_delta: Duration,
) -> Result<u32, AppError> {
    let (fixed_delta, scaled_delta, elapsed_delta, fixed_steps) = {
        let mut control = world.resource_mut::<TimeControl>();
        if control.fixed_delta.is_zero() {
            return Err(AppError::InvalidFixedDelta);
        }

        let scaled_delta = if control.paused {
            Duration::ZERO
        } else {
            real_delta.mul_f64(control.time_scale.max(0.0))
        };
        control.accumulator = control.accumulator.saturating_add(scaled_delta);

        let mut fixed_steps = 0;
        let fixed_delta = control.fixed_delta;
        while control.accumulator >= fixed_delta
            && fixed_steps < control.max_fixed_steps
            && !control.paused
        {
            control.accumulator -= fixed_delta;
            fixed_steps += 1;
        }
        let stepped_while_paused = control.paused && control.pending_steps > 0;
        if stepped_while_paused {
            control.pending_steps -= 1;
            fixed_steps = 1;
        }
        let elapsed_delta = if stepped_while_paused {
            fixed_delta
        } else {
            scaled_delta
        };
        (fixed_delta, scaled_delta, elapsed_delta, fixed_steps)
    };

    let mut time = world.resource_mut::<FrameTime>();
    time.frame = time.frame.saturating_add(1);
    time.real_delta = real_delta;
    time.delta = scaled_delta;
    time.elapsed = time.elapsed.saturating_add(elapsed_delta);
    time.fixed_delta = fixed_delta;
    time.fixed_tick = time.fixed_tick.saturating_add(u64::from(fixed_steps));
    Ok(fixed_steps)
}
