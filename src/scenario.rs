//! Input-driven scenario tests.
//!
//! A scenario presses and releases named actions at fixed ticks, checks
//! reflected scene state and events, and optionally captures frames. It runs
//! inside the game process, so the game's own systems run as they would in
//! play. The same seed gives the same run: the scenario sets
//! [`RandomSeed`], and every tick advances by exactly one fixed step.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy_ecs::prelude::{Entity, World};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::rendering::capture::HeadlessCapture;
use crate::runtime::{
    picking, scene_document, ActionMap, Camera, CollisionEvent, EventQueue,
    FrameTime, GlobalTransform, InputBinding, MeshRenderer, Name, RandomSeed,
    RenderWorld, RuntimeInput, SceneId,
};
use crate::{App, AssetServer, Transform};

/// Environment variable naming the scenario file a game build runs instead
/// of opening a window.
pub const TEST_SCENARIO_ENV: &str = "RUSTING_TEST_SCENARIO";
/// Environment variable naming where the game writes its
/// [`ScenarioReport`] as JSON.
pub const TEST_REPORT_ENV: &str = "RUSTING_TEST_REPORT";

/// Time to advance before tick `tick`: nothing for tick 0, which only
/// extracts the loaded scene, then exactly one fixed step per tick.
#[must_use]
pub fn tick_delta(app: &App, tick: u32) -> Duration {
    if tick == 0 {
        Duration::ZERO
    } else {
        app.world().resource::<FrameTime>().fixed_delta
    }
}

/// Finds an entity by persistent ID or by name. With several matches, the
/// lowest persistent ID wins, so the choice does not depend on spawn order.
pub fn find_entity(
    world: &mut World,
    wanted: &str,
    accept: impl Fn(&World, Entity) -> bool,
) -> Option<Entity> {
    let id = Uuid::parse_str(wanted).ok();
    let mut query = world.query::<(Entity, Option<&SceneId>, Option<&Name>)>();
    let mut matches: Vec<_> = query
        .iter(world)
        .filter(|(_, scene_id, name)| {
            scene_id.is_some_and(|scene_id| Some(scene_id.0) == id)
                || name.is_some_and(|name| name.0 == wanted)
        })
        .map(|(entity, scene_id, _)| (scene_id.map(|id| id.0), entity))
        .collect();
    matches.sort();
    matches
        .into_iter()
        .map(|(_, entity)| entity)
        .find(|&entity| accept(world, entity))
}

/// Finds a camera by persistent ID or name.
pub fn find_camera(world: &mut World, wanted: &str) -> Option<Entity> {
    find_entity(world, wanted, |world, entity| {
        world.get::<Camera>(entity).is_some()
    })
}

/// The camera the last extracted frame renders from, at an image size.
pub struct CameraView {
    entity: Entity,
    camera: Camera,
    transform: GlobalTransform,
    extent: [u32; 2],
}

impl CameraView {
    /// The camera selected by the last render extraction.
    #[must_use]
    pub fn active(world: &World, extent: [u32; 2]) -> Option<Self> {
        let active = world.resource::<RenderWorld>().active_camera?;
        Some(Self {
            entity: active.entity,
            camera: *world.get::<Camera>(active.entity)?,
            transform: active.transform,
            extent,
        })
    }

    /// The object under `pixel`, found by casting a ray against mesh bounds.
    /// Returns its persistent ID, name, distance and world position, or a
    /// null `id` when the ray hits nothing.
    // ponytail: bounds-only picking, like gameplay clicks; a rendered ID
    // buffer would give exact silhouettes.
    pub fn pick(&self, world: &mut World, pixel: [u32; 2]) -> Value {
        let ray = picking::scene_ray(
            [pixel[0] as f32 + 0.5, pixel[1] as f32 + 0.5],
            [0.0, 0.0],
            [self.extent[0] as f32, self.extent[1] as f32],
            self.camera,
            self.transform,
        );
        let mut meshes =
            world.query::<(Entity, &MeshRenderer, &GlobalTransform)>();
        let assets = world.resource::<AssetServer>();
        let hit = ray.and_then(|ray| {
            meshes
                .iter(world)
                .filter_map(|(entity, renderer, transform)| {
                    let mesh = assets.meshes.get(renderer.mesh)?;
                    let distance =
                        picking::ray_mesh_bounds(ray, *transform, mesh)?;
                    Some((
                        distance,
                        entity,
                        ray.origin + ray.direction * distance,
                    ))
                })
                .min_by(|left, right| left.0.total_cmp(&right.0))
        });
        match hit {
            Some((distance, entity, point)) => json!({
                "pixel": pixel,
                "id": world.get::<SceneId>(entity).map(|id| id.0),
                "name": world.get::<Name>(entity).map(|name| &name.0),
                "distance": distance,
                "world_position": [point.x, point.y, point.z],
            }),
            None => json!({"pixel": pixel, "id": null}),
        }
    }

    /// Persistent ID, name, projection and transforms of the camera.
    #[must_use]
    pub fn data(&self, world: &World) -> Value {
        let transform = world.get::<Transform>(self.entity);
        json!({
            "id": world.get::<SceneId>(self.entity).map(|id| id.0),
            "name": world.get::<Name>(self.entity).map(|name| &name.0),
            "projection": format!("{:?}", self.camera.projection),
            "position": transform.map(|transform| transform.position),
            "rotation": transform.map(|transform| transform.rotation),
            "world_matrix": self.transform.matrix,
        })
    }
}

/// A scripted run: named inputs, checks and captures at fixed ticks.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Scenario {
    pub name: String,
    /// Written to [`RandomSeed`] before the first tick.
    #[serde(default)]
    pub seed: u64,
    /// Last tick to simulate; the run covers ticks 0 through `ticks`.
    pub ticks: u32,
    /// Image size of every capture.
    #[serde(default = "default_capture_size")]
    pub capture_size: [u32; 2],
    #[serde(default)]
    pub steps: Vec<ScenarioStep>,
}

fn default_capture_size() -> [u32; 2] {
    [1280, 720]
}

/// One step, for example `{"tick": 10, "press": "jump"}`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ScenarioStep {
    /// Inputs apply before this tick's update; checks and captures run
    /// after it.
    pub tick: u32,
    /// Repeats a check on every tick through this one, so a failure
    /// reports the first tick it stopped holding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u32>,
    #[serde(flatten)]
    pub action: StepAction,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepAction {
    /// Presses every input bound to a named action.
    Press(String),
    /// Releases every input bound to a named action.
    Release(String),
    /// Checks one value of an entity's reflected scene state.
    Expect(Expectation),
    /// Counts events seen by gameplay from tick 0 through this tick.
    ExpectEvents(EventExpectation),
    /// Writes the frame to this path, relative to the scenario file.
    Capture(PathBuf),
}

/// A check on the entity's scene form, the same JSON that scene files and
/// `rusting scene query` use. Registered components are parsed, so
/// `/components/rusting.player_controller/move_speed` reaches a field.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Expectation {
    /// Persistent ID or name.
    pub entity: String,
    /// JSON pointer, for example `/transform/position/1`.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub greater_than: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub less_than: Option<f64>,
    /// Allowed difference when `equals` compares numbers.
    #[serde(default)]
    pub tolerance: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventExpectation {
    pub kind: EventKind,
    /// Counts only events that involve this entity (persistent ID or name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_least: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<usize>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// CPU physics [`CollisionEvent`]s.
    Collision,
}

impl Scenario {
    /// Rejects steps that could never run or never fail.
    pub fn validate(&self) -> Result<(), String> {
        for (index, step) in self.steps.iter().enumerate() {
            let fail = |message: &str| Err(format!("step {index}: {message}"));
            let last = step.until.unwrap_or(step.tick);
            if last > self.ticks || last < step.tick {
                return fail("tick or until is outside the scenario");
            }
            let repeatable = matches!(
                step.action,
                StepAction::Expect(_) | StepAction::ExpectEvents(_)
            );
            if step.until.is_some() && !repeatable {
                return fail("only expect steps accept until");
            }
            match &step.action {
                StepAction::Expect(expect)
                    if expect.equals.is_none()
                        && expect.greater_than.is_none()
                        && expect.less_than.is_none() =>
                {
                    return fail(
                        "expect needs equals, greater_than or less_than",
                    )
                }
                StepAction::ExpectEvents(expect)
                    if expect.at_least.is_none()
                        && expect.at_most.is_none() =>
                {
                    return fail("expect_events needs at_least or at_most")
                }
                _ => {}
            }
        }
        if self.capture_size.contains(&0) {
            return Err("capture_size must be at least 1x1".into());
        }
        Ok(())
    }
}

/// Outcome of one step on one tick.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct StepResult {
    pub tick: u32,
    /// Index into the scenario's steps.
    pub step: usize,
    pub ok: bool,
    pub message: String,
    /// The value a failed check saw.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub actual: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ScenarioReport {
    pub name: String,
    pub seed: u64,
    pub passed: bool,
    /// Fixed ticks simulated before the run ended.
    pub ticks_run: u64,
    /// The first failed step; the run stops there.
    pub first_failure: Option<StepResult>,
    /// Every step result in order, ending at the first failure. Repeated
    /// checks appear once, at their last passing tick or their failure.
    pub steps: Vec<StepResult>,
    pub captures: Vec<PathBuf>,
}

/// One collision seen by gameplay, with persistent IDs.
struct SeenEvent {
    a: Option<Uuid>,
    b: Option<Uuid>,
}

/// Runs `scenario` on a loaded game. Relative capture paths resolve against
/// `base`, normally the scenario file's folder.
pub fn run_scenario(
    app: &mut App,
    scenario: &Scenario,
    base: &Path,
) -> ScenarioReport {
    let mut report = ScenarioReport {
        name: scenario.name.clone(),
        seed: scenario.seed,
        passed: false,
        ticks_run: 0,
        first_failure: None,
        steps: Vec::new(),
        captures: Vec::new(),
    };
    if let Err(message) = scenario.validate() {
        report.first_failure = Some(StepResult {
            tick: 0,
            step: 0,
            ok: false,
            message,
            actual: Value::Null,
        });
        return report;
    }
    app.world_mut().insert_resource(RandomSeed(scenario.seed));
    let wants_capture = scenario
        .steps
        .iter()
        .any(|step| matches!(step.action, StepAction::Capture(_)));
    let mut capture =
        wants_capture.then(|| HeadlessCapture::new(scenario.capture_size));
    let mut events = Vec::new();
    // Repeated checks report once; this holds their last passing result.
    let mut pending: Vec<Option<StepResult>> = vec![None; scenario.steps.len()];

    'ticks: for tick in 0..=scenario.ticks {
        for (index, step) in scenario.steps.iter().enumerate() {
            if step.tick != tick {
                continue;
            }
            let (action, pressed) = match &step.action {
                StepAction::Press(action) => (action, true),
                StepAction::Release(action) => (action, false),
                _ => continue,
            };
            let result = press(app.world_mut(), action, pressed);
            let failed = result.is_err();
            report.steps.push(StepResult {
                tick,
                step: index,
                ok: !failed,
                message: result.unwrap_or_else(|error| error),
                actual: Value::Null,
            });
            if failed {
                break 'ticks;
            }
        }

        let delta = tick_delta(app, tick);
        let updated = match capture.as_mut() {
            Some(Ok(capture)) => capture.frame(app, delta),
            _ => app
                .update(delta)
                .map(drop)
                .map_err(|error| error.to_string()),
        };
        if let Err(error) = updated {
            report.steps.push(StepResult {
                tick,
                step: 0,
                ok: false,
                message: format!("game update failed: {error}"),
                actual: Value::Null,
            });
            break;
        }
        report.ticks_run = app.world().resource::<FrameTime>().fixed_tick;
        let world = app.world_mut();
        let collisions: Vec<_> = world
            .resource::<EventQueue<CollisionEvent>>()
            .iter()
            .map(|event| (event.a, event.b))
            .collect();
        events.extend(collisions.into_iter().map(|(a, b)| SeenEvent {
            a: world.get::<SceneId>(a).map(|id| id.0),
            b: world.get::<SceneId>(b).map(|id| id.0),
        }));

        for (index, step) in scenario.steps.iter().enumerate() {
            let last = step.until.unwrap_or(step.tick);
            if !(step.tick..=last).contains(&tick) {
                continue;
            }
            let outcome = match &step.action {
                StepAction::Expect(expect) => check_value(world, expect),
                StepAction::ExpectEvents(expect) => {
                    check_events(world, expect, &events)
                }
                StepAction::Capture(path) => {
                    let path = base.join(path);
                    match capture.as_ref() {
                        Some(Ok(capture)) => match capture.save(&path) {
                            Ok(()) => {
                                report.captures.push(path.clone());
                                Ok(format!("captured {}", path.display()))
                            }
                            Err(error) => Err((error, Value::Null)),
                        },
                        Some(Err(error)) => {
                            Ok(format!("capture skipped: {error}"))
                        }
                        None => unreachable!("captures open a renderer"),
                    }
                }
                StepAction::Press(_) | StepAction::Release(_) => continue,
            };
            let (ok, message, actual) = match outcome {
                Ok(message) => (true, message, Value::Null),
                Err((message, actual)) => (false, message, actual),
            };
            let result = StepResult {
                tick,
                step: index,
                ok,
                message,
                actual,
            };
            if !ok || tick == last {
                pending[index] = None;
                report.steps.push(result);
                if !ok {
                    break 'ticks;
                }
            } else {
                pending[index] = Some(result);
            }
        }
        app.world_mut()
            .resource_mut::<RuntimeInput>()
            .clear_frame_edges();
    }
    report.first_failure = report.steps.iter().find(|step| !step.ok).cloned();
    report.passed = report.first_failure.is_none();
    report
}

fn press(
    world: &mut World,
    action: &str,
    pressed: bool,
) -> Result<String, String> {
    let bindings = world.resource::<ActionMap>().bindings(action).to_vec();
    if bindings.is_empty() {
        return Err(format!("action `{action}` has no bindings"));
    }
    let mut input = world.resource_mut::<RuntimeInput>();
    for binding in bindings {
        match binding {
            InputBinding::Key(key) => input.record_key(key, pressed),
            InputBinding::Mouse(button) => {
                input.record_mouse_button(button, pressed);
            }
        }
    }
    let verb = if pressed { "pressed" } else { "released" };
    Ok(format!("{verb} `{action}`"))
}

type Check = Result<String, (String, Value)>;

/// The entity's scene form, with registered components parsed.
fn reflected(world: &mut World, wanted: &str) -> Result<Value, String> {
    let entity = find_entity(world, wanted, |_, _| true)
        .ok_or_else(|| format!("no entity has the ID or name `{wanted}`"))?;
    let id = world
        .get::<SceneId>(entity)
        .ok_or_else(|| format!("`{wanted}` has no persistent ID"))?
        .0;
    let document =
        scene_document(world, "").map_err(|error| error.to_string())?;
    let entity = document
        .entities
        .into_iter()
        .find(|entity| entity.id == id)
        .ok_or_else(|| format!("`{wanted}` is not part of the scene"))?;
    let mut value =
        serde_json::to_value(entity).map_err(|error| error.to_string())?;
    if let Some(Value::Object(components)) = value.get_mut("components") {
        for component in components.values_mut() {
            if let Some(parsed) = component
                .as_str()
                .and_then(|text| serde_json::from_str(text).ok())
            {
                *component = parsed;
            }
        }
    }
    Ok(value)
}

fn check_value(world: &mut World, expect: &Expectation) -> Check {
    let subject = format!("`{}` {}", expect.entity, expect.path);
    let state = reflected(world, &expect.entity)
        .map_err(|error| (error, Value::Null))?;
    let actual = state
        .pointer(&expect.path)
        .cloned()
        .ok_or_else(|| (format!("{subject} does not exist"), Value::Null))?;
    let number = actual.as_f64();
    let fail = |wanted: String| {
        Err((
            format!("{subject} is {actual}, expected {wanted}"),
            actual.clone(),
        ))
    };
    if let Some(equals) = &expect.equals {
        let same = match (number, equals.as_f64()) {
            (Some(actual), Some(equals)) => {
                (actual - equals).abs() <= expect.tolerance
            }
            _ => &actual == equals,
        };
        if !same {
            return fail(format!("{equals} ± {}", expect.tolerance));
        }
    }
    if let Some(bound) = expect.greater_than {
        if number.is_none_or(|number| number <= bound) {
            return fail(format!("greater than {bound}"));
        }
    }
    if let Some(bound) = expect.less_than {
        if number.is_none_or(|number| number >= bound) {
            return fail(format!("less than {bound}"));
        }
    }
    Ok(format!("{subject} is {actual}"))
}

fn check_events(
    world: &mut World,
    expect: &EventExpectation,
    events: &[SeenEvent],
) -> Check {
    let involved = match &expect.entity {
        Some(wanted) => Some(
            find_entity(world, wanted, |_, _| true)
                .and_then(|entity| world.get::<SceneId>(entity))
                .map(|id| id.0)
                .ok_or_else(|| {
                    (
                        format!("no entity has the ID or name `{wanted}`"),
                        Value::Null,
                    )
                })?,
        ),
        None => None,
    };
    let count = events
        .iter()
        .filter(|event| {
            involved.is_none_or(|id| event.a == Some(id) || event.b == Some(id))
        })
        .count();
    let subject = match &expect.entity {
        Some(entity) => {
            format!("{:?} events involving `{entity}`", expect.kind)
        }
        None => format!("{:?} events", expect.kind),
    };
    if expect.at_least.is_some_and(|least| count < least)
        || expect.at_most.is_some_and(|most| count > most)
    {
        return Err((
            format!(
                "{subject}: {count}, expected {}..={}",
                expect.at_least.unwrap_or(0),
                expect.at_most.map_or("any".into(), |most| most.to_string())
            ),
            json!(count),
        ));
    }
    Ok(format!("{subject}: {count}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        Collider, ColliderShape, HybridPhysicsPlugin, InputBinding, KeyCode,
        PhysicsBody, RenderExtractPlugin, RigidBody, RigidBodyKind,
        ScheduleStage,
    };

    /// A cube falling onto fixed ground, and a game system that lifts the
    /// ground by 1 while `jump` is held and writes a seeded value to the
    /// ground's x.
    fn game() -> App {
        let mut app = App::new();
        app.add_plugin(crate::AssetPlugin).unwrap();
        app.add_plugin(HybridPhysicsPlugin).unwrap();
        app.add_plugin(RenderExtractPlugin).unwrap();
        app.world_mut()
            .resource_mut::<ActionMap>()
            .bind("jump", InputBinding::Key(KeyCode::Space));
        app.add_system(ScheduleStage::Update, |world: &mut World| {
            let held = world
                .resource::<ActionMap>()
                .held(world.resource::<RuntimeInput>(), "jump");
            let tick = world.resource::<FrameTime>().fixed_tick;
            let x = world.resource::<RandomSeed>().unit(tick, 0);
            let mut query = world.query::<(&Name, &mut Transform)>();
            for (name, mut transform) in query.iter_mut(world) {
                if name.0 == "Ground" {
                    transform.position =
                        [x, if held { 1.0 } else { -0.5 }, 0.0];
                }
            }
        });
        for (name, y, kind, half) in [
            ("Cube", 3.0, RigidBodyKind::Dynamic, [0.5; 3]),
            ("Ground", -0.5, RigidBodyKind::Fixed, [5.0, 0.5, 5.0]),
        ] {
            app.world_mut().spawn((
                SceneId(Uuid::new_v4()),
                Name(name.into()),
                Transform::new([0.0, y, 0.0]),
                PhysicsBody::default(),
                RigidBody {
                    kind,
                    ..RigidBody::default()
                },
                Collider {
                    shape: ColliderShape::Box { half_extents: half },
                    ..Collider::default()
                },
            ));
        }
        app
    }

    fn scenario(ticks: u32, steps: Value) -> Scenario {
        serde_json::from_value(
            json!({"name": "test", "seed": 7, "ticks": ticks, "steps": steps}),
        )
        .unwrap()
    }

    fn run(scenario: &Scenario) -> ScenarioReport {
        run_scenario(&mut game(), scenario, Path::new("."))
    }

    #[test]
    fn inputs_state_and_events_pass_at_fixed_ticks() {
        let report = run(&scenario(
            90,
            json!([
                {"tick": 0, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "equals": 3.0}},
                {"tick": 10, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "less_than": 3.0}},
                {"tick": 90, "expect_events": {"kind": "collision",
                    "entity": "Cube", "at_least": 1}},
                {"tick": 91, "press": "jump"},
            ]),
        ));
        // Tick 91 is past the end, so validation rejects it.
        assert!(!report.passed);
        assert!(report.first_failure.unwrap().message.contains("outside"));

        let report = run(&scenario(
            90,
            json!([
                {"tick": 0, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "equals": 3.0}},
                {"tick": 10, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "less_than": 3.0}},
                {"tick": 90, "expect_events": {"kind": "collision",
                    "entity": "Cube", "at_least": 1}},
                {"tick": 5, "press": "jump"},
                {"tick": 5, "expect": {"entity": "Ground",
                    "path": "/transform/position/1", "equals": 1.0}},
                {"tick": 6, "release": "jump"},
                {"tick": 6, "expect": {"entity": "Ground",
                    "path": "/transform/position/1", "equals": -0.5}},
            ]),
        ));
        assert!(report.passed, "{report:#?}");
        assert_eq!(report.ticks_run, 90);
    }

    #[test]
    fn failure_trace_names_the_first_failing_tick() {
        let report = run(&scenario(
            60,
            json!([
                {"tick": 0, "until": 60, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "greater_than": 2.5}},
            ]),
        ));
        assert!(!report.passed);
        let failure = report.first_failure.unwrap();
        assert!(failure.tick > 5 && failure.tick < 60, "{failure:?}");
        assert!(failure.actual.as_f64().unwrap() <= 2.5);
        // The run stops at the failure and reports only that step.
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.ticks_run, u64::from(failure.tick));

        let unbound = run(&scenario(1, json!([{"tick": 1, "press": "fly"}])));
        assert!(unbound.first_failure.unwrap().message.contains("fly"));
    }

    #[test]
    fn seed_reaches_game_code_and_repeats() {
        let ground_x = |seed: u64| {
            let mut scenario = scenario(3, json!([]));
            scenario.seed = seed;
            let mut app = game();
            assert!(run_scenario(&mut app, &scenario, Path::new(".")).passed);
            let mut query = app.world_mut().query::<(&Name, &Transform)>();
            query
                .iter(app.world())
                .find(|(name, _)| name.0 == "Ground")
                .unwrap()
                .1
                .position[0]
        };
        assert_eq!(ground_x(1), ground_x(1));
        assert_ne!(ground_x(1), ground_x(2));
    }
}
