# Writing a plugin

A plugin is a bundle of systems, components, resources and input actions
that a game adds in one call. [Tutorial 3](tutorials/03-gameplay-plugin.md)
writes one inside a game. This page is about plugins meant for more than one
game: a health system, a dialogue box, a camera rig you reuse.

Plugins run in the game. There is no editor plugin API yet: a plugin cannot
add editor panels, gizmos or importers.

## A plugin crate

Put the plugin in its own library crate next to your games:

```toml
[package]
name = "wobble"
version = "0.1.0"
edition = "2021"

[dependencies]
rusting_engine = { path = "../RustingEngine" }
bevy_ecs = "0.19"
serde = { version = "1", features = ["derive"] }
```

Use the same engine as the games that load the plugin, and keep `bevy_ecs`
on the engine's minor version (0.19), so everything shares one copy of each
crate.

```rust
use bevy_ecs::prelude::*;
use rusting_engine::prelude::*;
use rusting_engine::runtime::{App, AppError, Plugin, ScheduleStage};
use serde::{Deserialize, Serialize};

/// Bobs an object up and down. Saved in scenes as `wobble.bob`.
#[derive(Component, Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bob {
    pub height: f32,
    pub speed: f32,
    #[serde(skip)]
    phase: f32,
}

rusting_engine::reflect! {
    struct Bob {
        height: f32 { unit: "m", min: 0.0 },
        speed: f32 { unit: "rad/s" },
        #[skip]
        phase: f32,
    }
}

fn bob(time: Res<FrameTime>, mut objects: Query<(&mut Bob, &mut Transform)>) {
    let step = time.fixed_delta.as_secs_f32();
    for (mut bob, mut transform) in &mut objects {
        let before = bob.phase.sin() * bob.height;
        bob.phase += bob.speed * step;
        transform.position[1] += bob.phase.sin() * bob.height - before;
    }
}

pub struct WobblePlugin;

impl Plugin for WobblePlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.register_scene_component::<Bob>("wobble.bob")
            .map_err(|error| AppError::PluginSetup {
                plugin: self.name(),
                message: error.to_string(),
            })?;
        app.add_systems(ScheduleStage::FixedUpdate, bob);
        Ok(())
    }
}
```

A game adds it from its own plugin's `build`:

```rust
app.add_plugin(wobble::WobblePlugin)?;
```

Adding the same plugin twice fails with `AppError::DuplicatePlugin`, so a
plugin that needs another can add it, and a game that adds both gets a
clear error instead of systems running twice.

## Rules for reusable plugins

- **Prefix every name with the plugin's.** Component names (`wobble.bob`),
  action names (`wobble.toggle`), signal handlers and data asset types share
  one namespace with the game and other plugins. `rusting.` belongs to the
  engine.
- **Simulation goes in `FixedUpdate`.** Anything that changes game state
  steps on the fixed tick by `FrameTime::fixed_delta`, so replays and
  scenario tests repeat. Effects that only change the picture can go in
  `Update` with `FrameTime::delta_seconds`.
- **Stay deterministic.** Visit entities in a stable order (sort by a key
  or use `ClassIndex`, which returns ascending entity order), never read a
  global or thread-local random number generator, and do not accumulate
  simulation state through order-dependent atomics. See
  [Determinism](determinism.md).
- **Describe saved components with `reflect!`.** The description drives
  scene saving, the editor's Inspector, `rusting schema` and scenario field
  paths. A field you rename later needs a `FieldMigration`, or old scenes
  stop loading.
- **Report setup errors.** Return `AppError::PluginSetup` with the plugin's
  name instead of panicking, so the game's start-up error says which plugin
  failed.
- **Bind actions, not keys.** Bind defaults on the `ActionMap` resource in
  `build`, and read them with `just_pressed` and `pressed`. Players can
  rebind them and scenarios can press them by name.

## Testing a plugin

A plugin runs without a window. Unit-test its systems on a bare `App`:

```rust
#[test]
fn a_bob_moves_its_object() {
    let mut app = App::new();
    app.add_plugin(WobblePlugin).unwrap();
    let entity = app
        .world_mut()
        .spawn((Bob { height: 1.0, speed: 1.0, ..Bob::default() }, Transform::default()))
        .id();
    app.update(std::time::Duration::from_millis(100)).unwrap();
    let transform = app.world().get::<Transform>(entity).unwrap();
    assert!(transform.position[1] > 0.0);
}
```

For behaviour in a real scene, add the plugin to a small test game and
write a scenario: `rusting test <project>` presses actions, waits ticks,
and checks component fields such as `/components/wobble.bob/height`. See
"Tests you can run without a screen" in [Core concepts](concepts.md).
