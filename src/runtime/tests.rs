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

#[test]
fn physics_ids_reject_delayed_events_after_slot_reuse() {
    let first = Entity::from_raw_u32(10).unwrap();
    let second = Entity::from_raw_u32(11).unwrap();
    let mut ids = PhysicsIdRegistry::default();

    let old_id = ids.assign(first);
    assert_eq!(ids.resolve(old_id), Some(first));
    ids.release(first);
    let new_id = ids.assign(second);

    assert_eq!(old_id.slot, new_id.slot);
    assert_ne!(old_id.generation, new_id.generation);
    assert_eq!(ids.resolve(old_id), None);
    assert_eq!(ids.resolve(new_id), Some(second));
}

#[test]
fn rust_condition_builder_compiles_to_postfix_gpu_instructions() {
    let condition = GpuCondition::position_y()
        .less_than(-100.0)
        .and(GpuCondition::velocity_y().less_than(0.0))
        .or(!GpuCondition::sleeping());

    let instructions = condition.compile().unwrap();

    // Two comparisons, AND, sleeping, NOT, OR.
    assert_eq!(instructions.len(), 6);
    assert_eq!(instructions[0].values[0], -100.0);
    assert_eq!(instructions[1].values[0], 0.0);
    assert_ne!(instructions[2].opcode, instructions[5].opcode);
}

#[test]
fn one_class_rule_is_prepared_for_ten_thousand_matching_gpu_bodies() {
    const BODY_COUNT: usize = 10_000;

    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    let rule = GpuPhysicsRule::new(
        "body_fell",
        GpuCondition::position_y().less_than(-100.0),
    );
    {
        let mut watches =
            app.world_mut().resource_mut::<GpuPhysicsClassWatches>();
        watches.add("falling_cubes", rule.clone());
        // The same rule reached through two classes must still emit only once.
        watches.add("gravity", rule);
    }

    for index in 0..BODY_COUNT {
        app.spawn((
            Transform {
                position: [index as f32, 0.0, 0.0],
                ..Transform::default()
            },
            PhysicsBody {
                simulation: SimulationClass::GpuDynamic,
                ..PhysicsBody::default()
            },
            RigidBody::default(),
            ObjectClasses::new(["falling_cubes", "gravity"]),
        ));
    }
    app.spawn((
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::GpuDynamic,
            ..PhysicsBody::default()
        },
        RigidBody::default(),
        ObjectClasses::new(["unrelated"]),
    ));

    // PostUpdate assigns one stable PhysicsId to every GPU-owned body.
    app.update(Duration::ZERO).unwrap();
    let extracted =
        super::hybrid_physics::extract_gpu_physics_bodies(app.world_mut());

    assert_eq!(
        app.world().resource::<GpuPhysicsClassWatches>().classes
            ["falling_cubes"]
            .len(),
        1
    );
    assert_eq!(extracted.len(), BODY_COUNT + 1);
    assert_eq!(
        extracted
            .iter()
            .filter(|body| body.rules.len() == 1)
            .count(),
        BODY_COUNT
    );
    assert_eq!(
        extracted
            .iter()
            .filter(|body| body.rules.is_empty())
            .count(),
        1
    );
    assert!(extracted
        .windows(2)
        .all(|bodies| bodies[0].physics_id != bodies[1].physics_id));
}

#[test]
fn gpu_event_and_instruction_layouts_are_stable() {
    use std::mem::{offset_of, size_of};

    assert_eq!(size_of::<PhysicsId>(), 8);
    assert_eq!(size_of::<GpuConditionInstruction>(), 32);
    assert_eq!(offset_of!(GpuConditionInstruction, values), 16);
    assert_eq!(size_of::<RawGpuPhysicsEvent>(), 48);
    assert_eq!(offset_of!(RawGpuPhysicsEvent, tick_low), 16);
    assert_eq!(offset_of!(RawGpuPhysicsEvent, payload), 32);
}

#[test]
fn raw_gpu_events_reach_the_live_ecs_entity() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    let entity = app.spawn(PhysicsBody {
        simulation: SimulationClass::GpuDynamic,
        ..PhysicsBody::default()
    });
    app.update(Duration::from_secs_f64(1.0 / 60.0)).unwrap();

    let physics_id = *app.world().get::<PhysicsId>(entity).unwrap();
    let event_id = app
        .world_mut()
        .resource_mut::<GpuEventRegistry>()
        .register("cube_fell");
    let tick = u64::from(u32::MAX) + 25;
    let raw = RawGpuPhysicsEvent {
        body_slot: physics_id.slot,
        body_generation: physics_id.generation,
        event_id: event_id.0,
        tick_low: tick as u32,
        tick_high: (tick >> 32) as u32,
        payload_kind: GpuEventPayload::Position as u32,
        payload: [1.0, -101.0, 2.0, 1.0],
        ..Default::default()
    };

    let report = route_gpu_physics_events(app.world_mut(), &[raw]);
    assert_eq!(report.delivered, 1);
    app.update(Duration::ZERO).unwrap();

    let events = app.world().resource::<EventQueue<GpuPhysicsEvent>>();
    let event = events.iter().next().unwrap();
    assert_eq!(event.entity, entity);
    assert_eq!(event.tick, tick);
    assert_eq!(event.payload[1], -101.0);
}
