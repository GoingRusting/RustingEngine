use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{Display, Formatter};

use bevy_ecs::change_detection::{DetectChanges, Tick};
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Or, Ref, Resource, With, World};
use nalgebra::Matrix4;

use crate::components::{Children, GlobalTransform, Parent};
use crate::transform::Transform;

/// Errors produced while modifying an ECS hierarchy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HierarchyError {
    MissingEntity(Entity),
    Cycle { child: Entity, parent: Entity },
}

impl Display for HierarchyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEntity(entity) => write!(formatter, "entity {entity:?} does not exist"),
            Self::Cycle { child, parent } => write!(
                formatter,
                "parenting {child:?} beneath {parent:?} would create a hierarchy cycle"
            ),
        }
    }
}

impl Error for HierarchyError {}

#[derive(Resource, Clone, Debug, Default)]
pub struct HierarchyDiagnostics {
    pub cycles: Vec<Entity>,
    pub missing_parents: Vec<Entity>,
}

/// What the last full propagation saw. A run with no changed `Transform` or
/// `Parent` and the same component counts has nothing to update.
#[derive(Resource)]
struct PropagationFingerprint {
    last_run: Tick,
    counts: [usize; 3],
}

/// Parents `child` beneath `parent`, maintaining both sides of the relation.
pub fn set_parent(
    world: &mut World,
    child: Entity,
    parent: Entity,
) -> Result<(), HierarchyError> {
    if world.get_entity(child).is_err() {
        return Err(HierarchyError::MissingEntity(child));
    }
    if world.get_entity(parent).is_err() {
        return Err(HierarchyError::MissingEntity(parent));
    }

    let mut ancestor = Some(parent);
    let mut visited = HashSet::new();
    while let Some(entity) = ancestor {
        if entity == child || !visited.insert(entity) {
            return Err(HierarchyError::Cycle { child, parent });
        }
        ancestor = world.get::<Parent>(entity).map(|parent| parent.0);
    }

    clear_parent(world, child)?;
    world.entity_mut(child).insert(Parent(parent));
    let mut parent_entity = world.entity_mut(parent);
    if let Some(mut children) = parent_entity.get_mut::<Children>() {
        if !children.0.contains(&child) {
            children.0.push(child);
        }
    } else {
        parent_entity.insert(Children(vec![child]));
    }
    Ok(())
}

/// Removes `child` from its current parent, maintaining both relation sides.
pub fn clear_parent(
    world: &mut World,
    child: Entity,
) -> Result<(), HierarchyError> {
    if world.get_entity(child).is_err() {
        return Err(HierarchyError::MissingEntity(child));
    }
    let old_parent = world.get::<Parent>(child).map(|parent| parent.0);
    world.entity_mut(child).remove::<Parent>();
    if let Some(parent) = old_parent {
        if let Ok(mut parent_entity) = world.get_entity_mut(parent) {
            if let Some(mut children) = parent_entity.get_mut::<Children>() {
                children.0.retain(|entity| *entity != child);
            }
        }
    }
    Ok(())
}

/// Resolves the complete live hierarchy and rebuilds global transforms for
/// entities that have a local [`Transform`].
///
/// Existing entities without a local transform act as identity hierarchy
/// nodes. References to nonexistent parents and malformed cycles are recorded
/// in [`HierarchyDiagnostics`], which is initialized by this function when
/// absent. Every live entity is resolved in deterministic entity order, and
/// diagnostic entity lists are likewise sorted. A [`GlobalTransform`] is never
/// inserted on a transformless entity.
///
/// Returns early when no `Transform` or `Parent` changed since the last run
/// and no entity lost one of them or `Children`. Removing or despawning such
/// an entity lowers a count; adding one back is itself a change.
// ponytail: any single moved object still rebuilds everything; walk only the
// changed subtrees if many moving objects make this hot.
pub fn propagate_transforms(world: &mut World) {
    let this_run = world.increment_change_tick();
    let mut counts = [0; 3];
    let mut changed = false;
    let last_run = world
        .get_resource::<PropagationFingerprint>()
        .map(|fingerprint| fingerprint.last_run);
    let mut query = world.query_filtered::<(
        Option<Ref<Transform>>,
        Option<Ref<Parent>>,
        Option<&Children>,
    ), Or<(With<Transform>, With<Parent>, With<Children>)>>(
    );
    for (transform, parent, children) in query.iter(world) {
        counts[0] += usize::from(transform.is_some());
        counts[1] += usize::from(parent.is_some());
        counts[2] += usize::from(children.is_some());
        changed |= last_run.is_some_and(|last_run| {
            transform.is_some_and(|value| {
                value.last_changed().is_newer_than(last_run, this_run)
            }) || parent.is_some_and(|value| {
                value.last_changed().is_newer_than(last_run, this_run)
            })
        });
    }
    let unchanged = world
        .get_resource::<PropagationFingerprint>()
        .is_some_and(|fingerprint| fingerprint.counts == counts)
        && !changed
        && world.contains_resource::<HierarchyDiagnostics>();
    world.insert_resource(PropagationFingerprint {
        last_run: this_run,
        counts,
    });
    if unchanged {
        return;
    }

    if !world.contains_resource::<HierarchyDiagnostics>() {
        world.insert_resource(HierarchyDiagnostics::default());
    }
    {
        let mut diagnostics = world.resource_mut::<HierarchyDiagnostics>();
        diagnostics.cycles.clear();
        diagnostics.missing_parents.clear();
    }

    let mut existing = HashSet::new();
    let mut locals = HashMap::new();
    let mut parents = HashMap::new();
    let mut query =
        world.query::<(Entity, Option<&Transform>, Option<&Parent>)>();
    for (entity, transform, parent) in query.iter(world) {
        existing.insert(entity);
        if let Some(transform) = transform {
            locals.insert(entity, matrix_from_array(transform.to_matrix()));
        }
        if let Some(parent) = parent {
            parents.insert(entity, parent.0);
        }
    }

    let mut resolved = HashMap::new();
    let mut visiting = HashSet::new();
    let mut cycles = Vec::new();
    let mut missing_parents = Vec::new();
    let mut worklist = existing.iter().copied().collect::<Vec<_>>();
    worklist.sort_unstable();
    for entity in worklist.iter().copied() {
        resolve_global(
            entity,
            &existing,
            &locals,
            &parents,
            &mut resolved,
            &mut visiting,
            &mut cycles,
            &mut missing_parents,
        );
    }

    let mut transformed = locals.keys().copied().collect::<Vec<_>>();
    transformed.sort_unstable();
    for entity in transformed {
        let matrix = resolved[&entity];
        let global = GlobalTransform {
            matrix: matrix.into(),
        };
        if world.get::<GlobalTransform>(entity) != Some(&global) {
            world.entity_mut(entity).insert(global);
        }
    }
    cycles.sort_unstable();
    missing_parents.sort_unstable();
    let mut diagnostics = world.resource_mut::<HierarchyDiagnostics>();
    diagnostics.cycles = cycles;
    diagnostics.missing_parents = missing_parents;
}

#[allow(clippy::too_many_arguments)]
fn resolve_global(
    entity: Entity,
    existing: &HashSet<Entity>,
    locals: &HashMap<Entity, Matrix4<f32>>,
    parents: &HashMap<Entity, Entity>,
    resolved: &mut HashMap<Entity, Matrix4<f32>>,
    visiting: &mut HashSet<Entity>,
    cycles: &mut Vec<Entity>,
    missing_parents: &mut Vec<Entity>,
) -> Matrix4<f32> {
    if let Some(matrix) = resolved.get(&entity) {
        return *matrix;
    }
    let local = locals
        .get(&entity)
        .copied()
        .unwrap_or_else(Matrix4::identity);
    if !visiting.insert(entity) {
        cycles.push(entity);
        return local;
    }

    let global = if let Some(parent) = parents.get(&entity).copied() {
        if existing.contains(&parent) {
            resolve_global(
                parent,
                existing,
                locals,
                parents,
                resolved,
                visiting,
                cycles,
                missing_parents,
            ) * local
        } else {
            missing_parents.push(entity);
            local
        }
    } else {
        local
    };
    visiting.remove(&entity);
    resolved.insert(entity, global);
    global
}

fn matrix_from_array(matrix: [[f32; 4]; 4]) -> Matrix4<f32> {
    Matrix4::from_column_slice(&[
        matrix[0][0],
        matrix[0][1],
        matrix[0][2],
        matrix[0][3],
        matrix[1][0],
        matrix[1][1],
        matrix[1][2],
        matrix[1][3],
        matrix[2][0],
        matrix[2][1],
        matrix[2][2],
        matrix[2][3],
        matrix[3][0],
        matrix[3][1],
        matrix[3][2],
        matrix[3][3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_diagnostics() -> World {
        let mut world = World::new();
        world.insert_resource(HierarchyDiagnostics::default());
        world
    }

    #[test]
    fn propagation_composes_parent_and_child_transforms() {
        let mut world = world_with_diagnostics();
        let parent = world.spawn(Transform::new([2.0, 3.0, 4.0])).id();
        let child = world.spawn(Transform::new([5.0, 7.0, 11.0])).id();
        set_parent(&mut world, child, parent).unwrap();
        propagate_transforms(&mut world);
        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3],
            [7.0, 10.0, 15.0, 1.0]
        );
    }

    #[test]
    fn propagation_initializes_diagnostics_in_a_fresh_world() {
        let mut world = World::new();
        let entity = world.spawn(Transform::new([1.0, 2.0, 3.0])).id();

        propagate_transforms(&mut world);

        assert!(world.contains_resource::<HierarchyDiagnostics>());
        let diagnostics = world.resource::<HierarchyDiagnostics>();
        assert!(diagnostics.cycles.is_empty());
        assert!(diagnostics.missing_parents.is_empty());
        assert_eq!(
            world.get::<GlobalTransform>(entity).unwrap().matrix[3],
            [1.0, 2.0, 3.0, 1.0]
        );
    }

    #[test]
    fn propagation_uses_transformless_intermediate_entities_as_identity() {
        let mut world = World::new();
        let grandparent = world.spawn(Transform::new([2.0, 3.0, 4.0])).id();
        let middle = world.spawn(Parent(grandparent)).id();
        let child = world
            .spawn((Transform::new([5.0, 7.0, 11.0]), Parent(middle)))
            .id();

        propagate_transforms(&mut world);

        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3],
            [7.0, 10.0, 15.0, 1.0]
        );
        assert!(world.get::<GlobalTransform>(middle).is_none());
        assert!(world
            .resource::<HierarchyDiagnostics>()
            .missing_parents
            .is_empty());
    }

    #[test]
    fn clearing_the_final_parent_resets_an_unchanged_child_global() {
        let mut world = World::new();
        let parent = world.spawn(Transform::new([2.0, 3.0, 4.0])).id();
        let child = world.spawn(Transform::new([5.0, 7.0, 11.0])).id();
        set_parent(&mut world, child, parent).unwrap();
        propagate_transforms(&mut world);
        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3],
            [7.0, 10.0, 15.0, 1.0]
        );

        clear_parent(&mut world, child).unwrap();
        propagate_transforms(&mut world);

        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3],
            [5.0, 7.0, 11.0, 1.0]
        );
    }

    #[test]
    fn reparenting_and_clearing_maintain_both_sides() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        let child = world.spawn_empty().id();
        set_parent(&mut world, child, first).unwrap();
        set_parent(&mut world, child, second).unwrap();
        assert_eq!(world.get::<Parent>(child), Some(&Parent(second)));
        assert!(world.get::<Children>(first).unwrap().0.is_empty());
        assert_eq!(world.get::<Children>(second).unwrap().0, vec![child]);
        clear_parent(&mut world, child).unwrap();
        assert!(world.get::<Parent>(child).is_none());
        assert!(world.get::<Children>(second).unwrap().0.is_empty());
    }

    #[test]
    fn cycle_rejection_does_not_mutate_existing_hierarchy() {
        let mut world = World::new();
        let root = world.spawn_empty().id();
        let child = world.spawn_empty().id();
        set_parent(&mut world, child, root).unwrap();
        assert_eq!(
            set_parent(&mut world, root, child),
            Err(HierarchyError::Cycle {
                child: root,
                parent: child
            })
        );
        assert!(world.get::<Parent>(root).is_none());
        assert_eq!(world.get::<Parent>(child), Some(&Parent(root)));
        assert_eq!(world.get::<Children>(root).unwrap().0, vec![child]);
    }

    #[test]
    fn missing_child_and_parent_are_reported() {
        let mut world = World::new();
        let child = world.spawn_empty().id();
        let parent = world.spawn_empty().id();
        world.despawn(child);
        assert_eq!(
            set_parent(&mut world, child, parent),
            Err(HierarchyError::MissingEntity(child))
        );
        let child = world.spawn_empty().id();
        world.despawn(parent);
        assert_eq!(
            set_parent(&mut world, child, parent),
            Err(HierarchyError::MissingEntity(parent))
        );
        assert_eq!(
            clear_parent(&mut world, parent),
            Err(HierarchyError::MissingEntity(parent))
        );
    }

    #[test]
    fn propagation_diagnoses_a_missing_parent() {
        let mut world = world_with_diagnostics();
        let missing = world.spawn_empty().id();
        world.despawn(missing);
        let child = world
            .spawn((Transform::new([1.0, 2.0, 3.0]), Parent(missing)))
            .id();
        propagate_transforms(&mut world);
        let diagnostics = world.resource::<HierarchyDiagnostics>();
        assert_eq!(diagnostics.missing_parents, vec![child]);
        assert!(diagnostics.cycles.is_empty());
        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3],
            [1.0, 2.0, 3.0, 1.0]
        );
    }

    #[test]
    fn propagation_diagnoses_a_transformless_dangling_parent() {
        let mut world = World::new();
        let missing = world.spawn_empty().id();
        world.despawn(missing);
        let child = world.spawn(Parent(missing)).id();

        propagate_transforms(&mut world);

        let diagnostics = world.resource::<HierarchyDiagnostics>();
        assert_eq!(diagnostics.missing_parents, vec![child]);
        assert!(diagnostics.cycles.is_empty());
        assert!(world.get::<GlobalTransform>(child).is_none());
    }

    #[test]
    fn propagation_diagnoses_a_malformed_cycle() {
        let mut world = world_with_diagnostics();
        let first = world.spawn(Transform::default()).id();
        let second = world.spawn(Transform::default()).id();
        world.entity_mut(first).insert(Parent(second));
        world.entity_mut(second).insert(Parent(first));
        propagate_transforms(&mut world);
        let diagnostics = world.resource::<HierarchyDiagnostics>();
        assert_eq!(diagnostics.cycles, vec![std::cmp::min(first, second)]);
        assert!(diagnostics.missing_parents.is_empty());
    }

    #[test]
    fn propagation_diagnoses_a_transformless_cycle_deterministically() {
        let mut world = World::new();
        let first = world.spawn_empty().id();
        let second = world.spawn_empty().id();
        world.entity_mut(first).insert(Parent(second));
        world.entity_mut(second).insert(Parent(first));

        propagate_transforms(&mut world);

        let diagnostics = world.resource::<HierarchyDiagnostics>();
        assert_eq!(diagnostics.cycles, vec![std::cmp::min(first, second)]);
        assert!(diagnostics.missing_parents.is_empty());
        assert!(world.get::<GlobalTransform>(first).is_none());
        assert!(world.get::<GlobalTransform>(second).is_none());
    }

    #[test]
    fn propagation_skips_unchanged_worlds_but_sees_later_edits() {
        let mut world = World::new();
        let parent = world.spawn(Transform::new([2.0, 0.0, 0.0])).id();
        let child = world.spawn(Transform::new([1.0, 0.0, 0.0])).id();
        set_parent(&mut world, child, parent).unwrap();
        propagate_transforms(&mut world);

        // Nothing changed, so a planted value survives the next run.
        world.get_mut::<GlobalTransform>(child).unwrap().matrix[3][0] = 99.0;
        propagate_transforms(&mut world);
        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3][0],
            99.0
        );

        world.get_mut::<Transform>(parent).unwrap().position[0] = 5.0;
        propagate_transforms(&mut world);
        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3][0],
            6.0
        );

        // Despawning the parent changes no remaining component, only counts.
        world.despawn(parent);
        propagate_transforms(&mut world);
        assert_eq!(
            world.get::<GlobalTransform>(child).unwrap().matrix[3][0],
            1.0
        );
        assert_eq!(
            world.resource::<HierarchyDiagnostics>().missing_parents,
            vec![child]
        );
    }

    #[test]
    fn hierarchy_error_has_the_public_error_traits() {
        fn assert_traits<
            T: Error + Clone + Copy + Eq + Send + Sync + 'static,
        >() {
        }
        assert_traits::<HierarchyError>();
        let entity = World::new().spawn_empty().id();
        assert_eq!(
            HierarchyError::MissingEntity(entity).to_string(),
            format!("entity {entity:?} does not exist")
        );
    }
}
