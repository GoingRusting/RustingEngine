use rusting_engine::prelude::*;

/// Drones enter on this circle around the core.
const SPAWN_RADIUS: f32 = 11.0;
/// Drones fly at this height, and so do bolts.
const FLIGHT_Y: f32 = 1.0;
/// Ticks between drones at the start, and the shortest gap later on.
const FIRST_GAP: u64 = 110;
const LAST_GAP: u64 = 35;
/// Ticks between two shots.
const RELOAD: i32 = 12;
const BOLT_SPEED: f32 = 22.0;
/// A bolt this close to a drone destroys it.
const HIT_RADIUS: f32 = 0.8;
/// A drone this close to the core damages it.
const CORE_RADIUS: f32 = 1.4;
/// Bolts past this distance from the core are removed.
const ARENA_RADIUS: f32 = 18.0;
/// Golden angle: consecutive drones come from well-spread directions.
const SPREAD: f32 = 2.399_963;

fn value(scene: &mut GameScene<'_>, name: &str) -> i32 {
    scene.counter(name).map_or(0, |counter| counter.value)
}

fn add(scene: &mut GameScene<'_>, name: &str, amount: i32) {
    if let Some(mut counter) = scene.counter(name) {
        counter.value += amount;
    }
}

fn complete(scene: &mut GameScene<'_>, name: &str) -> bool {
    scene
        .counter(name)
        .is_some_and(|counter| counter.complete())
}

fn flat_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// Unit direction from `from` to `to` on the ground plane.
fn heading(from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
    let (dx, dz) = (to[0] - from[0], to[2] - from[2]);
    let length = dx.hypot(dz).max(1e-4);
    [dx / length, 0.0, dz / length]
}

/// The tick the next drone enters on: the gap shrinks by 5 ticks per drone.
fn spawn_tick(spawned: u64) -> u64 {
    (0..spawned)
        .map(|index| FIRST_GAP.saturating_sub(5 * index).max(LAST_GAP))
        .sum::<u64>()
        + 60
}

/// The ground point under the mouse cursor, if it is over the floor.
fn aim_point(scene: &mut GameScene<'_>) -> Option<[f32; 3]> {
    let (origin, direction) = scene.pointer_ray()?;
    let hit = scene.raycast(origin, direction, 100.0)?;
    Some(hit.point)
}

fn update(scene: &mut GameScene<'_>, time: &FrameTime) {
    if scene.pressed("restart") {
        scene.restart();
        return;
    }
    if complete(scene, "kills") || complete(scene, "breaches") {
        return;
    }
    let tick = time.fixed_tick as i32;
    let step = time.delta.as_secs_f32();
    let core = [0.0, FLIGHT_Y, 0.0];

    // The turret turns toward the cursor and fires along its barrel.
    if let Some(target) = aim_point(scene) {
        let [x, _, z] = heading(core, target);
        scene
            .object("Turret")
            .set_rotation([0.0, (-x).atan2(-z), 0.0]);
    }
    let yaw = scene.object("Turret").rotation()[1];
    let aim = [-yaw.sin(), 0.0, -yaw.cos()];
    if scene.held("fire") && tick - value(scene, "reload") >= RELOAD {
        if let Some(mut reload) = scene.counter("reload") {
            reload.value = tick;
        }
        add(scene, "shots", 1);
        let name = format!("Bolt {}", value(scene, "shots"));
        let muzzle = [aim[0] * 1.6, FLIGHT_Y, aim[2] * 1.6];
        scene.spawn_copy("Bolt", name.as_str(), muzzle);
        scene.set_visible(&name, true);
        // The bolt flies the way it faces.
        scene.object(&name).set_rotation([0.0, yaw, 0.0]);
    }

    let spawned = value(scene, "spawned");
    if time.fixed_tick >= spawn_tick(spawned as u64) {
        let name = format!("Drone {}", spawned + 1);
        let angle = spawned as f32 * SPREAD;
        let gate = [
            angle.sin() * SPAWN_RADIUS,
            FLIGHT_Y,
            angle.cos() * SPAWN_RADIUS,
        ];
        scene.spawn_copy("Drone", name.as_str(), gate);
        scene.set_visible(&name, true);
        add(scene, "spawned", 1);
    }

    let drones: Vec<(String, [f32; 3])> = scene
        .in_class("drone")
        .into_iter()
        .filter(|name| name != "Drone")
        .map(|name| {
            let position = scene.object(&name).position();
            (name, position)
        })
        .collect();
    let mut destroyed = Vec::new();
    for bolt in scene.in_class("bolt") {
        if bolt == "Bolt" {
            continue;
        }
        let yaw = scene.object(&bolt).rotation()[1];
        let (dx, dz) = (-yaw.sin(), -yaw.cos());
        scene.object(&bolt).move_by([
            dx * BOLT_SPEED * step,
            0.0,
            dz * BOLT_SPEED * step,
        ]);
        let position = scene.object(&bolt).position();
        let hit = drones.iter().find(|(name, drone)| {
            !destroyed.contains(name)
                && flat_distance(position, *drone) < HIT_RADIUS
        });
        if let Some((drone, _)) = hit {
            destroyed.push(drone.clone());
            scene.despawn(drone);
            scene.despawn(&bolt);
            add(scene, "kills", 1);
        } else if flat_distance(position, core) > ARENA_RADIUS {
            scene.despawn(&bolt);
        }
    }

    // Drones speed up as the kill count grows.
    let speed = (2.0 + 0.1 * value(scene, "kills") as f32).min(4.5);
    for (drone, position) in drones {
        if destroyed.contains(&drone) {
            continue;
        }
        if flat_distance(position, core) < CORE_RADIUS {
            scene.despawn(&drone);
            add(scene, "core", -1);
            add(scene, "breaches", 1);
            continue;
        }
        let [x, _, z] = heading(position, core);
        scene
            .object(&drone)
            .move_by([x * speed * step, 0.0, z * speed * step]);
    }
}

rusting_game!(update);
