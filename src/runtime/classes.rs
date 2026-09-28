//! Fast class membership: which entities carry a given [`ObjectClasses`]
//! name.
//!
//! Classes are the engine's groups or tags. Component hooks keep
//! [`ClassIndex`] up to date whenever `ObjectClasses` is inserted, replaced,
//! removed or despawned, so a lookup costs one hash access instead of a scan
//! over every object.

use std::collections::{BTreeMap, HashMap};

use bevy_ecs::entity::Entity;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::{DeferredWorld, World};

use super::ObjectClasses;

/// Entities per class name, kept in ascending entity order.
///
/// Read it with `Res<ClassIndex>` in a system or [`super::App::class_members`].
/// The index follows inserts of `ObjectClasses`. Editing the component in
/// place through `Mut<ObjectClasses>` bypasses it; insert a new value
/// instead.
#[derive(Resource, Default, Debug)]
pub struct ClassIndex {
    // Keyed by entity index: one live entity per index, ascending order.
    members: HashMap<String, BTreeMap<u32, Entity>>,
}

impl ClassIndex {
    /// Entities in `class`, in ascending entity order.
    pub fn members(&self, class: &str) -> impl Iterator<Item = Entity> + '_ {
        self.members
            .get(class)
            .into_iter()
            .flat_map(|members| members.values().copied())
    }

    /// True when `entity` belongs to `class`.
    #[must_use]
    pub fn contains(&self, class: &str, entity: Entity) -> bool {
        self.members.get(class).is_some_and(|members| {
            members.get(&entity.index_u32()) == Some(&entity)
        })
    }

    fn add(&mut self, entity: Entity, classes: &ObjectClasses) {
        for class in &classes.names {
            self.members
                .entry(class.clone())
                .or_default()
                .insert(entity.index_u32(), entity);
        }
    }

    fn remove(&mut self, entity: Entity, classes: &ObjectClasses) {
        for class in &classes.names {
            if let Some(members) = self.members.get_mut(class) {
                members.remove(&entity.index_u32());
                if members.is_empty() {
                    self.members.remove(class);
                }
            }
        }
    }
}

/// Adds the index and the hooks that keep it current.
pub(super) fn install(world: &mut World) {
    world.init_resource::<ClassIndex>();
    world
        .register_component_hooks::<ObjectClasses>()
        .on_insert(|world, context| update(world, context.entity, true))
        .on_discard(|world, context| update(world, context.entity, false));
}

fn update(mut world: DeferredWorld, entity: Entity, add: bool) {
    let Some(classes) = world.get::<ObjectClasses>(entity).cloned() else {
        return;
    };
    if let Some(mut index) = world.get_resource_mut::<ClassIndex>() {
        if add {
            index.add(entity, &classes);
        } else {
            index.remove(entity, &classes);
        }
    }
}

/// Builds the index again from the world. Snapshot restores overwrite
/// components in place, which the hooks do not see.
pub(super) fn rebuild(world: &mut World) {
    let mut index = ClassIndex::default();
    let mut query = world.query::<(Entity, &ObjectClasses)>();
    for (entity, classes) in query.iter(world) {
        index.add(entity, classes);
    }
    world.insert_resource(index);
}

#[cfg(test)]
mod tests {
    use super::super::App;
    use super::*;

    fn members(app: &App, class: &str) -> Vec<Entity> {
        app.class_members(class).collect()
    }

    #[test]
    fn class_index_follows_insert_replace_remove_despawn_and_restore() {
        let mut app = App::new();
        let world = app.world_mut();
        let a = world.spawn(ObjectClasses::new(["enemy", "hot"])).id();
        let b = world.spawn(ObjectClasses::new(["enemy"])).id();
        assert_eq!(members(&app, "enemy"), [a, b]);
        assert_eq!(members(&app, "hot"), [a]);

        let snapshot = app.snapshot().unwrap();

        app.world_mut()
            .entity_mut(a)
            .insert(ObjectClasses::new(["pickup"]));
        assert_eq!(members(&app, "enemy"), [b]);
        assert!(members(&app, "hot").is_empty());
        assert_eq!(members(&app, "pickup"), [a]);

        // Restore overwrites a's classes in place, past the hooks.
        app.restore(&snapshot).unwrap();
        assert_eq!(members(&app, "enemy"), [a, b]);
        assert_eq!(members(&app, "hot"), [a]);
        assert!(members(&app, "pickup").is_empty());
        assert!(app.world().resource::<ClassIndex>().contains("hot", a));

        app.world_mut().entity_mut(b).remove::<ObjectClasses>();
        assert_eq!(members(&app, "enemy"), [a]);
        app.world_mut().despawn(a);
        assert!(members(&app, "enemy").is_empty());
        assert!(members(&app, "hot").is_empty());
    }
}
