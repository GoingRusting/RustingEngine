# rusting-core

Core runtime types for [RustingEngine](https://github.com/GoingRusting/RustingEngine),
a Vulkan 3D game engine with GPU-accelerated physics.

This crate has no rendering or GPU code. It holds the engine-independent
building blocks that the rest of the engine is built on:

- `components`: scene IDs, `GlobalTransform`, `Parent`/`Children`, cameras
- `transform`: the `Transform` component
- `hierarchy`: parenting, cycle-safe reparenting, and transform propagation
- `time`: frame time, fixed-step accumulation, and time control
- `input`: raw keyboard and mouse state plus named action mapping
- `events`: double-buffered typed event queues
- `schedule`: the ordered schedule stages and per-frame report
- `collisions`: collision type definitions

It is built on `bevy_ecs`. Most users should depend on
[`rusting_engine`](https://crates.io/crates/rusting_engine), which re-exports
these types.

## License

See [LICENSE.md](LICENSE.md).
