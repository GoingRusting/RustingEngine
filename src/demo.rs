//! Small compiled gameplay plugin shared by the editor and runtime example.
//!
//! Real games should define equivalent plugins in their own game crate.

use bevy_ecs::prelude::{Component, Query, Res};
use serde::{Deserialize, Serialize};

use crate::runtime::{App, AppError, FrameTime, Plugin, ScheduleStage};
use crate::Transform;

#[derive(Component, Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Spin {
    pub speed: [f32; 2],
}

impl Default for Spin {
    fn default() -> Self {
        Self { speed: [0.35, 0.7] }
    }
}

fn spin_system(
    time: Res<FrameTime>,
    mut transforms: Query<(&Spin, &mut Transform)>,
) {
    let delta = time.delta.as_secs_f32();
    for (spin, mut transform) in &mut transforms {
        transform.rotation[0] += delta * spin.speed[0];
        transform.rotation[1] += delta * spin.speed[1];
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DemoPlugin;

impl Plugin for DemoPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.register_scene_component::<Spin>("rusting.demo.spin")
            .map_err(|error| AppError::PluginSetup {
                plugin: self.name(),
                message: error.to_string(),
            })?;
        app.add_systems(ScheduleStage::Update, spin_system);
        Ok(())
    }
}
