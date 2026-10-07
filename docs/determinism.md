# Deterministic simulation

This page records the rules behind `DeterminismMode` (Milestone 8). `Off`
promises nothing. `Local` promises identical results on one machine and
build. `CrossPlatform` promises identical results on every supported
device. The rules in this page are what `CrossPlatform` enforces.

## Number format

Decision: simulation math is **IEEE-754 binary32 (`f32`) with a
constrained operation set**, not fixed-point.

Why not fixed-point: every CPU and GPU solver already runs on `f32`, GPU
integer multiplies wider than 32 bits need the optional `shaderInt64`
feature, and a fixed-point solver would be a rewrite with a range and
precision budget per quantity. Constrained `f32` keeps the solvers and
their speed. The cost is the rule list below, which the shared math module
(the next roadmap item) implements once for Rust and GLSL.

The owner can revisit this choice before the shared math module lands;
after that, changing it means rewriting that module and every solver
that uses it.

### Allowed as hardware operations

These have one correctly rounded result under round-to-nearest-even on
every supported CPU (x86-64 SSE2, AArch64 NEON) and in Vulkan SPIR-V for
32-bit floats:

- `+`, `-`, `*`, negation, `abs`, comparisons, `min`, `max`;
- `floor`, `ceil`, `trunc`, and `f32` to `i32` conversion of in-range
  values;
- bit casts between `f32` and `u32`.

### Replaced by shared routines

These differ between devices and drivers, so simulation shaders call the
shared module (`src/shaders/sim_math.glsl`, with the bit-exact Rust
reference `rusting_engine::runtime::sim_math`) instead of the hardware or
library function:

- division, `sqrt`, and `inversesqrt` (Vulkan allows 2.5 ULP error for
  division and several ULP for square roots): computed from an integer
  bit-trick seed and a fixed number of Newton steps that use only the
  allowed operations above, then the best of the result and its two
  neighbouring floats, so round values such as `1 / 1` and `sqrt(9)` are
  exact (reciprocal and square root within 1 ULP, inverse square root
  within 2);
- `dot`, `length`, `normalize`, and matrix-vector products: written out as
  explicit operations in a fixed order, because a driver may evaluate the
  built-ins with fused multiply-adds or in a different order;
- `f32` to `i32` conversion: GLSL `int()` is undefined for NaN and for
  values outside the `i32` range. Shaders call `sim_to_int`, which does
  what Rust's `as i32` (`sim_math::to_int`) does: truncate toward zero,
  saturate to `i32::MIN` or `i32::MAX`, and turn NaN into 0. Clamping uses
  `clamp`, `min`, and `max` on values that are not NaN, which every device
  rounds the same way. Integer arithmetic wraps on both sides (SPIR-V
  integer adds wrap; Rust simulation code uses `wrapping_*` where a value
  can overflow). The physics grid's `cell_of` and fixed-point `to_fixed`
  go through `sim_to_int`, and the bit-exact GPU test covers NaN,
  infinities, `±2^31`, `±3e9`, and halves. The test device's own `int()`
  happens to saturate the same way, so the test guards other drivers;
- `sin`, `cos`, `atan2`, `asin`, and other transcendental functions:
  polynomial versions in `runtime::sim_math` (`sin_cos`, `atan2`, `asin`,
  ported from Cephes, within about 4e-7 of the true value; `sin_cos` is
  that accurate for `|x| <= 8192` and still bit-identical beyond it). The
  rotation helpers `rotation_from_euler`, `euler_from_rotation`, and
  `rotation_from_scaled_axis` build on them. Simulation shaders may not
  call any transcendental function (the shader check below rejects them);
  a GLSL port is added when one needs to.

### Forbidden in simulation code

- Fused multiply-add. Rust never contracts `a * b + c` on its own;
  simulation code must not call `mul_add`. GLSL may contract unless the
  result is `precise`, so every simulation value a shader writes back is
  declared `precise`, which also forbids reordering the operations that
  produce it.
- Fast-math and relaxed precision (`mediump`, `lowp`, `RelaxedPrecision`,
  `-ffast-math`-style compiler flags). Rendering shaders may still use
  them.
- Reassociation for speed, such as summing contacts in whatever order a
  reduction tree finishes. Sums run in body or constraint order.
- NaN and infinity. `min`, `max`, and comparisons treat NaN differently
  across devices, so a NaN in simulation state is a bug to report, not a
  value to carry.
- Relying on subnormal numbers. `CrossPlatform` requires a device that
  reports `shaderDenormPreserveFloat32` and
  `shaderRoundingModeRTEFloat32` (Vulkan 1.2 float controls); a device
  without them cannot run in that mode.

## Simulation state

Simulation state is everything a fixed tick reads or writes that can
change what a later tick computes. It must be identical between runs in
the selected mode, and the world-state hash (a later roadmap item) covers
exactly this list. Everything else is presentation: it may differ per
machine, and it must never feed back into simulation state.

### Simulation state

- Components of simulating entities: `Transform` (position, rotation,
  scale) of every entity with a `PhysicsBody` whose class is `Cpu` or
  `Gpu`, or that a `FixedUpdate` system moves; `RigidBody` (kind, mass,
  gravity scale, linear and angular velocity); `Collider`;
  `CollisionLayers`; `PhysicsBody`; the `Sleeping` marker; `GpuProxyOf`.
- Gameplay components that `FixedUpdate` systems update:
  `PlayerController`, `PlatformerController` (including `vertical_speed`,
  `grounded`, `jump_buffer`, `air_time`), `Tween` (including its played
  time), `Pickup`, `Counter`, and `BurstEmitter` trigger flags.
- Resources: `PhysicsWorld` (warm-start impulses and sleep counters),
  `PhysicsSettings`, `FrameTime::fixed_tick` and `fixed_delta`,
  `RandomSeed`, `DeterminismMode`, `PhysicsIdRegistry`, and
  `GpuPhysicsCommands` as queued for each tick.
- GPU buffers: each body's `PhysicsState` (model matrix, velocity,
  angular velocity, properties, custom values, metadata), rule state
  (edge, emitted flag, last emission time), and the collider list
  uploaded from `PhysicsWorld`. The GPU clock `pc.elapsed` is
  `fixed_tick * fixed_delta`, never the frame clock.
- Events that `FixedUpdate` systems react to: `CollisionEvent` and GPU
  physics events, delivered in order of tick, `PhysicsId`, and event ID.
- Input as sampled for each tick. `player_look` (yaw) and
  `platformer_jump` (jump buffer) run in `Update` and write simulation
  state from input once per frame; replays record their effect per tick
  (Replay items).

### Presentation only

- `FrameTime::frame`, `real_delta`, `delta`, and `elapsed`; `TimeControl`
  pause and time scale, which decide how many ticks run per frame but not
  what a tick computes.
- `GlobalTransform` of entities that do not simulate, render
  interpolation, and `GpuStateMirror` (the GPU keeps its own copy of body
  state).
- Rendering: cameras, lights, `RenderSettings`, quality profile, culling,
  materials, `MeshRenderer`, `Visibility`, the render world, and every
  rendering shader (which may keep `mediump` and fusion).
- `BurstParticle` entities, `SoundEvent`s, `HudElement`, UI, the
  profiler, and editor state.

A presentation system may read simulation state but never write it; a
system in `FixedUpdate` must not read presentation state.

Known gap: GPU physics events, and the `GpuQueryProxy` poses and
`GpuStateMirror` samples they carry, reach the CPU one to three frames
after their tick, depending on frame timing. CPU simulation that reacts to
them is not deterministic until they are delivered at a fixed tick lag
(the deterministic execution order items).

### World-state hash

After every fixed step, `App::update` hashes the simulation state above
and appends `(tick, hash)` to the `StateHashes` resource, which keeps the
last `STATE_HASH_HISTORY` (1024) ticks. A scenario report copies them
after every tick, so `rusting determinism --scenario` compares every tick
of a run of any length. The tick is the number of steps completed. `world_state_hash` feeds raw `f32` bits in entity order,
sorts `PhysicsWorld`'s sleep counters and warm-start impulses by entity,
and writes small gameplay components through `Debug`, whose shortest
round-trip float formatting tells every value apart. It stays on in
release builds: 10,000 CPU bodies take about 0.4 ms per tick.
`state_hashes_cover_every_tick_and_only_simulation_state` checks that
frame pacing does not change the sequence, that one ULP of velocity
changes every hash from its tick on, and that a GPU body's readback
`Transform` does not count.

`simulate_project_headless` loads a cooked scene and runs a given number
of fixed steps with no window, surface, Vulkan device, or renderer (GPU
bodies stay still there), then returns the `App` and every tick's hash.
A game run with `RUSTING_HEADLESS_TICKS` and `RUSTING_STATE_HASH_OUT`
writes a `StateHashReport` there: every tick's hash, and each entity's
hash, `Name`, and `SceneId` after the last tick. `compare_runs` builds
two apps from one closure and steps them in lockstep, comparing the hash
after every step. At the first mismatch it reports the tick and the first
entity, in entity order, whose own hash differs, with its `Name` and
`SceneId`; the entity is `None` when only resources differ. Contacts can
spread a change to several bodies within one tick, so the named entity is
not always the one where the change started.

`rusting determinism <project> --ticks N` compares separate processes. It
builds the game in debug and release, runs each headless, and runs the
release build again under `taskset --cpu-list 0`, so bevy's task pool and
rayon start one thread. It compares every tick against the debug run. At
the first mismatch it runs both configurations again up to that tick and
compares their entity hashes, then fails with `DETERMINISM_DIVERGED`, the
tick, and the entity. On the starter template the three configurations
give the same 120 hashes; a debug-only `1e-4` nudge to the player's
position on tick 30 is reported as tick 31, `Player`. GPU vendors and
drivers are not covered: GPU bodies do not simulate headless, and this
machine has one GPU.
`.github/workflows/determinism.yml` runs the GPU tests on lavapipe and
`rusting determinism` on a starter project whenever simulation, shader,
or rendering code changes.

GPU-class bodies contribute only their configuration to that hash: their
CPU `Transform` is a readback copy that arrives frames late. Their real
state is hashed on the GPU instead. After every tick, `physics_hash.comp`
hashes each body's `PhysicsState` and rule state, and sums the per-body
hashes with integer atomics, which give the same result in any order.
`SceneRenderer::take_completed_physics_state_hashes` returns
`(tick, hash)` pairs one to three frames later, and the runner and
`HeadlessCapture` append them to `StateHashes::gpu`.
`commands_apply_on_their_tick_however_frames_batch_ticks` gets the same
GPU hashes however frames batch ticks, and a one-ULP change to a command
changes the hash from its tick on.

### Replays

`App::start_recording` records every following `App::update` into a
`Replay` (format version, seed, start tick): each frame's real delta, the
fixed tick it starts on, and its `RuntimeInput` when that changed since
the previous frame, plus every tick's world-state hash. Recording the
frames rather than one input per tick also covers systems that read input
in `Update`; `platformer_jump`, for example, buffers a press after the
frame's fixed steps, so the press first counts on the next frame's first
tick. `play_replay` sets the seed, feeds the same deltas and input into an
app holding the same scene, and returns the first tick whose hash
differs. `replays_reproduce_recorded_hashes_and_find_changed_input`
records 90 uneven frames, plays the JSON form back to the same 140
hashes, and finds a removed jump press on the tick after it.

A game run with `RUSTING_REPLAY_OUT` writes the session's replay there on
exit; `RUSTING_REPLAY_PLAY` plays one headless and fails on divergence.
Playback uses no renderer, so render settings, resolution, quality
profile, and window size cannot change it; the recorded input carries the
viewport size gameplay saw. GPU bodies do not simulate headless, so
playback checks CPU state only.

In bevy 0.19 every resource is an entity. Inserting a resource mid-run
allocates an entity and shifts the ids of entities spawned later, and the
CPU solver orders bodies by `Entity`. The recorder therefore lives in
`App`, not the `World`, and playback writes `RandomSeed` and
`RuntimeInput` in place.

#### Seeking

`ReplaySeeker::new(replay, interval, make_app)` plays a replay and takes a
full `WorldSnapshot` every `interval` ticks. `seek(tick)` goes backward, or
forward past a later snapshot, by restoring the nearest snapshot into a new
app from `make_app` and re-simulating from there. Every re-simulated tick
still checks its recorded hash, and a mismatch returns
`ReplayError::Diverged`.

`App::snapshot` copies every entity, resources included, with its
component values, and the entity allocator's state. `App::restore` puts
them back into an app built the same way, at the same entity ids and
generations, so later spawns get the same ids and the hashes continue
unchanged. Bevy hides part of the allocator: up to 128 freed ids wait in a
local list before they can be reused. The snapshot reads that list by
allocating and freeing, then rebuilds the allocator.

Every component and resource type in the world must be registered:

- `App::register_snapshot_component::<T>()` copies `T` with `Clone`.
- `App::ignore_in_snapshots::<T>()` keeps whatever the restored app has,
  for caches, handles to outside state, and values fixed after setup.

A snapshot of a world holding an unregistered type fails with
`SnapshotError::Unregistered` and names the type. The engine registers its
own types, and `SimpleGamePlugin` registers the `rusting_game!` ones.

Limits:

- Assets created at runtime (meshes, textures) are not restored; the new
  app has only what its setup loaded.
- Change detection starts fresh: restored values count as unchanged, and
  removals from before the snapshot are gone.
- Game systems whose results depend on query order, rather than sorting by
  `Entity` as engine systems do, can diverge after a restore, because
  restored entities sit in a different table order. The per-tick hash check
  reports it.

`snapshots_restore_entity_ids_and_state_into_a_new_app` restores a scene
with churning particles and matches 52 frames of hashes;
`replays_of_the_starter_template_seek_through_snapshots` seeks the starter
game back and forth and finds a tampered hash after a snapshot.

## Current state

The engine meets `Local` today and does not meet `CrossPlatform` yet:

- On the CPU, Rust's `+`, `-`, `*`, `/`, and `sqrt` are correctly rounded
  IEEE-754 operations on every supported target and are never fused, so
  the CPU solver and nalgebra may use them directly. The CPU solver takes
  its Euler conversions and rotation integration from `sim_math`, and the
  pinned checksum in `transcendentals_stay_close_to_f64_and_keep_their_bits`
  fails if any of those bits change. Player movement and tween easing use
  `sim_math::sin_cos` too. GPU bodies and teleport commands start from
  `sim_math::transform_matrix`, and GPU readback converts back with
  `sim_math::transform_from_matrix`. Rendering and glTF import keep
  nalgebra's platform trigonometry.
- The GPU physics shaders (`physics.comp`, `physics_contacts.comp`,
  `physics_shapes.glsl`) take division, square roots, dot products,
  lengths, normalization, and rotations from `sim_math.glsl`. Every other
  float operation in them is `precise` too. `precise` only covers the
  function it is written in and the values that feed a `precise`
  variable, so a value that only feeds a comparison needs its own
  `precise` local. They use `roundEven`, never `round`, whose direction at
  `.5` is up to the device. The legacy `src/shaders/compute/*.comp` solvers
  other than those are not compiled.
- `simulation_shaders_are_precise_and_never_relaxed` (runs with
  `--features gpu-tests`, needs `glslc`) compiles those shaders and fails
  on any float add, subtract, multiply, divide, or vector or matrix
  product without the SPIR-V `NoContraction` decoration, on any
  `RelaxedPrecision` value, and on any GLSL.std.450 function other than
  `abs`, `sign`, `floor`, `ceil`, `trunc`, `roundEven`, `min`, `max`, and
  `clamp`. Rendering shaders are not checked and keep `mediump` and
  fusion. Custom solver shaders are not checked either: their
  `// rusting: determinism` pragma is the author's claim.
- The GPU test `shader_sim_math_matches_the_rust_reference_bit_for_bit`
  runs the routines on the test device and compares every bit with the
  Rust reference. Its first case differs if the driver fuses `sim_dot`,
  and lavapipe does fuse it when `precise` is removed.

## Execution order

- `FixedUpdate` runs on bevy's single-threaded executor, so its systems
  run in the schedule's topological order, never in the order threads
  finish. The engine's own fixed systems form one chain (CPU physics,
  player and platformer movement, contact triggers, pickups, sound cues,
  burst particles, new bursts, tweens); `engine_fixed_update_systems_have_one_order`
  builds the schedule with bevy's ambiguity detection set to error. A game
  that adds fixed systems should order the ones that touch the same data.
- The CPU solver sorts bodies by `Entity`, sorts broad-phase pairs into
  body order whatever sweep axis it picks, and solves contacts in that
  order. `PhysicsWorld::gpu_colliders` keeps that order; GPU bodies upload
  sorted by `PhysicsId` slot.
- Because bodies are solved in `Entity` order, adding, removing or
  reordering entities in the scene, even ones without bodies, can change
  which body a contact resolves first. The same scene still replays
  exactly, but a chaotic result such as where a toppled stack of barrels
  ends up can move after an unrelated scene edit. Scenario checks on such
  results should test something robust (distance travelled, fell over,
  left an area) rather than a final position to a few centimetres.
- GPU contacts: the grid pass fills hash cells and the fallback list with
  atomics, so which body lands where changes between runs. That never
  changes the result. A body that is not oversized reaches no further than
  one cell, so every touching pair is found whether a body sits in a cell
  or in the fallback list. Each body keeps the largest push and velocity
  change each way per axis, and max and min give the same result in any
  order. A body too big for one cell gets its own workgroup: its threads
  split the bodies to test, then merge their max and min in shared memory,
  which matches one thread testing every body in turn bit for bit.
  `gpu_bodies_collide_with_each_other_through_grid_and_fallback` checks that two
  runs, and a run with no hash memory at all, match bit for bit.
- Each fixed step sees its own `FrameTime::fixed_tick` (the ticks
  completed before it), even when one frame runs several steps, so
  tick-indexed randomness differs per tick. Before this, every step in a
  frame saw the frame's final count.
- CPU-to-GPU commands carry the GPU tick they apply before: a command
  pushed during fixed tick `n`, or in `Update` right after it, applies
  before GPU tick `n` or `n + 1` respectively, however many ticks a frame
  batches. Within a tick, a body's commands apply in submission order,
  which the fixed system order makes deterministic. The GPU never drops a
  tick: past eight per frame, the rest wait for the next frame.
  `commands_apply_on_their_tick_however_frames_batch_ticks` gets the same
  bits from one tick per frame, from batches of three, and from a ten-tick
  frame that carries two ticks over.
- Randomness: simulation code draws every random value with
  `RandomSeed::value(tick, RandomSeed::stream(subsystem, key))` (or
  `unit`). The seed comes from the scene or scenario, the tick is the
  fixed tick being simulated, and the subsystem name keeps streams apart.
  Bursts use `"bursts"` keyed by the emitter's `SceneId`, or by its entity
  when it has none. Simulation code never reads a global or thread-local
  generator, `Uuid::new_v4`, or `HashMap` iteration order; GPU physics
  shaders draw no random values. The `rand` dependency is not used by any
  simulation code.
- Solvers do a fixed amount of work per tick: the CPU solver runs
  `SOLVER_ITERATIONS` (8) velocity passes with no early exit, bodies sleep
  after `SLEEP_STEPS` ticks, and the GPU runs one contact pass and one step
  per tick. Every fixed system takes `FrameTime::fixed_delta`, never the
  frame delta. Frame pacing only changes how many ticks run in one frame;
  `simulation_bits_depend_on_ticks_not_frame_pacing` runs 120 ticks at one
  per frame and in uneven frames (three, half, half, none, five) and gets
  the same bits.
- Not covered: when the GPU event buffer overflows, which events are lost
  depends on atomic order. The loss is counted and logged; a scene that
  overflows is not deterministic.

This is why every built-in solver declares `Local` in
`DeterminismSupport`, and a `CrossPlatform` project with physics bodies
fails its startup check and `rusting validate`.
