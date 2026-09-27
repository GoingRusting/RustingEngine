# Tutorial 3: Gameplay plugins

The `rusting_game!` API from Tutorial 1 is one function per frame. Real
games want ECS systems: code that queries components, reads input, and
runs at a chosen stage. This tutorial adds two systems to the Coin Run game
from [Tutorial 2](02-coin-run-cli.md):

- **Reset**: pressing `R` puts the player back at the start.
- **Spin**: a new `coin_run.spin` component, saved in the scene like any
  built-in one, that makes the flag spin.

The engine's ECS is [`bevy_ecs`](https://docs.rs/bevy_ecs/0.19). If you know
Bevy, systems, queries, and resources work the same way.

## 1. Add the dependencies

Your game uses `bevy_ecs` types directly, and `serde` to save your component.
Add both to `Coin Run/Cargo.toml` under `[dependencies]`:

```toml
bevy_ecs = "0.19"
serde = { version = "1", features = ["derive"] }
```

Keep `bevy_ecs` on the same minor version as the engine (0.19), so both use
one copy of the crate.

## 2. Write the plugin

Replace `Coin Run/src/main.rs`:

```rust
use bevy_ecs::prelude::*;
use rusting_engine::prelude::*;
use rusting_engine::project_runner::{resolve_game_scene_path, run_project};
use rusting_engine::runtime::{
    ActionMap, App, AppError, InputBinding, KeyCode, Name, Plugin,
    RuntimeInput, ScheduleStage,
};
use serde::{Deserialize, Serialize};

/// Spins an object around its Z axis. Saved in scenes as `coin_run.spin`.
#[derive(Component, Clone, Debug, Default, Serialize, Deserialize)]
struct Spin {
    speed: f32,
}

// Describes Spin's fields for scenes, the Inspector, and `rusting schema`.
rusting_engine::reflect! {
    struct Spin {
        speed: f32 { unit: "rad/s", doc: "around the Z axis" },
    }
}

fn spin(time: Res<FrameTime>, mut objects: Query<(&Spin, &mut Transform)>) {
    for (spin, mut transform) in &mut objects {
        transform.rotation[2] += spin.speed * time.delta_seconds();
    }
}

/// Puts the player back at the start when `game.reset` is pressed.
fn reset_player(
    input: Res<RuntimeInput>,
    actions: Res<ActionMap>,
    mut objects: Query<(&Name, &mut Transform)>,
) {
    if !actions.just_pressed(&input, "game.reset") {
        return;
    }
    for (name, mut transform) in &mut objects {
        if name.0 == "Player" {
            transform.position = [-9.5, 0.5, 0.0];
        }
    }
}

struct CoinRunPlugin;

impl Plugin for CoinRunPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.register_scene_component::<Spin>("coin_run.spin")
            .map_err(|error| AppError::PluginSetup {
                plugin: self.name(),
                message: error.to_string(),
            })?;
        app.world_mut()
            .resource_mut::<ActionMap>()
            .bind("game.reset", InputBinding::Key(KeyCode::KeyR));
        app.add_systems(ScheduleStage::Update, (spin, reset_player));
        Ok(())
    }
}

fn main() -> GameResult {
    let scene = resolve_game_scene_path(
        "build/main.rscene.bin",
        env!("CARGO_MANIFEST_DIR"),
    );
    run_project("Coin Run", scene, CoinRunPlugin)
}
```

What each part does:

- **`main`** replaces `rusting_game!`. `resolve_game_scene_path` finds the
  cooked scene next to an exported executable, or in the project during
  development. `run_project` takes a window title and your plugin; it also
  handles headless runs, scenario tests, and replays.
- **`Plugin::build`** runs once at startup. It registers the component,
  binds the action, and adds the systems.
- **`register_scene_component`** lets scenes store `Spin` under the name
  `coin_run.spin`. The type needs `Component`, `Serialize`, `Deserialize`,
  `Default`, and a `reflect!` description. Pick a prefix for your game;
  `rusting.` is the engine's.
- **`reflect!`** lists the fields scenes save, with optional hints: `unit`,
  `min` and `max` (floats), `doc`, and `color: true` for `[f32; 3]` or
  `[f32; 4]` colors. Mark `#[serde(skip)]` fields `#[skip]`. The macro checks
  the list against the struct, so a field added to one but not the other
  does not compile. Enums, nested structs, `Vec`, `Option`, string-keyed
  maps, `Entity` and `Handle<TextureAsset>` fields work too; an `Entity`
  saves as the target's object ID and a handle as `{"$asset": path}`. A
  reference to an object that was deleted saves as `null` and loads as
  `None` in an `Option<Entity>`, or as `Entity::PLACEHOLDER` otherwise.
- **Actions.** `ActionMap::bind` adds a binding to an action name, and
  `just_pressed(&input, name)` is true on the frame the key goes down.
  Also available: `held` and `just_released`. Binding more keys to the same
  action is fine; any of them triggers it.
- **Stages.** Both systems run in `Update`, once per frame, reading
  `FrameTime::delta_seconds()`. Put logic that must replay exactly (anything
  that feeds physics) in `ScheduleStage::FixedUpdate` and use
  `FrameTime::fixed_delta` instead. See [Core concepts](../concepts.md).

## 3. Put the component in the scene

Find the flag's ID and patch a `coin_run.spin` onto it:

```bash
rusting scene query "Coin Run/scenes/main.rscene" --name Flag
```

```json
{
  "operations": [
    {"op": "set", "id": "FLAG-ID", "path": "/components/coin_run.spin", "value": {"speed": 2.0}}
  ]
}
```

```bash
rusting scene patch "Coin Run/scenes/main.rscene" spin.json
```

```text
  Flag/components/coin_run.spin: null -> {"speed":2.0}
warning [PATCH_UNVALIDATED_COMPONENT]: component `coin_run.spin` is not built in; the game validates it on load
```

The CLI does not link your game code, so it cannot check your component's
fields. The game does, when it loads the scene: a wrong field or a component
you forgot to register stops it with an error that names the component.

The editor and `rusting capture` also run without your code. They keep your
components as saved data, so opening and saving the scene in the editor
leaves `coin_run.spin` intact, but the Inspector does not show it yet.

## 4. Run and test it

```bash
rusting run "Coin Run"
```

The flag spins. Walk right, press `R`, and the player is back at the start.

Scenarios press actions by name, including your own. Save this as
`Coin Run/reset.scenario.json`:

```json
{
  "name": "R puts the player back at the start",
  "seed": 1,
  "ticks": 60,
  "steps": [
    {"tick": 1, "press": "player.right"},
    {"tick": 40, "expect": {"entity": "Player", "path": "/transform/position/0", "greater_than": -8.0}},
    {"tick": 41, "press": "game.reset"},
    {"tick": 41, "expect": {"entity": "Player", "path": "/transform/position/0", "equals": -9.5, "tolerance": 0.01}},
    {"tick": 60, "expect": {"entity": "Flag", "path": "/transform/rotation/2", "greater_than": 1.0}}
  ]
}
```

```bash
rusting test "Coin Run" "Coin Run/reset.scenario.json"
```

```text
Scenario "R puts the player back at the start": passed after 60 ticks, seed 1
```

## Going further

- **Other stages**: `Startup` runs once before the first frame;
  `PostUpdate` runs after `Update`.
- **Resources**: `app.insert_resource(MyState::default())` in `build`, then
  `Res<MyState>` or `ResMut<MyState>` in a system.
- **Events**: read a frame's events with `Res<EventQueue<CollisionEvent>>`
  and `.iter()`. Other built-in events include `HudButtonPressed`,
  `SoundEvent`, and `ClickEvent`. Declare your own with
  `app.add_event::<MyEvent>()` and send them through
  `ResMut<EventQueue<MyEvent>>`.
- **Built-in components** you can query: `Transform`, `RigidBody`,
  `Collider`, `Counter`, `Pickup`, `HudElement`, `Tween`, and the rest in
  `rusting_engine::runtime`. `rusting schema` lists their scene fields.
- **Renaming a field**: scenes saved with the old name now fail to load
  with an error naming the component, the field's path, and the saved
  version. Register the change instead:
  `app.migrate_scene_component("coin_run.spin", FieldMigration::rename("speed", "turn_rate"))`
  (or `FieldMigration::remove("old_field")`), from `rusting_engine::reflect`.
  Each migration raises the component's version; newer saves record it as
  `"$version"`, and older ones run the migrations they missed.
- **Random numbers**: read the `RandomSeed` resource and draw with
  `seed.value(tick, stream)` or `seed.unit(tick, stream)`, so a replay or
  scenario with the same seed gets the same numbers.

Next, [Tutorial 4](04-gpu-cube-rain.md) moves thousands of bodies to the
GPU.
