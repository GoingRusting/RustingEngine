# Lantern Grid

A lights-out puzzle. Click a lantern to flip it and its four neighbours
between lit and dark. Light all 25 lanterns in as few moves as you can. R
starts over.

The game was built with the `rusting` CLI alone: `rusting new --template 3d`,
two scene patches, about seventy lines of Rust and two scenarios. It is the
tenth dogfooding game for the agent workflow and the first to recolor
objects from game code and to click objects through an orthographic camera.

- `scenes/main.rscene`: the board, 25 lantern cells named `Cell <column>
  <row>` with fixed colliders, the top-down orthographic camera, the
  `select` and `restart` input actions, the counters and HUD.
- `src/main.rs`: scrambles the full board once per round by flipping three
  fixed cells, finds the clicked cell with `pointer_ray` and `raycast`,
  flips it and its neighbours with `set_color` and `set_emissive`, and
  counts the lit lanterns.
- `tests/`: `center` checks the scramble and one click in the middle;
  `solve` clicks the three scrambled cells, wins, and restarts.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/lantern_grid
rusting test samples/lantern_grid
```
