use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;

use rusting_core::hierarchy::{self, HierarchyError};
pub use rusting_core::hierarchy::{propagate_transforms, HierarchyDiagnostics};

use super::AppError;

pub(crate) fn set_parent(
    world: &mut World,
    child: Entity,
    parent: Entity,
) -> Result<(), AppError> {
    hierarchy::set_parent(world, child, parent).map_err(map_error)
}

pub(super) fn clear_parent(
    world: &mut World,
    child: Entity,
) -> Result<(), AppError> {
    hierarchy::clear_parent(world, child).map_err(map_error)
}

fn map_error(error: HierarchyError) -> AppError {
    match error {
        HierarchyError::MissingEntity(entity) => {
            AppError::MissingEntity(entity)
        }
        HierarchyError::Cycle { child, parent } => {
            AppError::HierarchyCycle { child, parent }
        }
    }
}
