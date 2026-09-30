# Core Defense

A top-down turret shooter. Drones fly in from the edge of the arena toward the
glowing core in the middle. Aim the turret with the mouse and click (or press
Space) to fire. Destroy 25 drones to win; five drones reaching the core ends
the round. R starts over at any time.

The game was built with the `rusting` CLI alone: `rusting new --template 3d`,
two scene patches, about 170 lines of Rust and four scenarios. It is the
eighth dogfooding game for the agent workflow and the first to aim with the
mouse cursor.

- `scenes/main.rscene`: the floor (with a fixed collider, so the cursor ray
  has something to hit), the walls, the core, the turret and its barrel, the
  hidden `Drone` and `Bolt` templates, the `fire` and `restart` input
  actions, the counters and HUD.
- `src/main.rs`: turns the turret toward the floor point under the cursor
  (`pointer_ray` then `raycast`), fires bolts on a reload timer, sends drones
  in from spread-out directions at a shrinking interval, moves drones toward
  the core, and scores hits and breaches.
- `tests/`: `aimed_shot` points at the first drone and destroys it;
  `missed_shot` fires away from it; `core_falls` lets the last drone through
  and restarts; `defended` makes the winning kill.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/core_defense
rusting test samples/core_defense samples/core_defense/tests
```
