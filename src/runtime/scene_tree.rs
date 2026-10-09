//! Scene-tree lookups for systems: find a child, a path, a named object or
//! a class member.
//!
//! [`SceneTree`] only answers with [`Entity`] values. Read and write the
//! components with your own queries, so the ECS access stays visible in the
//! system's signature.

use bevy_ecs::entity::Entity;
use bevy_ecs::system::{Query, Res, SystemParam};

use super::{Children, ClassIndex, Name, Parent};

/// System parameter for common scene-tree lookups.
///
/// ```
/// use bevy_ecs::system::Query;
/// use rusting_engine::runtime::SceneTree;
/// use rusting_engine::Transform;
///
/// fn open_door(tree: SceneTree, mut doors: Query<&mut Transform>) {
///     let Some(house) = tree.find_by_name("House") else { return };
///     if let Some(door) = tree.find_path(house, "Front/Door") {
///         if let Ok(mut transform) = doors.get_mut(door) {
///             transform.position[1] += 3.0;
///         }
///     }
/// }
/// ```
#[derive(SystemParam)]
pub struct SceneTree<'w, 's> {
    names: Query<'w, 's, (Entity, &'static Name)>,
    children: Query<'w, 's, &'static Children>,
    parents: Query<'w, 's, &'static Parent>,
    classes: Res<'w, ClassIndex>,
}

impl SceneTree<'_, '_> {
    /// The object called `name`. With several, the one with the lowest
    /// entity index, so the answer does not depend on storage order.
    // ponytail: scans every named object; add a name index if a game calls
    // this per frame on large scenes.
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Option<Entity> {
        self.names
            .iter()
            .filter(|(_, current)| current.0 == name)
            .map(|(entity, _)| entity)
            .min_by_key(|entity| entity.index_u32())
    }

    /// The name of `entity`, if it has one.
    #[must_use]
    pub fn name(&self, entity: Entity) -> Option<&str> {
        self.names.get(entity).ok().map(|(_, name)| name.0.as_str())
    }

    /// The parent of `entity`.
    #[must_use]
    pub fn parent(&self, entity: Entity) -> Option<Entity> {
        self.parents.get(entity).ok().map(|parent| parent.0)
    }

    /// Direct children of `entity`, in hierarchy order.
    #[must_use]
    pub fn children(&self, entity: Entity) -> &[Entity] {
        self.children
            .get(entity)
            .map_or(&[], |children| children.0.as_slice())
    }

    /// The first direct child of `parent` called `name`, or, under a copy
    /// made by `spawn_copy`, called `"<parent name>/<name>"`.
    #[must_use]
    pub fn child(&self, parent: Entity, name: &str) -> Option<Entity> {
        let parent_name = self.name(parent);
        self.children(parent).iter().copied().find(|&child| {
            self.name(child).is_some_and(|child| {
                super::animation::is_step(child, parent_name, name)
            })
        })
    }

    /// Follows a `/`-separated path of child names from `root`, like
    /// Godot's `get_node("Arm/Hand")`. `..` steps to the parent.
    #[must_use]
    pub fn find_path(&self, root: Entity, path: &str) -> Option<Entity> {
        path.split('/')
            .filter(|step| !step.is_empty() && *step != ".")
            .try_fold(root, |current, step| match step {
                ".." => self.parent(current),
                name => self.child(current, name),
            })
    }

    /// Every descendant of `root`, depth first, in hierarchy order.
    #[must_use]
    pub fn descendants(&self, root: Entity) -> Vec<Entity> {
        let mut found = Vec::new();
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            let children = self.children(entity);
            stack.extend(children.iter().rev());
            if entity != root {
                found.push(entity);
            }
        }
        found
    }

    /// Members of an [`super::ObjectClasses`] class, in ascending entity
    /// order.
    pub fn in_class(&self, class: &str) -> impl Iterator<Item = Entity> + '_ {
        self.classes.members(class)
    }

    /// True when `entity` is in `class`.
    #[must_use]
    pub fn is_in_class(&self, entity: Entity, class: &str) -> bool {
        self.classes.contains(class, entity)
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::system::SystemState;

    use super::super::{App, ObjectClasses};
    use super::*;

    #[test]
    fn scene_tree_finds_children_paths_names_and_classes() {
        let mut app = App::new();
        let world = app.world_mut();
        let named = |name: &str| Name(name.to_owned());
        let robot = world.spawn(named("Robot")).id();
        let arm = world.spawn(named("Arm")).id();
        let hand = world
            .spawn((named("Hand"), ObjectClasses::new(["grabber"])))
            .id();
        let leg = world
            .spawn((named("Leg"), ObjectClasses::new(["grabber"])))
            .id();
        // A second "Robot", spawned later: the first one wins.
        world.spawn(named("Robot"));
        app.set_parent(arm, robot).unwrap();
        app.set_parent(hand, arm).unwrap();
        app.set_parent(leg, robot).unwrap();

        let mut state = SystemState::<SceneTree>::new(app.world_mut());
        let tree = state.get(app.world()).unwrap();
        assert_eq!(tree.find_by_name("Robot"), Some(robot));
        assert_eq!(tree.find_by_name("Tail"), None);
        assert_eq!(tree.children(robot), [arm, leg]);
        assert_eq!(tree.child(robot, "Leg"), Some(leg));
        assert_eq!(tree.child(robot, "Hand"), None);
        assert_eq!(tree.find_path(robot, "Arm/Hand"), Some(hand));
        assert_eq!(tree.find_path(hand, "../../Leg"), Some(leg));
        assert_eq!(tree.find_path(robot, "Arm/Missing"), None);
        assert_eq!(tree.find_path(robot, ""), Some(robot));
        assert_eq!(tree.parent(hand), Some(arm));
        assert_eq!(tree.descendants(robot), [arm, hand, leg]);
        assert_eq!(tree.in_class("grabber").collect::<Vec<_>>(), [hand, leg]);
        assert!(tree.is_in_class(hand, "grabber"));
        assert!(!tree.is_in_class(arm, "grabber"));
    }
}
