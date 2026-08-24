# RustingEngine

RustingEngine is a Vulkan-based 3D engine written in Rust.

## Running

Install Rust and make sure that a Vulkan-compatible graphics driver is available.

Run the editor GUI:

```bash
./scripts/run_editor.sh
```

Run the simulation without the editor:

```bash
cargo run --bin user_main
```

The editor can also be started directly through Cargo:

```bash
cargo run --bin editor
```
