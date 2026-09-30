# Night Vault

A third-person stealth game at night. Two guards walk their rounds with
lamps; stay out of their sight, take the three keys, and reach the vault
door. Each moment a guard sees you raises the alert meter, and it falls
again while you stay hidden. At 40 the guards catch you. R starts over.

The game was built with the `rusting` CLI alone: `rusting new --template
third-person`, two scene patches, about 110 lines of Rust and three
scenarios. It is the twelfth dogfooding game for the agent workflow and the
first to use spot lights, fog and line-of-sight raycasts for gameplay.

- `scenes/main.rscene`: the walled courtyard, hedges and crates for cover,
  the guards with child spot lights, the keys (`rusting.pickup`), the vault
  door sensor, moonlight, fog, the counters and HUD.
- `src/main.rs`: moves each guard back and forth along its path as a pure
  function of the fixed tick, checks whether it sees the player (range,
  view cone, then a raycast that must reach the player first), lights up
  a guard that sees you, and runs the alert meter and the win and lose
  conditions.
- `tests/`: `spotted` stands in front of a guard and gets caught; `hidden`
  stands behind a crate in the same guard's path and stays unseen;
  `heist` takes a key, checks that the door stays shut without all three,
  then escapes and restarts.

```sh
cargo build --bin rusting               # from the repository root
rusting run samples/night_vault
rusting test samples/night_vault
```
