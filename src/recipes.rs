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
}, Recipe {
    name: "health",
    summary: "The counter `health` starts at 3; touching an object in class `hazard` costs one point, flashes the player and makes it safe for a second, and at 0 the round restarts. Adds a sensor `Hazard 1` 2.5 units along +X from the player.",
    source: include_str!("recipes/health.rs"),
    call: "health::health(scene, time);",
    build: health_scene,
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

fn health_scene(player: [f32; 3]) -> (Vec<Value>, Value) {
    let hazard = [player[0] + 2.5, player[1], player[2]];
    let entities = vec![json!({
        "name": "Hazard 1",
        "classes": ["hazard"],
        "transform": {"position": hazard, "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
        "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
        "rigid_body": {"kind": "Fixed", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
        "collider": {"shape": {"Box": {"half_extents": [0.5, 1.0, 0.5]}}, "friction": 0.0, "restitution": 0.0, "sensor": true},
    })];
    // One hit a second while the player stands in the hazard: 3, 2, 1, then
    // the restart refills health and puts the player back at the start.
    let scenario = json!({
        "name": "health: a hazard costs one point a second and 0 health restarts the round",
        "ticks": 160,
        "steps": [
            {"tick": 1, "set": {"entity": "Player", "path": "/transform/position", "value": hazard}},
            {"tick": 30, "expect": {"counter": "health", "equals": 2}},
            {"tick": 90, "expect": {"counter": "health", "equals": 1}},
            {"tick": 160, "expect": {"counter": "health", "equals": 3}},
            {"tick": 160, "expect": {"entity": "Player", "path": "/transform/position/0", "less_than": hazard[0] - 1.5}},
        ]
    });
    (entities, scenario)
}

#[cfg(test)]
#[path = "recipes/checkpoints.rs"]
mod checkpoints;

#[cfg(test)]
#[path = "recipes/health.rs"]
mod health;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bevy_ecs::prelude::World;

    use crate::project::ProjectTemplate;
    use crate::project_runner::GameScene;
    use crate::runtime::{AppError, FrameTime, Plugin, ScheduleStage};
    use crate::App;

    /// Runs a recipe's call as a game's `update` would.
    struct RecipeUpdate(fn(&mut World));

    impl Plugin for RecipeUpdate {
        fn build(&self, app: &mut App) -> Result<(), AppError> {
            app.add_system(ScheduleStage::Update, self.0);
            Ok(())
        }
    }

    /// Applies the recipe `name` to each player template (dry run first,
    /// then for real, then again to see it refused) and runs its scenario
    /// with `update` compiled in.
    fn applies_and_passes(name: &str, update: fn(&mut World)) {
        let recipe = super::recipe(name).unwrap();
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
            let source = root.join(format!("src/{name}.rs"));
            let dry = crate::cli::apply_recipe(root, name, true);
            assert!(dry.ok, "{dry:?}");
            assert!(!source.exists());
            let applied = crate::cli::apply_recipe(root, name, false);
            assert!(applied.ok, "{applied:?}");
            assert_eq!(
                std::fs::read_to_string(&source).unwrap(),
                recipe.source
            );
            let again = crate::cli::apply_recipe(root, name, false);
            assert_eq!(again.diagnostics[0].code, "PROJECT_EXISTS");
            crate::project_runner::run_project_scenario(
                project.scene_path.clone(),
                RecipeUpdate(update),
                root.join(format!("tests/{name}.json")),
                Some(root.join("report.json")),
            )
            .unwrap_or_else(|error| panic!("{name} {template:?}: {error}"));
            let _ = std::fs::remove_dir_all(&parent);
        }
    }

    #[test]
    fn checkpoint_recipe_applies_to_the_player_templates_and_its_scenario_passes(
    ) {
        applies_and_passes("checkpoints", |world| {
            super::checkpoints::checkpoints(&mut GameScene { world });
        });
        let unknown = crate::cli::apply_recipe(Path::new("."), "nope", false);
        assert_eq!(unknown.diagnostics[0].code, "RECIPE_UNKNOWN");
    }

    #[test]
    fn health_recipe_applies_to_the_player_templates_and_its_scenario_passes() {
        applies_and_passes("health", |world| {
            let time = *world.resource::<FrameTime>();
            super::health::health(&mut GameScene { world }, &time);
        });
    }
}
