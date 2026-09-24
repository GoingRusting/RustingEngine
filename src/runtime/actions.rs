//! Engine integration for reusable named input actions.

use bevy_ecs::prelude::World;

pub use rusting_core::input::{ActionMap, InputBinding};

pub(super) fn install(world: &mut World) {
    world.insert_resource(ActionMap::default());
}
