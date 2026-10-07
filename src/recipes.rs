//! Named gameplay mechanics that `rusting recipe apply` writes into a
//! project: a Rust source file, the scene changes it needs, and a scenario
//! that proves it works.

use serde_json::{json, Value};

use crate::runtime::{PlatformerController, PlayerController, SceneEntity};

/// Patch operations and a scenario, or why the player cannot take the
/// recipe.
pub type Built = Result<(Vec<Value>, Value), String>;

/// One recipe. The scene changes and scenario are built from the object
/// named `Player` so the scenario passes in any template.
pub struct Recipe {
    /// `snake_case` name; also the source file and scenario name.
    pub name: &'static str,
    /// One sentence on what the mechanic does.
    pub summary: &'static str,
    /// Contents of `src/<name>.rs`; `None` when the scene change is the
    /// whole mechanic.
    pub source: Option<&'static str>,
    /// The call `update` makes once a frame, when there is a source.
    pub call: Option<&'static str>,
    /// Scene patch operations and the scenario, given the object named
    /// `Player`, or why that player cannot take the recipe.
    pub build: fn(&SceneEntity) -> Built,
}

/// Every recipe, in `recipe list` order.
pub const RECIPES: &[Recipe] = &[Recipe {
    name: "checkpoints",
    summary: "Touching an object in class `checkpoint` stores it as the respawn point; falling below y = -10 puts the player back there. Adds `Checkpoint Respawn` at the player and a sensor `Checkpoint 1` four units along +X.",
    source: Some(include_str!("recipes/checkpoints.rs")),
    call: Some("checkpoints::checkpoints(scene);"),
    build: checkpoint_scene,
}, Recipe {
    name: "health",
    summary: "The counter `health` starts at 3; touching an object in class `hazard` costs one point, flashes the player and makes it safe for a second, and at 0 the round restarts. Adds a sensor `Hazard 1` 2.5 units along +X from the player.",
    source: Some(include_str!("recipes/health.rs")),
    call: Some("health::health(scene, time);"),
    build: health_scene,
}, Recipe {
    name: "double_jump",
    summary: "Sets `air_jumps` to 1 on the player's controller (`rusting.player_controller` or `rusting.platformer_controller`), so a second jump press in the air jumps again; landing refills it. No source file.",
    source: None,
    call: None,
    build: double_jump_scene,
}, Recipe {
    name: "inventory",
    summary: "Touching an object in class `item` picks it up into the counter `inventory_<kind>` (`Key 1` adds to `inventory_key`); touching one in class `locked` while holding a key spends the key and removes it. `inventory::count` and `inventory::take` read and spend items. Adds sensors `Key 1` 2.5 units and `Door 1` 5 units along +X from the player; set the door's `sensor` to false to make it block.",
    source: Some(include_str!("recipes/inventory.rs")),
    call: Some("inventory::inventory(scene);"),
    build: inventory_scene,
}, Recipe {
    name: "day_timer",
    summary: "The counter `day` starts at 1 and goes up every 30 seconds (`DAY_SECONDS`); `day_seconds_left` counts down to the next day. Adds the HUD label `Day Clock` at the top of the screen showing both.",
    source: Some(include_str!("recipes/day_timer.rs")),
    call: Some("day_timer::day_timer(scene, time);"),
    build: day_timer_scene,
}, Recipe {
    name: "pause_menu",
    summary: "The action `menu` (Escape, gamepad Start) pauses the game and shows a HUD menu; pressing it again or clicking `Pause Resume` carries on, and `Pause Quit` closes the game. Adds the input action `Pause Action` and the hidden HUD objects `Pause Title`, `Pause Resume` and `Pause Quit`.",
    source: Some(include_str!("recipes/pause_menu.rs")),
    call: Some("pause_menu::pause_menu(scene);"),
    build: pause_menu_scene,
}, Recipe {
    name: "wave_spawner",
    summary: "When no enemies are left the counter `wave` goes up and 2 x wave copies of the hidden `Wave Enemy` (class `enemy`, kinematic) appear in a row from `Wave Spawn`; game code defeats an enemy by despawning it, and one that falls below y = -10 is removed. `wave_enemies_left` counts the rest. Adds `Wave Spawn` 3 units along +X and 1 up from the player, and the template far below the level.",
    source: Some(include_str!("recipes/wave_spawner.rs")),
    call: Some("wave_spawner::wave_spawner(scene);"),
    build: wave_spawner_scene,
}, Recipe {
    name: "turret",
    summary: "`Turret` shoots the nearest object in class `enemy` within 12 m that it can see, every half second: the shot removes it and adds to the counter `turret_kills`. Adds `Turret` 3 units above the player and red enemies `Target 1`, `Target 2` and `Target Far` 4, 6 and 20 units along +X from it; delete the targets once real enemies exist.",
    source: Some(include_str!("recipes/turret.rs")),
    call: Some("turret::turret(scene, time);"),
    build: turret_scene,
}];

/// The recipe called `name`.
#[must_use]
pub fn recipe(name: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|recipe| recipe.name == name)
}

/// The player's position, or the origin when it has no transform.
fn position(player: &SceneEntity) -> [f32; 3] {
    player.transform.map_or([0.0; 3], |t| t.position)
}

/// `create` operations for `entities`.
fn creates(entities: Vec<Value>) -> Vec<Value> {
    entities
        .into_iter()
        .map(|entity| json!({"op": "create", "entity": entity}))
        .collect()
}

/// A fixed sensor box called `name` in `class` at `at`.
fn sensor(name: &str, class: &str, at: [f32; 3]) -> Value {
    json!({
        "name": name,
        "classes": [class],
        "transform": {"position": at, "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
        "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
        "rigid_body": {"kind": "Fixed", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
        "collider": {"shape": {"Box": {"half_extents": [0.5, 1.0, 0.5]}}, "friction": 0.0, "restitution": 0.0, "sensor": true},
    })
}

/// A lit cube of `color` for `mesh_renderer`.
fn cube(color: [f32; 4]) -> Value {
    json!({
        "mesh": {"BuiltinPrimitive": "Cube"},
        "material": {"Inline": {
            "model": "Pbr", "alpha_mode": "Opaque", "base_color": color,
            "emissive": [0.0, 0.0, 0.0], "metallic": 0.0, "roughness": 0.8,
            "base_color_texture": null, "normal_texture": null,
            "metallic_roughness_texture": null,
            "occlusion_texture": null, "emissive_texture": null
        }},
        "cast_shadows": true, "receive_shadows": true
    })
}

/// A red kinematic box called `name` in class `enemy` at `at`.
fn enemy(name: &str, at: [f32; 3]) -> Value {
    json!({
        "name": name,
        "classes": ["enemy"],
        "transform": {"position": at, "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]},
        "mesh_renderer": cube([0.85, 0.2, 0.2, 1.0]),
        "physics_body": {"simulation": "Cpu", "solver": "Simplified", "custom_shader": null},
        "rigid_body": {"kind": "Kinematic", "mass": 1.0, "linear_velocity": [0.0, 0.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0},
        "collider": {"shape": {"Box": {"half_extents": [0.5, 0.5, 0.5]}}, "friction": 0.5, "restitution": 0.0, "sensor": false},
    })
}

fn checkpoint_scene(player: &SceneEntity) -> Built {
    let player = position(player);
    let checkpoint = [player[0] + 4.0, player[1], player[2]];
    let transform = |at: [f32; 3]| json!({"position": at, "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]});
    let entities = vec![
        json!({"name": "Checkpoint Respawn", "transform": transform(player)}),
        sensor("Checkpoint 1", "checkpoint", checkpoint),
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
    Ok((creates(entities), scenario))
}

fn health_scene(player: &SceneEntity) -> Built {
    let player = position(player);
    let hazard = [player[0] + 2.5, player[1], player[2]];
    let entities = vec![sensor("Hazard 1", "hazard", hazard)];
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
    Ok((creates(entities), scenario))
}

fn double_jump_scene(player: &SceneEntity) -> Built {
    let component = |name: &str| player.components.get(name);
    // Jump speed and gravity give the height of one jump and when it peaks.
    let (key, jump_speed, gravity) = if let Some(text) =
        component("rusting.player_controller")
    {
        let c: PlayerController =
            serde_json::from_str(text).map_err(|e| e.to_string())?;
        ("rusting.player_controller", c.jump_speed, c.gravity)
    } else if let Some(text) = component("rusting.platformer_controller") {
        let c: PlatformerController =
            serde_json::from_str(text).map_err(|e| e.to_string())?;
        ("rusting.platformer_controller", c.jump_speed, c.gravity)
    } else {
        return Err("`Player` has no `rusting.player_controller` or `rusting.platformer_controller`".into());
    };
    if jump_speed <= 0.0 || gravity <= 0.0 {
        return Err(format!(
            "`Player` jumps at {jump_speed} m/s under gravity {gravity}; both must be above 0"
        ));
    }
    // The ground is at or below where the player starts (a template may
    // drop it a little first), so one jump never climbs above this.
    let start = position(player)[1];
    let one_jump = jump_speed * jump_speed / (2.0 * gravity);
    // Second press just before the first jump peaks.
    let peak_ticks = (jump_speed / gravity * 60.0 * 0.9).round() as u32;
    let second = 20 + peak_ticks;
    let operations = vec![json!({
        "op": "set", "id": "Player",
        "path": format!("/components/{key}/air_jumps"), "value": 1,
    })];
    let scenario = json!({
        "name": "double_jump: a second press in the air jumps higher than one jump can",
        "ticks": second + 4 * peak_ticks,
        "steps": [
            {"tick": 20, "press": "player.jump"},
            {"tick": 22, "release": "player.jump"},
            {"tick": second, "press": "player.jump"},
            {"tick": second + 2, "release": "player.jump"},
            {"tick": second + 2, "within": second + 2 * peak_ticks, "expect": {"entity": "Player", "path": "/transform/position/1", "greater_than": start + 1.1 * one_jump}},
        ]
    });
    Ok((operations, scenario))
}

fn inventory_scene(player: &SceneEntity) -> Built {
    let player = position(player);
    let key = [player[0] + 2.5, player[1], player[2]];
    let door = [player[0] + 5.0, player[1], player[2]];
    let entities = vec![
        sensor("Key 1", "item", key),
        sensor("Door 1", "locked", door),
    ];
    // The door stays shut without the key, the key is picked up once, and
    // the door then opens and spends it.
    let scenario = json!({
        "name": "inventory: a key is picked up and spent to open the locked door",
        "ticks": 40,
        "steps": [
            {"tick": 1, "set": {"entity": "Player", "path": "/transform/position", "value": door}},
            {"tick": 10, "expect": {"entity": "Door 1", "path": "/transform/position/0", "exists": true}},
            {"tick": 12, "set": {"entity": "Player", "path": "/transform/position", "value": key}},
            {"tick": 20, "expect": {"counter": "inventory_key", "equals": 1}},
            {"tick": 20, "expect": {"entity": "Key 1", "path": "/transform/position/0", "exists": false}},
            {"tick": 22, "set": {"entity": "Player", "path": "/transform/position", "value": door}},
            {"tick": 30, "expect": {"entity": "Door 1", "path": "/transform/position/0", "exists": false}},
            {"tick": 30, "expect": {"counter": "inventory_key", "equals": 0}},
        ]
    });
    Ok((creates(entities), scenario))
}

fn day_timer_scene(_: &SceneEntity) -> Built {
    let hud = json!({"text": "Day {day}  {day_seconds_left}s", "anchor": "Top", "offset": [0.0, 24.0], "font_size": 28.0, "color": [1.0, 1.0, 1.0, 1.0], "button": false, "requires": null, "camera": null});
    let entities = vec![json!({
        "name": "Day Clock",
        "components": {"rusting.hud": hud},
    })];
    // The clock counts down from 30 s; moving the day's start 30 s back
    // then makes the next day begin at once.
    let scenario = json!({
        "name": "day_timer: the clock counts down and a new day begins after 30 seconds",
        "ticks": 200,
        "steps": [
            {"tick": 120, "expect": {"counter": "day", "equals": 1}},
            {"tick": 120, "expect": {"counter": "day_seconds_left", "equals": 28}},
            {"tick": 121, "set": {"counter": "day_started_ms", "value": -28000}},
            {"tick": 180, "expect": {"counter": "day", "equals": 2}},
            {"tick": 180, "expect": {"counter": "day_seconds_left", "equals": 29}},
        ]
    });
    Ok((creates(entities), scenario))
}

fn pause_menu_scene(player: &SceneEntity) -> Built {
    let hud = |text: &str, y: f32, size: f32, button: bool| json!({"text": text, "anchor": "Center", "offset": [0.0, y], "font_size": size, "color": [1.0, 1.0, 1.0, 1.0], "button": button, "requires": null, "camera": null});
    let hidden = |name: &str, hud: Value| json!({"name": name, "visible": false, "components": {"rusting.hud": hud}});
    let entities = vec![
        json!({"name": "Pause Action", "components": {"rusting.input_action": {"action": "menu", "inputs": ["Escape", "PadStart"]}}}),
        hidden("Pause Title", hud("Paused", -80.0, 48.0, false)),
        hidden("Pause Resume", hud("Resume", 0.0, 28.0, true)),
        hidden("Pause Quit", hud("Quit", 60.0, 28.0, true)),
    ];
    // Lifted while paused, the player hangs in the air; after Resume is
    // clicked it falls again.
    let lifted = position(player)[1] + 5.0;
    let scenario = json!({
        "name": "pause_menu: menu pauses the game and shows the menu, clicking Resume carries on",
        "ticks": 100,
        "steps": [
            {"tick": 10, "press": "menu"},
            {"tick": 11, "release": "menu"},
            {"tick": 15, "expect": {"entity": "Pause Title", "path": "/visible", "equals": true}},
            {"tick": 16, "set": {"entity": "Player", "path": "/transform/position/1", "value": lifted}},
            {"tick": 40, "expect": {"entity": "Player", "path": "/transform/position/1", "greater_than": lifted - 0.01}},
            {"tick": 41, "click": "Resume"},
            {"tick": 45, "expect": {"entity": "Pause Resume", "path": "/visible", "equals": false}},
            {"tick": 100, "expect": {"entity": "Player", "path": "/transform/position/1", "less_than": lifted - 1.0}},
        ]
    });
    Ok((creates(entities), scenario))
}

fn wave_spawner_scene(player: &SceneEntity) -> Built {
    let player = position(player);
    let spawn = [player[0] + 3.0, player[1] + 1.0, player[2]];
    let transform = |at: [f32; 3]| json!({"position": at, "rotation": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0]});
    let mut template = enemy("Wave Enemy", [0.0, -1000.0, 0.0]);
    template["visible"] = json!(false);
    let entities = vec![
        json!({"name": "Wave Spawn", "transform": transform(spawn)}),
        template,
    ];
    let exists = |name: &str, exists: bool| json!({"entity": name, "path": "/transform/position/0", "exists": exists});
    // Wave 1 has two enemies; once both fall out, wave 2 brings four.
    let scenario = json!({
        "name": "wave_spawner: a bigger wave comes when every enemy is gone",
        "ticks": 20,
        "steps": [
            {"tick": 5, "expect": {"counter": "wave", "equals": 1}},
            {"tick": 5, "expect": {"counter": "wave_enemies_left", "equals": 2}},
            {"tick": 5, "expect": {"entity": "Wave Enemy 1-2", "path": "/visible", "equals": true}},
            {"tick": 10, "set": {"entity": "Wave Enemy 1-1", "path": "/transform/position/1", "value": -20.0}},
            {"tick": 12, "expect": {"counter": "wave_enemies_left", "equals": 1}},
            {"tick": 13, "set": {"entity": "Wave Enemy 1-2", "path": "/transform/position/1", "value": -20.0}},
            {"tick": 18, "expect": {"counter": "wave", "equals": 2}},
            {"tick": 18, "expect": {"counter": "wave_enemies_left", "equals": 4}},
            {"tick": 18, "expect": exists("Wave Enemy 2-4", true)},
            {"tick": 18, "expect": exists("Wave Enemy 1-1", false)},
            {"tick": 18, "expect": {"entity": "Wave Enemy 2-1", "path": "/transform/position/0", "equals": spawn[0], "tolerance": 0.01}},
        ]
    });
    Ok((creates(entities), scenario))
}

fn turret_scene(player: &SceneEntity) -> Built {
    let player = position(player);
    let turret = [player[0], player[1] + 3.0, player[2]];
    let along = |x: f32| [turret[0] + x, turret[1], turret[2]];
    let entities = vec![
        json!({
            "name": "Turret",
            "classes": ["turret"],
            "transform": {"position": turret, "rotation": [0.0, 0.0, 0.0], "scale": [0.6, 0.6, 0.6]},
            "mesh_renderer": cube([0.3, 0.35, 0.4, 1.0]),
        }),
        enemy("Target 1", along(4.0)),
        enemy("Target 2", along(6.0)),
        enemy("Target Far", along(20.0)),
    ];
    let exists = |name: &str, exists: bool| json!({"entity": name, "path": "/transform/position/0", "exists": exists});
    // The near target goes first; the cooldown spares the second for half
    // a second; the far one is out of range.
    let scenario = json!({
        "name": "turret: shoots the nearest enemy in range, one every half second",
        "ticks": 90,
        "steps": [
            {"tick": 15, "expect": exists("Target 1", false)},
            {"tick": 15, "expect": exists("Target 2", true)},
            {"tick": 15, "expect": {"counter": "turret_kills", "equals": 1}},
            {"tick": 50, "expect": exists("Target 2", false)},
            {"tick": 90, "expect": exists("Target Far", true)},
            {"tick": 90, "expect": {"counter": "turret_kills", "equals": 2}},
        ]
    });
    Ok((creates(entities), scenario))
}

#[cfg(test)]
#[path = "recipes/checkpoints.rs"]
mod checkpoints;

#[cfg(test)]
#[path = "recipes/health.rs"]
mod health;

#[cfg(test)]
#[path = "recipes/wave_spawner.rs"]
mod wave_spawner;

#[cfg(test)]
#[path = "recipes/turret.rs"]
mod turret;

#[cfg(test)]
#[path = "recipes/pause_menu.rs"]
mod pause_menu;

#[cfg(test)]
#[path = "recipes/day_timer.rs"]
mod day_timer;

#[cfg(test)]
#[path = "recipes/inventory.rs"]
mod inventory;

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
            let scene = std::fs::read(&project.scene_path).unwrap();
            let dry = crate::cli::apply_recipe(root, name, true);
            assert!(dry.ok, "{dry:?}");
            assert!(!source.exists());
            assert_eq!(std::fs::read(&project.scene_path).unwrap(), scene);
            let applied = crate::cli::apply_recipe(root, name, false);
            assert!(applied.ok, "{applied:?}");
            assert_eq!(
                std::fs::read_to_string(&source).ok().as_deref(),
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

    #[test]
    fn double_jump_recipe_applies_to_the_player_templates_and_its_scenario_passes(
    ) {
        applies_and_passes("double_jump", |_| {});
        let no_controller: crate::runtime::SceneEntity = serde_json::from_value(
            serde_json::json!({"id": uuid::Uuid::nil(), "parent": null, "name": "Player"}),
        )
        .unwrap();
        let error =
            (super::recipe("double_jump").unwrap().build)(&no_controller);
        assert!(error.unwrap_err().contains("rusting.platformer_controller"));
    }

    #[test]
    fn inventory_recipe_applies_to_the_player_templates_and_its_scenario_passes(
    ) {
        applies_and_passes("inventory", |world| {
            super::inventory::inventory(&mut GameScene { world });
        });
    }

    #[test]
    fn day_timer_recipe_applies_to_the_player_templates_and_its_scenario_passes(
    ) {
        applies_and_passes("day_timer", |world| {
            let time = *world.resource::<FrameTime>();
            super::day_timer::day_timer(&mut GameScene { world }, &time);
        });
    }

    #[test]
    fn pause_menu_recipe_applies_to_the_player_templates_and_its_scenario_passes(
    ) {
        applies_and_passes("pause_menu", |world| {
            super::pause_menu::pause_menu(&mut GameScene { world });
        });
    }

    #[test]
    fn wave_spawner_recipe_applies_to_the_player_templates_and_its_scenario_passes(
    ) {
        applies_and_passes("wave_spawner", |world| {
            super::wave_spawner::wave_spawner(&mut GameScene { world });
        });
    }

    #[test]
    fn turret_recipe_applies_to_the_player_templates_and_its_scenario_passes() {
        applies_and_passes("turret", |world| {
            let time = *world.resource::<FrameTime>();
            super::turret::turret(&mut GameScene { world }, &time);
        });
    }
}
