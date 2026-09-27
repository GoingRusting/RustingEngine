//! Vulkan-independent application errors shared by runtime hosts.

use std::error::Error;
use std::fmt::{Display, Formatter};

use bevy_ecs::entity::Entity;

/// Errors produced while constructing or controlling an application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppError {
    InvalidFixedDelta,
    DuplicatePlugin(&'static str),
    HierarchyCycle {
        child: Entity,
        parent: Entity,
    },
    MissingEntity(Entity),
    PluginSetup {
        plugin: &'static str,
        message: String,
    },
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFixedDelta => formatter.write_str("fixed delta must be greater than zero"),
            Self::DuplicatePlugin(name) => write!(formatter, "plugin `{name}` was already added"),
            Self::HierarchyCycle { child, parent } => write!(
                formatter,
                "parenting {child:?} beneath {parent:?} would create a hierarchy cycle"
            ),
            Self::MissingEntity(entity) => write!(formatter, "entity {entity:?} does not exist"),
            Self::PluginSetup { plugin, message } => {
                write!(formatter, "plugin `{plugin}` setup failed: {message}")
            }
        }
    }
}

impl Error for AppError {}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use bevy_ecs::entity::Entity;

    use super::AppError;

    #[test]
    fn errors_preserve_entity_and_plugin_context() {
        let entity = Entity::from_bits(1);
        let missing = AppError::MissingEntity(entity);
        assert!(missing.to_string().contains(&format!("{entity:?}")));

        let setup = AppError::PluginSetup {
            plugin: "TestPlugin",
            message: "invalid configuration".into(),
        };
        assert_eq!(
            setup.to_string(),
            "plugin `TestPlugin` setup failed: invalid configuration"
        );

        let _: &dyn Error = &setup;
    }
}
