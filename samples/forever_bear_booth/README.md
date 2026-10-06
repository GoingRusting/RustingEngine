# Forever Bear Booth

A first-person night shift in a toy store, the opening of the horror game
FOREVER BEAR. You sit in the security booth with six CCTV monitors. Shelf
bears repeat their line in pitched child voices, one after another, and
Big Button, the store mascot, walks the aisles. You hear its steps before
you find it on a screen.

The sample shows the engine features the game needs, built with the
`rusting` CLI and about 230 lines of Rust:

- `scenes/main.rscene`: the booth and desk, six monitors with
  `rusting.camera_screen` (320x180, every other frame, a CRT `grading` with
  scanlines, grain, color bleed and a rolling noise band), six inactive
  store cameras, four shelf rows, the hidden bear template, Big Button and
  the `night` art preset.
- `src/main.rs`: copies 192 bears along the shelves, draws a "CAM n" plate
  under each monitor with `text_texture`, walks Big Button through the
  aisles on a `WaypointGraph` as a pure function of the tick, plays its
  steps as occluded 3D sounds with top priority, and plays one bear at a
  time at a random pitch on the `bears` bus, which has reverb and a
  low-pass filter. Each bear line has a caption.
- `assets/`: two short sounds made for the sample, a two-syllable "hello"
  and a heavy footstep.
- `tests/night.json`: checks Big Button's route at three ticks, the bear
  and step counts, the bus effects and that no voice was dropped, writes
  the whole mix to `tests/shots/night.wav`, and captures the booth.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/forever_bear_booth
rusting test samples/forever_bear_booth
```
