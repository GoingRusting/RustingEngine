# Snake Trail

A snake game on a tile-map arena. Steer with the arrow keys or WASD, eat ten
apples, and do not run into a wall or your own tail. The snake speeds up as
it grows. R starts over.

The game was built with the `rusting` CLI alone: `rusting new --template 2d`,
one scene patch, about 120 lines of Rust and three scenarios. It is the
eleventh dogfooding game for the agent workflow and the first to move on a
grid tick by tick and to read walls from a `rusting.tile_map`.

- `scenes/main.rscene`: the `Arena` tile map (walls are `#` tiles), the
  head, two starting segments and a hidden `Segment` template, the apple,
  the orthographic camera, the steering and `restart` input actions, the
  counters and HUD.
- `src/main.rs`: turns on the next step (never straight back), moves every
  segment into the cell ahead of it, ends the round on a wall tile or a
  bite, and grows the snake with `spawn_copy` when it eats an apple. Apples
  appear at a fixed list of free spots.
- `tests/`: `eat` runs into the first apple and grows; `crash` checks that
  turning back is ignored and that steering up hits the top wall; `win`
  eats the tenth apple and restarts.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/snake_trail
rusting test samples/snake_trail
```
