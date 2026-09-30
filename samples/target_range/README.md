# Target Range

A small first-person shooting gallery. Six red discs stand down the range,
two of them sliding back and forth. Aim with the mouse and fire with the left
button or F; each hit removes the disc and bursts sparks. Clear all six and the
HUD shows how many shots it took.

The game was built with the `rusting` CLI alone: `rusting new --template
first-person`, one scene patch, fifteen lines of Rust and three scenarios. It
is the second dogfooding game for the agent workflow, and the first that needs
custom input and ray casts.

- `scenes/main.rscene`: targets (static and `rusting.tween` sliders), the
  `fire` action (`rusting.input_action`), the `hits` and `shots` counters, the
  score, crosshair and win HUD, and a hidden burst emitter.
- `src/main.rs`: on `fire`, counts the shot, casts along the camera with
  `GameScene::aim`, despawns the target it hits and bursts at the hit point.
- `tests/`: `first_shot` hits the centre target, `miss` fires at the sky,
  `clear_range` turns the player with `set` steps and clears all six.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/target_range
rusting test samples/target_range samples/target_range/tests
```
