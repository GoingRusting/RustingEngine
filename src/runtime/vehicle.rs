//! Raycast vehicles: a dynamic CPU body held up by one suspension ray per
//! wheel, gripped by a tire friction curve, and driven through a gearbox
//! and an open differential.
//!
//! Each fixed step, before the physics step, every wheel casts a ray down
//! the body's up axis. A hit compresses a spring-damper that pushes the
//! body up at the wheel. The tire then pushes along the ground: sideways
//! against the slip angle and forward against the slip between the wheel's
//! spin and the ground speed. Both forces follow the curve
//! `sin(1.65 * atan(10 * slip))` (a Pacejka shape), so grip peaks at a small
//! slip and falls off as the tire slides, and their sum stays inside
//! `grip * load`. A tire never pushes harder than it takes to stop its own
//! slip in one step, which keeps the stiff curve stable at any frame rate.
//!
//! The engine gives a flat torque up to `max_rpm`; the gearbox shifts up
//! and down by engine speed, and the open differential splits the torque
//! equally between the driven wheels. Vehicles run in spawn order and add
//! all wheel forces at once, so the result does not depend on wheel order.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, Query, Res, Without};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

use super::player::{
    PLAYER_BACK, PLAYER_FORWARD, PLAYER_JUMP, PLAYER_LEFT, PLAYER_RIGHT,
};
use super::{
    sim_math, ActionMap, Collider, FrameTime, Parent, PhysicsWorld, RigidBody,
    RigidBodyKind, RuntimeInput, SpawnOrder,
};
use crate::Transform;

/// Mass of one wheel in kg, for how fast torque spins it up.
// ponytail: fixed; make it a Wheel field when a game needs heavy wheels.
const WHEEL_MASS: f32 = 15.0;
/// Engine and flywheel inertia in kg·m². Through the gears it makes a
/// driven wheel much heavier to spin in first than in top gear.
// ponytail: fixed; make it a Vehicle field when a game needs it.
const ENGINE_INERTIA: f32 = 0.15;

/// One wheel of a [`Vehicle`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wheel {
    /// Where the suspension is mounted, in the body's local space. Forward
    /// is -Z and up is +Y.
    pub position: [f32; 3],
    pub radius: f32,
    /// Suspension travel below the mount in meters.
    pub suspension: f32,
    /// Spring stiffness in N/m.
    pub stiffness: f32,
    /// Damping in N·s/m.
    pub damping: f32,
    /// Turns with `Vehicle::steer`.
    pub steer: bool,
    /// Gets engine torque.
    pub drive: bool,
    /// Friction coefficient at the peak of the tire curve.
    pub grip: f32,
    /// State: spin in rad/s, positive rolls forward.
    pub spin: f32,
    /// State: how far the spring is pressed in, in meters.
    pub compression: f32,
    /// State: the wheel touched ground this step.
    pub grounded: bool,
}

impl Default for Wheel {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            radius: 0.35,
            suspension: 0.3,
            stiffness: 35_000.0,
            damping: 4_000.0,
            steer: false,
            drive: false,
            grip: 1.0,
            spin: 0.0,
            compression: 0.0,
            grounded: false,
        }
    }
}

/// A car, truck or kart on a dynamic CPU body with a box collider above
/// the ground. Set `throttle`, `brake` and `steer` from game code, or turn
/// on `player_input` to drive with the player actions.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vehicle {
    pub wheels: Vec<Wheel>,
    /// Engine torque in N·m, flat up to `max_rpm`.
    pub engine_torque: f32,
    pub max_rpm: f32,
    /// Forward gears from first; reverse uses the first.
    pub gear_ratios: Vec<f32>,
    pub final_drive: f32,
    /// Brake torque per wheel in N·m at full brake.
    pub brake_torque: f32,
    /// Steering angle of the steered wheels at full lock, in radians.
    pub max_steer: f32,
    /// Forward/back drives, left/right steers, jump brakes.
    pub player_input: bool,
    /// -1..1; below 0 drives backward in first gear.
    pub throttle: f32,
    /// 0..1.
    pub brake: f32,
    /// -1..1; above 0 turns right.
    pub steer: f32,
    /// State: index into `gear_ratios`.
    pub gear: usize,
    /// State: engine speed.
    pub rpm: f32,
}

impl Default for Vehicle {
    fn default() -> Self {
        let wheel = |x: f32, z: f32, front: bool| Wheel {
            position: [x, -0.3, z],
            steer: front,
            drive: !front,
            ..Wheel::default()
        };
        Self {
            wheels: vec![
                wheel(-0.8, -1.3, true),
                wheel(0.8, -1.3, true),
                wheel(-0.8, 1.3, false),
                wheel(0.8, 1.3, false),
            ],
            engine_torque: 300.0,
            max_rpm: 6500.0,
            gear_ratios: vec![3.5, 2.2, 1.5, 1.1, 0.9],
            final_drive: 3.4,
            brake_torque: 1500.0,
            max_steer: 0.6,
            player_input: false,
            throttle: 0.0,
            brake: 0.0,
            steer: 0.0,
            gear: 0,
            rpm: 0.0,
        }
    }
}

/// Share of the peak grip at `slip`.
fn tire_curve(slip: f32) -> f32 {
    let (sin, _) = sim_math::sin_cos(1.65 * sim_math::atan2(10.0 * slip, 1.0));
    sin.abs()
}

#[allow(clippy::type_complexity, clippy::too_many_lines)]
pub(super) fn drive_vehicles(
    time: Res<FrameTime>,
    input: Res<RuntimeInput>,
    actions: Res<ActionMap>,
    physics: Res<PhysicsWorld>,
    mut vehicles: Query<
        (
            Entity,
            Option<&SpawnOrder>,
            &mut Vehicle,
            &Transform,
            &mut RigidBody,
            &Collider,
        ),
        Without<Parent>,
    >,
) {
    let dt = time.fixed_delta.as_secs_f32();
    let axis = |positive, negative| {
        f32::from(u8::from(actions.held(&input, positive)))
            - f32::from(u8::from(actions.held(&input, negative)))
    };
    let mut all: Vec<_> = vehicles.iter_mut().collect();
    all.sort_by_key(|(entity, order, ..)| {
        (order.is_none(), order.map_or(0, |order| order.0), *entity)
    });
    for (entity, _, mut vehicle, transform, mut rigid, collider) in all {
        if rigid.kind != RigidBodyKind::Dynamic || rigid.mass <= 0.0 {
            continue;
        }
        let [roll, pitch, yaw] = transform.rotation;
        let rotation = sim_math::rotation_from_euler(roll, pitch, yaw);
        let position = Vector3::from(transform.position);
        let velocity = Vector3::from(rigid.linear_velocity);
        let angular = Vector3::from(rigid.angular_velocity);
        let inverse_inertia = super::cpu_physics::world_inverse_inertia(
            collider,
            transform.scale,
            &rotation,
            rigid.mass,
        );
        let up = rotation * Vector3::y();
        let ahead = rotation * -Vector3::z();
        if vehicle.player_input {
            // Back brakes while rolling forward, then reverses.
            let push = axis(PLAYER_FORWARD, PLAYER_BACK);
            let reversing = push < 0.0 && velocity.dot(&ahead) > 0.5;
            vehicle.throttle = if reversing { 0.0 } else { push };
            vehicle.brake = if reversing || actions.held(&input, PLAYER_JUMP) {
                1.0
            } else {
                0.0
            };
            vehicle.steer = axis(PLAYER_RIGHT, PLAYER_LEFT);
        }

        // Drivetrain: engine speed from the driven wheels, then shift.
        let driven = vehicle.wheels.iter().filter(|w| w.drive).count();
        let gears = vehicle.gear_ratios.len();
        if gears == 0 {
            continue;
        }
        vehicle.gear = vehicle.gear.min(gears - 1);
        if vehicle.throttle < 0.0 {
            vehicle.gear = 0;
        }
        let spin = vehicle
            .wheels
            .iter()
            .filter(|w| w.drive)
            .map(|w| w.spin.abs())
            .sum::<f32>()
            / driven.max(1) as f32;
        let ratios: Vec<f32> = vehicle
            .gear_ratios
            .iter()
            .map(|ratio| ratio * vehicle.final_drive)
            .collect();
        vehicle.rpm = spin * ratios[vehicle.gear] * 60.0 / TAU;
        if vehicle.rpm > 0.9 * vehicle.max_rpm && vehicle.gear + 1 < gears {
            vehicle.gear += 1;
        } else if vehicle.rpm < 0.45 * vehicle.max_rpm && vehicle.gear > 0 {
            vehicle.gear -= 1;
        }
        vehicle.rpm = spin * ratios[vehicle.gear] * 60.0 / TAU;
        let engine = if vehicle.rpm < vehicle.max_rpm {
            vehicle.engine_torque
        } else {
            0.0
        };
        // Open differential: equal shares.
        let gearing = ratios[vehicle.gear];
        let wheel_torque = vehicle.throttle.clamp(-1.0, 1.0) * engine * gearing
            / driven.max(1) as f32;

        let hits: Vec<_> = vehicle
            .wheels
            .iter()
            .map(|wheel| {
                let mount = position + rotation * Vector3::from(wheel.position);
                let reach = wheel.suspension + wheel.radius;
                physics
                    .raycast_where(
                        mount.into(),
                        (-up).into(),
                        reach,
                        u32::MAX,
                        |e| e != entity,
                    )
                    .map(|hit| (hit, reach - hit.distance))
            })
            .collect();
        let grounded = hits.iter().flatten().count().max(1) as f32;
        let (steer, brake, max_steer) = (
            vehicle.steer.clamp(-1.0, 1.0),
            vehicle.brake.clamp(0.0, 1.0),
            vehicle.max_steer,
        );
        let brake_torque = vehicle.brake_torque;
        let mut impulse = Vector3::zeros();
        let mut turn = Vector3::zeros();
        for (wheel, hit) in vehicle.wheels.iter_mut().zip(hits) {
            let mut inertia = 0.5 * WHEEL_MASS * wheel.radius * wheel.radius;
            if wheel.drive {
                inertia += ENGINE_INERTIA * gearing * gearing / driven as f32;
                wheel.spin += wheel_torque * dt / inertia;
            }
            let stop = brake * brake_torque * dt / inertia;
            wheel.spin -= wheel.spin.clamp(-stop, stop);
            wheel.grounded = hit.is_some();
            let Some((hit, compression)) = hit else {
                wheel.compression = 0.0;
                continue;
            };
            wheel.compression = compression;
            let normal = Vector3::from(hit.normal);
            let arm = Vector3::from(hit.point) - position;
            let point_velocity = velocity + angular.cross(&arm);
            let load = (wheel.stiffness * compression
                - wheel.damping * point_velocity.dot(&up))
            .max(0.0);
            let angle = if wheel.steer { -steer * max_steer } else { 0.0 };
            let (sin, cos) = sim_math::sin_cos(angle);
            let heading = rotation * Vector3::new(-sin, 0.0, -cos);
            let Some(forward) =
                (heading - normal * heading.dot(&normal)).try_normalize(1e-6)
            else {
                continue;
            };
            let side = forward.cross(&normal);
            // Mass the contact point moves with along `direction`, shared
            // between the grounded wheels.
            let mass_along = |direction: Vector3<f32>| {
                let lever = arm.cross(&direction);
                1.0 / (1.0 / rigid.mass + lever.dot(&(inverse_inertia * lever)))
                    / grounded
            };
            let ground_speed = point_velocity.dot(&forward);
            let slide = point_velocity.dot(&side);
            let lateral_cap = wheel.grip
                * load
                * tire_curve(sim_math::atan2(slide, ground_speed.abs()));
            let mut lateral = (-slide * mass_along(side) / dt)
                .clamp(-lateral_cap, lateral_cap);
            let slip = wheel.spin * wheel.radius - ground_speed;
            let long_cap = wheel.grip
                * load
                * tire_curve(slip / ground_speed.abs().max(1.0));
            let coupled = 1.0
                / (1.0 / mass_along(forward)
                    + wheel.radius * wheel.radius / inertia);
            let mut longitudinal =
                (slip * coupled / dt).clamp(-long_cap, long_cap);
            // Friction circle.
            let total = longitudinal.hypot(lateral);
            let limit = wheel.grip * load;
            if total > limit {
                longitudinal *= limit / total;
                lateral *= limit / total;
            }
            wheel.spin -= longitudinal * wheel.radius * dt / inertia;
            let force = up * load + forward * longitudinal + side * lateral;
            impulse += force * dt;
            turn += arm.cross(&(force * dt));
        }
        // ponytail: the ground gets no reaction, so a car never shoves a
        // dynamic body it drives on; add one when a game needs that.
        let velocity = velocity + impulse / rigid.mass;
        let angular = angular + inverse_inertia * turn;
        rigid.linear_velocity = velocity.into();
        rigid.angular_velocity = angular.into();
    }
}
