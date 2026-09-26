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
fn schedule_types_are_exact_core_reexports() {
    fn core_stage_to_runtime(
        stage: rusting_core::schedule::ScheduleStage,
    ) -> ScheduleStage {
        stage
    }
    fn runtime_stage_to_core(
        stage: ScheduleStage,
    ) -> rusting_core::schedule::ScheduleStage {
        stage
    }
    fn core_report_to_runtime(
        report: rusting_core::schedule::FrameReport,
    ) -> FrameReport {
        report
    }
    fn runtime_report_to_core(
        report: FrameReport,
    ) -> rusting_core::schedule::FrameReport {
        report
    }

    let _stage = runtime_stage_to_core(core_stage_to_runtime(
        rusting_core::schedule::ScheduleStage::Update,
    ));
    let _report = runtime_report_to_core(core_report_to_runtime(
        rusting_core::schedule::FrameReport::default(),
    ));
}

#[test]
fn time_types_are_exact_core_reexports() {
    fn core_frame_to_runtime(time: rusting_core::time::FrameTime) -> FrameTime {
        time
    }
    fn runtime_frame_to_core(time: FrameTime) -> rusting_core::time::FrameTime {
        time
    }
    fn core_control_to_runtime(
        control: rusting_core::time::TimeControl,
    ) -> TimeControl {
        control
    }
    fn runtime_control_to_core(
        control: TimeControl,
    ) -> rusting_core::time::TimeControl {
        control
    }

    let _time =
        runtime_frame_to_core(core_frame_to_runtime(FrameTime::default()));
    let _control = runtime_control_to_core(core_control_to_runtime(
        TimeControl::default(),
    ));
}

#[test]
fn zero_fixed_delta_after_build_maps_to_app_error_without_advancing_time() {
    let mut app = EngineBuilder::new().build().unwrap();
    app.world_mut().resource_mut::<TimeControl>().fixed_delta = Duration::ZERO;

    assert_eq!(
        app.update(Duration::from_secs(1)),
        Err(AppError::InvalidFixedDelta)
    );
    let time = app.world().resource::<FrameTime>();
    assert_eq!(time.frame, 0);
    assert_eq!(time.real_delta, Duration::ZERO);
    assert_eq!(time.delta, Duration::ZERO);
    assert_eq!(time.elapsed, Duration::ZERO);
    assert_eq!(time.fixed_tick, 0);
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

fn slow(_: ResMut<Counts>) {
    std::thread::sleep(Duration::from_millis(2));
}

#[test]
fn update_times_physics_and_extraction() {
    let mut app = EngineBuilder::new()
        .fixed_delta(Duration::from_millis(10))
        .build()
        .unwrap();
    app.insert_resource(Counts::default())
        .add_systems(ScheduleStage::FixedUpdate, slow)
        .add_systems(ScheduleStage::RenderExtract, slow);

    app.update(Duration::from_millis(10)).unwrap();
    let timings = *app.world().resource::<CpuFrameTimings>();
    assert!(timings.physics >= Duration::from_millis(2), "{timings:?}");
    assert!(
        timings.extraction >= Duration::from_millis(2),
        "{timings:?}"
    );

    // A frame without a fixed step spends no time in physics.
    app.update(Duration::ZERO).unwrap();
    let timings = *app.world().resource::<CpuFrameTimings>();
    assert!(timings.physics < Duration::from_millis(2), "{timings:?}");
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

#[test]
fn adding_an_event_type_twice_does_not_duplicate_maintenance() {
    let mut app = App::new();
    app.add_event::<TestEvent>().add_event::<TestEvent>();
    app.send_event(TestEvent);

    app.update(Duration::ZERO).unwrap();
    assert_eq!(app.world().resource::<EventQueue<TestEvent>>().len(), 1);

    app.update(Duration::ZERO).unwrap();
    assert!(app.world().resource::<EventQueue<TestEvent>>().is_empty());
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
                simulation: SimulationClass::Gpu,
                ..PhysicsBody::default()
            },
            RigidBody::default(),
            ObjectClasses::new(["falling_cubes", "gravity"]),
        ));
    }
    app.spawn((
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::Gpu,
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
fn watched_gpu_bodies_keep_their_revision_while_unchanged() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.add_plugin(RenderExtractPlugin).unwrap();
    app.world_mut()
        .resource_mut::<GpuPhysicsClassWatches>()
        .add(
            "falling",
            GpuPhysicsRule::new(
                "fell",
                GpuCondition::position_y().less_than(-100.0),
            ),
        );
    app.spawn((
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::Gpu,
            ..PhysicsBody::default()
        },
        RigidBody::default(),
        ObjectClasses::new(["falling"]),
    ));
    app.update(Duration::ZERO).unwrap();
    app.update(Duration::ZERO).unwrap();
    let revision = app.world().resource::<RenderWorld>().gpu_physics_revision;
    assert_eq!(app.world().resource::<RenderWorld>().gpu_physics.len(), 1);

    app.update(Duration::ZERO).unwrap();
    app.update(Duration::ZERO).unwrap();

    // Each revision bump restarts the GPU simulation from authored poses.
    assert_eq!(
        app.world().resource::<RenderWorld>().gpu_physics_revision,
        revision
    );
}

#[test]
fn watched_gpu_bodies_skip_extraction_until_a_watch_changes() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.add_plugin(RenderExtractPlugin).unwrap();
    let body = app.spawn((
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::Gpu,
            ..PhysicsBody::default()
        },
        RigidBody::default(),
        GpuPhysicsWatch {
            rules: vec![GpuPhysicsRule::new(
                "fell",
                GpuCondition::position_y().less_than(-100.0),
            )],
        },
    ));
    app.update(Duration::ZERO).unwrap();
    let signature =
        super::hybrid_physics::simple_gpu_physics_signature(app.world_mut());
    app.update(Duration::ZERO).unwrap();
    // A stable signature lets extraction skip cloning every watched body.
    assert_eq!(
        super::hybrid_physics::simple_gpu_physics_signature(app.world_mut()),
        signature
    );
    let revision = app.world().resource::<RenderWorld>().gpu_physics_revision;

    app.world_mut()
        .get_mut::<GpuPhysicsWatch>(body)
        .unwrap()
        .rules[0]
        .cooldown_seconds = 1.0;
    app.update(Duration::ZERO).unwrap();

    let render_world = app.world().resource::<RenderWorld>();
    assert_eq!(render_world.gpu_physics_revision, revision.wrapping_add(1));
    assert_eq!(render_world.gpu_physics[0].rules.len(), 1);
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
        simulation: SimulationClass::Gpu,
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

#[test]
fn gpu_events_are_delivered_in_tick_and_body_order() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    let bodies = [
        app.spawn(PhysicsBody::default()),
        app.spawn(PhysicsBody::default()),
    ]
    .map(|entity| {
        app.world_mut()
            .resource_mut::<PhysicsIdRegistry>()
            .assign(entity)
    });
    let event_id = app
        .world_mut()
        .resource_mut::<GpuEventRegistry>()
        .register("hit");
    let raw = |body: PhysicsId, tick: u32| RawGpuPhysicsEvent {
        body_slot: body.slot,
        body_generation: body.generation,
        event_id: event_id.0,
        tick_low: tick,
        ..Default::default()
    };

    // The GPU appends in atomic order; this is one possible buffer order.
    route_gpu_physics_events(
        app.world_mut(),
        &[raw(bodies[1], 2), raw(bodies[1], 1), raw(bodies[0], 2)],
    );
    app.update(Duration::ZERO).unwrap();

    let order: Vec<_> = app
        .world()
        .resource::<EventQueue<GpuPhysicsEvent>>()
        .iter()
        .map(|event| (event.tick, event.physics_id))
        .collect();
    assert_eq!(order, [(1, bodies[1]), (2, bodies[0]), (2, bodies[1])]);
}

#[test]
fn sync_mode_controls_rules_and_state_mirror() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.world_mut()
        .resource_mut::<GpuPhysicsClassWatches>()
        .add(
            "watched",
            GpuPhysicsRule::new(
                "fell",
                GpuCondition::position_y().less_than(0.0),
            ),
        );
    let spawn = |app: &mut App, sync| {
        app.spawn((
            Transform::default(),
            PhysicsBody {
                simulation: SimulationClass::Gpu,
                ..PhysicsBody::default()
            },
            RigidBody::default(),
            ObjectClasses::new(["watched"]),
            sync,
        ))
    };
    let silent = spawn(&mut app, PhysicsSyncMode::None);
    let mirrored = spawn(&mut app, PhysicsSyncMode::SelectedState);
    app.update(Duration::ZERO).unwrap();

    let extracted =
        super::hybrid_physics::extract_gpu_physics_bodies(app.world_mut());
    let sync_and_rules: Vec<_> = extracted
        .iter()
        .map(|body| (body.sync, body.rules.len()))
        .collect();
    assert_eq!(
        sync_and_rules,
        [
            (PhysicsSyncMode::None, 0),
            (PhysicsSyncMode::SelectedState, 1)
        ]
    );

    let id = app.world().get::<PhysicsId>(mirrored).copied().unwrap();
    let sample = |tick, y| GpuStateSample {
        physics_id: id,
        tick,
        transform: Transform::default().with_position(0.0, y, 0.0),
        linear_velocity: [0.0, -1.0, 0.0],
        angular_velocity: [0.0; 3],
        custom_values: None,
    };
    let removed = PhysicsId {
        generation: id.generation + 1,
        ..id
    };
    let report = apply_gpu_state_samples(
        app.world_mut(),
        &[
            sample(5, -1.0),
            // A late sample from an older frame never rewinds the mirror.
            sample(4, 3.0),
            GpuStateSample {
                physics_id: removed,
                ..sample(6, 0.0)
            },
        ],
    );
    assert_eq!((report.delivered, report.stale), (1, 1));
    let mirror = app.world().get::<GpuStateMirror>(mirrored).unwrap();
    assert_eq!(mirror.transform.position[1], -1.0);
    assert_eq!(mirror.age_ticks(8), 3);
    // The authored transform stays the editor's source of truth.
    assert_eq!(
        app.world().get::<Transform>(mirrored).unwrap().position[1],
        0.0
    );
    assert!(app.world().get::<GpuStateMirror>(silent).is_none());
}

#[test]
fn gpu_commands_reach_the_render_world_once_per_batch() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.add_plugin(RenderExtractPlugin).unwrap();
    let body = PhysicsId {
        slot: 4,
        generation: 2,
    };
    app.world_mut()
        .resource_mut::<GpuPhysicsCommands>()
        .push(body, GpuBodyCommand::Impulse([0.0, 5.0, 0.0]));
    app.update(Duration::ZERO).unwrap();

    let render_world = app.world().resource::<RenderWorld>();
    assert_eq!(
        render_world.gpu_physics_commands,
        [(body, GpuBodyCommand::Impulse([0.0, 5.0, 0.0]))]
    );
    assert_eq!(render_world.gpu_physics_commands_serial, 1);
    assert!(app
        .world()
        .resource::<GpuPhysicsCommands>()
        .commands
        .is_empty());

    // Without new commands the serial stays, so the batch is not re-sent.
    app.update(Duration::ZERO).unwrap();
    assert_eq!(
        app.world()
            .resource::<RenderWorld>()
            .gpu_physics_commands_serial,
        1
    );

    // A reset alone is a batch too.
    app.world_mut()
        .resource_mut::<GpuPhysicsCommands>()
        .reset_to_authored = true;
    app.update(Duration::ZERO).unwrap();
    let render_world = app.world().resource::<RenderWorld>();
    assert!(render_world.gpu_physics_reset);
    assert!(render_world.gpu_physics_commands.is_empty());
    assert_eq!(render_world.gpu_physics_commands_serial, 2);
}

#[test]
fn class_snapshots_request_one_read_per_member() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    let world = app.world_mut();
    let id = |slot| PhysicsId {
        slot,
        generation: 1,
    };
    world.spawn((id(0), ObjectClasses::new(["debris"])));
    world.spawn((id(1), ObjectClasses::new(["debris", "hot"])));
    world.spawn((id(2), ObjectClasses::new(["enemy"])));
    world.spawn(ObjectClasses::new(["debris"]));

    assert_eq!(request_gpu_class_snapshot(world, "debris"), 2);
    let mut commands = world.resource::<GpuPhysicsCommands>().commands.clone();
    commands.sort_by_key(|(id, _)| id.slot);
    assert_eq!(
        commands,
        [
            (id(0), GpuBodyCommand::ReadState),
            (id(1), GpuBodyCommand::ReadState)
        ]
    );
}

#[test]
fn condition_shaders_reach_the_render_world_with_registered_events() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.add_plugin(RenderExtractPlugin).unwrap();
    let existing = app
        .world_mut()
        .resource_mut::<GpuEventRegistry>()
        .register("landed");
    app.world_mut().resource_mut::<GpuConditionShaders>().0 = vec![
        GpuConditionShader {
            events: vec!["fell".into(), "landed".into()],
            glsl: "void condition(inout PhysicsState body) {}".into(),
        },
        GpuConditionShader {
            events: Vec::new(),
            glsl: "void condition(inout PhysicsState body) {}".into(),
        },
    ];
    app.update(Duration::ZERO).unwrap();

    let fell = app
        .world()
        .resource::<GpuEventRegistry>()
        .id("fell")
        .unwrap();
    let sources = &app.world().resource::<RenderWorld>().gpu_condition_shaders;
    assert_eq!(
        sources[0],
        format!(
            "const uint EVENTS[2] = uint[]({}u, {}u);\n#line 1\n\
             void condition(inout PhysicsState body) {{}}",
            fell.0, existing.0
        )
    );
    // No events means no table: GLSL has no zero-length arrays.
    assert_eq!(
        sources[1],
        "#line 1\nvoid condition(inout PhysicsState body) {}"
    );
}

#[test]
fn clicking_a_rendered_cube_fires_a_click_event() {
    let mut app = App::new();
    app.add_plugin(crate::assets::AssetPlugin).unwrap();
    app.add_plugin(RenderExtractPlugin).unwrap();
    let (mesh, material) = {
        let assets = app.world().resource::<crate::assets::AssetServer>();
        (assets.fallback_mesh, assets.fallback_material)
    };
    app.spawn((
        Transform::default().with_position(0.0, 0.0, 5.0),
        Camera {
            projection: Projection::Perspective {
                vertical_fov_radians: 1.0,
                near: 0.1,
                far: 100.0,
            },
            active: true,
            priority: 0,
        },
    ));
    let cube = app.spawn((
        Transform::default(),
        MeshRenderer {
            mesh,
            material,
            cast_shadows: true,
            receive_shadows: true,
        },
    ));

    // Frame 1 only extracts the active camera into `RenderWorld`; click
    // routing reads that extraction, so the click itself is set up for the
    // next frame.
    app.update(Duration::ZERO).unwrap();

    {
        let mut input = app.world_mut().resource_mut::<RuntimeInput>();
        input.record_viewport_size([800.0, 600.0]);
        input.record_cursor_position([400.0, 300.0]);
        input.record_mouse_button(MouseButton::Left, true);
    }
    // Frame 2 runs `route_click_events`, which enqueues the event as
    // `pending`. `EventQueue::begin_frame` swaps `pending` into `current` at
    // the *start* of a frame, so the event only becomes readable in the
    // frame after it was sent.
    app.update(Duration::ZERO).unwrap();
    app.update(Duration::ZERO).unwrap();

    let events = app.world().resource::<EventQueue<ClickEvent>>();
    let event = events.iter().next().expect("a click event was fired");
    assert_eq!(event.entity, cube);
    assert_eq!(event.button, MouseButton::Left);
}

fn cpu_body(
    world: &mut bevy_ecs::world::World,
    position: [f32; 3],
    shape: ColliderShape,
    kind: RigidBodyKind,
) -> bevy_ecs::entity::Entity {
    world
        .spawn((
            Transform::new(position),
            PhysicsBody::default(),
            RigidBody {
                kind,
                ..RigidBody::default()
            },
            Collider {
                shape,
                ..Collider::default()
            },
        ))
        .id()
}

fn run_fixed_steps(app: &mut App, steps: u32) {
    for _ in 0..steps {
        app.update(Duration::from_secs_f64(1.0 / 60.0)).unwrap();
    }
}

const UNIT_BOX: ColliderShape = ColliderShape::Box {
    half_extents: [0.5; 3],
};

fn cpu_ground(world: &mut bevy_ecs::world::World) {
    cpu_body(
        world,
        [0.0, -0.5, 0.0],
        ColliderShape::Box {
            half_extents: [5.0, 0.5, 5.0],
        },
        RigidBodyKind::Fixed,
    );
}

#[test]
fn cpu_tilted_box_tips_over_onto_a_face() {
    let mut app = App::new();
    let world = app.world_mut();
    cpu_ground(world);
    let tilted =
        cpu_body(world, [0.0, 1.5, 0.0], UNIT_BOX, RigidBodyKind::Dynamic);
    world.get_mut::<Transform>(tilted).unwrap().rotation = [0.3, 0.0, 0.2];
    run_fixed_steps(&mut app, 300);

    // Resting on an edge or corner would leave it above half its size.
    let world = app.world();
    let position = world.get::<Transform>(tilted).unwrap().position;
    assert!(
        (position[1] - 0.5).abs() < 0.03,
        "box rests at {position:?}"
    );
    let spin = world.get::<RigidBody>(tilted).unwrap().angular_velocity;
    assert!(
        spin.iter().all(|w| w.abs() < 0.1),
        "box still spins {spin:?}"
    );
}

#[test]
fn cpu_sphere_sliding_on_the_ground_starts_rolling() {
    let mut app = App::new();
    let world = app.world_mut();
    cpu_ground(world);
    let ball = cpu_body(
        world,
        [0.0, 0.5, 0.0],
        ColliderShape::Sphere { radius: 0.5 },
        RigidBodyKind::Dynamic,
    );
    world.get_mut::<RigidBody>(ball).unwrap().linear_velocity = [3.0, 0.0, 0.0];
    run_fixed_steps(&mut app, 60);

    // Friction turns sliding into rolling: spin matches speed over radius.
    let rigid = app.world().get::<RigidBody>(ball).unwrap();
    let speed = rigid.linear_velocity[0];
    assert!(speed > 1.0 && speed < 3.0, "ball moves {speed}");
    let rolling = -speed / 0.5;
    assert!(
        (rigid.angular_velocity[2] - rolling).abs() < 0.1 * rolling.abs(),
        "ball spins {:?} at speed {speed}",
        rigid.angular_velocity
    );
}

#[test]
fn cpu_convex_mesh_lands_flat_on_a_triangle_mesh_floor() {
    use crate::assets::{MeshAsset, MeshVertex, PrimitiveShape};
    let mut app = App::new();
    app.add_plugin(crate::assets::AssetPlugin).unwrap();
    let (cube, floor, material) = {
        let mut assets =
            app.world_mut().resource_mut::<crate::assets::AssetServer>();
        let floor = assets.meshes.insert(MeshAsset {
            vertices: [
                [-10.0, -10.0],
                [10.0, -10.0],
                [10.0, 10.0],
                [-10.0, 10.0],
            ]
            .map(|[x, z]| MeshVertex {
                position: [x, 0.0, z],
                ..MeshVertex::default()
            })
            .to_vec(),
            indices: vec![0, 2, 1, 0, 3, 2],
        });
        (
            assets.builtin_primitives[&PrimitiveShape::Cube],
            floor,
            assets.fallback_material,
        )
    };
    let world = app.world_mut();
    let mut spawn = |mesh, position, shape, kind| {
        let entity = cpu_body(world, position, shape, kind);
        world.entity_mut(entity).insert(MeshRenderer {
            mesh,
            material,
            cast_shadows: true,
            receive_shadows: true,
        });
        entity
    };
    // A dynamic triangle mesh still never moves.
    let ground = spawn(
        floor,
        [0.0; 3],
        ColliderShape::TriangleMesh,
        RigidBodyKind::Dynamic,
    );
    let falling = spawn(
        cube,
        [0.0, 1.5, 0.0],
        ColliderShape::ConvexMesh,
        RigidBodyKind::Dynamic,
    );
    world.get_mut::<Transform>(falling).unwrap().rotation = [0.3, 0.0, 0.2];
    run_fixed_steps(&mut app, 300);

    let world = app.world();
    assert_eq!(world.get::<Transform>(ground).unwrap().position, [0.0; 3]);
    let position = world.get::<Transform>(falling).unwrap().position;
    assert!(
        (position[1] - 0.5).abs() < 0.03,
        "cube rests at {position:?}"
    );
    let hit = world
        .resource::<PhysicsWorld>()
        .raycast([4.0, 5.0, 4.0], [0.0, -1.0, 0.0], 20.0, u32::MAX)
        .unwrap();
    assert_eq!(hit.entity, ground);
    assert!((hit.distance - 5.0).abs() < 1e-4);
}

#[test]
fn cpu_boxes_fall_and_rest_in_a_stack_on_static_ground() {
    let mut app = App::new();
    let world = app.world_mut();
    let ground = cpu_body(
        world,
        [0.0, -0.5, 0.0],
        ColliderShape::Box {
            half_extents: [5.0, 0.5, 5.0],
        },
        RigidBodyKind::Fixed,
    );
    let boxes = [0.5, 1.6, 2.7].map(|y| {
        cpu_body(world, [0.0, y, 0.0], UNIT_BOX, RigidBodyKind::Dynamic)
    });
    run_fixed_steps(&mut app, 180);

    let world = app.world();
    assert_eq!(world.get::<Transform>(ground).unwrap().position[1], -0.5);
    for (level, entity) in boxes.into_iter().enumerate() {
        let position = world.get::<Transform>(entity).unwrap().position;
        let expected = 0.5 + level as f32;
        assert!(
            (position[1] - expected).abs() < 0.03,
            "box {level} rests at {position:?}"
        );
        // Landing leaves a millimetre of sideways slip and a sliver of tilt.
        assert!(
            position[0].abs() < 5e-3 && position[2].abs() < 5e-3,
            "box {level} drifts to {position:?}"
        );
        let rotation = world.get::<Transform>(entity).unwrap().rotation;
        assert!(
            rotation.iter().all(|angle| angle.abs() < 0.01),
            "box {level} tilts to {rotation:?}"
        );
        let velocity = world.get::<RigidBody>(entity).unwrap().linear_velocity;
        assert!(velocity[1].abs() < 0.2, "box {level} moves {velocity:?}");
    }
    let hit = world
        .resource::<PhysicsWorld>()
        .raycast([0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 20.0, u32::MAX)
        .unwrap();
    assert_eq!(hit.entity, boxes[2]);
    assert!((hit.point[1] - 3.0).abs() < 0.03);
}

#[test]
fn cpu_restitution_bounces_and_sensors_only_report() {
    let mut app = App::new();
    let world = app.world_mut();
    cpu_body(
        world,
        [0.0, -0.5, 0.0],
        ColliderShape::Box {
            half_extents: [5.0, 0.5, 5.0],
        },
        RigidBodyKind::Fixed,
    );
    let ball = cpu_body(
        world,
        [0.0, 3.0, 0.0],
        ColliderShape::Sphere { radius: 0.5 },
        RigidBodyKind::Dynamic,
    );
    world.get_mut::<Collider>(ball).unwrap().restitution = 0.8;
    let sensor =
        cpu_body(world, [4.0, 0.5, 0.0], UNIT_BOX, RigidBodyKind::Fixed);
    world.get_mut::<Collider>(sensor).unwrap().sensor = true;
    let walker = cpu_body(
        world,
        [2.0, 0.5, 0.0],
        ColliderShape::Sphere { radius: 0.4 },
        RigidBodyKind::Kinematic,
    );
    world.get_mut::<RigidBody>(walker).unwrap().linear_velocity =
        [3.0, 0.0, 0.0];

    let mut peak_after_bounce = f32::MIN;
    let mut bounced = false;
    let mut sensor_events = 0;
    for _ in 0..90 {
        run_fixed_steps(&mut app, 1);
        let world = app.world();
        let ball_state = world.get::<RigidBody>(ball).unwrap();
        bounced |= ball_state.linear_velocity[1] > 1.0;
        if bounced {
            peak_after_bounce = peak_after_bounce
                .max(world.get::<Transform>(ball).unwrap().position[1]);
        }
        sensor_events += world
            .resource::<EventQueue<CollisionEvent>>()
            .iter()
            .filter(|event| event.sensor)
            .count();
    }
    assert!(bounced, "the ball never bounced");
    assert!(
        (1.5..3.0).contains(&peak_after_bounce),
        "bounce peak {peak_after_bounce}"
    );
    // The kinematic walker passed through the sensor, which never pushed it.
    assert!(sensor_events > 0);
    let walker_x = app.world().get::<Transform>(walker).unwrap().position[0];
    assert!((walker_x - (2.0 + 3.0 * 1.5)).abs() < 1e-3, "{walker_x}");
}

#[test]
fn cpu_physics_is_deterministic_and_ignores_gpu_bodies() {
    let run = || {
        let mut app = App::new();
        let world = app.world_mut();
        cpu_body(
            world,
            [0.0, -0.5, 0.0],
            ColliderShape::Box {
                half_extents: [5.0, 0.5, 5.0],
            },
            RigidBodyKind::Fixed,
        );
        let bodies = (0..6)
            .map(|index| {
                let shape = if index % 2 == 0 {
                    UNIT_BOX
                } else {
                    ColliderShape::Capsule {
                        half_height: 0.3,
                        radius: 0.3,
                    }
                };
                cpu_body(
                    world,
                    [index as f32 * 0.3, 1.0 + index as f32, 0.1],
                    shape,
                    RigidBodyKind::Dynamic,
                )
            })
            .collect::<Vec<_>>();
        let gpu =
            cpu_body(world, [0.0, 5.0, 0.0], UNIT_BOX, RigidBodyKind::Dynamic);
        world.get_mut::<PhysicsBody>(gpu).unwrap().simulation =
            SimulationClass::Gpu;
        run_fixed_steps(&mut app, 120);
        let world = app.world();
        assert_eq!(world.get::<Transform>(gpu).unwrap().position[1], 5.0);
        bodies
            .iter()
            .map(|entity| world.get::<Transform>(*entity).unwrap().position)
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn cpu_collision_layers_filter_pairs_and_queries() {
    let mut app = App::new();
    let world = app.world_mut();
    let ground = cpu_body(
        world,
        [0.0, -0.5, 0.0],
        ColliderShape::Box {
            half_extents: [5.0, 0.5, 5.0],
        },
        RigidBodyKind::Fixed,
    );
    world.entity_mut(ground).insert(CollisionLayers {
        memberships: 0b01,
        filters: 0b01,
    });
    let solid =
        cpu_body(world, [-2.0, 0.5, 0.0], UNIT_BOX, RigidBodyKind::Dynamic);
    // On layer 2 only: the ground does not accept it, so it falls through.
    let ghost =
        cpu_body(world, [2.0, 0.5, 0.0], UNIT_BOX, RigidBodyKind::Dynamic);
    world.entity_mut(ghost).insert(CollisionLayers {
        memberships: 0b10,
        filters: u32::MAX,
    });
    run_fixed_steps(&mut app, 60);

    let world = app.world();
    assert!(
        (world.get::<Transform>(solid).unwrap().position[1] - 0.5).abs() < 0.03
    );
    assert!(world.get::<Transform>(ghost).unwrap().position[1] < -2.0);
    let physics = world.resource::<PhysicsWorld>();
    let down = [0.0, -1.0, 0.0];
    let hit = physics
        .raycast([-2.0, 5.0, 0.0], down, 20.0, u32::MAX)
        .unwrap();
    assert_eq!(hit.entity, solid);
    let hit = physics.raycast([-2.0, 5.0, 0.0], down, 20.0, 0b01).unwrap();
    assert_eq!(hit.entity, solid, "the solid box has no layers: all bits");
    let ground_only = physics.overlap_sphere([4.0, 0.0, 0.0], 0.5, 0b01);
    assert_eq!(ground_only, vec![ground]);
    assert!(physics
        .overlap_sphere([4.0, 0.0, 0.0], 0.5, 0b100)
        .is_empty());
}

#[test]
fn cpu_bodies_sleep_when_still_and_wake_on_touch_or_edit() {
    let mut app = App::new();
    let world = app.world_mut();
    cpu_body(
        world,
        [0.0, -0.5, 0.0],
        ColliderShape::Box {
            half_extents: [5.0, 0.5, 5.0],
        },
        RigidBodyKind::Fixed,
    );
    let resting =
        cpu_body(world, [0.0, 0.5, 0.0], UNIT_BOX, RigidBodyKind::Dynamic);
    let pushed =
        cpu_body(world, [3.0, 0.5, 0.0], UNIT_BOX, RigidBodyKind::Dynamic);
    run_fixed_steps(&mut app, SLEEP_STEPS + 10);
    assert!(app.world().get::<Sleeping>(resting).is_some());
    assert!(app.world().get::<Sleeping>(pushed).is_some());

    // A sleeping body stays put and keeps no change-detection churn.
    let before = *app.world().get::<Transform>(resting).unwrap();
    run_fixed_steps(&mut app, 10);
    assert_eq!(*app.world().get::<Transform>(resting).unwrap(), before);

    // Gameplay velocity wakes a body.
    app.world_mut()
        .get_mut::<RigidBody>(pushed)
        .unwrap()
        .linear_velocity = [0.0, 4.0, 0.0];
    run_fixed_steps(&mut app, 1);
    assert!(app.world().get::<Sleeping>(pushed).is_none());
    assert!(app.world().get::<Transform>(pushed).unwrap().position[1] > 0.5);

    // A falling box wakes the sleeper it lands on.
    let dropped = cpu_body(
        app.world_mut(),
        [0.0, 3.0, 0.0],
        UNIT_BOX,
        RigidBodyKind::Dynamic,
    );
    let mut woke = false;
    for _ in 0..60 {
        run_fixed_steps(&mut app, 1);
        woke |= app.world().get::<Sleeping>(resting).is_none();
    }
    assert!(woke, "the landing box must wake the resting one");
    let top = app.world().get::<Transform>(dropped).unwrap().position[1];
    assert!((top - 1.5).abs() < 0.05, "dropped box rests on top: {top}");
}

#[test]
fn fast_cpu_bodies_do_not_tunnel_through_thin_walls() {
    let mut app = App::new();
    let world = app.world_mut();
    world.resource_mut::<PhysicsSettings>().gravity = [0.0; 3];
    cpu_body(
        world,
        [0.0, 0.0, -5.0],
        ColliderShape::Box {
            half_extents: [2.0, 2.0, 0.05],
        },
        RigidBodyKind::Fixed,
    );
    let bullet = cpu_body(
        world,
        [0.0; 3],
        ColliderShape::Sphere { radius: 0.1 },
        RigidBodyKind::Dynamic,
    );
    // 270 m/s moves 4.5 m per 60 Hz step: z = -4.5, then -9 without a sweep.
    world.get_mut::<RigidBody>(bullet).unwrap().linear_velocity =
        [0.0, 0.0, -270.0];
    run_fixed_steps(&mut app, 5);
    let z = app.world().get::<Transform>(bullet).unwrap().position[2];
    assert!(z > -5.0 && z < -4.8, "bullet stops at the wall: {z}");
}

#[test]
fn gpu_bodies_extract_their_collider_and_see_cpu_colliders() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.add_plugin(RenderExtractPlugin).unwrap();
    let world = app.world_mut();
    cpu_body(
        world,
        [0.0, -0.5, 0.0],
        ColliderShape::Box {
            half_extents: [5.0, 0.5, 5.0],
        },
        RigidBodyKind::Fixed,
    );
    let sensor =
        cpu_body(world, [3.0, 0.0, 0.0], UNIT_BOX, RigidBodyKind::Fixed);
    world.get_mut::<Collider>(sensor).unwrap().sensor = true;
    let gpu =
        cpu_body(world, [0.0, 5.0, 0.0], UNIT_BOX, RigidBodyKind::Dynamic);
    world.get_mut::<PhysicsBody>(gpu).unwrap().simulation =
        SimulationClass::Gpu;
    run_fixed_steps(&mut app, 2);

    let render_world = app.world().resource::<RenderWorld>();
    assert_eq!(render_world.gpu_physics.len(), 1);
    let (collider, _) = render_world.gpu_physics[0].collider.unwrap();
    assert_eq!(collider.shape, UNIT_BOX);
    // The ground only: sensors and GPU bodies are not GPU colliders.
    assert_eq!(render_world.gpu_colliders.len(), 1);
    let ground = render_world.gpu_colliders[0];
    assert_eq!(ground.shape, [0.0, 5.0, 0.5, 5.0]);
    assert_eq!(ground.model[3], [0.0, -0.5, 0.0, 1.0]);

    // Editing a GPU body's collider rebuilds its GPU tables.
    let revision = render_world.gpu_physics_revision;
    app.world_mut().get_mut::<Collider>(gpu).unwrap().friction = 0.9;
    run_fixed_steps(&mut app, 1);
    assert_ne!(
        app.world().resource::<RenderWorld>().gpu_physics_revision,
        revision
    );
}

#[test]
fn custom_solver_files_reach_the_render_world_once_per_path() {
    let dir = std::env::temp_dir()
        .join(format!("rusting-custom-solver-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rise.glsl").to_string_lossy().into_owned();
    std::fs::write(&path, "void solve(inout PhysicsState body) {}").unwrap();
    let missing = dir.join("missing.glsl").to_string_lossy().into_owned();

    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.add_plugin(RenderExtractPlugin).unwrap();
    let world = app.world_mut();
    for (solver, shader) in [
        (PhysicsSolver::Custom, Some(&path)),
        (PhysicsSolver::Custom, Some(&path)),
        (PhysicsSolver::Custom, Some(&missing)),
        // A leftover path on a non-custom body is ignored.
        (PhysicsSolver::Full, Some(&path)),
    ] {
        let body = cpu_body(world, [0.0; 3], UNIT_BOX, RigidBodyKind::Dynamic);
        let mut physics = world.get_mut::<PhysicsBody>(body).unwrap();
        physics.simulation = SimulationClass::Gpu;
        physics.solver = solver;
        physics.custom_shader = shader.cloned();
    }
    run_fixed_steps(&mut app, 1);

    let render_world = app.world().resource::<RenderWorld>();
    let shaders = render_world
        .gpu_physics
        .iter()
        .map(|body| body.custom_shader.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(shaders.iter().filter(|shader| shader.is_some()).count(), 3);
    let solvers = &render_world.gpu_solver_shaders;
    assert_eq!(solvers.len(), 2, "{solvers:?}");
    let rise = format!("SOLVER_ID = {}u", custom_solver_id(&path));
    assert!(solvers.iter().any(|source| source.contains(&rise)
        && source.contains("void solve(inout PhysicsState body) {}")));
    assert!(solvers
        .iter()
        .any(|source| source.contains("#error cannot read custom solver")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cpu_shape_casts_stop_before_colliders_and_characters_slide() {
    let mut app = App::new();
    let world = app.world_mut();
    let ground = cpu_body(
        world,
        [0.0, -0.5, 0.0],
        ColliderShape::Box {
            half_extents: [10.0, 0.5, 10.0],
        },
        RigidBodyKind::Fixed,
    );
    let wall = cpu_body(
        world,
        [3.0, 1.0, 0.0],
        ColliderShape::Box {
            half_extents: [0.5, 1.0, 5.0],
        },
        RigidBodyKind::Fixed,
    );
    run_fixed_steps(&mut app, 1);
    let physics = app.world().resource::<PhysicsWorld>();
    let ball = ColliderShape::Sphere { radius: 0.3 };
    let capsule = ColliderShape::Capsule {
        half_height: 0.5,
        radius: 0.3,
    };

    let down = physics
        .shape_cast(
            ball,
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            5.0,
            u32::MAX,
            None,
        )
        .unwrap();
    assert_eq!(down.entity, ground);
    assert!((down.distance - 0.7).abs() < 1e-3, "{down:?}");
    assert!(down.normal[1] > 0.99, "{down:?}");

    let side = physics
        .shape_cast(
            capsule,
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            5.0,
            u32::MAX,
            None,
        )
        .unwrap();
    assert_eq!(side.entity, wall);
    assert!((side.distance - 2.2).abs() < 1e-3, "{side:?}");
    assert!(side.normal[0] < -0.99, "{side:?}");
    // Excluded, masked-out, or out-of-range colliders are not hit.
    let cast = |exclude, mask, max| {
        physics.shape_cast(
            capsule,
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            max,
            mask,
            exclude,
        )
    };
    assert_eq!(cast(Some(wall), u32::MAX, 5.0), None);
    assert_eq!(cast(None, 0, 5.0), None);
    assert_eq!(cast(None, u32::MAX, 2.0), None);

    // A ball resting on the ground can lift off but not sink.
    let resting = [0.0, 0.29, 0.0];
    assert_eq!(
        physics.shape_cast(ball, resting, [0.0, 1.0, 0.0], 1.0, u32::MAX, None),
        None
    );
    let sink = physics
        .shape_cast(ball, resting, [0.0, -1.0, 0.0], 1.0, u32::MAX, None)
        .unwrap();
    assert_eq!((sink.entity, sink.distance), (ground, 0.0));

    // Moving down and into the wall slides along the floor to the wall.
    let moved = physics.move_character(
        capsule,
        [0.0, 1.0, 0.0],
        [5.0, -2.0, 1.0],
        u32::MAX,
        None,
    );
    assert!(moved.grounded);
    assert!((moved.position[0] - 2.19).abs() < 0.02, "{moved:?}");
    assert!((moved.position[1] - 0.81).abs() < 0.02, "{moved:?}");
    // The motion along the wall is kept.
    assert!((moved.position[2] - 1.0).abs() < 0.02, "{moved:?}");
}

#[test]
fn gpu_events_place_move_and_remove_a_cpu_query_proxy() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    let (place, remove) = {
        let mut registry = app.world_mut().resource_mut::<GpuEventRegistry>();
        (registry.register("seen"), registry.register("gone"))
    };
    let body = app.spawn((
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::Gpu,
            ..PhysicsBody::default()
        },
        GpuQueryProxy {
            collider: Collider {
                shape: ColliderShape::Sphere { radius: 0.5 },
                ..Collider::default()
            },
            layers: CollisionLayers::default(),
            place_on: place,
            remove_on: Some(remove),
        },
    ));
    app.update(Duration::ZERO).unwrap();
    let id = *app.world().get::<PhysicsId>(body).unwrap();
    let send = |app: &mut App, event: GpuEventId, x: f32| {
        let raw = RawGpuPhysicsEvent {
            body_slot: id.slot,
            body_generation: id.generation,
            event_id: event.0,
            payload_kind: GpuEventPayload::Position as u32,
            payload: [x, 0.0, 0.0, 0.0],
            ..Default::default()
        };
        route_gpu_physics_events(app.world_mut(), &[raw]);
        app.update(Duration::ZERO).unwrap();
        run_fixed_steps(app, 1);
    };
    let down = [0.0, -1.0, 0.0];
    let hit_x = |app: &App, x: f32| {
        app.world()
            .resource::<PhysicsWorld>()
            .raycast([x, 5.0, 0.0], down, 20.0, u32::MAX)
            .map(|hit| hit.entity)
    };

    send(&mut app, place, 3.0);
    let proxy = hit_x(&app, 3.0).expect("proxy is placed");
    assert_eq!(
        app.world().get::<GpuProxyOf>(proxy),
        Some(&GpuProxyOf(body))
    );

    send(&mut app, place, -3.0);
    assert_eq!(hit_x(&app, 3.0), None);
    assert_eq!(hit_x(&app, -3.0), Some(proxy), "the same proxy moves");

    send(&mut app, remove, 0.0);
    assert_eq!(hit_x(&app, -3.0), None);
    assert!(app.world().get_entity(proxy).is_err());
}

#[test]
fn auto_simulation_picks_cpu_or_gpu_and_stays_overridable() {
    let mut app = App::new();
    app.add_plugin(HybridPhysicsPlugin).unwrap();
    app.insert_resource(PhysicsBackendStatus {
        gameplay_available: true,
        gpu_dynamic_available: true,
    });
    app.insert_resource(AutoAllocationPolicy {
        gpu_min_bodies: 3,
        ..AutoAllocationPolicy::default()
    });
    let auto = |app: &mut App, extra: PhysicsBody| {
        app.spawn((Transform::default(), extra, AutoSimulation::default()))
    };
    let decision = |app: &App, entity| {
        let decision = app.world().get::<AutoSimulation>(entity).unwrap();
        let class = app.world().get::<PhysicsBody>(entity).unwrap().simulation;
        let decision = decision.decision.unwrap();
        assert_eq!(decision.class, class);
        (class, decision.reason)
    };

    // Two flexible bodies stay under the threshold.
    let first = auto(&mut app, PhysicsBody::default());
    let second = auto(&mut app, PhysicsBody::default());
    app.update(Duration::ZERO).unwrap();
    for entity in [first, second] {
        assert_eq!(
            decision(&app, entity),
            (SimulationClass::Cpu, AllocationReason::FewBodies)
        );
    }

    // A third reaches it: only the new body goes to the GPU, and gets its
    // GPU ID in the same update. Decided bodies never migrate.
    let third = auto(&mut app, PhysicsBody::default());
    app.update(Duration::ZERO).unwrap();
    assert_eq!(
        decision(&app, third),
        (SimulationClass::Gpu, AllocationReason::ManyBodies)
    );
    assert!(app.world().get::<PhysicsId>(third).is_some());
    assert_eq!(decision(&app, first).0, SimulationClass::Cpu);

    // Hard requirements win over the count.
    let kinematic = auto(&mut app, PhysicsBody::default());
    app.world_mut().entity_mut(kinematic).insert(RigidBody {
        kind: RigidBodyKind::Kinematic,
        ..RigidBody::default()
    });
    let sensor = auto(&mut app, PhysicsBody::default());
    app.world_mut().entity_mut(sensor).insert(Collider {
        sensor: true,
        ..Collider::default()
    });
    let watched = auto(&mut app, PhysicsBody::default());
    app.world_mut()
        .entity_mut(watched)
        .insert(PhysicsSyncMode::SelectedState);
    let custom = auto(
        &mut app,
        PhysicsBody {
            solver: PhysicsSolver::Custom,
            ..PhysicsBody::default()
        },
    );
    let wall = auto(
        &mut app,
        PhysicsBody {
            simulation: SimulationClass::Static,
            ..PhysicsBody::default()
        },
    );
    app.update(Duration::ZERO).unwrap();
    assert_eq!(decision(&app, kinematic).1, AllocationReason::Kinematic);
    assert_eq!(decision(&app, sensor).1, AllocationReason::CpuOnlyCollider);
    assert_eq!(
        decision(&app, watched).1,
        AllocationReason::ReadsStateEveryTick
    );
    assert_eq!(
        decision(&app, custom),
        (SimulationClass::Gpu, AllocationReason::CustomSolver)
    );
    assert_eq!(
        decision(&app, wall),
        (SimulationClass::Static, AllocationReason::NotDynamic)
    );

    // Manual selection: without the component the class is left alone.
    let manual = app.spawn((
        Transform::default(),
        PhysicsBody {
            simulation: SimulationClass::Gpu,
            ..PhysicsBody::default()
        },
    ));
    app.world_mut().entity_mut(first).remove::<AutoSimulation>();
    app.world_mut()
        .get_mut::<PhysicsBody>(first)
        .unwrap()
        .simulation = SimulationClass::Gpu;
    // Clearing a decision asks for a new one under the current policy.
    app.world_mut()
        .resource_mut::<AutoAllocationPolicy>()
        .gpu_min_bodies = 2;
    app.world_mut()
        .get_mut::<AutoSimulation>(second)
        .unwrap()
        .decision = None;
    app.update(Duration::ZERO).unwrap();
    let world = app.world();
    assert_eq!(
        world.get::<PhysicsBody>(manual).unwrap().simulation,
        SimulationClass::Gpu
    );
    assert_eq!(
        world.get::<PhysicsBody>(first).unwrap().simulation,
        SimulationClass::Gpu
    );
    assert_eq!(
        decision(&app, second),
        (SimulationClass::Gpu, AllocationReason::ManyBodies)
    );

    // Without a GPU backend new bodies fall back to the CPU.
    app.insert_resource(PhysicsBackendStatus::default());
    let offline = auto(
        &mut app,
        PhysicsBody {
            solver: PhysicsSolver::Custom,
            ..PhysicsBody::default()
        },
    );
    app.update(Duration::ZERO).unwrap();
    assert_eq!(
        decision(&app, offline),
        (SimulationClass::Cpu, AllocationReason::NoGpuBackend)
    );

    // Measured CPU physics time over budget sends flexible bodies to the
    // GPU below the count threshold. The update reads last frame's time.
    app.insert_resource(PhysicsBackendStatus {
        gameplay_available: true,
        gpu_dynamic_available: true,
    });
    app.world_mut()
        .resource_mut::<AutoAllocationPolicy>()
        .gpu_min_bodies = 1_000;
    app.world_mut().resource_mut::<CpuFrameTimings>().physics =
        Duration::from_millis(10);
    let late = auto(&mut app, PhysicsBody::default());
    app.update(Duration::ZERO).unwrap();
    assert_eq!(
        decision(&app, late),
        (SimulationClass::Gpu, AllocationReason::CpuOverBudget)
    );
}
