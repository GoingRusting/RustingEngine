use std::time::Duration;

use bevy_ecs::prelude::{Mut, World};
use rusting_core::time::{self as core_time, TimeAdvanceError};
pub use rusting_core::time::{FrameTime, TimeControl};

use super::AppError;

pub(super) fn advance(
    world: &mut World,
    real_delta: Duration,
) -> Result<u32, AppError> {
    world.resource_scope(|world, mut control: Mut<TimeControl>| {
        let mut time = world.resource_mut::<FrameTime>();
        match core_time::advance(&mut control, &mut time, real_delta) {
            Ok(fixed_steps) => Ok(fixed_steps),
            Err(TimeAdvanceError::InvalidFixedDelta) => {
                Err(AppError::InvalidFixedDelta)
            }
        }
    })
}
