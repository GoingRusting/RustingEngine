# Starter game request

The acceptance exercise in `tests/starter_game.rs` starts from this request
and builds the game with the public `rusting` CLI only:

> Make a small 2D side-view game called Coin Run. A runner moves with A/D or
> the arrow keys and jumps with Space over two blocks and a pit. Five gold
> coins sit along the way, some only reachable by jumping; each one pulses
> gently, vanishes with a sparkle when collected, and a counter in the top
> left shows how many are collected out of five. The flag on the ledge at the
> end only counts once every coin is collected, and then a large gold
> "You win!" appears in the middle of the screen. Give it a warm sunset look.

`win.scenario.json` plays the level: it runs right, jumps at four fixed
ticks, checks the coin counter and the win state, and captures the winning
frame.
