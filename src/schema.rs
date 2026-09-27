//! Machine-readable catalog of CLI operations, scene sections, and scene
//! components, printed by `rusting schema`.
//!
//! Defaults are not written by hand: they are the scene form of each
//! component's `Default`, read back through the same code that saves scenes.
//! Tests check that every documented field exists, every saved field is
//! documented, and every example loads through the scene parser and the
//! component registry.

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::reflect::{EnumInfo, Hints, TypeInfo, TypeRegistry, VariantFields};
use crate::runtime::{
    add_registered_component, registered_component_names,
    registered_component_values, scene_document, Camera, Collider,
    CollisionLayers, DirectionalLight, GpuPhysicsWatch, Name, PhysicsBody,
    PhysicsSyncMode, PointLight, RigidBody, SceneComponentRegistry, SceneId,
    SpotLight, SCENE_FORMAT_VERSION,
};
use crate::{App, AssetPlugin, Transform};

/// Version of the catalog layout. Raise it when a key changes meaning.
/// Version 2 derives component fields from reflection, with `*` for map
/// keys, and adds `resources` and `asset_types`.
pub const SCHEMA_CATALOG_VERSION: u32 = 2;

/// One `rusting` command.
pub struct Operation {
    /// Words that select the command, as `rusting --help <topic>` takes them.
    pub name: &'static str,
    /// Arguments after `rusting`.
    pub usage: &'static str,
    pub summary: &'static str,
    /// Whether the command needs Vulkan, and what it reads back.
    pub gpu: &'static str,
    /// Flag defaults as `(flag, value)`.
    pub defaults: &'static [(&'static str, &'static str)],
    /// Arguments after `rusting`; relative paths name a project `my_game`
    /// in the current folder.
    pub example: &'static str,
}

const NO_GPU: &str = "none";

pub const OPERATIONS: &[Operation] = &[
    Operation {
        name: "doctor",
        usage: "doctor [--json]",
        summary: "Report engine, platform, build tools, and Vulkan device capability. No GPU is a valid result.",
        gpu: "optional: lists devices when a driver is present",
        defaults: &[],
        example: "doctor --json",
    },
    Operation {
        name: "new",
        usage: "new <parent-directory> <project-name> [--template 3d|2d|starter] [--json]",
        summary: "Create a project from a template: 3d is the editor's lit cube, 2d a playable side-view level, starter the complete Coin Run game (collect five coins, then touch the flag). The parent must exist and the project folder must not.",
        gpu: NO_GPU,
        defaults: &[("--template", "3d")],
        example: "new . my_game --json",
    },
    Operation {
        name: "project inspect",
        usage: "project inspect <project-root> [--json]",
        summary: "Inspect and validate project.json, Cargo.toml, the main scene path, and src/main.rs.",
        gpu: NO_GPU,
        defaults: &[],
        example: "project inspect my_game --json",
    },
    Operation {
        name: "scene inspect",
        usage: "scene inspect <scene-path> [--json]",
        summary: "Inspect a migrated scene: entities, cameras, classes, assets, and reference warnings.",
        gpu: NO_GPU,
        defaults: &[],
        example: "scene inspect my_game/scenes/main.rscene --json",
    },
    Operation {
        name: "scene query",
        usage: "scene query <scene-path> [--id ID | --name NAME | --class CLASS | --component COMPONENT] [--json]",
        summary: "Find entities by one filter. Results are ordered by persistent UUID and include full component data.",
        gpu: NO_GPU,
        defaults: &[("filter", "all entities")],
        example: "scene query my_game/scenes/main.rscene --name Cube --json",
    },
    Operation {
        name: "scene patch",
        usage: "scene patch <scene-path> <patch.json> [--dry-run] [--json]",
        summary: "Apply a patch batch atomically: every operation succeeds and the scene validates, or nothing is written. Reports the revision and a per-field diff. expected_revision and per-field expected values report SCENE_CONFLICT instead of overwriting newer edits.",
        gpu: NO_GPU,
        defaults: &[("dry-run", "false")],
        example: "scene patch my_game/scenes/main.rscene patch.json --dry-run --json",
    },
    Operation {
        name: "validate",
        usage: "validate <project-root> [--json]",
        summary: "Check project files, scene structure, referenced assets, and that every physics solver and custom shader in the main scene supports project.json `determinism` (Off | Local | CrossPlatform), without Vulkan or a window.",
        gpu: NO_GPU,
        defaults: &[],
        example: "validate my_game --json",
    },
    Operation {
        name: "cook",
        usage: "cook <project-root> [--json]",
        summary: "Validate and cook the manifest's main scene to its configured cooked_scene path.",
        gpu: NO_GPU,
        defaults: &[],
        example: "cook my_game --json",
    },
    Operation {
        name: "check",
        usage: "check <project-root> [--json]",
        summary: "Validate the project, then type-check its Rust code with `cargo check`.",
        gpu: NO_GPU,
        defaults: &[],
        example: "check my_game --json",
    },
    Operation {
        name: "run",
        usage: "run <project-root> [--release] [--ticks N] [--timeout SECONDS] [--json]",
        summary: "Cook, build, and run the game from the project folder. --ticks N runs N fixed ticks without a window and exits. --timeout stops a game still running; reaching it is not a failure.",
        gpu: "required for a window; none with --ticks",
        defaults: &[("--release", "off (debug build)"), ("--ticks", "off (opens a window)"), ("--timeout", "none")],
        example: "run my_game --ticks 120 --json",
    },
    Operation {
        name: "test",
        usage: "test <project-root> <scenario.json> [--release] [--timeout SECONDS] [--json]",
        summary: "Cook and build the game, then run a scenario file in it without a window: named actions at fixed ticks, checks on reflected scene state and events, and optional captures. Fails with SCENARIO_FAILED and the first failing tick and step. The scenario format is under `scenario` in `rusting schema`.",
        gpu: "optional: only capture steps render, and they are skipped without Vulkan; one frame readback per capture",
        defaults: &[("--release", "off (debug build)"), ("--timeout", "none")],
        example: "test my_game my_game/tests/falls.json --json",
    },
    Operation {
        name: "determinism",
        usage: "determinism <project-root> [--ticks N] [--json]",
        summary: "Build the game in debug and release, run each headless for N ticks (release also pinned to one CPU when `taskset` exists), and compare every tick's world-state hash. Fails with DETERMINISM_DIVERGED naming the first divergent tick and entity. Hash reports go to build/determinism/<configuration>-<ticks>.json. GPU physics bodies do not simulate headless.",
        gpu: NO_GPU,
        defaults: &[("--ticks", "600")],
        example: "determinism my_game --ticks 300 --json",
    },
    Operation {
        name: "export",
        usage: "export <project-root> <parent-directory> [--target TRIPLE] [--json]",
        summary: "Cook, build a release, and package the game into a new folder under the parent. A native export is verified by running a copy for 60 headless ticks from a fresh temporary folder.",
        gpu: NO_GPU,
        defaults: &[("--target", "host")],
        example: "export my_game exports --json",
    },
    Operation {
        name: "capture",
        usage: "capture <scene-path> <output.png> [--camera ID|NAME] [--tick N] [--size WxH] [--pick X,Y]... [--json]",
        summary: "Render one camera of a scene offscreen to a PNG after N fixed ticks. Each --pick maps a pixel to the persistent ID of the object under it. Game code is not run. Without Vulkan, camera data and picks are still reported with a VULKAN_UNAVAILABLE error.",
        gpu: "required for the PNG: renders every tick through N and reads back one frame (width x height x 4 bytes)",
        defaults: &[("--camera", "the scene's active camera"), ("--tick", "0"), ("--size", "1280x720")],
        example: "capture my_game/scenes/main.rscene shot.png --tick 60 --pick 640,360 --json",
    },
    Operation {
        name: "asset import",
        usage: "asset import <project-root> <source-file> [--to FOLDER] [--author A] [--license L] [--url U] [--generator G] [--notes N] [--max-size PIXELS] [--dry-run] [--json]",
        summary: "Copy a png, jpeg, bmp, tga, gltf, glb, wav or ogg file (and a glTF's external buffers and images) into assets/FOLDER after loading it as the runtime would, and write <file>.rmeta with a new stable ID, settings, dependencies, content hash, and source/license provenance. Scenes reference it by the returned `reference` path. Warns when no license is given. --dry-run runs every check on a staged copy and writes nothing (`dry_run: true` in the report).",
        gpu: NO_GPU,
        defaults: &[("--to", "assets/ itself"), ("--max-size", "none (images keep their size)")],
        example: "asset import my_game art/crate.png --to props --license CC0-1.0 --json",
    },
    Operation {
        name: "asset reimport",
        usage: "asset reimport <project-root> <asset-id-or-path> [--from FILE] [--author A] [--license L] [--url U] [--generator G] [--notes N] [--max-size PIXELS] [--dry-run] [--json]",
        summary: "Replace an imported asset and keep its ID: from --from, else the recorded original file if it still exists, else revalidate the asset in place. Given provenance flags and settings replace the recorded ones. Reports the scenes that use it; a running game or editor hot-reloads the file. --dry-run previews the replacement without writing.",
        gpu: NO_GPU,
        defaults: &[("--from", "the recorded original, else the asset itself")],
        example: "asset reimport my_game assets/props/crate.png --from art/crate_v2.png --json",
    },
    Operation {
        name: "asset generate",
        usage: "asset generate <project-root> <hook> <prompt> [--to FOLDER | --replace ASSET-ID-OR-PATH] [--dry-run] [--json]",
        summary: "Run the optional generator hook `generators.<hook>` from project.json ({\"command\": [program, args...], \"license\": L, \"settings\": {\"max_size\": N}}) with RUSTING_PROMPT and an empty RUSTING_OUTPUT_DIR. The hook writes a file there and prints a last line of JSON {\"file\": NAME, \"license\"?, \"author\"?, \"url\"?, \"generator\"?, \"notes\"?}. The file then goes through `asset import` (or `asset reimport` with --replace), so type, size, dependencies, and metadata are checked the same way; output without a license from the hook or its config is rejected. Projects work without any hook.",
        gpu: NO_GPU,
        defaults: &[("--to", "assets/ itself"), ("--replace", "none (imports a new asset)")],
        example: "asset generate my_game icons red_crate --to props --dry-run --json",
    },
    Operation {
        name: "asset list",
        usage: "asset list <project-root> [--json]",
        summary: "List imported assets with ID, dependencies, provenance, settings, and referencing scenes. Fails on missing files or dependencies, invalid or duplicate metadata; warns on files changed since import, files without .rmeta, and assets without a license.",
        gpu: NO_GPU,
        defaults: &[],
        example: "asset list my_game --json",
    },
    Operation {
        name: "preset list",
        usage: "preset list [--json]",
        summary: "List the starter art-direction presets and every value each one writes.",
        gpu: NO_GPU,
        defaults: &[],
        example: "preset list --json",
    },
    Operation {
        name: "preset apply",
        usage: "preset apply <scene-path> <preset> [--dry-run] [--json]",
        summary: "Apply an art-direction preset as one scene patch: sun, ambient and sky light, tone mapping, background color, perspective camera field of view, and HUD text size and color. Creates a Sun entity when the scene has no directional light. The values stay ordinary, editable scene data; reapplying edits the same entities.",
        gpu: NO_GPU,
        defaults: &[("--dry-run", "false")],
        example: "preset apply my_game/scenes/main.rscene golden_hour --dry-run --json",
    },
    Operation {
        name: "schema",
        usage: "schema [--json]",
        summary: "Print this catalog: operations, scene sections and components with defaults, examples, units, ranges, and GPU cost.",
        gpu: NO_GPU,
        defaults: &[],
        example: "schema --json",
    },
];

/// Documentation for one saved field. `path` is a JSON pointer relative to
/// the section; enum variants appear as keys, as they do in scene files.
struct Field {
    path: &'static str,
    unit: &'static str,
    range: &'static str,
    doc: &'static str,
}

const fn field(
    path: &'static str,
    unit: &'static str,
    range: &'static str,
    doc: &'static str,
) -> Field {
    Field {
        path,
        unit,
        range,
        doc,
    }
}

const RGB: &str = "linear RGB";
const METRES: &str = "m";
const RADIANS: &str = "rad";

/// A top-level entity key or a registered component.
struct Section {
    key: &'static str,
    summary: &'static str,
    gpu: &'static str,
    /// Scene form of an authored, non-default value.
    example: fn() -> Value,
    fields: &'static [Field],
}

/// A registered component. Its fields come from its reflected type.
struct ComponentSection {
    key: &'static str,
    summary: &'static str,
    gpu: &'static str,
    /// Scene form of an authored, non-default value.
    example: fn() -> Value,
}

const ENTITY_SECTIONS: &[Section] = &[
    Section {
        key: "id",
        summary: "Persistent object ID. Unique in the scene; keep it when editing so references and patches stay valid.",
        gpu: NO_GPU,
        example: || json!("6f1c1d6e-9a53-4a6d-8f59-2c3e1c0c5a10"),
        fields: &[field("", "UUID", "unique", "")],
    },
    Section {
        key: "parent",
        summary: "Parent object ID; the transform is relative to the parent.",
        gpu: NO_GPU,
        example: || Value::Null,
        fields: &[field("", "UUID", "an existing object; no cycles", "null for a root object")],
    },
    Section {
        key: "name",
        summary: "Object name. Unique in the scene, so scenarios and queries can find it.",
        gpu: NO_GPU,
        example: || json!("Crate"),
        fields: &[field("", "", "unique", "")],
    },
    Section {
        key: "classes",
        summary: "Reusable class names. One class selects any number of objects, for queries and GPU physics rules.",
        gpu: NO_GPU,
        example: || json!(["crates", "breakable"]),
        fields: &[field("", "", "non-empty, no repeats", "")],
    },
    Section {
        key: "visible",
        summary: "Whether the object and its children render.",
        gpu: NO_GPU,
        example: || json!(true),
        fields: &[field("", "", "", "null means visible")],
    },
    Section {
        key: "transform",
        summary: "Position, rotation, and scale relative to the parent.",
        gpu: NO_GPU,
        example: || json!({"position": [2.0, 0.5, -1.0], "rotation": [0.0, 0.785, 0.0], "scale": [1.0, 1.0, 1.0]}),
        fields: &[
            field("/position", METRES, "", "[x, y, z]; +Y is up and -Z is forward"),
            field("/rotation", RADIANS, "", "Euler angles around X, Y, and Z"),
            field("/scale", "factor", "> 0 on each axis", ""),
        ],
    },
    Section {
        key: "mesh_renderer",
        summary: "Mesh and material to draw.",
        gpu: "one draw per mesh and material batch; mesh and texture memory",
        example: || json!({
            "mesh": {"BuiltinPrimitive": "Torus"},
            "material": {"Inline": {
                "model": "Pbr", "alpha_mode": "Opaque",
                "base_color": [0.8, 0.5, 0.2, 1.0], "emissive": [0.0, 0.0, 0.0],
                "metallic": 0.0, "roughness": 0.6,
                "base_color_texture": null, "normal_texture": null,
                "metallic_roughness_texture": null, "occlusion_texture": null,
                "emissive_texture": null
            }},
            "cast_shadows": true, "receive_shadows": true
        }),
        fields: &[
            field("/mesh/BuiltinPrimitive", "", "Cube, Sphere, Triangle, Plane, Tetrahedron, Octahedron, Dodecahedron, Icosahedron, Pyramid, Cylinder, Cone, Torus, Quad", "other variants: \"BuiltinCube\", \"BuiltinSphere\", {\"AssetPath\": \"assets/model.gltf\"}"),
            field("/material/Inline/model", "", "Pbr | Unlit", "other variant: \"BuiltinError\""),
            field("/material/Inline/alpha_mode", "", "Opaque | {\"Mask\": {\"cutoff\": 0..1}} | Blend", ""),
            field("/material/Inline/base_color", "linear RGBA", "0..1", ""),
            field("/material/Inline/emissive", RGB, ">= 0", "HDR; values above 1 glow"),
            field("/material/Inline/metallic", "", "0..1", ""),
            field("/material/Inline/roughness", "", "0..1", ""),
            field("/material/Inline/base_color_texture", "project path", "", "null for none; same for the other texture slots"),
            field("/material/Inline/normal_texture", "project path", "", ""),
            field("/material/Inline/metallic_roughness_texture", "project path", "", "glTF layout: roughness in G, metallic in B"),
            field("/material/Inline/occlusion_texture", "project path", "", ""),
            field("/material/Inline/emissive_texture", "project path", "", ""),
            field("/cast_shadows", "", "", ""),
            field("/receive_shadows", "", "", ""),
        ],
    },
    Section {
        key: "camera",
        summary: "A view. The active camera with the highest priority renders.",
        gpu: "one view; inactive cameras cost nothing",
        example: || json!({"projection": {"Perspective": {"vertical_fov_radians": 1.0, "near": 0.1, "far": 500.0}}, "active": true, "priority": 1}),
        fields: &[
            field("/projection/Perspective/vertical_fov_radians", RADIANS, "0 < fov < 3.14", "other variant: {\"Orthographic\": {\"vertical_size\" (m), \"near\", \"far\"}}"),
            field("/projection/Perspective/near", METRES, "> 0", ""),
            field("/projection/Perspective/far", METRES, "> near", ""),
            field("/active", "", "", ""),
            field("/priority", "", "i32", "higher wins among active cameras"),
        ],
    },
    Section {
        key: "physics_body",
        summary: "Where the body simulates. Needs a collider to collide.",
        gpu: "Gpu: compute per fixed tick; nothing is read back unless rusting.physics_sync asks for state",
        example: || json!({"simulation": "Gpu", "solver": "Simplified", "custom_shader": null}),
        fields: &[
            field("/simulation", "", "None | Static | Cpu | Gpu", "Cpu bodies are readable by gameplay every tick"),
            field("/solver", "", "Full | Simplified | NoCollision | Custom | Space", "GPU compute profile; Custom needs custom_shader"),
            field("/custom_shader", "project path", "", "a compute shader; null unless solver is Custom"),
        ],
    },
    Section {
        key: "rigid_body",
        summary: "Mass and motion of a simulated body.",
        gpu: NO_GPU,
        example: || json!({"kind": "Dynamic", "mass": 5.0, "linear_velocity": [0.0, 2.0, 0.0], "angular_velocity": [0.0, 0.0, 0.0], "gravity_scale": 1.0}),
        fields: &[
            field("/kind", "", "Dynamic | Kinematic | Fixed", ""),
            field("/mass", "kg", "> 0", ""),
            field("/linear_velocity", "m/s", "", "initial velocity"),
            field("/angular_velocity", "rad/s", "", "initial spin"),
            field("/gravity_scale", "factor", "", "0 disables gravity"),
        ],
    },
    Section {
        key: "collider",
        summary: "Collision shape in the body's local space.",
        gpu: NO_GPU,
        example: || json!({"shape": {"Box": {"half_extents": [0.5, 0.5, 0.5]}}, "friction": 0.6, "restitution": 0.2, "sensor": false}),
        fields: &[
            field("/shape/Box/half_extents", METRES, "> 0", "other variants: {\"Sphere\": {\"radius\"}}, {\"Capsule\": {\"half_height\", \"radius\"}}, \"ConvexMesh\", \"TriangleMesh\" (static only)"),
            field("/friction", "", ">= 0", ""),
            field("/restitution", "", "0..1", "bounciness"),
            field("/sensor", "", "", "reports overlaps without pushing"),
        ],
    },
    Section {
        key: "collision_layers",
        summary: "Which bodies may collide: both must list each other's membership in their filters.",
        gpu: NO_GPU,
        example: || json!({"memberships": 2, "filters": 1}),
        fields: &[
            field("/memberships", "bitmask", "u32", ""),
            field("/filters", "bitmask", "u32", ""),
        ],
    },
    Section {
        key: "gpu_physics_watch",
        summary: "GPU conditions that raise named gameplay events for this body.",
        gpu: "evaluated in GPU physics; only matching events are read back",
        example: || json!({"rules": []}),
        fields: &[field("/rules", "", "", "see GpuPhysicsRule: event, condition, mode, payload, cooldown_seconds")],
    },
    Section {
        key: "directional_light",
        summary: "Sun-like light from the object's forward direction.",
        gpu: "one shadow pass when shadows is true",
        example: || json!({"color": [1.0, 0.95, 0.9], "illuminance": 50000.0, "shadows": true}),
        fields: &[
            field("/color", RGB, "0..1", ""),
            field("/illuminance", "lux", ">= 0", "100000 is direct sunlight"),
            field("/shadows", "", "", ""),
        ],
    },
    Section {
        key: "point_light",
        summary: "Light radiating from the object's position.",
        gpu: "shading cost per lit pixel; up to 64 lights per frame (16 at Eco), the rest are dropped",
        example: || json!({"color": [1.0, 0.8, 0.6], "intensity": 800.0, "range": 8.0}),
        fields: &[
            field("/color", RGB, "0..1", ""),
            field("/intensity", "renderer units", ">= 0", ""),
            field("/range", METRES, "> 0", "light reaches zero here"),
        ],
    },
    Section {
        key: "spot_light",
        summary: "Cone of light along the object's forward direction.",
        gpu: "as point_light",
        example: || json!({"color": [1.0, 1.0, 1.0], "intensity": 1200.0, "range": 12.0, "inner_angle": 0.3, "outer_angle": 0.5}),
        fields: &[
            field("/color", RGB, "0..1", ""),
            field("/intensity", "renderer units", ">= 0", ""),
            field("/range", METRES, "> 0", ""),
            field("/inner_angle", RADIANS, "0..outer_angle", "fully lit cone"),
            field("/outer_angle", RADIANS, "inner_angle..1.57", "light reaches zero here"),
        ],
    },
];

const COMPONENT_SECTIONS: &[ComponentSection] = &[
    ComponentSection {
        key: "rusting.ambient_light",
        summary: "Flat light added everywhere.",
        gpu: NO_GPU,
        example: || json!({"color": [0.8, 0.85, 1.0], "intensity": 0.2}),
    },
    ComponentSection {
        key: "rusting.sky_light",
        summary: "Hemisphere light: up-facing surfaces see sky_color, down-facing ones ground_color.",
        gpu: NO_GPU,
        example: || json!({"sky_color": [0.5, 0.7, 1.0], "ground_color": [0.3, 0.25, 0.2], "intensity": 0.4}),
    },
    ComponentSection {
        key: "rusting.tone_mapping",
        summary: "Exposure and the curve that maps HDR color to the display. The first one found is used.",
        gpu: NO_GPU,
        example: || json!({"mapper": "Aces", "exposure": 1.2}),
    },
    ComponentSection {
        key: "rusting.background",
        summary: "Clear color behind the scene. The one on the entity with the lowest ID is used; without one the game keeps its render settings.",
        gpu: NO_GPU,
        example: || json!({"color": [0.5, 0.7, 0.9, 1.0]}),
    },
    ComponentSection {
        key: "rusting.render_bounds",
        summary: "Render-only visibility bounds in local space, separate from the collider.",
        gpu: "smaller bounds let culling skip more draws",
        example: || json!({"Sphere": {"center": [0.0, 0.5, 0.0], "radius": 1.0}}),
    },
    ComponentSection {
        key: "rusting.physics_sync",
        summary: "What a GPU body sends back to the CPU.",
        gpu: "SelectedState and FullState read back 144 bytes per body per fixed tick; Events reads back only fired events",
        example: || json!("SelectedState"),
    },
    ComponentSection {
        key: "rusting.auto_simulation",
        summary: "Lets the engine choose CPU or GPU simulation for the body.",
        gpu: NO_GPU,
        example: || json!({}),
    },
    ComponentSection {
        key: "rusting.player_controller",
        summary: "First-person walking body. Reads the player.* actions; parent a camera to it at eye height.",
        gpu: NO_GPU,
        example: || json!({"walk_speed": 5.0, "sprint_multiplier": 1.5, "jump_speed": 6.0, "gravity": 12.0, "look_sensitivity": 0.003, "collision_mask": 1, "yaw": 1.57, "pitch": 0.0}),
    },
    ComponentSection {
        key: "rusting.tween",
        summary: "Animates one Transform property from `from` to `to` on the fixed step.",
        gpu: NO_GPU,
        example: || json!({"property": "Scale", "from": [1.0, 1.0, 1.0], "to": [1.2, 1.2, 1.2], "duration": 0.4, "delay": 0.0, "easing": "BackOut", "repeat": "PingPong"}),
    },
    ComponentSection {
        key: "rusting.sound_cue",
        summary: "Sends a SoundEvent when the body starts touching something or game code calls trigger(). The game plays the clip; the engine has no audio output.",
        gpu: NO_GPU,
        example: || json!({"clip": "sounds/hit.ogg", "volume": 0.8, "on_collision": true}),
    },
    ComponentSection {
        key: "rusting.burst_emitter",
        summary: "Spawns particles that fly out, fall, and shrink, when the body starts touching something or game code calls trigger(). Particles copy the emitter's mesh.",
        gpu: NO_GPU,
        example: || json!({"count": 20, "speed": 4.0, "lifetime": 0.8, "particle_scale": 0.1, "gravity": 9.81, "on_collision": false}),
    },
    ComponentSection {
        key: "rusting.hud",
        summary: "Text label or button drawn over the game view. A clicked button sends HudButtonPressed.",
        gpu: "a few egui triangles",
        example: || json!({"text": "Score: 0", "anchor": "TopRight", "offset": [24.0, 24.0], "font_size": 24.0, "color": [1.0, 0.9, 0.4, 1.0], "button": false, "requires": null}),
    },
    ComponentSection {
        key: "rusting.counter",
        summary: "Named integer. HUD text shows it as {name}; pickups add to it.",
        gpu: NO_GPU,
        example: || json!({"name": "coins", "value": 0, "target": 5}),
    },
    ComponentSection {
        key: "rusting.pickup",
        summary: "Collected once when a platformer or player body touches its collider (make it a sensor): adds value to the counter, hides the entity, and triggers its sound cue and burst emitter.",
        gpu: NO_GPU,
        example: || json!({"counter": "coins", "value": 1, "requires": null, "collected": false}),
    },
    ComponentSection {
        key: "rusting.tile_map",
        summary: "Grid of square tiles written as text rows, top row first; the Transform position is the top-left corner. Tiles spawn as unsaved Quad entities, and each run of solid tiles in a row gets one fixed box collider.",
        gpu: "one instanced Quad draw per tile kind",
        example: || json!({"tile_size": 0.5, "rows": ["....", "#..#", "####"], "tiles": {"#": {"color": [0.3, 0.6, 0.3, 1.0], "texture": null, "solid": true}}}),
    },
    ComponentSection {
        key: "rusting.platformer_controller",
        summary: "Side-view run and jump in the XY plane. Reads player.left, player.right and player.jump; parent an orthographic camera to follow.",
        gpu: NO_GPU,
        example: || json!({"run_speed": 6.0, "jump_speed": 9.0, "gravity": 25.0, "collision_mask": 1}),
    },
];

/// The scene form of an entity holding every built-in section and every
/// registered component at its default value.
fn defaults() -> (Map<String, Value>, Map<String, Value>) {
    let mut app = App::new();
    app.add_plugin(AssetPlugin)
        .expect("a new app accepts the asset plugin");
    let world = app.world_mut();
    let entity = world
        .spawn((
            SceneId(Uuid::nil()),
            Name("Default".into()),
            Transform::default(),
            Camera::default(),
            PhysicsBody::default(),
            RigidBody::default(),
            Collider::default(),
            CollisionLayers::default(),
            GpuPhysicsWatch::default(),
            DirectionalLight::default(),
            PointLight::default(),
            SpotLight::default(),
        ))
        .id();
    for name in registered_component_names(world) {
        add_registered_component(world, entity, &name)
            .expect("registered components have defaults");
    }
    let components = registered_component_values(world, entity)
        .expect("defaults serialize")
        .into_iter()
        .map(|(name, text)| {
            (
                name,
                serde_json::from_str(&text).expect("registry writes JSON"),
            )
        })
        .collect();
    let document = scene_document(world, "defaults").expect("defaults save");
    let Ok(Value::Object(mut entity)) =
        serde_json::to_value(&document.entities[0])
    else {
        unreachable!("an entity saves as an object");
    };
    entity.remove("components");
    (entity, components)
}

fn section_value(section: &Section, default: Option<&Value>) -> Value {
    json!({
        "key": section.key,
        "summary": section.summary,
        "gpu_cost": section.gpu,
        "default": default,
        "example": (section.example)(),
        "fields": section.fields.iter().map(|field| json!({
            "path": field.path,
            "unit": field.unit,
            "range": field.range,
            "doc": field.doc,
        })).collect::<Vec<_>>(),
    })
}

/// Field documentation of a reflected type, in the layout of [`Field`].
/// Map keys show as `*`, enum payloads under their variant name, and
/// containers appear only when they carry a doc.
fn reflected_fields(info: &TypeInfo) -> Vec<Value> {
    let mut fields = Vec::new();
    describe(info, &Hints::default(), String::new(), &mut fields);
    fields
}

fn describe(
    info: &TypeInfo,
    hints: &Hints,
    path: String,
    out: &mut Vec<Value>,
) {
    let entry = |range: String| json!({"path": path, "unit": hints.unit, "range": range, "doc": hints.doc});
    match info {
        TypeInfo::Option(inner) => describe(inner, hints, path, out),
        TypeInfo::Struct(info) => {
            if !hints.doc.is_empty() {
                out.push(entry(String::new()));
            }
            for field in &info.fields {
                describe(
                    &field.ty,
                    &field.hints,
                    format!("{path}/{}", field.name),
                    out,
                );
            }
        }
        TypeInfo::Map(item) if matches!(**item, TypeInfo::Struct(_)) => {
            if !hints.doc.is_empty() {
                out.push(entry(String::new()));
            }
            describe(item, &Hints::default(), format!("{path}/*"), out);
        }
        TypeInfo::Enum(info) if !info.is_unit_only() => {
            out.push(entry(variant_names(info)));
            for variant in &info.variants {
                let at = format!("{path}/{}", variant.name);
                match &variant.fields {
                    VariantFields::Unit => {}
                    VariantFields::Tuple(items) if items.len() == 1 => {
                        describe(&items[0], &Hints::default(), at, out);
                    }
                    VariantFields::Tuple(items) => {
                        for (index, item) in items.iter().enumerate() {
                            describe(
                                item,
                                &Hints::default(),
                                format!("{at}/{index}"),
                                out,
                            );
                        }
                    }
                    VariantFields::Struct(fields) => {
                        for field in fields {
                            describe(
                                &field.ty,
                                &field.hints,
                                format!("{at}/{}", field.name),
                                out,
                            );
                        }
                    }
                }
            }
        }
        leaf => out.push(entry(range_text(leaf, hints))),
    }
}

fn variant_names(info: &EnumInfo) -> String {
    info.variants
        .iter()
        .map(|variant| variant.name)
        .collect::<Vec<_>>()
        .join(" | ")
}

fn range_text(info: &TypeInfo, hints: &Hints) -> String {
    match (hints.min, hints.max) {
        (Some(min), Some(max)) => return format!("{min}..{max}"),
        (Some(min), None) => return format!(">= {min}"),
        (None, Some(max)) => return format!("<= {max}"),
        (None, None) => {}
    }
    match info {
        TypeInfo::Int { name, .. } => (*name).to_owned(),
        TypeInfo::Enum(info) => variant_names(info),
        TypeInfo::Entity | TypeInfo::Handle(_) => info.kind_name().to_owned(),
        TypeInfo::Array(item, _)
        | TypeInfo::List(item)
        | TypeInfo::Map(item) => range_text(item, &Hints::default()),
        _ => String::new(),
    }
}

fn component_value(
    section: &ComponentSection,
    info: &TypeInfo,
    default: Option<&Value>,
) -> Value {
    json!({
        "key": section.key,
        "summary": section.summary,
        "gpu_cost": section.gpu,
        "default": default,
        "example": (section.example)(),
        "fields": reflected_fields(info),
    })
}

fn reflected_types<'a>(
    types: impl Iterator<Item = (&'a str, &'a TypeInfo)>,
) -> Vec<Value> {
    types
        .map(
            |(key, info)| json!({"key": key, "fields": reflected_fields(info)}),
        )
        .collect()
}

/// The full catalog as JSON.
#[must_use]
pub fn catalog() -> Value {
    let (entity, components) = defaults();
    let registry = SceneComponentRegistry::default();
    let types = TypeRegistry::default();
    json!({
        "catalog_version": SCHEMA_CATALOG_VERSION,
        "engine_version": env!("CARGO_PKG_VERSION"),
        "scene_format_version": SCENE_FORMAT_VERSION,
        "operations": OPERATIONS.iter().map(|operation| json!({
            "name": operation.name,
            "usage": format!("rusting {}", operation.usage),
            "summary": operation.summary,
            "gpu_cost": operation.gpu,
            "defaults": operation.defaults.iter()
                .map(|(flag, value)| (flag.to_string(), json!(value)))
                .collect::<Map<_, _>>(),
            "example": format!("rusting {}", operation.example),
        })).collect::<Vec<_>>(),
        "scene": {
            "file": "JSON object with format_version, name, entities, and render {quality: Auto|Eco|Balanced|High, culling: Auto|Disabled|Frustum|FrustumAndOcclusion}",
            "entity_sections": ENTITY_SECTIONS.iter()
                .map(|section| section_value(section, entity.get(section.key)))
                .collect::<Vec<_>>(),
            "components_note": "Registered components are stored under `components` as JSON strings keyed by name. Scenario `expect` paths and this catalog show them parsed. Fields come from each type's reflection: `*` stands for any map key, enum payloads sit under the variant name, an entity reference saves as the target's object ID, an asset handle as {\"$asset\": path}, and a component with migrations saves \"$version\". Unknown fields fail the load; register a rename or remove migration instead.",
            "components": COMPONENT_SECTIONS.iter()
                .map(|section| {
                    let info = registry.info(section.key)
                        .expect("catalog components are registered");
                    component_value(section, info, components.get(section.key))
                })
                .collect::<Vec<_>>(),
        },
        "resources": reflected_types(types.resources()),
        "asset_types": reflected_types(types.assets()),
        "physics_sync_readback_bytes": PhysicsSyncMode::STATE_READBACK_BYTES,
        "scene_patch": {
            "file": "JSON object: expected_revision (optional, from scene inspect), operations",
            "paths": "JSON pointers into the entity's scene form with registered components parsed, as in scenario expect paths; /id cannot change",
            "operations": {
                "create": "{\"op\": \"create\", \"entity\": {\"name\": \"Crate\", \"parent\": null}}: a missing id gets a new UUID",
                "set": "{\"op\": \"set\", \"id\": UUID, \"path\": \"/transform/position/1\", \"value\": 2.0, \"expected\": 1.0}: expected is optional",
                "remove": "{\"op\": \"remove\", \"id\": UUID, \"path\": \"/collider\"}",
                "reparent": "{\"op\": \"reparent\", \"id\": UUID, \"parent\": UUID or null}",
                "duplicate": "{\"op\": \"duplicate\", \"id\": UUID, \"new_id\": UUID, \"name\": \"Copy\"}: one entity, no children; unnamed unless name is given",
                "delete": "{\"op\": \"delete\", \"id\": UUID}: also deletes descendants",
            },
            "errors": "SCENE_CONFLICT (revision or expected value differs), PATCH_OPERATION, PATCH_INVALID",
        },
        "scenario": {
            "file": "JSON object: name, seed (u64, default 0), ticks (last tick run), capture_size ([w, h], default [1280, 720]), steps",
            "steps": {
                "press": "{\"tick\": 5, \"press\": \"player.jump\"}: press every input bound to the action before the tick's update",
                "release": "{\"tick\": 6, \"release\": \"player.jump\"}",
                "expect": "{\"tick\": 30, \"until\": 60, \"expect\": {\"entity\": \"Cube\", \"path\": \"/transform/position/1\", \"less_than\": 0.0}}: JSON pointer into the entity's scene form; equals (with tolerance), greater_than, less_than",
                "expect_events": "{\"tick\": 60, \"expect_events\": {\"kind\": \"collision\", \"entity\": \"Cube\", \"at_least\": 1}}: events gameplay saw from tick 0",
                "capture": "{\"tick\": 30, \"capture\": \"shots/tick30.png\"}: path relative to the scenario file; skipped without Vulkan",
            },
            "tick_length_seconds": 1.0 / 60.0,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::runtime::{
        load_scene_document, set_registered_component, SceneDocument,
        SceneLoadMode,
    };

    /// JSON pointers of every leaf. Arrays are leaves.
    fn leaves(value: &Value, prefix: String, out: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) if !map.is_empty() => {
                for (key, value) in map {
                    leaves(value, format!("{prefix}/{key}"), out);
                }
            }
            _ => {
                out.insert(prefix);
            }
        }
    }

    /// Whether `path` is `pattern`, or under it when `prefix` is set, with
    /// `*` in the pattern matching any one segment.
    fn matches(pattern: &str, path: &str, prefix: bool) -> bool {
        let mut pattern = pattern.split('/');
        let mut path = path.split('/');
        loop {
            match (pattern.next(), path.next()) {
                (None, None) => return true,
                (None, Some(_)) => return prefix,
                (Some(_), None) => return false,
                (Some(wanted), Some(segment)) => {
                    if wanted != "*" && wanted != segment {
                        return false;
                    }
                }
            }
        }
    }

    /// Every saved leaf is documented, and every documented path has a
    /// saved leaf at or under it in the example or the default.
    fn check_fields(
        key: &str,
        documented: &[String],
        example: &Value,
        default: Option<&Value>,
    ) {
        let mut saved = BTreeSet::new();
        leaves(example, String::new(), &mut saved);
        if let Some(default) = default.filter(|value| !value.is_null()) {
            leaves(default, String::new(), &mut saved);
        }
        saved.remove("");
        for path in &saved {
            assert!(
                documented
                    .iter()
                    .any(|pattern| matches(pattern, path, false)),
                "{key}{path} is saved but not documented"
            );
        }
        for pattern in documented.iter().filter(|path| !path.is_empty()) {
            assert!(
                saved.iter().any(|path| matches(pattern, path, true)),
                "{key}{pattern} is documented but not saved"
            );
        }
    }

    fn documented(section: &Section) -> Vec<String> {
        section
            .fields
            .iter()
            .map(|field| field.path.to_owned())
            .collect()
    }

    #[test]
    fn every_saved_field_is_documented_and_every_section_is_listed() {
        let (entity, components) = defaults();
        let listed: BTreeSet<_> =
            ENTITY_SECTIONS.iter().map(|section| section.key).collect();
        let saved: BTreeSet<_> = entity.keys().map(String::as_str).collect();
        assert_eq!(listed, saved, "entity sections and saved keys differ");
        for section in ENTITY_SECTIONS {
            check_fields(
                section.key,
                &documented(section),
                &(section.example)(),
                entity.get(section.key),
            );
        }
        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        let registered = registered_component_names(app.world());
        let mut listed: Vec<_> = COMPONENT_SECTIONS
            .iter()
            .map(|section| section.key)
            .collect();
        listed.sort_unstable();
        assert_eq!(listed, registered, "catalog and component registry differ");
        let registry = SceneComponentRegistry::default();
        for section in COMPONENT_SECTIONS {
            let fields = reflected_fields(registry.info(section.key).unwrap());
            let paths: Vec<String> = fields
                .iter()
                .map(|field| field["path"].as_str().unwrap().to_owned())
                .collect();
            check_fields(
                section.key,
                &paths,
                &(section.example)(),
                components.get(section.key),
            );
        }
    }

    #[test]
    fn examples_load_through_the_scene_parser_and_registry_unchanged() {
        let mut entity: Map<String, Value> = ENTITY_SECTIONS
            .iter()
            .map(|section| (section.key.to_owned(), (section.example)()))
            .collect();
        let components: Map<String, Value> = COMPONENT_SECTIONS
            .iter()
            .map(|section| {
                let text = (section.example)().to_string();
                (section.key.to_owned(), Value::String(text))
            })
            .collect();
        entity.insert("components".into(), Value::Object(components));
        let document: SceneDocument = serde_json::from_value(json!({
            "format_version": SCENE_FORMAT_VERSION,
            "name": "examples",
            "entities": [entity],
        }))
        .expect("examples parse as a scene");

        let mut app = App::new();
        app.add_plugin(AssetPlugin).unwrap();
        let world = app.world_mut();
        load_scene_document(world, &document, SceneLoadMode::Replace)
            .expect("examples load");
        let saved = scene_document(world, "examples").unwrap();
        let mut saved = serde_json::to_value(&saved.entities[0]).unwrap();
        let mut expected = serde_json::to_value(&document.entities[0]).unwrap();
        // Compare components as values: field order may differ.
        for value in [&mut saved, &mut expected] {
            for component in
                value["components"].as_object_mut().unwrap().values_mut()
            {
                *component =
                    serde_json::from_str(component.as_str().unwrap()).unwrap();
            }
        }
        assert_eq!(saved, expected);

        // Each component example also restores alone through the registry.
        let entity = world.spawn(SceneId(Uuid::new_v4())).id();
        for section in COMPONENT_SECTIONS {
            set_registered_component(
                world,
                entity,
                section.key,
                &(section.example)().to_string(),
            )
            .unwrap_or_else(|error| panic!("{}: {error}", section.key));
        }
    }

    #[test]
    fn catalog_lists_every_operation_with_defaults_and_cost() {
        let catalog = catalog();
        let operations = catalog["operations"].as_array().unwrap();
        assert_eq!(operations.len(), OPERATIONS.len());
        for operation in operations {
            assert!(operation["gpu_cost"]
                .as_str()
                .is_some_and(|c| !c.is_empty()));
            assert!(operation["example"].as_str().unwrap().starts_with(
                &format!("rusting {}", operation["name"].as_str().unwrap())
            ));
        }
        let camera = &catalog["scene"]["entity_sections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|section| section["key"] == "camera")
            .unwrap()["default"];
        assert_eq!(camera["active"], false);
        let player = catalog["scene"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|section| section["key"] == "rusting.player_controller")
            .unwrap();
        assert_eq!(player["default"]["walk_speed"], 4.0);
        let pitch = player["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|field| field["path"] == "/pitch")
            .unwrap();
        assert_eq!(pitch["range"], "-1.55..1.55");
        assert_eq!(pitch["unit"], "rad");
        let tiles = catalog["scene"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|section| section["key"] == "rusting.tile_map")
            .unwrap();
        assert!(tiles["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["path"] == "/tiles/*/solid"));
        let resources = catalog["resources"].as_array().unwrap();
        assert!(resources
            .iter()
            .any(|resource| resource["key"] == "rusting.render_settings"));
        let material = &catalog["asset_types"][0];
        assert_eq!(material["key"], "rusting.material");
        assert!(material["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["path"] == "/alpha_mode/Mask/cutoff"));
    }
}
