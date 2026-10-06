use rusting_engine::prelude::*;
use rusting_engine::text_texture::{text_texture, TextStyle};

/// Shelf rows, as x positions, and the shelf heights the bears sit on.
const SHELF_ROWS: [f32; 4] = [-7.5, -2.5, 2.5, 7.5];
const SHELF_LEVELS: [f32; 2] = [0.95, 1.7];
/// Bears per shelf side and level, spread along the 22 m shelf.
const BEARS_PER_RUN: usize = 12;
/// A bear repeats a sound every this many ticks, and Big Button steps every
/// `STEP_EVERY` ticks while it walks.
const BEAR_EVERY: u64 = 12;
const STEP_EVERY: u64 = 30;
/// Big Button's walking speed in metres per tick (1.2 m/s at 60 Hz).
const MASCOT_SPEED: f32 = 0.02;
/// Floor points in the aisles and the cross aisles at both ends.
const WAYPOINTS: [[f32; 3]; 10] = [
    [-5.0, 1.3, -4.0],
    [0.0, 1.3, -4.0],
    [5.0, 1.3, -4.0],
    [-5.0, 1.3, -16.0],
    [0.0, 1.3, -16.0],
    [5.0, 1.3, -16.0],
    [-5.0, 1.3, -28.0],
    [0.0, 1.3, -28.0],
    [5.0, 1.3, -28.0],
    [10.0, 1.3, -16.0],
];
const AISLES: [(usize, usize); 12] = [
    (0, 1),
    (1, 2),
    (0, 3),
    (1, 4),
    (2, 5),
    (3, 6),
    (4, 7),
    (5, 8),
    (6, 7),
    (7, 8),
    (5, 9),
    (2, 9),
];
/// The points Big Button visits in turn; the graph finds the aisles between
/// them.
const ROUNDS: [usize; 5] = [1, 6, 9, 8, 0];

fn store_graph() -> WaypointGraph {
    let mut graph = WaypointGraph::default();
    for point in WAYPOINTS {
        graph.add_node(point);
    }
    for (a, b) in AISLES {
        graph.connect(a, b);
    }
    graph
}

/// Big Button's route as a list of waypoint nodes, closing back on its
/// start.
fn route(graph: &WaypointGraph) -> Vec<usize> {
    let mut nodes = vec![ROUNDS[0]];
    for (index, &from) in ROUNDS.iter().enumerate() {
        let to = ROUNDS[(index + 1) % ROUNDS.len()];
        let leg = graph.path(from, to).expect("the store is connected");
        nodes.extend_from_slice(&leg[1..]);
    }
    nodes
}

/// Where Big Button stands at `tick` and the yaw it faces: a pure function
/// of the tick, so restarts and replays walk the same way.
fn mascot_pose(graph: &WaypointGraph, tick: u64) -> ([f32; 3], f32) {
    let nodes = route(graph);
    let total = graph.length(&nodes);
    let mut along = (tick as f32 * MASCOT_SPEED) % total;
    for pair in nodes.windows(2) {
        let (from, to) = (graph.nodes[pair[0]], graph.nodes[pair[1]]);
        let length = graph.length(pair);
        if along <= length {
            let t = along / length;
            let position =
                [0, 1, 2].map(|axis| from[axis] + (to[axis] - from[axis]) * t);
            let yaw = (-(to[0] - from[0])).atan2(-(to[2] - from[2]));
            return (position, yaw);
        }
        along -= length;
    }
    (graph.nodes[nodes[0]], 0.0)
}

fn bear_name(index: usize) -> String {
    format!("Bear {index:03}")
}

/// Copies the hidden bear along both faces of every shelf.
fn stock_shelves(scene: &mut GameScene<'_>) {
    let mut index = 0;
    for x in SHELF_ROWS {
        for side in [-0.55, 0.55] {
            for y in SHELF_LEVELS {
                for slot in 0..BEARS_PER_RUN {
                    let z =
                        -6.0 - 20.0 * slot as f32 / (BEARS_PER_RUN - 1) as f32;
                    let name = bear_name(index);
                    scene.spawn_copy("Bear", name.as_str(), [x + side, y, z]);
                    scene.object(&name).set_rotation([
                        0.0,
                        side.signum() * 1.57,
                        0.0,
                    ]);
                    scene.set_visible(&name, true);
                    scene.set_visible(&format!("{name}/Head"), true);
                    index += 1;
                }
            }
        }
    }
}

/// Puts a "CAM n" plate under each monitor, drawn into a texture.
fn label_monitors(scene: &mut GameScene<'_>) {
    for camera in 1..=6 {
        let plate = text_texture(
            &format!("CAM {camera}"),
            TextStyle {
                size: 28.0,
                color: [230, 220, 160, 255],
                background: [12, 12, 12, 255],
                padding: 4,
                monospace: true,
            },
        );
        let texture = scene
            .world()
            .resource_mut::<AssetServer>()
            .textures
            .insert(plate);
        let material = scene.create_material(MaterialAsset {
            model: MaterialModel::Unlit,
            base_color_texture: Some(texture),
            ..MaterialAsset::default()
        });
        let monitor = scene.object(&format!("Monitor {camera}")).position();
        scene.spawn_cube_with_material(
            format!("Plate {camera}"),
            Transform {
                position: [monitor[0], monitor[1] - 0.25, monitor[2] + 0.02],
                scale: [0.22, 0.06, 0.001],
                ..Transform::default()
            },
            &CubeSpawn::default(),
            material,
        );
    }
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    scene.once("store", |scene| {
        stock_shelves(scene);
        label_monitors(scene);
        // A big, dull room for the shelf chorus.
        scene.set_bus_effect(
            "bears",
            BusEffect::Reverb {
                room: 0.85,
                damping: 0.5,
                mix: 0.35,
            },
            0.0,
        );
        scene.set_bus_effect(
            "bears",
            BusEffect::LowPass { cutoff_hz: 3500.0 },
            0.0,
        );
    });

    let tick = time.fixed_tick;
    let graph = store_graph();
    let (position, yaw) = mascot_pose(&graph, tick);
    let mut mascot = scene.object("Big Button");
    mascot.set_position(position);
    mascot.set_rotation([0.0, yaw, 0.0]);

    if tick % STEP_EVERY == 0 {
        scene.play_sound_with(
            "mascot/step.wav",
            Sound {
                volume: 0.9,
                bus: "mascot".into(),
                position: Some([position[0], 0.1, position[2]]),
                priority: 255,
                occlude: true,
                ..Sound::default()
            },
        );
    }

    // One bear at a time repeats its line in a pitched child's voice.
    if tick % BEAR_EVERY == 0 {
        let bears = SHELF_ROWS.len() * 2 * SHELF_LEVELS.len() * BEARS_PER_RUN;
        let bear = (scene.random(1) * bears as f32) as usize % bears;
        let at = scene.object(&bear_name(bear)).position();
        scene.play_sound_with(
            "bears/hello.wav",
            Sound {
                volume: 0.7,
                bus: "bears".into(),
                rate: 1.25 + 0.6 * scene.random(2),
                position: Some(at),
                priority: 64,
                occlude: true,
                captions: vec![Caption::new(0.0, 0.6, "[bear] Hello, friend!")],
                ..Sound::default()
            },
        );
    }
}

rusting_game!(update);
