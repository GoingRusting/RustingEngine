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
    picking, scene_document, set_registered_component,
    set_registered_component_field, ActionMap, Camera, CollisionEvent,
    EventQueue, FrameTime, GlobalTransform, InputBinding, MeshRenderer, Name,
    RandomSeed, RenderWorld, RuntimeInput, SceneId, SceneTransform,
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
    /// Runs on after a failed step, so one run reports every failure. A
    /// check that failed is not repeated on later ticks.
    #[serde(default)]
    pub keep_going: bool,
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
    /// Passes on the first tick through this one where the check holds, so
    /// a test need not know the exact tick something happens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<u32>,
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
    /// Changes one value before the tick's update, to set up a test: move
    /// the player, fill a counter.
    Set(Assignment),
    /// Moves the mouse cursor to this point of the view, as fractions of
    /// its width and height from the top-left corner: `[0.5, 0.5]` is the
    /// center.
    Pointer([f32; 2]),
    /// Records one value of an entity's scene form in the report, on every
    /// tick through `until`. Never fails; use it to debug a scenario.
    Log(LoggedValue),
}

/// The value a `log` step records.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LoggedValue {
    /// Entity name or scene ID.
    pub entity: String,
    /// JSON pointer into the entity's scene form, as for `expect`.
    pub path: String,
}

/// A write to `/transform/...` or `/components/<name>/...` of an entity's
/// scene form.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Assignment {
    /// Persistent ID or name.
    pub entity: String,
    pub path: String,
    pub value: Value,
}

/// A check on the entity's scene form, the same JSON that scene files and
/// `rusting scene query` use. Registered components are parsed, so
/// `/components/rusting.player_controller/move_speed` reaches a field.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Expectation {
    /// Persistent ID or name.
    pub entity: String,
    /// JSON pointer, for example `/transform/position/1`. Empty for the
    /// whole entity.
    #[serde(default)]
    pub path: String,
    /// Whether the entity and path exist, for example `false` for an object
    /// the game despawned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub greater_than: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub less_than: Option<f64>,
    /// Allowed difference when `equals` compares numbers, also each number
    /// inside an array or object such as a position.
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
            let last = step.until.or(step.within).unwrap_or(step.tick);
            if last > self.ticks || last < step.tick {
                return fail("tick or until is outside the scenario");
            }
            let repeatable = matches!(
                step.action,
                StepAction::Expect(_) | StepAction::ExpectEvents(_)
            );
            if step.within.is_some() && !repeatable
                || step.until.is_some()
                    && !repeatable
                    && !matches!(step.action, StepAction::Log(_))
            {
                return fail(
                    "only expect and log steps accept until or within",
                );
            }
            if step.until.is_some() && step.within.is_some() {
                return fail("until and within cannot both be set");
            }
            match &step.action {
                StepAction::Expect(expect)
                    if expect.equals.is_none()
                        && expect.greater_than.is_none()
                        && expect.less_than.is_none()
                        && expect.exists.is_none() =>
                {
                    return fail(
                        "expect needs equals, greater_than, less_than or exists",
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
    /// The first failed step; the run stops there unless `keep_going`.
    pub first_failure: Option<StepResult>,
    /// Every step result in order, ending at the first failure unless
    /// `keep_going`. Repeated
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
    // `within` checks that already passed.
    let mut passed = vec![false; scenario.steps.len()];

    'ticks: for tick in 0..=scenario.ticks {
        for (index, step) in scenario.steps.iter().enumerate() {
            if step.tick != tick {
                continue;
            }
            let result = match &step.action {
                StepAction::Press(action) => {
                    press(app.world_mut(), action, true)
                }
                StepAction::Release(action) => {
                    press(app.world_mut(), action, false)
                }
                StepAction::Set(assignment) => {
                    assign(app.world_mut(), assignment)
                }
                StepAction::Pointer(point) => {
                    point_at(app.world_mut(), *point, scenario.capture_size)
                }
                _ => continue,
            };
            let failed = result.is_err();
            report.steps.push(StepResult {
                tick,
                step: index,
                ok: !failed,
                message: result.unwrap_or_else(|error| error),
                actual: Value::Null,
            });
            if failed && !scenario.keep_going {
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
            let last = step.until.or(step.within).unwrap_or(step.tick);
            if !(step.tick..=last).contains(&tick) || passed[index] {
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
                StepAction::Log(logged) => {
                    let subject =
                        format!("`{}` {}", logged.entity, logged.path);
                    let actual = reflected(world, &logged.entity)
                        .ok()
                        .and_then(|state| state.pointer(&logged.path).cloned())
                        .unwrap_or(Value::Null);
                    report.steps.push(StepResult {
                        tick,
                        step: index,
                        ok: true,
                        message: format!("log: {subject} is {actual}"),
                        actual,
                    });
                    continue;
                }
                StepAction::Press(_)
                | StepAction::Release(_)
                | StepAction::Set(_)
                | StepAction::Pointer(_) => continue,
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
            if step.within.is_some() && !ok && tick < last {
                continue;
            }
            passed[index] = step.within.is_some() && ok;
            if !ok || tick == last || passed[index] {
                pending[index] = None;
                report.steps.push(result);
                if !ok {
                    if !scenario.keep_going {
                        break 'ticks;
                    }
                    passed[index] = true;
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
    // Scene bindings are added by the first update; tick 0 comes before it.
    crate::runtime::bind_input_actions(world);
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

/// Places the cursor at `point`, a fraction of the view. A headless run
/// has no window, so the view takes the capture size.
fn point_at(
    world: &mut World,
    point: [f32; 2],
    capture_size: [u32; 2],
) -> Result<String, String> {
    let mut input = world.resource_mut::<RuntimeInput>();
    let mut size = input.viewport_size();
    if size.contains(&0.0) {
        size = capture_size.map(|side| side as f32);
        input.record_viewport_size(size);
    }
    input.record_cursor_position([point[0] * size[0], point[1] * size[1]]);
    Ok(format!("pointer at {point:?}"))
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

fn assign(world: &mut World, set: &Assignment) -> Result<String, String> {
    let entity =
        find_entity(world, &set.entity, |_, _| true).ok_or_else(|| {
            format!("no entity has the ID or name `{}`", set.entity)
        })?;
    let subject = format!("`{}` {}", set.entity, set.path);
    if let Some(rest) = set.path.strip_prefix("/components/") {
        let (name, field) = rest.split_once('/').unwrap_or((rest, ""));
        if field.is_empty() {
            set_registered_component(
                world,
                entity,
                name,
                &set.value.to_string(),
            )
        } else {
            set_registered_component_field(
                world,
                entity,
                name,
                &format!("/{field}"),
                set.value.clone(),
            )
        }
        .map_err(|error| format!("{subject}: {error}"))?;
    } else if let Some(field) = set.path.strip_prefix("/transform") {
        let transform = world
            .get::<Transform>(entity)
            .ok_or_else(|| format!("`{}` has no transform", set.entity))?;
        let mut value = serde_json::to_value(SceneTransform::from(*transform))
            .expect("transforms serialize");
        let slot = value
            .pointer_mut(field)
            .ok_or_else(|| format!("{subject} does not exist"))?;
        *slot = set.value.clone();
        let transform: SceneTransform = serde_json::from_value(value)
            .map_err(|error| format!("{subject}: {error}"))?;
        world.entity_mut(entity).insert(Transform::from(transform));
    } else {
        return Err(format!(
            "{subject}: set reaches /transform/... and /components/... only"
        ));
    }
    Ok(format!("{subject} set to {}", set.value))
}

fn check_value(world: &mut World, expect: &Expectation) -> Check {
    let subject = format!("`{}` {}", expect.entity, expect.path);
    let state = reflected(world, &expect.entity);
    if let Some(wanted) = expect.exists {
        let found = state
            .as_ref()
            .is_ok_and(|state| state.pointer(&expect.path).is_some());
        if found != wanted {
            let missing = if wanted {
                "does not exist"
            } else {
                "still exists"
            };
            return Err((format!("{subject} {missing}"), Value::Bool(found)));
        }
        if !found {
            return Ok(format!("{subject} does not exist"));
        }
    }
    let state = state.map_err(|error| (error, Value::Null))?;
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
        if !close(&actual, equals, expect.tolerance) {
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

/// Whether `actual` equals `wanted`, with numbers, including those inside
/// arrays and objects such as a position, allowed to differ by `tolerance`.
fn close(actual: &Value, wanted: &Value, tolerance: f64) -> bool {
    match (actual, wanted) {
        (Value::Number(a), Value::Number(b)) => {
            // A whole number is not f32 state: counters above 2^24 must
            // match exactly.
            let whole = |n: &serde_json::Number| n.is_i64() || n.is_u64();
            a.as_f64().zip(b.as_f64()).is_some_and(|(x, y)| {
                // Engine state is f32. serde_json parses decimal text only to
                // within an f64 ulp, so an exact f32 also matches any
                // spelling that rounds to the same f32.
                (x - y).abs() <= tolerance
                    || (!(whole(a) && whole(b))
                        && f64::from(x as f32) == x
                        && x as f32 == y as f32)
            })
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(a, b)| close(a, b, tolerance))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, a)| {
                    b.get(key).is_some_and(|b| close(a, b, tolerance))
                })
        }
        _ => actual == wanted,
    }
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

    #[test]
    fn whole_numbers_above_the_f32_range_match_exactly() {
        assert!(!close(
            &serde_json::json!(16_777_216_u64),
            &serde_json::json!(16_777_217_u64),
            0.0
        ));
        assert!(close(
            &serde_json::json!(16_777_217_u64),
            &serde_json::json!(16_777_217_u64),
            0.0
        ));
    }

    #[test]
    fn a_printed_f32_matches_itself_with_tolerance_zero() {
        let actual = serde_json::json!(-1.733_868_f32);
        let printed: Value = serde_json::from_str(&actual.to_string()).unwrap();
        assert!(close(&actual, &printed, 0.0));
        let nearby: Value =
            serde_json::from_str("-1.7338680028915403").unwrap();
        assert!(close(&actual, &nearby, 0.0));
        assert!(!close(&actual, &serde_json::json!(-1.7338), 0.0));
    }

    #[test]
    fn tolerance_applies_to_every_number_in_a_vector_or_object() {
        let position = serde_json::json!([1.5, -2.0, 0.100_000_001_5]);
        assert!(close(&position, &serde_json::json!([1.5, -2, 0.1]), 1e-3));
        assert!(!close(&position, &serde_json::json!([1.5, -2, 0.1]), 0.0));
        assert!(!close(&position, &serde_json::json!([1.5, -2.0]), 1.0));
        let color = serde_json::json!({"r": 0.5, "name": "red"});
        assert!(close(
            &color,
            &serde_json::json!({"r": 0.51, "name": "red"}),
            0.02
        ));
        assert!(!close(
            &color,
            &serde_json::json!({"r": 0.5, "name": "blue"}),
            1.0
        ));
    }

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
    fn keep_going_reports_every_failed_check() {
        let mut scenario = scenario(
            5,
            json!([
                {"tick": 1, "until": 3, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "equals": 99.0}},
                {"tick": 4, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "equals": -99.0}},
            ]),
        );
        assert_eq!(run(&scenario).steps.len(), 1);
        scenario.keep_going = true;
        let report = run(&scenario);
        assert!(!report.passed);
        let failed: Vec<_> = report.steps.iter().filter(|s| !s.ok).collect();
        assert_eq!(
            failed.iter().map(|s| (s.tick, s.step)).collect::<Vec<_>>(),
            [(1, 0), (4, 1)]
        );
        assert_eq!(report.ticks_run, 5);
    }

    #[test]
    fn set_steps_move_entities_before_the_tick_runs() {
        let report = run(&scenario(
            20,
            json!([
                {"tick": 10, "set": {"entity": "Cube",
                    "path": "/transform/position", "value": [0.0, 20.0, 0.0]}},
                {"tick": 10, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "greater_than": 19.0}},
                {"tick": 11, "set": {"entity": "Cube",
                    "path": "/mesh", "value": 1}},
            ]),
        ));
        assert!(report.steps[0].ok && report.steps[1].ok, "{report:#?}");
        assert!(!report.passed);
        assert!(report.steps[2].message.contains("/transform/... and"));
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
    fn pointer_steps_place_the_cursor_in_the_capture_sized_view() {
        let mut world = World::new();
        world.insert_resource(RuntimeInput::default());
        point_at(&mut world, [0.25, 0.5], [800, 600]).unwrap();
        let input = world.resource::<RuntimeInput>();
        assert_eq!(input.viewport_size(), [800.0, 600.0]);
        assert_eq!(input.cursor_position(), Some([200.0, 300.0]));
        let step: ScenarioStep =
            serde_json::from_value(json!({"tick": 3, "pointer": [0.5, 0.5]}))
                .unwrap();
        assert!(matches!(step.action, StepAction::Pointer([0.5, 0.5])));
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
    fn within_passes_on_the_first_tick_the_check_holds() {
        let falls = |within: u32| {
            run(&scenario(
                60,
                json!([{"tick": 0, "within": within, "expect": {"entity": "Cube",
                    "path": "/transform/position/1", "less_than": 2.5}}]),
            ))
        };
        let report = falls(60);
        assert!(report.passed, "{report:#?}");
        let pass = &report.steps[0];
        assert!(pass.tick > 5 && pass.tick < 60, "{pass:?}");

        let report = falls(3);
        assert!(!report.passed);
        assert_eq!(report.first_failure.unwrap().tick, 3);
    }

    #[test]
    fn log_steps_record_a_value_every_tick_and_never_fail() {
        let report = run(&scenario(
            10,
            json!([
                {"tick": 1, "until": 4, "log": {"entity": "Cube",
                    "path": "/transform/position/1"}},
                {"tick": 5, "log": {"entity": "Ghost", "path": ""}},
            ]),
        ));
        assert!(report.passed, "{report:#?}");
        let ticks: Vec<_> = report.steps.iter().map(|step| step.tick).collect();
        assert_eq!(ticks, [1, 2, 3, 4, 5]);
        assert!(report.steps[3].actual.as_f64().unwrap() < 3.0, "falling");
        assert_eq!(report.steps[4].actual, Value::Null);
    }

    #[test]
    fn exists_checks_entities_and_paths() {
        let check = |entity: &str, path: &str, exists: bool| {
            run(&scenario(
                1,
                json!([{"tick": 1, "expect": {"entity": entity, "path": path, "exists": exists}}]),
            ))
            .passed
        };
        assert!(check("Cube", "", true));
        assert!(check("Cube", "/transform", true));
        assert!(check("Ghost", "", false));
        assert!(check("Cube", "/components/nope", false));
        assert!(!check("Cube", "", false));
        assert!(!check("Ghost", "", true));
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
