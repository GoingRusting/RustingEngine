use bevy_ecs::prelude::World;

pub use rusting_core::events::EventQueue;

pub(crate) fn begin_frame<T: Send + Sync + 'static>(world: &mut World) {
    world.resource_mut::<EventQueue<T>>().begin_frame();
}
