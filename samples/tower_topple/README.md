# Tower Topple

A first-person throwing game. Two block towers and a small pyramid stand on
three stands in front of you. Aim with the crosshair and throw balls (left
mouse or F) to knock every block off its stand. You have ten balls a round;
when every block is down, or the last ball has settled, a throw starts a new
round.

The game was built with the `rusting` CLI alone: `rusting new --template
first-person`, the `golden_hour` preset, scene patches, about sixty lines of
Rust and three scenarios. It is the fifth dogfooding game for the agent
workflow and the first to launch dynamic bodies from game code.

- `scenes/main.rscene`: the stands, the fourteen `block` objects, the hidden
  kinematic `Ball` template, the `throw` input action, the counters and HUD.
- `src/main.rs`: copies the ball template along `camera_ray`, switches the
  copy to dynamic and sets its velocity, counts blocks below the stand tops,
  and calls `restart` for a new round.
- `tests/`: `topple_pyramid` throws at the pyramid, `win_and_restart` lays
  every block on the floor and starts a new round, and `out_of_balls` ends
  a round with no balls left.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/tower_topple
rusting test samples/tower_topple samples/tower_topple/tests
```
