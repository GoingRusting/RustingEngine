use std::collections::{HashMap, HashSet};

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Resource, World};
use nalgebra::Matrix4;

use crate::Transform;

use super::{AppError, Children, GlobalTransform, Parent};

#[derive(Resource, Clone, Debug, Default)]
pub struct HierarchyDiagnostics {
    pub cycles: Vec<Entity>,
    pub missing_parents: Vec<Entity>,
}

pub(super) fn set_parent(
    world: &mut World,
    child: Entity,
    parent: Entity,
) -> Result<(), AppError> {
    if world.get_entity(child).is_err() {
        return Err(AppError::MissingEntity(child));
    }
    if world.get_entity(parent).is_err() {
        return Err(AppError::MissingEntity(parent));
    }

    let mut ancestor = Some(parent);
    let mut visited = HashSet::new();
    while let Some(entity) = ancestor {
        if entity == child || !visited.insert(entity) {
            return Err(AppError::HierarchyCycle { child, parent });
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

pub(super) fn clear_parent(
    world: &mut World,
    child: Entity,
) -> Result<(), AppError> {
    if world.get_entity(child).is_err() {
        return Err(AppError::MissingEntity(child));
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

/// Rebuilds every global transform while safely diagnosing malformed cycles.
pub fn propagate_transforms(world: &mut World) {
    let mut locals = HashMap::new();
    let mut parents = HashMap::new();
    let mut query = world.query::<(Entity, &Transform, Option<&Parent>)>();
    for (entity, transform, parent) in query.iter(world) {
        locals.insert(entity, matrix_from_array(transform.to_matrix()));
        if let Some(parent) = parent {
            parents.insert(entity, parent.0);
        }
    }

    let mut resolved = HashMap::new();
    let mut visiting = HashSet::new();
    let mut cycles = Vec::new();
    let mut missing_parents = Vec::new();
    for entity in locals.keys().copied() {
        resolve_global(
            entity,
            &locals,
            &parents,
            &mut resolved,
            &mut visiting,
            &mut cycles,
            &mut missing_parents,
        );
    }

    for (entity, matrix) in resolved {
        let global = GlobalTransform {
            matrix: matrix.into(),
        };
        if world.get::<GlobalTransform>(entity) != Some(&global) {
            world.entity_mut(entity).insert(global);
        }
    }
    let mut diagnostics = world.resource_mut::<HierarchyDiagnostics>();
    diagnostics.cycles = cycles;
    diagnostics.missing_parents = missing_parents;
}

#[allow(clippy::too_many_arguments)]
fn resolve_global(
    entity: Entity,
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
        if locals.contains_key(&parent) {
            resolve_global(
                parent,
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
