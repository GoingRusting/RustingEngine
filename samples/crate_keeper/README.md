# Crate Keeper

A top-down crate-pushing puzzle. Walk the grid with the arrow keys or WASD
and push the three crates onto the gold squares. A crate moves only when the
cell past it is free, so a crate against a wall or another crate stays put.
R starts the puzzle over; the HUD counts moves and crates on target.

The game was built with the `rusting` CLI alone: `rusting new --template 2d`,
scene patches, about ninety lines of Rust and three scenarios. It is the sixth
dogfooding game for the agent workflow and the first grid game: the walls and
gold squares are `rusting.tile_map` cells that game code reads with
`GameScene::tile`.

- `scenes/main.rscene`: the `Level` tile map (`#` wall, `,` floor, `o` gold
  square), the player sprite, the three `crate` objects, the fixed
  orthographic camera, the `restart` input action, counters and HUD text.
- `src/main.rs`: grid moves and pushes, the move count, crates on target,
  and restart.
- `tests/`: `solve` plays a sixteen-move solution, `blocked` checks that a
  crate against a crate and a wall do not move, and `restart` checks that R
  puts everything back.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/crate_keeper
rusting test samples/crate_keeper samples/crate_keeper/tests
```
