use bevy_ecs::prelude::Resource;

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

    /// Expires the currently visible events and promotes pending events.
    ///
    /// Runtime integrations must call this exactly once at the start of each
    /// frame, before event consumers run. This is runtime integration API,
    /// not application API.
    #[doc(hidden)]
    pub fn begin_frame(&mut self) {
        self.current.clear();
        std::mem::swap(&mut self.current, &mut self.pending);
    }
}

#[cfg(test)]
mod tests {
    use super::EventQueue;
    use bevy_ecs::prelude::Resource;

    struct NoDefault;

    #[test]
    fn default_does_not_require_the_event_type_to_implement_default() {
        let mut events = EventQueue::<NoDefault>::default();

        assert!(events.is_empty());
        events.send(NoDefault);
        events.begin_frame();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn pending_events_are_invisible() {
        let mut events = EventQueue::default();

        events.send(1);

        assert!(events.is_empty());
        assert_eq!(events.len(), 0);
        assert_eq!(
            events.iter().copied().collect::<Vec<_>>(),
            Vec::<i32>::new()
        );
    }

    #[test]
    fn events_keep_insertion_order_on_the_next_frame() {
        let mut events = EventQueue::default();
        events.send("first");
        events.send("second");
        events.send("third");

        events.begin_frame();

        assert_eq!(
            events.iter().copied().collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
    }

    #[test]
    fn visible_events_expire_after_one_frame() {
        let mut events = EventQueue::default();
        events.send(1);
        events.begin_frame();
        assert_eq!(events.iter().copied().collect::<Vec<_>>(), vec![1]);

        events.begin_frame();

        assert!(events.is_empty());
    }

    #[test]
    fn sends_while_current_is_visible_are_deferred() {
        let mut events = EventQueue::default();
        events.send("current");
        events.begin_frame();

        events.send("next");

        assert_eq!(events.iter().copied().collect::<Vec<_>>(), vec!["current"]);
        events.begin_frame();
        assert_eq!(events.iter().copied().collect::<Vec<_>>(), vec!["next"]);
    }

    #[test]
    fn event_queue_is_an_ecs_resource() {
        fn assert_resource<T: Resource>() {}

        assert_resource::<EventQueue<u32>>();
    }
}
