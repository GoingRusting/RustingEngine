# Sky Hop

A small side-view platformer. Run right across a tile map, ride a lift over
the first pit, jump the red crawler, hop the stepping stones, collect four
coins and touch the gold block. Falling in a pit or touching the crawler sends
you back to the start and counts a fall.

The game was built with the `rusting` CLI alone: `rusting new --template 2d`,
two scene patches, a dozen lines of Rust and four scenarios. It is the third
dogfooding game for the agent workflow; it found that character controllers
did not ride moving platforms.

- `scenes/main.rscene`: the `rusting.tile_map` level, the lift and crawler
  (kinematic bodies moved by `rusting.tween`), coins (`rusting.pickup`
  sensors with a pulse tween and a burst), the goal pickup that `requires`
  every coin, the counters and HUD text.
- `src/main.rs`: respawns the player on a fall or a crawler touch and counts
  it.
- `tests/`: `first_coin` jumps onto the first ledge, `ride_lift` stands on
  the lift across the pit, `crawler_catches` waits in the crawler's path,
  and `clear_level` finishes the level with timed inputs only.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/sky_hop
rusting test samples/sky_hop samples/sky_hop/tests
```
