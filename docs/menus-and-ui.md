# Menus and UI

The runtime UI is [egui](https://docs.rs/egui/0.31) 0.31, drawn over the game
view. Game code gets the context with `scene.ui()` every frame and uses the
egui version the engine re-exports as `rusting_engine::egui`, so the game
needs no egui dependency of its own.

## A main menu

Keep the current screen in a counter, so scenarios can `set`, `expect` and
`log` it:

```rust
use rusting_engine::egui;
use rusting_engine::prelude::*;

const MAIN: i32 = 0;
const PLAYING: i32 = 1;
const PAUSED: i32 = 2;

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    let ui = scene.ui();
    match scene.counter_value("screen") {
        MAIN => {
            egui::CentralPanel::default().show(&ui, |ui| {
                if ui.button("New Game").clicked() {
                    scene.set_counter("screen", PLAYING);
                }
                if ui.button("Quit").clicked() {
                    scene.quit();
                }
            });
        }
        PLAYING if scene.pressed("pause") => {
            scene.set_paused(true);
            scene.set_counter("screen", PAUSED);
        }
        PAUSED => {
            egui::Window::new("Paused").show(&ui, |ui| {
                if ui.button("Resume").clicked() {
                    scene.set_paused(false);
                    scene.set_counter("screen", PLAYING);
                }
            });
        }
        _ => {}
    }
}
```

- `scene.set_paused(true)` stops fixed ticks: physics, tweens, player
  controllers and emitters. The update function keeps running every frame,
  so the pause menu still draws and reads input.
- `scene.quit()` closes the window after the frame. A headless run stops,
  and a scenario ends its run.
- The keyboard works with no extra code. Tab and Shift+Tab move the focus
  between widgets, and Enter or Space clicks the focused one.
- A gamepad works too. The d-pad or left stick focuses the first widget,
  then moves the focus. South (A on an Xbox pad) clicks, and East (B) is
  Escape.
- In a window, egui keeps the clicks and keys it uses, so they do not reach
  `pressed`. A headless run passes every input to both egui and the game.
- `rusting.hud` entities with `button: true` are egui buttons too, and work
  the same way.

## Settings, saves and key rebinding

- `scene.save_data(key, text)` writes a file to the game's user data folder,
  and `scene.load_data(key)` reads it back. The key is a relative path, such
  as `settings.json` or `saves/1.json`.
- The folder is `~/.local/share/<game>` on Linux,
  `~/Library/Application Support/<game>` on macOS and `%APPDATA%\<game>` on
  Windows. The `RUSTING_USER_DATA` variable overrides it.
- Store values with `rusting_engine::serde_json`.
- `scene.counters()` lists every counter's name and value, so a save can
  hold them all.
- To rebind an action, read `scene.keys_pressed()` while a "press a key"
  prompt shows, then call `scene.rebind("serve", &[key])`. Gamepad
  buttons appear there too, as `PadSouth` and so on.
- A rebinding is not saved. Save it yourself, and call `rebind` again when
  the game starts.
- For UI that game code paints itself, `scene.cursor()` gives the cursor in
  pixels and `scene.viewport_size()` gives the view size.

## Video settings

```rust
scene.set_render_scale(0.75); // 3D at 75% of the window, stretched to fit
scene.set_vsync(true);
scene.set_max_fps(Some(60)); // None removes the cap
scene.set_fullscreen(true); // borderless, on the current monitor
scene.set_window_size([1280, 720]);
```

- The render scale goes from 0.25 to 2.0. Below 1 the game runs faster and
  looks softer; above 1 it supersamples. UI always draws at full resolution.
  Captures and scenario screenshots use the scale too.
- Fullscreen and window size apply after the frame. The platform can pick
  another window size; read `scene.viewport_size()` on a later frame.
  Headless runs ignore both.
- None of these are saved. Store them with `save_data` and set them again
  at startup.

## Testing menus

Scenarios drive egui the same way a player does:

```json
{"name": "menu", "ticks": 120, "capture_size": [640, 360],
 "files": {"saves/1.json": "fixtures/old_save.json"},
 "steps": [
   {"tick": 2, "click": "New Game"},
   {"tick": 4, "expect": {"counter": "screen", "equals": 1}},
   {"tick": 5, "tap": "pause"},
   {"tick": 7, "capture": "shots/paused.png"},
   {"tick": 8, "click": "Resume"},
   {"tick": 20, "restart": true},
   {"tick": 30, "click": "Quit"},
   {"tick": 31, "expect_quit": true}
 ]}
```

- `click` finds the topmost text the last frame drew with that label. It
  moves the cursor there, presses the left mouse button and releases it
  before the next tick, so egui sees one click. It works for egui buttons,
  for HUD buttons, and for text that game code painted. When nothing
  matches, the failure lists the texts on screen.
- `pointer` with `[x, y]` view fractions, plus a press of an action bound to
  `MouseLeft`, does the same thing by position.
- `tap` presses a key action for one tick. egui sees it too, so Tab and
  Enter navigate menus.
- The headless screen is `capture_size` from tick 0, for layout, egui and
  `viewport_size()` alike. Run the same scenario at 640×360 and 480×270 to
  check that small windows fit.
- Inputs on a tick run before the checks on that tick. Put an `expect` one
  tick or more after the `click` it depends on. A failed check names the
  inputs that ran on its tick.
- Each scenario gets an empty user data folder,
  `build/test-userdata/<scenario>/`. `files` copies fixtures into that folder
  before tick 0.
- `restart` reloads the starting scene. Files stay in place, so "save,
  restart, load" needs no hooks in game code.
- `expect_quit` checks that the game called `scene.quit()`. The run ends
  after the tick that quits, and steps at later ticks fail.

- Gamepad presses go through actions: bind `PadSouth` or `PadDpadDown` in a
  `rusting.input_action`, then `tap` the action. `left_stick` and
  `right_stick` steps tilt a stick.
