# WebAssembly scripts

Native Rust plugins are the main way to write gameplay. The optional
`rusting-script` crate adds small sandboxed scripts for modding and
designer-level logic. A script is a WebAssembly module attached to one
object. It runs in the [Wasmi](https://docs.rs/wasmi) interpreter with no
access to files, the network, the clock, or random numbers. It can read and
write the same reflected component fields that scene files, the Inspector,
and animation tracks use.

## Turn scripting on

Add the crate next to `rusting_engine` in your game's `Cargo.toml`:

```toml
rusting-script = { path = "<engine>/crates/rusting-script" }
```

Then add the plugin next to your own:

```rust
app.add_plugin(rusting_script::ScriptPlugin)?;
```

The plugin registers the `rusting.script` component. Its one field,
`module`, is the path of a `.wasm` binary or a `.wat` text module. The path
is relative to the project folder, like other asset paths, so keep scripts
in `assets/` so that export copies them. Add the component in a scene file or
in the Inspector:

```json
"rusting.script": { "module": "assets/scripts/door.wasm" }
```

## What a script exports

| Export | Required | Called |
| --- | --- | --- |
| `memory` | yes | Host functions read text from it and write results into it. |
| `start()` | no | Once, when the script loads. |
| `update(delta_seconds: f32)` | no | Every frame in `ScheduleStage::Update`. |

Scripts run one after another in entity order, so a replay runs them in the
same order. A script loads again when its `module` path changes. It is
dropped when its object loses the component or is despawned.

## Host functions

All host functions are imported from module `rusting`. Text is passed as a
pointer and a byte length into the script's memory, in UTF-8. Entities are
`i64` handles. A handle is valid while its object exists.

| Function | Result |
| --- | --- |
| `log(text_ptr, text_len)` | Prints `[script <path>] <text>` to the game's log, which the editor Console shows. |
| `entity() -> i64` | The object this script is attached to. |
| `find(name_ptr, name_len) -> i64` | The object with this `Name`, or -1. If several match, the one with the lowest handle. |
| `action(name_ptr, name_len) -> i32` | Input action bits: 1 held, 2 just pressed, 4 just released. |
| `get(entity, component_ptr, component_len, path_ptr, path_len, out_ptr, out_capacity) -> i32` | Writes a field as JSON to `out` and returns its length. If the length is more than `out_capacity`, nothing is written; call again with a larger buffer. |
| `set(entity, component_ptr, component_len, path_ptr, path_len, json_ptr, json_len) -> i32` | Writes a field from JSON and returns 0. |

`component` is a registered component name, such as `rusting.counter` or
your game's `coin_run.spin`. The name `transform` addresses the object's
position, rotation and scale. `path` is a JSON pointer into the component's
scene form, the same paths animation tracks use: `/speed`,
`/position/1`. An empty path is the whole component. `set` checks the value
against the field's reflected type, the same as a scene load.

`get` and `set` return a negative status when the request cannot be done:

| Status | Meaning |
| --- | --- |
| -1 | The entity does not exist. |
| -2 | The object lacks the component, or the field is an empty option (`get` only). |
| -3 | Unknown component, bad path, bad JSON, or a value of the wrong kind. The log says which. |

## Limits

A script that breaks a limit stops, and the log says why:
`[rusting] script <path> stopped: <reason>`. The game and every other script
go on. The script runs again only when its `module` path changes.

- **Fuel:** each call to `start` or `update` may run about 10 million Wasm
  instructions (`FUEL_PER_CALL`). An endless loop runs out and stops.
- **Memory:** 16 MiB of linear memory (`MEMORY_LIMIT_BYTES`).
- **Traps:** a trap stops the script. Traps include an out-of-bounds access,
  `unreachable`, and a host call with a pointer outside the script's memory.
- **Floats** follow the WebAssembly deterministic profile, so NaN results are
  the same on every machine.

## Example in Rust

Build a script with `cargo build --release --target wasm32-unknown-unknown`
from a `cdylib` crate (`crate-type = ["cdylib"]` under `[lib]`). The engine's
own tests use text modules, so this example has not been built by the
engine's test suite.

```rust
#![no_std]

#[link(wasm_import_module = "rusting")]
extern "C" {
    fn entity() -> i64;
    fn action(name: *const u8, name_len: usize) -> i32;
    fn set(e: i64, c: *const u8, cl: usize, p: *const u8, pl: usize,
           json: *const u8, jl: usize) -> i32;
}

fn text(s: &str) -> (*const u8, usize) {
    (s.as_ptr(), s.len())
}

/// Lifts the object to y = 1 when `game.jump` is pressed.
#[no_mangle]
pub extern "C" fn update(_delta_seconds: f32) {
    unsafe {
        let (name, len) = text("game.jump");
        if action(name, len) & 2 == 0 {
            return;
        }
        let (c, cl) = text("transform");
        let (p, pl) = text("/position/1");
        let (v, vl) = text("1.0");
        set(entity(), c, cl, p, pl, v, vl);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
```

## Example in WebAssembly text

A `.wat` file loads as it is, with no build step:

```wat
(module
  (import "rusting" "entity" (func $entity (result i64)))
  (import "rusting" "set"
    (func $set (param i64 i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "transform")
  (data (i32.const 16) "/position/0")
  (data (i32.const 32) "5.0")
  ;; Moves this object to x = 5 when it loads.
  (func (export "start")
    (drop (call $set (call $entity)
      (i32.const 0) (i32.const 9) (i32.const 16) (i32.const 11)
      (i32.const 32) (i32.const 3)))))
```

## Not yet

- Spawning and despawning objects, sending signals, and reading resources.
- Reloading a script when its file changes during play.
- Per-project fuel and memory limits.
