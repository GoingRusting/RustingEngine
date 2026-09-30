# Ember Arena

A night-time arena survival game. Embers drift in through four glowing gates
and chase you. Charge your pulse (left mouse or E) and fire it to quench every
ember within 3.5 m. Each ember that reaches you burns you; three burns end the
run, and a pulse starts a new one. Embers enter faster and move faster as your
score grows.

The game was built with the `rusting` CLI alone: `rusting new --template
third-person`, scene patches, about a hundred lines of Rust and three
scenarios. It is the fourth dogfooding game for the agent workflow, and the
first to lean on the atmosphere components: fog, bloom, ambient occlusion,
a sky light and point lights on the gates and embers.

- `scenes/main.rscene`: the arena, the gates, the `Environment` object with
  the fog, bloom, ambient occlusion and tone mapping settings, the player with
  its pulse burst, the hidden `Ember` template, the counters and HUD text.
- `src/main.rs`: spawns embers from the template with `spawn_copy`, steers
  them at the player, handles the pulse, burns and restart.
- `tests/`: `ember_burns` lets an ember reach the player, `pulse_quenches`
  fires the pulse at an ember in range, and `game_over` ends the run after
  three burns.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/ember_arena
rusting test samples/ember_arena samples/ember_arena/tests
```
