//! Named gameplay mechanics that `rusting recipe apply` writes into a
//! project: a Rust source file, the scene objects it needs, and a scenario
//! that proves it works.

use serde_json::{json, Value};

/// One recipe. The scene objects and scenario are built from the player's
/// starting position so the scenario passes in any template.
pub struct Recipe {
    /// `snake_case` name; also the source file and scenario name.
    pub name: &'static str,
    /// One sentence on what the mechanic does.
    pub summary: &'static str,
    /// Contents of `src/<name>.rs`.
    pub source: &'static str,
    /// The call `update` makes once a frame.
    pub call: &'static str,
    /// Scene objects to create and the scenario, given the position of the
    /// object named `Player`.
    pub build: fn([f32; 3]) -> (Vec<Value>, Value),
}

/// Every recipe, in `recipe list` order.
pub const RECIPES: &[Recipe] = &[Recipe {
    name: "checkpoints",
    summary: "Touching an object in class `checkpoint` stores it as the respawn point; falling below y = -10 puts the player back there. Adds `Checkpoint Respawn` at the player and a sensor `Checkpoint 1` four units along +X.",
    source: include_str!("recipes/checkpoints.rs"),
    call: "checkpoints::checkpoints(scene);",
    build: checkpoint_scene,
}];

/// The recipe called `name`.
#[must_use]
pub fn recipe(name: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|recipe| recipe.name == name)
}

fn checkpoint_scene(player: [f32; 3]) -> (Vec<Value>, Value) {
    let checkpoint = [player[0] + 4.0, player[1], player[2]];
    let transform = |at: [f32; 3]| json!({"position": at, "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]});
    let entities = vec![
        json!({"name": "Checkpoint Respawn", "transform": transform(player)}),
        json!({
            "name": "Checkpoint 1",
            "classes": ["checkpoint"],
            "transform": transform(checkpoint),
            "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
            "rigid_body": {"kind": "Fixed", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
            "collider": {"shape": {"Box": {"half_extents": [0.5, 1.0, 0.5]}}, "friction": 0.0, "restitution": 0.0, "sensor": true},
        }),
    ];
    let below = [checkpoint[0], -20.0, checkpoint[2]];
    let scenario = json!({
        "name": "checkpoints: touching a checkpoint moves the respawn point, falling returns there",
        "ticks": 20,
        "steps": [
            {"tick": 1, "set": {"entity": "Player", "path": "/transform/position", "value": checkpoint}},
            {"tick": 5, "expect": {"entity": "Checkpoint Respawn", "path": "/transform/position", "equals": checkpoint, "tolerance": 0.01}},
            {"tick": 10, "set": {"entity": "Player", "path": "/transform/position", "value": below}},
            {"tick": 13, "until": 20, "expect": {"entity": "Player", "path": "/transform/position/1", "greater_than": -10.0}},
        ]
    });
    (entities, scenario)
}

#[cfg(test)]
#[path = "recipes/checkpoints.rs"]
mod checkpoints;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bevy_ecs::prelude::World;

    use crate::project::ProjectTemplate;
    use crate::project_runner::GameScene;
    use crate::runtime::{AppError, Plugin, ScheduleStage};
    use crate::App;

    struct Checkpoints;

    impl Plugin for Checkpoints {
        fn build(&self, app: &mut App) -> Result<(), AppError> {
            app.add_system(ScheduleStage::Update, |world: &mut World| {
                super::checkpoints::checkpoints(&mut GameScene { world });
            });
            Ok(())
        }
    }

    #[test]
    fn checkpoint_recipe_applies_to_the_player_templates_and_its_scenario_passes(
    ) {
        for template in [
            ProjectTemplate::FirstPerson3d,
            ProjectTemplate::ThirdPerson3d,
            ProjectTemplate::Platformer2d,
        ] {
            let parent = std::env::temp_dir()
                .join(format!("rusting-recipe-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&parent).unwrap();
            let project =
                crate::project::create_project_from(&parent, "g", template)
                    .unwrap();
            let root = project.root.as_path();
            let dry = crate::cli::apply_recipe(root, "checkpoints", true);
            assert!(dry.ok, "{dry:?}");
            assert!(!root.join("src/checkpoints.rs").exists());
            let applied = crate::cli::apply_recipe(root, "checkpoints", false);
            assert!(applied.ok, "{applied:?}");
            assert_eq!(
                std::fs::read_to_string(root.join("src/checkpoints.rs"))
                    .unwrap(),
                super::RECIPES[0].source
            );
            let again = crate::cli::apply_recipe(root, "checkpoints", false);
            assert_eq!(again.diagnostics[0].code, "PROJECT_EXISTS");
            crate::project_runner::run_project_scenario(
                project.scene_path.clone(),
                Checkpoints,
                root.join("tests/checkpoints.json"),
                Some(root.join("report.json")),
            )
            .unwrap_or_else(|error| panic!("{template:?}: {error}"));
            let _ = std::fs::remove_dir_all(&parent);
        }
        let unknown = crate::cli::apply_recipe(Path::new("."), "nope", false);
        assert_eq!(unknown.diagnostics[0].code, "RECIPE_UNKNOWN");
    }
}
