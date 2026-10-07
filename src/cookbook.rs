//! Task cookbook: short game code for one task each, served by
//! `rusting docs show cookbook/<name>`. Every snippet is compiled and run
//! against a template project in this module's tests, so the docs cannot
//! show code that does not work.

/// Snippet name and source, in `src/cookbook/`.
pub const COOKBOOK: &[(&str, &str)] = &[
    ("follow_player", include_str!("cookbook/follow_player.rs")),
    ("level_select", include_str!("cookbook/level_select.rs")),
];

#[cfg(test)]
#[path = "cookbook/follow_player.rs"]
mod follow_player;

#[cfg(test)]
#[path = "cookbook/level_select.rs"]
mod level_select;

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::World;
    use serde_json::{json, Value};

    use crate::project::{OpenProject as Project, ProjectTemplate};
    use crate::project_runner::GameScene;
    use crate::runtime::{AppError, FrameTime, Plugin, ScheduleStage};
    use crate::scene_patch::{patch_scene_file, ScenePatch};
    use crate::App;

    /// Runs a snippet as a game's `update` would.
    struct SnippetUpdate(fn(&mut World));

    impl Plugin for SnippetUpdate {
        fn build(&self, app: &mut App) -> Result<(), AppError> {
            app.add_system(ScheduleStage::Update, self.0);
            Ok(())
        }
    }

    fn project(template: ProjectTemplate) -> (std::path::PathBuf, Project) {
        let parent = std::env::temp_dir()
            .join(format!("rusting-cookbook-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&parent).unwrap();
        let project =
            crate::project::create_project_from(&parent, "g", template)
                .unwrap();
        (parent, project)
    }

    fn create(scene: &std::path::Path, entities: Vec<Value>) {
        let operations: Vec<Value> = entities
            .into_iter()
            .map(|entity| json!({"op": "create", "entity": entity}))
            .collect();
        let patch: ScenePatch =
            serde_json::from_value(json!({"operations": operations})).unwrap();
        patch_scene_file(scene, &patch, false).unwrap();
    }

    fn player_position(project: &Project) -> [f32; 3] {
        let text = std::fs::read_to_string(&project.scene_path).unwrap();
        let scene: Value = serde_json::from_str(&text).unwrap();
        let player = scene["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == "Player")
            .unwrap();
        serde_json::from_value(player["transform"]["position"].clone()).unwrap()
    }

    fn passes(
        project: &Project,
        scenario: &Value,
        update: fn(&mut World),
    ) -> Result<(), String> {
        let path = project.root.join("cookbook.json");
        std::fs::write(&path, scenario.to_string()).unwrap();
        crate::project_runner::run_project_scenario(
            project.scene_path.clone(),
            SnippetUpdate(update),
            path,
            Some(project.root.join("report.json")),
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    #[test]
    fn every_snippet_has_a_title_and_usage() {
        for (name, source) in super::COOKBOOK {
            let title = source.lines().next().unwrap();
            assert!(
                title.starts_with("//! ") && title.ends_with('.'),
                "{name}"
            );
            assert!(source.contains(&format!("{name}::")), "{name} usage");
        }
    }

    #[test]
    fn follow_player_walks_a_chaser_to_the_player_and_stops() {
        for template in [
            ProjectTemplate::FirstPerson3d,
            ProjectTemplate::ThirdPerson3d,
        ] {
            let (parent, project) = project(template);
            let p = player_position(&project);
            create(
                &project.scene_path,
                vec![json!({
                    "name": "Chaser",
                    "classes": ["chaser"],
                    "transform": {"position": [p[0] + 6.0, p[1], p[2]], "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
                    "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
                    "rigid_body": {"kind": "Kinematic", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
                    "collider": {"shape": {"Box": {"half_extents": [0.4, 0.4, 0.4]}}, "friction": 0.5, "restitution": 0.0, "sensor": false},
                })],
            );
            let x = |tick: u32, key: &str, value: f32| json!({"tick": tick, "expect": {"entity": "Chaser", "path": "/transform/position/0", key: p[0] + value}});
            let yaw = |key: &str, value: f32| json!({"tick": 60, "expect": {"entity": "Chaser", "path": "/transform/rotation/1", key: value}});
            // 3 m/s for one second leaves it about 3 m from the start,
            // facing -X (yaw a quarter turn); later it waits 1.5 m away.
            let scenario = json!({
                "name": "follow_player",
                "ticks": 240,
                "steps": [
                    x(60, "less_than", 3.5), x(60, "greater_than", 2.5),
                    yaw("greater_than", 1.5), yaw("less_than", 1.65),
                    x(240, "less_than", 1.55), x(240, "greater_than", 1.45),
                ]
            });
            passes(&project, &scenario, |world| {
                let time = *world.resource::<FrameTime>();
                super::follow_player::follow_player(
                    &mut GameScene { world },
                    &time,
                );
            })
            .unwrap_or_else(|error| panic!("{template:?}: {error}"));
            let _ = std::fs::remove_dir_all(&parent);
        }
    }

    #[test]
    fn level_select_loads_the_clicked_level_and_keeps_the_score() {
        let (parent, project) = project(ProjectTemplate::FirstPerson3d);
        let level_2 = project.root.join("scenes/level_2.rscene");
        std::fs::copy(&project.scene_path, &level_2).unwrap();
        create(&level_2, vec![json!({"name": "Level 2 Marker"})]);
        create(
            &project.scene_path,
            vec![json!({"name": "Level 2", "components": {"rusting.hud": {
                "text": "Level 2", "anchor": "Center", "offset": [0.0, 0.0],
                "font_size": 28.0, "color": [1.0, 1.0, 1.0, 1.0],
                "button": true, "requires": null, "camera": null,
            }}})],
        );
        let exists = |name: &str, exists: bool| json!({"entity": name, "path": "/visible", "exists": exists});
        let scenario = json!({
            "name": "level_select",
            "ticks": 20,
            "steps": [
                {"tick": 2, "set": {"counter": "score", "value": 7}},
                {"tick": 5, "expect": exists("Level 2 Marker", false)},
                {"tick": 6, "click": "Level 2"},
                {"tick": 12, "expect": exists("Level 2 Marker", true)},
                {"tick": 12, "expect": exists("Level 2", false)},
                {"tick": 12, "expect": {"counter": "score", "equals": 7}},
            ]
        });
        passes(&project, &scenario, |world| {
            super::level_select::level_select(&mut GameScene { world });
        })
        .unwrap();
        let _ = std::fs::remove_dir_all(&parent);
    }
}
