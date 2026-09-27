# RustingEngine features for creating games with LLM agents

This wishlist comes from building **Neon Salvage** with RustingEngine in September 2026. The goal is to let a person describe a game and let an agent repeatedly **inspect → edit → run → observe → fix** it, while the person can understand and control every change. These proposals are model-independent: terminal agents, IDE tools, and editor integrations should reach the same engine capabilities.

The engine already has useful foundations: ordinary Cargo game projects, versioned JSON scenes with persistent IDs, ECS and input state, a scene cooker, a headless simulation runner, editor project creation/check/play/export, and internal offscreen renderer tests. The gap is a **public, composable workflow** outside the GUI. Evidence: [engine README](README.md), [project runner](src/project_runner.rs), [scene format](src/runtime/scene_file.rs), [editor guide](editor_gui.md), [roadmap](roadmap.md).

This does not require a built-in chatbot. A good CLI and library contract should come first; an optional agent protocol can wrap those exact operations. Godot documents headless command-line import, run, and export. Unity documents a project-aware assistant with read and action modes. MCP standardizes discoverable tools and resources. These are reference patterns, not proof that any competitor has solved agent-driven game creation. Sources: [Godot CLI](https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html), [Unity Assistant](https://docs.unity.com/en-us/engine/6000.5/manual/packages-list/packages-all/pack-safe/com-unity-ai-assistant), [MCP tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools), [MCP resources](https://modelcontextprotocol.io/specification/2025-11-25/server/resources).

## Build this first

1. **A public `rusting` CLI** for project creation, inspection, validation, cook, run, capture, and export. The first slice delivers `doctor`, `new`, project/scene inspection, scene query, `validate`, and `cook` with versioned JSON; run, capture, and export remain next steps.
2. **A machine-readable capability/schema catalog** so an agent can ask what this installed engine version supports, including defaults, examples, and GPU synchronization costs.
3. **Transactional scene queries and edits** using stable scene IDs, with dry-run, validation, revision checks, undo, and precise errors.
4. **A scenario runner** that injects input at specified ticks, checks game state, and captures an image. Today's `run_project_headless` advances time but has no public test contract for driving or inspecting gameplay.
5. **A public offscreen capture command** returning PNGs and camera/render metadata. Internal golden-image tests already prove much of the renderer path.
6. **Structured diagnostics** spanning scenes, assets, Rust builds, shaders, runtime, and export.

Example desired loop (the patch, test, capture, and export commands remain illustrative next steps):

```sh
rusting doctor --json
rusting project inspect . --json
rusting scene query scenes/main.rscene --class enemy --json
rusting scene patch scenes/main.rscene --file changes.json --dry-run --json
rusting scene patch scenes/main.rscene --file changes.json --json
rusting validate . --json
rusting test . --scenario tests/collect-core.json --json
rusting capture . --scenario tests/collect-core.json --tick 180 --output /tmp/collect.png --json
rusting export . --target linux-x86_64 --output dist/ --json
```

The CLI, editor, and agent integration should call **one engine service layer**. That prevents different implementations from disagreeing about validation, IDs, import settings, or undo.

## P0 — Make the engine legible and controllable

### 1. Project lifecycle without GUI

**Current:** the editor creates projects and exports games; `cook_scene` exists as a binary; Cargo builds code; the runner simulates headlessly. `rusting` now provides the first public slice: doctor, project creation and inspection, scene inspection and query, validation, and cooking.

**Add:** `doctor`, `new`, `inspect`, `validate`, `cook`, `check`, `run`, `test`, `capture`, and `export`. Commands accept a project root or manifest path, return meaningful exit codes, offer `--json`, and never depend on the working directory silently. `doctor` reports engine version, Vulkan devices, missing tools, platform support, and suggested fixes.

**Done when:** a clean checkout can create, validate, run, capture, and export a starter game from a script on Linux and Windows. The editor buttons use these same operations.

### 2. Capability discovery and generated schemas

**Current:** the Rust API is visible in source and scenes have a format version, but an agent must search code for available components, defaults, valid ranges, solver modes, and feature requirements.

**Add:** `rusting capabilities --json` and `rusting schema scene|component|material|physics|project`. Include engine compatibility, descriptions, units, defaults, bounds, enums, examples, and whether an operation needs a window, GPU, or async readback. Generate this from the same reflection/serialization registry the editor uses. The existing [Milestone 9 reflection plan](roadmap.md) is the right foundation.

**Done when:** a tool can create and validate a scene from schemas alone, and schema changes fail CI if examples or migrations become stale.

### 3. Semantic project and scene queries

**Current:** scenes have persistent IDs and names; `GameScene` can find a named transform; ECS queries are available to Rust code. `rusting scene query` now supports ID, name, class, and component filters on saved scenes in stable ID order. Hierarchy, spatial, asset-use, and live-state queries remain open.

**Add:** search entities by ID, name, class, component, hierarchy, spatial bounds, and asset use. Return an overview first and details on demand. Include source file, JSON path, stable ID, component values, unresolved references, and links such as “this material is used by 43 entities.” Support saved scenes and running games; label live state by tick and ownership (CPU or GPU mirror).

**Done when:** an agent can answer “which camera renders this scene?” or “what uses this texture?” without dumping the whole project into context.

### 4. Transactional scene editing

**Current:** source scenes are editable JSON and the GUI has snapshot undo. Raw external edits risk stale IDs, invalid references, wide diffs, and conflict with an open editor.

**Add:** typed operations such as `create_entity`, `set_component`, `reparent`, `assign_asset`, `duplicate`, and `delete`; apply a batch atomically. Require stable IDs and optional expected scene revision. `dry_run` returns the diff, warnings, and impact count. Route editor and agent changes through the same validation and undo journal. Reject stale edits with a field-level conflict report.

**Done when:** two clients cannot silently overwrite each other, and undo restores a multi-entity change completely.

### 5. Structured diagnostics

**Current:** Cargo/editor print build output and scene APIs return typed errors. The first CLI slice now has a versioned result envelope and stable diagnostic code, severity, message, file, and scene location fields. Richer spans, cause chains, and cross-step diagnostics remain open.

**Add:** diagnostics with `code`, severity, message, file, span/JSON pointer, entity ID, asset ID, subsystem, cause chain, and suggested correction. Include warnings for a camera facing away from renderables, objects outside clipping range, missing exported assets, duplicate names, incompatible shader ABI, and incorrect GPU readback assumptions. Human-readable output should be a view of the same data.

**Done when:** CI and agents can locate and classify failures without parsing terminal prose.

### 6. Input-driven scenario tests

**Current:** the headless runner advances a fixed number of ticks. `RuntimeInput` can be driven in Rust tests, but projects cannot declare “press Space at tick 12, collect a core by tick 60” in a portable file.

**Add:** a versioned scenario format with seed, fixed timestep, initial scene, input actions/events at exact ticks, assertions on reflected state/events, timeout, and artifacts. Support CPU tests without Vulkan and GPU tests when hardware or lavapipe is present. Target stable IDs/classes and named game state, not transient ECS indexes.

**Done when:** Neon Salvage can verify dash, pickup, damage, win, and restart in CI without a display; failure reports the first failing tick and relevant state.

### 7. Offscreen image and frame capture

**Current:** the renderer has internal offscreen/readback tests, but the game runner has no screenshot command. I could prove startup yet could not inspect the actual frame through the ordinary game workflow.

**Add:** capture a chosen camera at a chosen tick/resolution to PNG; optionally output depth, object-ID, normals, and material-ID passes. Return dimensions, camera transform/projection, render settings, visible object count, device info, and warnings. Add image-difference helpers with tolerance and masks. Use the same extraction and renderer path as a player sees.

**Done when:** a headless command produces a reproducible screenshot and identifies which entity occupies a selected pixel.

### 8. Starter packs for agents and humans

**Current:** the editor's generated project contains a Rust entry point, scene, and manifest. More templates and documentation are planned.

**Add:** maintained templates for a tiny 3D arena game, platformer, first-person game, physics sandbox, and empty project. Each includes a short architecture map, tested commands, controls, an example scenario/capture, and a project-local `AGENTS.md` explaining conventions. Compile every snippet in CI. Include an “engine limitations and alternatives” page per release.

**Done when:** an agent unfamiliar with the engine makes a small playable change using only the template and local docs.

## P1 — Complete the creation loop

### 9. Optional MCP adapter

Expose the _same_ service layer as a local MCP server. Resources: capabilities, project summary, schemas, diagnostics, scenes, assets, and captures. Tools: the CLI operations above, with clear read-only/mutating descriptions and structured outputs. Start with local stdio, no cloud requirement. Use project-root scoping, path checks, limits, and human-visible diffs for broad/destructive edits. MCP's tool/resource split is an interoperability model, not the sole way to use the engine. [MCP tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools), [MCP resources](https://modelcontextprotocol.io/specification/2025-11-25/server/resources).

**Done when:** a generic MCP client can inspect, patch, test, and capture a project with the same results as the CLI.

### 10. Live editor bridge

Let an external agent inspect the open scene, current selection, unsaved status, diagnostics, and play state. Apply changes through the editor's undo system; display a diff and highlight affected entities. Report whether a change touched disk, the editor buffer, or the running game. Never let an external write silently replace unsaved GUI work. Unity's documented project-aware read/action split is a useful UX reference. [Unity Assistant](https://docs.unity.com/en-us/engine/6000.5/manual/packages-list/packages-all/pack-safe/com-unity-ai-assistant).

### 11. High-level gameplay facade

Neon Salvage uses `bevy_ecs::World` directly for input, visibility, material swaps, spawning, and despawning because the concise `GameScene` API mainly exposes cubes, spheres, transforms, materials, and GPU watches. Add typed, fallible helpers for input/actions, entity handles, lifecycle, visibility, camera, lighting, collisions/triggers, events, and common asset operations. Document when to use the facade versus direct ECS. Avoid hidden O(N) name scans and panics for routine generated code.

**Done when:** this game's loop can be expressed with engine public APIs and no direct game dependency on `bevy_ecs`, while advanced projects retain ECS access.

### 12. First-class asset pipeline for automation

Provide CLI/library import for images, glTF, audio, fonts, and later sprites, with stable asset IDs, import settings, dependency graphs, and structured errors. Preserve sources and record origin, license, generation tool/model if applicable, and human edits. Support reimport and hot reload with an exact list of affected scenes. Users can bring their own generators; the engine should validate and replace their outputs cleanly.

### 13. Reusable scene units and data assets

The roadmap already plans prefabs, variants, typed data resources, reflection, and hot reload in [Milestone 9](roadmap.md). Prioritize these for agent workflows: create one enemy prefab, instantiate it many times, then edit its source safely. Provide dependency/override inspection and conflict diagnostics. Store balance data in editable assets rather than Rust constants.

### 14. Runtime UI and text

This is in [Milestone 16](roadmap.md). Neon Salvage uses 3D lamps and terminal logs because game runtime lacks a simple HUD/text API. A small first release could offer text, panels, buttons, progress bars, layout, focus, and input routing; richer typography and localization can follow. Make UI queryable/testable by semantic ID, with screenshots and accessibility labels. A scenario should assert “victory panel is visible.”

### 15. Audio and feedback effects

The roadmap covers audio and particles. Provide event-triggered sounds, music, buses, spatial sound, and simple transient burst APIs. Include no-audio and reduced-effects modes for headless tests. Agents need to preview effects or inspect an event trace showing they fired.

### 16. Input actions, gamepads, and remapping

Build on the current action map with project-defined actions, keyboard/mouse/gamepad bindings, rebinding UI, and test injection by **action name**, not device key. Surface binding conflicts, dead zones, and focus routing as diagnostics.

### 17. Camera/controller recipes and common mechanics

Offer inspectable, composable recipes for top-down, first-person, third-person, and 2D cameras; health/damage, pickups, timers, checkpoints, and pause. Generate ordinary Rust/components and data assets, not opaque behavior. Agent templates should share the same code as the engine's vertical slice.

### 18. Build, export, and installation verification

Make export a CLI/service operation with an artifact manifest: executable, cooked scenes, transitive assets, engine license, platform, hashes, and warnings. Smoke-test the **exported** build in an isolated folder. This catches paths that work only in the source tree. Reuse the editor's export logic.

### 19. Performance and budget reports

Expose CPU/GPU timing, draws/dispatches, upload/readback bytes, asset memory, shader compilation, and input-to-frame latency in stable JSON. Give tests budget assertions with environment metadata and hardware-independent checks where possible. Agents can optimize toward a target rather than guess from FPS.

### 20. Reliable iteration

Report “edit saved → code checked → first playable frame” latency. Attach shader/asset reload failures to the changed file. Add a clean restart path when planned Rust hot reload cannot preserve state. Support cancellation of long builds/captures without leaving the editor in an unknown state.

## P2 — Higher ambition after the loop works

21. **Deterministic replay and state hashes.** [Milestone 8](roadmap.md) would let an agent reproduce a bug from seed/input trace, identify the first divergent tick, and attach the replay. Prioritize local replay before promising cross-vendor guarantees.
22. **Structured runtime inspection.** Query selected entities, contacts, GPU events, active camera, UI tree, and recent gameplay events at a paused tick. Identify how fresh each GPU value is.
23. **Visual picking from capture.** Map a pixel/region to stable entity IDs and source scene paths. Then “make the red door larger” does not require guessing its name from an image.
24. **Accessibility checks.** Diagnose contrast, text size, focus order, color-only status, and missing captions, with locations and screenshots.
25. **Provenance and reproducibility.** Record engine/plugin versions, asset hashes, generator metadata, seeds, and scenario versions in a portable project report.
26. **Third-party extension API.** Versioned schemas/tool registration let importers, validators, profilers, and exporters add capabilities without patching the engine. Report which extension provided each capability.

## Design rules

- **One truth, several interfaces:** CLI, GUI, Rust API, and optional MCP adapter call the same operations and validation code.
- **Observation is a feature:** every mutation has a way to inspect its effect as state, diagnostics, image, or replay.
- **Small outputs by default:** summaries, filters, pagination, and stable references; details on demand.
- **Explicit ownership and cost:** mark CPU-authored, GPU-owned, delayed-readback, and presentation-only values, and the cost of expensive operations.
- **Safe edits:** stable IDs, revision checks, atomic batches, dry-run diffs, undo, and scoped file access.
- **Portable evidence:** another machine can run a scenario and inspect its result.
- **No model lock-in:** projects and CLI remain useful with no AI service, account, or network connection.

## Suggested product success test

Give several agents the same fresh install and a one-paragraph request for a small game. Allow only public docs and tools. Measure time to first playable build, manual interventions, valid scene edits, requested mechanics verified by scenarios, export success from an isolated folder, and whether a human can understand and undo each edit. Re-run this for every release; it will reveal missing engine affordances more reliably than a list of integrations alone.
