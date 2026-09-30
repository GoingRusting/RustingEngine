# Brick Bounce

A 2D brick breaker. Move the paddle with A/D or the arrow keys and launch the
ball with Space. Break all 32 bricks to win; three balls lost ends the round.
R starts over at any time.

The game was built with the `rusting` CLI alone: `rusting new --template 2d`,
two scene patches, about a hundred lines of Rust and four scenarios. It is
the seventh dogfooding game for the agent workflow and the first to bounce a
dynamic body around a closed field.

- `scenes/main.rscene`: the walls, the kinematic paddle, the ball, the 32
  `brick` objects, the `launch` and `restart` input actions, the counters
  and HUD. Every collider has restitution 1 and no friction.
- `src/main.rs`: moves the paddle, keeps the ball on it until launch, breaks
  touched bricks, aims paddle bounces by where the ball hits, holds the ball
  speed steady, and parks the ball again when it falls out.
- `tests/`: `first_hit` launches, breaks a brick and returns the ball with
  the paddle; `lose_ball` drops the ball; `paddle_limits` stops the paddle
  at the wall; `win_and_restart` fills the score and starts over.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/brick_bounce
rusting test samples/brick_bounce samples/brick_bounce/tests
```
