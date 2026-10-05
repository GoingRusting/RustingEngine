# Putt Course

A mini golf hole. Aim with the mouse, hold the left button or Space to
charge, and release to putt. Power rises to 100% and falls back if you hold
too long. Sink the ball in as few strokes as you can; par is 3. A ball that
leaves the green goes back to the tee with a penalty stroke. R starts over.

The game was built with the `rusting` CLI alone: `rusting new --template 3d`,
two scene patches, about a hundred lines of Rust and four scenarios. It is
the ninth dogfooding game for the agent workflow and the first to roll a
ball on physics and catch it with a sensor.

- `scenes/main.rscene`: the green and its bouncy rails, two angled bumpers,
  the kinematic sweeper moved by a `rusting.tween`, the cup with its sensor,
  the flag, the ball, the aim arrow, the `putt` and `restart` input actions,
  the counters and HUD.
- `src/main.rs`: slows the rolling ball on the grass, charges and releases
  the putt toward the floor point under the cursor, drops a slow ball into
  the cup, returns a lost ball to the tee, and places the aim arrow.
- `tests/`: `first_putt` charges and putts from the tee; `overcharge` holds
  past full power; `sink` putts into the cup and restarts; `too_fast` rolls
  over the cup and off the back rail.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/putt_course
rusting test samples/putt_course samples/putt_course/tests
```
