# Hammer Run

A small third-person obstacle course. Collect five gems across a chain of
platforms, avoid the spinning red sweepers, and step on the gold pad at the
end. Falling off or touching a sweeper sends you back to the start.

The game was built with the `rusting` CLI alone, the way an agent would build
it: `rusting new --template third-person`, one scene patch, a dozen lines of Rust
and three scenarios. It is the first dogfooding game for the agent workflow.

- `scenes/main.rscene`: platforms, sweepers (dynamic bars on motorised
  `rusting.joint` hinges), gems (`rusting.pickup` sensors with a spin
  `rusting.tween`), the goal pickup that `requires` every gem, and HUD text.
- `src/main.rs`: respawns the player on a fall or a sweeper touch.
- `tests/`: `first_gem` jumps the first gap and collects a gem,
  `fall_respawns` walks off the side, `hammer_hits` rides to the first
  sweeper and waits to be hit.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/hammer_run
rusting test samples/hammer_run samples/hammer_run/tests/hammer_hits.json
```
