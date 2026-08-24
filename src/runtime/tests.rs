use std::time::Duration;

use bevy_ecs::prelude::{Commands, Query, Res, ResMut, Resource, With};

use crate::Transform;

use super::*;

#[derive(Resource, Default)]
struct Counts {
    startup: u32,
    fixed: u32,
    update: u32,
    extracted: u32,
    events_seen: usize,
}

fn startup(mut counts: ResMut<Counts>) {
    counts.startup += 1;
}

fn fixed(mut counts: ResMut<Counts>) {
    counts.fixed += 1;
}

fn update(mut counts: ResMut<Counts>) {
    counts.update += 1;
}

fn extract(mut counts: ResMut<Counts>) {
    counts.extracted += 1;
}

#[test]
fn schedules_and_fixed_catch_up_are_deterministic() {
    let mut app = EngineBuilder::new()
        .fixed_delta(Duration::from_millis(10))
        .max_fixed_steps(3)
        .build()
        .unwrap();
    app.insert_resource(Counts::default())
        .add_systems(ScheduleStage::Startup, startup)
        .add_systems(ScheduleStage::FixedUpdate, fixed)
        .add_systems(ScheduleStage::Update, update)
        .add_systems(ScheduleStage::RenderExtract, extract);

    let first = app.update(Duration::from_millis(35)).unwrap();
    let second = app.update(Duration::from_millis(5)).unwrap();
    let counts = app.world().resource::<Counts>();

    assert_eq!(first.fixed_steps, 3);
    assert_eq!(second.fixed_steps, 1);
    assert_eq!(counts.startup, 1);
    assert_eq!(counts.fixed, 4);
    assert_eq!(counts.update, 2);
    assert_eq!(counts.extracted, 2);
}

#[test]
fn pause_and_single_step_keep_variable_systems_alive() {
    let mut app = EngineBuilder::new()
        .fixed_delta(Duration::from_millis(10))
        .build()
        .unwrap();
    app.insert_resource(Counts::default())
        .add_systems(ScheduleStage::FixedUpdate, fixed)
        .add_systems(ScheduleStage::Update, update);
    app.world_mut().resource_mut::<TimeControl>().pause();

    assert_eq!(app.update(Duration::from_secs(1)).unwrap().fixed_steps, 0);
    app.world_mut().resource_mut::<TimeControl>().step();
    assert_eq!(app.update(Duration::from_secs(1)).unwrap().fixed_steps, 1);

    let counts = app.world().resource::<Counts>();
    assert_eq!(counts.fixed, 1);
    assert_eq!(counts.update, 2);
    let time = app.world().resource::<FrameTime>();
    assert_eq!(time.delta, Duration::ZERO);
    assert_eq!(time.elapsed, Duration::from_millis(10));
}

#[derive(Clone, Copy)]
struct TestEvent;

fn count_events(
    events: Res<EventQueue<TestEvent>>,
    mut counts: ResMut<Counts>,
) {
    counts.events_seen += events.len();
}

#[test]
fn typed_events_are_visible_for_one_frame() {
    let mut app = App::new();
    app.insert_resource(Counts::default())
        .add_event::<TestEvent>()
        .add_systems(ScheduleStage::Update, count_events);
    app.send_event(TestEvent);

    app.update(Duration::ZERO).unwrap();
    app.update(Duration::ZERO).unwrap();

    assert_eq!(app.world().resource::<Counts>().events_seen, 1);
}

#[derive(bevy_ecs::component::Component)]
struct DeferredSpawn;

fn queue_spawn(mut commands: Commands) {
    commands.spawn(DeferredSpawn);
}

fn count_spawned(
    query: Query<(), With<DeferredSpawn>>,
    mut counts: ResMut<Counts>,
) {
    counts.extracted = query.iter().count() as u32;
}

#[test]
fn deferred_commands_are_applied_between_ordered_schedules() {
    let mut app = App::new();
    app.insert_resource(Counts::default())
        .add_systems(ScheduleStage::Update, queue_spawn)
        .add_systems(ScheduleStage::PostUpdate, count_spawned);

    app.update(Duration::ZERO).unwrap();
    assert_eq!(app.world().resource::<Counts>().extracted, 1);
}

#[test]
fn hierarchy_propagates_and_rejects_cycles() {
    let mut app = App::new();
    let root = app.spawn(Transform::new([1.0, 0.0, 0.0]));
    let child = app.spawn(Transform::new([2.0, 0.0, 0.0]));
    let grandchild = app.spawn(Transform::new([4.0, 0.0, 0.0]));
    app.set_parent(child, root).unwrap();
    app.set_parent(grandchild, child).unwrap();

    app.update(Duration::ZERO).unwrap();
    let global = app.world().get::<GlobalTransform>(grandchild).unwrap();
    assert!((global.matrix[3][0] - 7.0).abs() < f32::EPSILON);
    assert!(matches!(
        app.set_parent(root, grandchild),
        Err(AppError::HierarchyCycle { .. })
    ));
}

struct CountingPlugin;

impl Plugin for CountingPlugin {
    fn build(&self, app: &mut App) -> Result<(), AppError> {
        app.insert_resource(Counts::default())
            .add_systems(ScheduleStage::Startup, startup);
        Ok(())
    }
}

#[test]
fn plugins_configure_apps_and_cannot_be_added_twice() {
    let mut app = App::new();
    app.add_plugin(CountingPlugin).unwrap();
    assert!(matches!(
        app.add_plugin(CountingPlugin),
        Err(AppError::DuplicatePlugin(_))
    ));
    app.update(Duration::ZERO).unwrap();
    assert_eq!(app.world().resource::<Counts>().startup, 1);
}

#[test]
fn rendering_is_uncapped_by_default() {
    let settings = RenderSettings::default();
    assert!(!settings.vsync);
    assert!(!settings.limit_fps);
    assert!(settings.max_fps > 0);
}
