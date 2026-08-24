use bevy_ecs::prelude::{Resource, World};

/// A typed event channel whose visible events live for exactly one frame.
#[derive(Resource)]
pub struct EventQueue<T: Send + Sync + 'static> {
    current: Vec<T>,
    pending: Vec<T>,
}

impl<T: Send + Sync + 'static> Default for EventQueue<T> {
    fn default() -> Self {
        Self {
            current: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl<T: Send + Sync + 'static> EventQueue<T> {
    pub fn send(&mut self, event: T) {
        self.pending.push(event);
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &T> {
        self.current.iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.current.len()
    }

    pub(super) fn begin_frame(world: &mut World) {
        let mut events = world.resource_mut::<Self>();
        events.current.clear();
        let Self {
            current, pending, ..
        } = &mut *events;
        std::mem::swap(current, pending);
    }
}
