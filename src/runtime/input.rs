//! Engine integration for the reusable runtime input resource.

use bevy_ecs::prelude::World;

pub use rusting_core::input::{KeyCode, MouseButton, RuntimeInput};

pub(super) fn install(world: &mut World) {
    world.insert_resource(RuntimeInput::default());
}
