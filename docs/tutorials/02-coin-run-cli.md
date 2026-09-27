# Tutorial 2: Coin Run from the command line

You will create a complete 2D platformer, play it, change it without opening
the editor, catch the change breaking the level with an automated test,
take a screenshot, and export a finished build. Every step uses the
`rusting` CLI, so the same loop works in scripts, CI, and for coding agents.

In the commands below, `rusting` means `./target/debug/rusting` from the
engine checkout (build it with `cargo build --bin rusting`). Add `--json` to
any command for machine-readable output.

## 1. Create the game

```bash
mkdir -p ~/games && cd ~/games
rusting new . "Coin Run" --template starter
```

The starter template is a finished game made only of scene data. Nothing in
`src/main.rs` is game-specific:

- **Player**: a kinematic body with `rusting.platformer_controller`. It reads
  the `player.left`, `player.right`, and `player.jump` actions (A/D or the
  arrow keys, and Space).
- **Level**: a `rusting.tile_map`. Each text row is a row of tiles; solid
  runs become box colliders.
- **Coin 1** to **Coin 5** and **Flag**: `rusting.pickup` components. Each
  coin adds 1 to the `coins` counter and vanishes with a
  `rusting.burst_emitter` sparkle. The flag `requires` the `coins` counter to
  be complete.
- **Score**: a `rusting.counter` (target 5) and a `rusting.hud` line with the
  text `Coins {coins}/5`.
- **Win**: a HUD element that appears once the flag is collected.

List the entities and their IDs:

```bash
rusting scene query "Coin Run/scenes/main.rscene" --component rusting.pickup
```

```text
Matches: 6
23ed39b2-5570-4c30-96cb-307ed4f6c346 Coin 4
...
9b072a85-b675-47d9-ae11-a1c0f9d9fd61 Flag
Revision: 848322f96b4e1562
```

Your IDs will differ: every new project gets fresh ones. Add `--json` to see
every component's fields.

## 2. Play it

```bash
rusting run "Coin Run"
```

Collect the five coins, then touch the flag. The first build takes a few
minutes.

## 3. Test that the level can be won

The engine repository has a scenario that plays the level:
`samples/starter_game/win.scenario.json`. Copy it into the project:

```bash
cp <engine>/samples/starter_game/win.scenario.json "Coin Run/"
rusting test "Coin Run" "Coin Run/win.scenario.json"
```

```text
Scenario "coin run: collect every coin, then win": passed after 240 ticks, seed 1
```

The scenario holds `player.right` from tick 1, presses `player.jump` at four
ticks, and checks the game along the way:

```json
{"tick": 25, "expect": {"entity": "Coin 1", "path": "/components/rusting.pickup/collected", "equals": true}},
{"tick": 190, "expect": {"entity": "Score", "path": "/components/rusting.counter/value", "equals": 5}},
{"tick": 235, "expect": {"entity": "Win", "path": "/components/rusting.counter/value", "equals": 1}}
```

A path is a JSON pointer into the entity as `scene query --json` shows it,
with component fields parsed. The run has no window and one tick per
update, so it gives the same result on every machine.

## 4. Change the game with a scene patch

Make the player jump higher. Put the Player's ID in `jump.json`:

```json
{
  "operations": [
    {
      "op": "set",
      "id": "PLAYER-ID",
      "path": "/components/rusting.platformer_controller/jump_speed",
      "value": 10.0,
      "expected": 8.0
    }
  ]
}
```

Preview it, then apply it:

```bash
rusting scene patch "Coin Run/scenes/main.rscene" jump.json --dry-run
rusting scene patch "Coin Run/scenes/main.rscene" jump.json
```

```text
Patched a23c9aec82c1cca7 -> 840dfb80d693e5e0: 0 created, 0 deleted, 1 field changes
  Player/components/rusting.platformer_controller/jump_speed: 8.0 -> 10.0
```

A patch applies every operation or none. `expected` makes it fail with
`SCENE_CONFLICT` if someone changed the value since you read it; a top-level
`"expected_revision"` does the same for the whole file. Other operations
are `create`, `duplicate`, `reparent`, `remove` (a component), and `delete`
(an entity). See the [README](../../README.md#scene-patches).

If the editor has the scene open, it reloads the file behind an Undo step.

## 5. Let the test catch the regression

Run the scenario again:

```bash
rusting test "Coin Run" "Coin Run/win.scenario.json"
```

```text
error [SCENARIO_FAILED]: tick 235 step 13: `Win` /components/rusting.counter/value is 0, expected 1 ± 0
```

The higher jump changes the player's path, and the recorded inputs no
longer win the level. The failure names the first failing tick and step and
the value it saw. Put the jump back by applying the patch in reverse
(`"value": 8.0, "expected": 10.0`) and the test passes again.

## 6. Restyle it

Presets write a look as ordinary scene data (sun, ambient and sky light,
tone mapping, background, camera, and HUD style): `daylight`,
`golden_hour`, `night`, and `flat_toy`.

```bash
rusting preset apply "Coin Run/scenes/main.rscene" golden_hour --dry-run
```

The dry run lists each field it would change. Presets are tuned for lit 3D
scenes, so check the result on 2D unlit art before you keep it.

## 7. Take a screenshot

```bash
rusting capture "Coin Run/scenes/main.rscene" shot.png --tick 60 --size 960x540 --pick 367,305
```

```text
Pick [367,305]: 15dd1915-... Coin 1
Output: shot.png
```

`capture` simulates 60 ticks and renders the active camera with the HUD.
Each `--pick X,Y` names the object under that pixel. Game code does not run
during a capture. A scenario step `{"tick": 240, "capture": "shots/win.png"}`
takes a screenshot during a test instead, with game code running.

## 8. Export

```bash
rusting export "Coin Run" ~/exports
```

```text
Exported: /home/you/exports/coin_run_export
Game exit code: 0, timed out: false
```

`export` builds a release, copies the executable, cooked scene, assets,
license, and a README into a new folder, then runs the copy for 60 headless
ticks from a fresh temporary folder to prove it starts. An existing export is
never overwritten; the next one gets `_2`.

## What you learned

- Built-in gameplay components: platformer controller, tile map, pickups,
  counters, HUD, particle bursts.
- `scene query` and `scene patch` for exact, reviewable edits.
- Scenarios as regression tests.
- `capture` and `export`.

Next, [Tutorial 3](03-gameplay-plugin.md) adds your own Rust systems and
components to this game.
