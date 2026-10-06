# Core concepts

A short tour of the ideas the tutorials rely on.

## Projects

A project is a Cargo package plus a `project.json` manifest. The manifest
names the main scene, the cooked scene the game loads, the binary name, and
the determinism level. The editor and the `rusting` CLI read and write the
same files, so you can switch between them at any time.

## Scenes

A scene (`scenes/main.rscene`) is JSON: a list of entities, each with a
persistent ID (a UUID), a name, an optional parent, and components. Built-in
components have their own fields (`transform`, `mesh_renderer`, `camera`,
`rigid_body`, `collider`, lights, ...). Everything else, built-in gameplay
components such as `rusting.pickup` and your own, lives under `components`
by registered name.

- Entity names must be unique in a scene, except objects placed by an
  instance (see below). Game code finds objects by name,
  tools address them by ID.
- `rusting scene query` prints entities, `rusting scene patch` changes them
  as one atomic batch, and the editor edits them with undo.
- `rusting schema` lists every component and field, with defaults, units,
  and valid ranges.
- Registered components are described with `reflect!`. The description
  drives saving, the Inspector's widgets, the schema, and field paths
  (`set_registered_component_field`). A saved field the type does not have
  fails the load; a renamed or removed field needs a `FieldMigration`.

A scene can hold other scenes, like a prefab. An object with the
`rusting.scene_instance` component (`{"source": "prefabs/coin.rscene"}`)
gets that scene's objects as children when the scene loads. Their IDs are
the same on every load. The scene saves the link plus what you changed
on the placed objects (`rusting.instance_overrides`), so a change to
`coin.rscene` shows up everywhere it is used, except in fields an instance
changed itself. Instances can nest; a scene that contains itself fails to
load. A *variant* (Godot's inherited scene) is a scene whose one object is an
instance of a base scene: its own changes save as overrides, and it still
follows every other edit to the base. Create one with **New Variant** in the
Assets menu of a `.rscene` file. Right-click an instance in the Hierarchy to
revert your changes, write them into the source scene (**Apply to Source**),
or turn the instance into ordinary objects (**Unpack Completely**).

Game code can place a scene while the game runs. Load it once with
`AssetServer::load_prefab("assets/prefabs/coin.rscene")`, keep the returned
`Handle<Prefab>`, and place it from a system with
`commands.queue(move |world: &mut World| spawn_prefab(world, coin, transform).map(drop))`.
Each placement gets a root named `coin #1`, `coin #2`, and so on, with IDs
that are the same when the game replays the same placements in the same
order. Paths are relative to the project folder, where games run. Keep
runtime prefabs under `assets/` so that export includes them.

**Cooking** turns the JSON scene into a compact binary
(`build/main.rscene.bin`) that the game loads. Play, `rusting run`, `test`,
and `export` cook for you. A cooked scene includes the objects of its
instances, so the game does not need the source scenes.

## Game code

A game's `main.rs` picks one of two entry points:

- **`rusting_game!(update)`**: one function called every frame with a
  `GameScene`. Find objects by name, move and rotate them, spawn cubes and
  spheres, turn on GPU physics for a class, and read GPU events. Good for
  small games and procedural setup. See [Tutorial 1](tutorials/01-hello-cube.md).
- **A `Plugin` passed to `run_project`**: normal ECS systems (the engine uses
  `bevy_ecs`) with queries, resources, input actions, and your own
  components saved in scenes. See [Tutorial 3](tutorials/03-gameplay-plugin.md).

Both run the same engine: the same scene loader, physics, renderer, and
built-in gameplay systems.

### Signals

Signals are the ECS form of Godot's signals. Register a named handler once,
in your plugin: a system whose input says which signal it answers.

```rust
#[derive(EntityEvent, Clone)]
struct Collected { entity: Entity }

fn add_score(In(signal): In<Signal<Collected>>, mut score: ResMut<Score>) {
    score.0 += 1; // signal.source collected, signal.target receives
}

app.add_signal_handler("add_score", add_score)?;
app.connect(coin, "add_score", player)?; // or insert `Connections`
```

A system then sends `commands.trigger(Collected { entity: coin })`, and
every handler connected on the coin that takes `Signal<Collected>` runs, in
connection order, after the system's commands apply. `Signal<Added<T>>` and
`Signal<Removed<T>>` handlers answer component `T` being added to or removed
from the connected entity; a despawn counts as a removal. Connections are
data on the emitting entity, so snapshots and replays keep them, and
restoring a snapshot fires no signals. Register handlers during setup, like
systems.

Scenes save connections as the `rusting.connections` component, with each
target stored by object ID, so you can also set them up in the editor's
Inspector or a scene patch:

```json
"rusting.connections": {"list": [{"handler": "add_score", "target": "<player's id>"}]}
```

A game refuses to load a scene whose connections name a handler it did not
register or an object that is not in the scene, and the error names the
object. The editor and other tools load such scenes without checking.

### Classes (groups and tags)

A class is a name shared by any number of objects, and one object can have
several (`ObjectClasses`). Assign classes in the Inspector's Classes section;
scenes save them with each object. To find the members, read the class index
instead of scanning every object:

```rust
fn hurt_enemies(index: Res<ClassIndex>, mut bodies: Query<&mut Health>) {
    for enemy in index.members("enemy") {
        if let Ok(mut health) = bodies.get_mut(enemy) {
            health.0 -= 1;
        }
    }
}
```

`app.class_members("enemy")` does the same outside a system. Members come in
ascending entity order. The index follows every insert, removal and despawn
of `ObjectClasses`, so change an object's classes by inserting a new value,
not by editing it through `Mut<ObjectClasses>`.

### Finding objects in the scene tree

Add `SceneTree` to a system's parameters for the common lookups. It only
returns entities, so the components you read or change stay in your own
queries:

```rust
fn open_door(tree: SceneTree, mut doors: Query<&mut Transform>) {
    let Some(house) = tree.find_by_name("House") else { return };
    if let Some(door) = tree.find_path(house, "Front/Door") {
        if let Ok(mut transform) = doors.get_mut(door) {
            transform.position[1] += 3.0;
        }
    }
}
```

| Method | Returns |
| --- | --- |
| `find_by_name(name)` | The object with that name; with several, the lowest entity index. It scans every named object. |
| `child(parent, name)` | The first direct child with that name. |
| `find_path(root, "Arm/Hand")` | The object at a `/`-separated path of child names, like Godot's `get_node`. `..` steps up to the parent. |
| `parent(entity)`, `children(entity)`, `descendants(root)` | The hierarchy, in hierarchy order (`descendants` goes depth first). |
| `name(entity)` | The object's name. |
| `in_class(class)`, `is_in_class(entity, class)` | Class members from `ClassIndex`. |

## Data assets

A data asset is a file of typed values that several objects share, like a
Godot `Resource` saved as `.tres`. Enemy stats, loot tables and dialogue
settings are typical. Describe the type with `reflect!`, give it a name, and
register it:

```rust
#[derive(Default, Serialize, Deserialize)]
struct EnemyStats {
    health: u32,
    speed: f32,
    base: Option<Handle<EnemyStats>>,
}

rusting_engine::reflect! {
    struct EnemyStats {
        health: u32,
        speed: f32 { unit: "m/s" },
        base: Option<Handle<EnemyStats>>,
    }
}

impl DataAsset for EnemyStats {
    const NAME: &'static str = "my_game.enemy_stats";
}

app.register_data_asset::<EnemyStats>();
```

The file is JSON with the type name and the value. References to other
assets are paths relative to the file:

```json
{
  "type": "my_game.enemy_stats",
  "data": { "health": 40, "speed": 3.5, "base": { "$asset": "monster.rdata" } }
}
```

- `AssetServer::load_data::<EnemyStats>(path)` returns a `Handle`, and
  `assets.data.get(handle)` reads the value. Loading the same file again
  returns the same handle, so every user shares one copy.
- A component field of type `Handle<EnemyStats>` saves in the scene as the
  file's path and loads the file with the scene.
- `save_data` writes a value, and `reload_data` reads the file again into
  the existing handle.
- Loading fails with the reason when the file holds another type, has a
  field the type does not have, or refers back to itself. Data assets
  cannot refer to scene objects. Renamed or removed fields are not
  migrated yet.

### Shared and unique data

A handle to a file is shared: every object that loads `goblin.rdata` reads
the same values, and an edit to the file reaches all of them. To give one
object its own values, make the reference unique, like Godot's Make
Unique. A unique value has no file. It is saved inside the object that
refers to it as `{"$data": value}`:

```json
"my_game.spawner": { "stats": { "$data": { "health": 80, "speed": 3.5, "base": null } } }
```

- Each `{"$data": ...}` loads as its own copy. Two objects, or two
  instances of the same prefab, never share a unique value, and reloading
  a file does not change copies made from it.
- `AssetServer::duplicate_data(handle)` makes a unique copy in game code,
  like `Resource.duplicate()`. Handles inside the copy still point at the
  same assets.
- A data asset file can hold unique values in its own fields too.

### Changing data while the game runs

A running game checks its loaded `.rdata` files for changes twice a
second. Save a file, from the editor's Inspector or any text editor, and
the game reads it again into the same handle, so every object that uses
it sees the new values on the next frame. Nothing restarts and no object
state is lost.

- To react to a change, compare
  `assets.data.store::<T>().unwrap().revision(handle)` with the value you
  saw last time. It goes up on every reload.
- A file that fails to load keeps its last values. The game prints
  `hot reload failed:` with the reason, and the editor's Console shows it.
- Unique copies have no file, so reloads never change them.

## Waypoint graphs

`WaypointGraph` moves a monster between hand-placed points, such as the
aisles and doors a mascot patrols. It is plain data: positions and two-way
edges. A node's ID is its index.

```rust
use rusting_engine::runtime::WaypointGraph;

let mut graph = WaypointGraph::default();
let booth = graph.add_node([0.0, 0.0, 0.0]);
let hall = graph.add_node([0.0, 0.0, 8.0]);
let aisle = graph.add_node([6.0, 0.0, 8.0]);
graph.connect(booth, hall);
graph.connect(hall, aisle);

let start = graph.nearest(monster_position).unwrap();
let route = graph.path(start, booth); // Some(vec![aisle, hall, booth])
```

- `path` returns the shortest route by straight-line edge length, both ends
  included, or `None` when nothing joins them. `length` sums a route.
- `disconnect` removes an edge, for a door that closes, and `connect` adds it
  back.
- Ties resolve the same way every run: `nearest` picks the lowest ID among
  equal distances, and `path` prefers lower IDs among equal-cost routes. A
  path chosen in a fixed tick stays in step with replays.
- The graph serializes with serde, so it can be a field of a data asset or a
  component.
- There is no navmesh, no automatic graph building and no obstacle
  avoidance between nodes. Place nodes where a straight walk between
  neighbors is clear.

## Steam achievements

`rusting_engine::steam` holds the calls a game makes to Steam:
`unlock_achievement("night_5")`, `set_stat("nights_survived", 5)` and
`is_running()`. Call them from game code now. Today they do nothing and
return `false`, in every build, so the game runs the same with or without
Steam installed.

- The optional `steam` cargo feature is reserved for the real Steamworks
  backend. That backend needs the `steamworks` crate, which waits on the
  owner's approval, so the feature adds nothing yet.
- Unlocks are presentation, like audio. Never read their result back into
  simulation state, or replays stop matching.

## Frames and fixed ticks

The engine separates **frames** from **fixed ticks**:

- A frame is one rendered image. Frame length varies with the machine.
- A fixed tick is one simulation step of exactly `fixed_delta` (1/60 s by
  default). Each frame runs as many ticks as real time requires, up to 8.

Systems run in stages: `Startup`, `FixedUpdate` (once per tick), `Update`
(once per frame), `PostUpdate`, and `RenderExtract`. Physics and anything
that must replay identically runs in `FixedUpdate` and reads
`FrameTime::fixed_delta`. Visual effects and per-frame input handling run in
`Update` and read `FrameTime::delta`.

`FrameTime` also holds `elapsed` game time, `fixed_tick` (ticks completed),
and `frame`. `TimeControl` pauses, steps, and scales time.

## Physics: CPU and GPU

Every body has a `physics_body.simulation` class:

| Class | Where it runs | Use it for |
| --- | --- | --- |
| `Cpu` (default) | CPU solver | the player, doors, anything gameplay reads every frame |
| `Gpu` | Vulkan compute | debris, swarms, crowds, thousands of bodies |
| `Static` | nowhere, never moves | floors and walls; both solvers collide with it |
| `None` | nowhere | visual-only objects |

CPU bodies are ordinary ECS components: read and write them directly. GPU
bodies stay on the GPU. The CPU does not download their transforms every
frame. Instead you attach **rules** (`GpuPhysicsRule`: "position Y below
-20", "colliding", "timer elapsed") to an object or a whole **class**, and the
GPU sends back an event only when a rule matches. See
[Tutorial 4](tutorials/04-gpu-cube-rain.md).

`rigid_body.kind` is `Dynamic` (moved by forces), `Kinematic` (moved by your
code or a controller), or `Fixed`.

A `rusting.joint` component joins a CPU body to another CPU body, its
`target`, or to the world when `target` is null. `anchor` and `frame` place
the joint on this body; `target_anchor` and `target_frame` place it on the
target (in world space when there is no target). The frame's X axis is the
hinge, slider, and twist axis. `kind` picks the joint:

| Kind | Allows |
| --- | --- |
| `Fixed` | nothing; the bodies move as one |
| `Hinge` | turning about X, with an optional limit, spring, and motor |
| `Slider` | sliding along X, with an optional limit, spring, and motor |
| `BallSocket` | turning freely about the anchor |
| `ConeTwist` | swinging within a cone around X and twisting within a range |
| `Distance` | anything that keeps the anchors between `min` and `max` apart |
| `Spring` | anything, pulled toward `rest_length` |
| `Generic` | each of the six axes `Locked`, `Free`, or `Limited`, with its own spring and motor |

Jointed bodies do not collide with each other unless `collide_connected` is
true. Set `break_force` (N) or `break_torque` (N·m) above 0 to make a joint
breakable: when a fixed step's load passes it, the engine removes the
`rusting.joint` component and sends a `JointBroken` event naming the joint,
its target, and the load. GPU bodies do not support joints yet.

For chains, robots, and ragdolls, add a `rusting.articulation` component
(`{}`) to the root body. Every `Fixed`, `Hinge`, `Slider`, or `BallSocket`
joint that hangs from the root, directly or through other bodies in the
tree, is then solved in joint space (reduced coordinates). Its bodies are
placed from their parents through their joints every step, so they never
drift apart, and hinge and slider limits hold exactly. Contacts and other
joints on any link move the whole tree. A `Dynamic` root floats; a `Fixed`
or `Kinematic` root is the tree's base. Other joint kinds, and joints that
would close a loop, stay regular joints. Reduced joints do not break,
ball sockets in a tree have no cone limit, and articulations never sleep.

## Input

Game code reads **actions**, not keys. The `ActionMap` resource binds action
names to keys and mouse buttons; `RuntimeInput` holds this frame's raw
state. The engine binds `player.forward`, `player.back`, `player.left`,
`player.right` (WASD and arrows), `player.jump` (Space), and
`player.sprint` (Shift). Scenario tests press and release actions by name,
so a game that uses actions is testable without a keyboard.

## Tests you can run without a screen

- `rusting run <project> --ticks N` runs the game headless for N ticks.
- `rusting test <project> <scenario.json>` presses actions at fixed ticks and
  checks scene values, collision events, and screenshots.
- `rusting determinism <project>` checks that debug, release, and one-CPU
  builds produce the same simulation, tick by tick.

GPU bodies do not move in `rusting run --ticks`, because no renderer runs
their compute shaders. A headless run warns when the loaded scene has GPU
bodies; bodies that game code moves to the GPU later are not counted.
`rusting test` runs them when the scenario sets `"gpu": true` or has a
`capture` step, because either opens the headless Vulkan device; software
Vulkan (lavapipe) works but is slow. Without both, a scenario's GPU bodies
stay where they spawned.

## Determinism

`determinism` in `project.json` is `Off`, `Local` (reproduces on one
machine), or `CrossPlatform`. The CPU simulation is deterministic today, and
recorded replays reproduce exactly. See [Determinism](determinism.md).
