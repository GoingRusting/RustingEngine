//! Joints between CPU bodies, solved with the contacts in
//! [`step_cpu_physics`](super::step_cpu_physics).
//!
//! A [`Joint`] sits on the body it moves and names the body it hangs from,
//! its `target`, or no target to hang from the world. Each side has an
//! anchor point and a joint frame in its own local space; the frame's X axis
//! is the hinge, slider, and twist axis. Every [`JointKind`] is a preset of
//! the generic six-axis joint: each of the three linear and three angular
//! axes is locked, free, or limited, with an optional spring and motor.
//! Angles are this body's rotation relative to the target: twist about X,
//! then swing about Y and Z.
//!
//! Every axis becomes one or more one-dimensional velocity rows, solved by
//! sequential impulses with position drift removed through a bias. Springs
//! are soft rows, so a stiff spring stays stable at any time step. Rows are
//! built and solved in joint entity order, and each joint's impulses warm
//! start its next step.
//!
//! A joint with a `break_force` or `break_torque` breaks when a step's
//! solved impulse exceeds it: the `Joint` component is removed and a
//! [`JointBroken`] event is sent.

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, World};
use nalgebra::{Matrix3, Quaternion, Rotation3, UnitQuaternion, Vector3};
use serde::{Deserialize, Serialize};

use super::articulation::{Articulations, Side};
use super::Body;
use crate::runtime::sim_math;

/// A joint from this entity's body to `target`'s body. See the module docs.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Joint {
    /// Body this one hangs from. `Entity::PLACEHOLDER` (null in scenes)
    /// hangs it from the world; then `target_anchor` and `target_frame` are
    /// in world space.
    pub target: Entity,
    pub kind: JointKind,
    /// Joint point in this body's local space, in metres.
    pub anchor: [f32; 3],
    /// Joint axes in this body's local space, as Euler angles in radians.
    pub frame: [f32; 3],
    /// Joint point in the target's local space.
    pub target_anchor: [f32; 3],
    /// Joint axes in the target's local space.
    pub target_frame: [f32; 3],
    /// False keeps the two bodies from colliding with each other.
    pub collide_connected: bool,
    /// Force in newtons along the linear axes that breaks the joint; 0
    /// never breaks.
    pub break_force: f32,
    /// Torque in newton metres about the angular axes that breaks the
    /// joint; 0 never breaks.
    pub break_torque: f32,
}

/// Sent in the fixed step a [`Joint`] broke, after its component was
/// removed from `joint`. `force` and `torque` are the loads that step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointBroken {
    pub joint: Entity,
    /// The joint's target, or `Entity::PLACEHOLDER` for the world.
    pub target: Entity,
    pub force: f32,
    pub torque: f32,
}

impl Default for Joint {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
            kind: JointKind::default(),
            anchor: [0.0; 3],
            frame: [0.0; 3],
            target_anchor: [0.0; 3],
            target_frame: [0.0; 3],
            collide_connected: false,
            break_force: 0.0,
            break_torque: 0.0,
        }
    }
}

impl Joint {
    /// A `kind` joint to `target` at `anchor` on this body and
    /// `target_anchor` on the target, with both frames unrotated.
    #[must_use]
    pub fn new(
        kind: JointKind,
        target: Entity,
        anchor: [f32; 3],
        target_anchor: [f32; 3],
    ) -> Self {
        Self {
            target,
            kind,
            anchor,
            target_anchor,
            ..Self::default()
        }
    }
}

/// What a [`Joint`] lets move. Angles are in radians, distances in metres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum JointKind {
    /// Holds the bodies together as one.
    #[default]
    Fixed,
    /// Turns about the frame's X axis only.
    Hinge {
        /// Lowest and highest angle; `None` turns freely.
        limit: Option<[f32; 2]>,
        spring: Option<JointSpring>,
        motor: Option<JointMotor>,
    },
    /// Slides along the frame's X axis only.
    Slider {
        /// Lowest and highest offset; `None` slides freely.
        limit: Option<[f32; 2]>,
        spring: Option<JointSpring>,
        motor: Option<JointMotor>,
    },
    /// Keeps the anchors together and turns freely.
    BallSocket,
    /// A ball socket whose X axis stays within `swing` of the target's, and
    /// whose twist about X stays within `twist`. For shoulders and hips.
    ConeTwist { swing: f32, twist: [f32; 2] },
    /// Keeps the anchors between `min` and `max` apart; a rope has `min` 0.
    Distance { min: f32, max: f32 },
    /// Pulls the anchors toward `rest_length` apart.
    Spring {
        rest_length: f32,
        /// N/m.
        stiffness: f32,
        /// N·s/m.
        damping: f32,
    },
    /// Each axis on its own: linear X, Y, Z along the target's frame, then
    /// twist about X and swing about Y and Z.
    Generic {
        linear: [JointAxis; 3],
        angular: [JointAxis; 3],
    },
}

/// One axis of a [`JointKind::Generic`] joint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JointAxis {
    pub motion: AxisMotion,
    pub spring: Option<JointSpring>,
    pub motor: Option<JointMotor>,
}

impl JointAxis {
    pub const LOCKED: Self = Self {
        motion: AxisMotion::Locked,
        spring: None,
        motor: None,
    };
    pub const FREE: Self = Self {
        motion: AxisMotion::Free,
        spring: None,
        motor: None,
    };

    fn free_or_limited(
        limit: Option<[f32; 2]>,
        spring: Option<JointSpring>,
        motor: Option<JointMotor>,
    ) -> Self {
        Self {
            motion: limit.map_or(AxisMotion::Free, |[min, max]| {
                AxisMotion::Limited { min, max }
            }),
            spring,
            motor,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum AxisMotion {
    #[default]
    Locked,
    Free,
    Limited {
        min: f32,
        max: f32,
    },
}

/// Pulls an axis toward `target`. Stiffness is in N/m or N·m/rad, damping
/// in N·s/m or N·m·s/rad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JointSpring {
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
}

/// Drives an axis at `speed` (m/s or rad/s) with at most `max_force`
/// (N or N·m).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JointMotor {
    pub speed: f32,
    pub max_force: f32,
}

/// Every [`JointKind`] as six axes plus the two constraints that are not
/// per axis.
struct Axes {
    linear: [JointAxis; 3],
    angular: [JointAxis; 3],
    /// Largest swing angle, whatever its direction.
    cone: Option<f32>,
    /// Anchor distance limit and spring.
    distance: Option<(AxisMotion, Option<JointSpring>)>,
}

impl JointKind {
    fn axes(&self) -> Axes {
        use JointAxis as A;
        let axes = |linear, angular| Axes {
            linear,
            angular,
            cone: None,
            distance: None,
        };
        match *self {
            Self::Fixed => axes([A::LOCKED; 3], [A::LOCKED; 3]),
            Self::Hinge {
                limit,
                spring,
                motor,
            } => axes(
                [A::LOCKED; 3],
                [
                    A::free_or_limited(limit, spring, motor),
                    A::LOCKED,
                    A::LOCKED,
                ],
            ),
            Self::Slider {
                limit,
                spring,
                motor,
            } => axes(
                [
                    A::free_or_limited(limit, spring, motor),
                    A::LOCKED,
                    A::LOCKED,
                ],
                [A::LOCKED; 3],
            ),
            Self::BallSocket => axes([A::LOCKED; 3], [A::FREE; 3]),
            Self::ConeTwist { swing, twist } => Axes {
                cone: Some(swing),
                ..axes(
                    [A::LOCKED; 3],
                    [
                        A::free_or_limited(Some(twist), None, None),
                        A::FREE,
                        A::FREE,
                    ],
                )
            },
            Self::Distance { min, max } => Axes {
                distance: Some((AxisMotion::Limited { min, max }, None)),
                ..axes([A::FREE; 3], [A::FREE; 3])
            },
            Self::Spring {
                rest_length,
                stiffness,
                damping,
            } => Axes {
                distance: Some((
                    AxisMotion::Free,
                    Some(JointSpring {
                        target: rest_length,
                        stiffness,
                        damping,
                    }),
                )),
                ..axes([A::FREE; 3], [A::FREE; 3])
            },
            Self::Generic { linear, angular } => axes(linear, angular),
        }
    }
}

/// Share of a locked axis's position error removed per step.
const JOINT_BIAS: f32 = 0.2;

/// A joint whose bodies were both found this step.
pub(super) struct JointLink {
    pub entity: Entity,
    /// Target body, or `None` for the world.
    pub a: Option<usize>,
    pub b: usize,
    pub joint: Joint,
}

/// One-dimensional velocity constraint `linear·(vb − va) + angular_b·wb −
/// angular_a·wa`, driven toward `-bias` with its accumulated impulse kept
/// within `lower..=upper`.
pub(super) struct JointRow {
    a: Option<usize>,
    b: usize,
    linear: Vector3<f32>,
    angular_a: Vector3<f32>,
    angular_b: Vector3<f32>,
    mass: f32,
    bias: f32,
    /// Spring softness; zero for rigid rows.
    softness: f32,
    lower: f32,
    upper: f32,
    pub impulse: f32,
    /// Identifies the row within its joint across steps for warm starting.
    pub key: u8,
}

/// Jacobian of one row before its effective mass is known.
#[derive(Clone, Copy)]
struct Jacobian {
    linear: Vector3<f32>,
    angular_a: Vector3<f32>,
    angular_b: Vector3<f32>,
}

impl Jacobian {
    fn negated(self) -> Self {
        Self {
            linear: -self.linear,
            angular_a: -self.angular_a,
            angular_b: -self.angular_b,
        }
    }
}

struct RowBuilder<'a> {
    bodies: &'a [Body],
    inertia: &'a [Matrix3<f32>],
    articulations: &'a Articulations,
    link: &'a JointLink,
    dt: f32,
    rows: Vec<JointRow>,
}

impl RowBuilder<'_> {
    fn push(
        &mut self,
        jacobian: Jacobian,
        key: u8,
        bias: f32,
        softness: f32,
        [lower, upper]: [f32; 2],
    ) {
        let b = &self.bodies[self.link.b];
        let articulated = self.articulations.contains(self.link.b)
            || self.link.a.is_some_and(|a| self.articulations.contains(a));
        let mut k = b.inverse_mass * jacobian.linear.norm_squared()
            + jacobian
                .angular_b
                .dot(&(self.inertia[self.link.b] * jacobian.angular_b));
        if let Some(a) = self.link.a {
            k += self.bodies[a].inverse_mass * jacobian.linear.norm_squared()
                + jacobian
                    .angular_a
                    .dot(&(self.inertia[a] * jacobian.angular_a));
        }
        if articulated {
            k = self.articulations.inverse_mass(
                self.bodies,
                self.inertia,
                &sides(self.link.a, self.link.b, &jacobian),
            );
        }
        if k + softness <= 0.0 {
            return;
        }
        self.rows.push(JointRow {
            a: self.link.a,
            b: self.link.b,
            linear: jacobian.linear,
            angular_a: jacobian.angular_a,
            angular_b: jacobian.angular_b,
            mass: 1.0 / (k + softness),
            bias,
            softness,
            lower,
            upper,
            impulse: 0.0,
            key,
        });
    }

    /// Rows of one axis whose position is `value` along `jacobian`. Keys
    /// are `base` plus 0 locked or lower limit, 1 upper limit, 2 spring,
    /// 3 motor.
    fn axis(
        &mut self,
        axis: JointAxis,
        motion: AxisMotion,
        jacobian: Jacobian,
        value: f32,
        base: u8,
    ) {
        let rigid = JOINT_BIAS / self.dt;
        match motion {
            AxisMotion::Locked => self.push(
                jacobian,
                base,
                rigid * value,
                0.0,
                [f32::NEG_INFINITY, f32::INFINITY],
            ),
            AxisMotion::Free => {}
            AxisMotion::Limited { min, max } => {
                // Each side keeps `side · (value − bound) ≥ 0`. A side not
                // reached yet lets the bodies close the gap in one step.
                for (side, bound, key) in [(1.0, min, 0), (-1.0, max, 1)] {
                    if !bound.is_finite() {
                        continue;
                    }
                    let gap = side * (value - bound);
                    let bias = if gap > 0.0 {
                        gap / self.dt
                    } else {
                        rigid * gap
                    };
                    let jacobian = if side > 0.0 {
                        jacobian
                    } else {
                        jacobian.negated()
                    };
                    self.push(
                        jacobian,
                        base + key,
                        bias,
                        0.0,
                        [0.0, f32::INFINITY],
                    );
                }
            }
        }
        if let Some(spring) = axis.spring {
            // Soft constraint: an implicit spring, stable at any stiffness.
            let JointSpring {
                target,
                stiffness,
                damping,
            } = spring;
            let denominator = self.dt * (damping + self.dt * stiffness);
            if denominator > 0.0 {
                let factor =
                    self.dt * stiffness / (damping + self.dt * stiffness);
                self.push(
                    jacobian,
                    base + 2,
                    factor / self.dt * (value - target),
                    1.0 / denominator,
                    [f32::NEG_INFINITY, f32::INFINITY],
                );
            }
        }
        if let Some(motor) = axis.motor {
            let limit = motor.max_force.max(0.0) * self.dt;
            self.push(jacobian, base + 3, -motor.speed, 0.0, [-limit, limit]);
        }
    }
}

/// Joints whose body and target (if any) are CPU bodies, in body order.
pub(super) fn gather_joints(
    world: &mut World,
    bodies: &[Body],
) -> Vec<JointLink> {
    let index: std::collections::HashMap<Entity, usize> = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| (body.entity, index))
        .collect();
    let find = |entity: Entity| index.get(&entity).copied();
    let mut links: Vec<_> = world
        .query::<(Entity, &Joint)>()
        .iter(world)
        .filter_map(|(entity, joint)| {
            let a = if joint.target == Entity::PLACEHOLDER {
                None
            } else {
                Some(find(joint.target)?)
            };
            Some(JointLink {
                entity,
                a,
                b: find(entity)?,
                joint: *joint,
            })
        })
        .collect();
    // Each joint sits on its own body, so body order is a stable order.
    links.sort_unstable_by_key(|link| link.b);
    links
}

/// Whether the pair of body indices is held by a joint that keeps them
/// from colliding.
pub(super) fn excluded(links: &[JointLink], a: usize, b: usize) -> bool {
    links.iter().any(|link| {
        !link.joint.collide_connected
            && link.a.is_some_and(|target| {
                (target, link.b) == (a, b) || (target, link.b) == (b, a)
            })
    })
}

/// Whether any axis of the joint has a motor, which keeps its bodies awake.
pub(super) fn has_motor(joint: &Joint) -> bool {
    let axes = joint.kind.axes();
    axes.linear
        .iter()
        .chain(&axes.angular)
        .any(|axis| axis.motor.is_some())
}

/// Velocity rows of every joint, in link order.
pub(super) fn joint_rows(
    bodies: &[Body],
    inertia: &[Matrix3<f32>],
    articulations: &Articulations,
    links: &[JointLink],
    dt: f32,
) -> Vec<(usize, Vec<JointRow>)> {
    links
        .iter()
        .enumerate()
        .map(|(index, link)| {
            let mut builder = RowBuilder {
                bodies,
                inertia,
                articulations,
                link,
                dt,
                rows: Vec::new(),
            };
            build(&mut builder);
            (index, builder.rows)
        })
        .collect()
}

fn build(builder: &mut RowBuilder<'_>) {
    let link = builder.link;
    let joint = &link.joint;
    let b = &builder.bodies[link.b];
    let (a_position, a_rotation) = link
        .a
        .map_or((Vector3::zeros(), Rotation3::identity()), |a| {
            (builder.bodies[a].position, builder.bodies[a].rotation)
        });
    let euler = |angles: [f32; 3]| {
        sim_math::rotation_from_euler(angles[0], angles[1], angles[2])
    };
    let frame_a = a_rotation * euler(joint.target_frame);
    let frame_b = b.rotation * euler(joint.frame);
    let anchor_a = a_position + a_rotation * Vector3::from(joint.target_anchor);
    let anchor_b = b.position + b.rotation * Vector3::from(joint.anchor);
    let axes = joint.kind.axes();

    // Linear axes along the target's frame. Measuring both lever arms to
    // the moving anchor keeps a slider's side rows right however far out
    // it is.
    let offset = anchor_b - anchor_a;
    let (arm_a, arm_b) = (anchor_b - a_position, anchor_b - b.position);
    for (index, axis) in axes.linear.into_iter().enumerate() {
        let direction = frame_a * Vector3::ith(index, 1.0);
        builder.axis(
            axis,
            axis.motion,
            Jacobian {
                linear: direction,
                angular_a: arm_a.cross(&direction),
                angular_b: arm_b.cross(&direction),
            },
            offset.dot(&direction),
            index as u8 * 4,
        );
    }

    let relative = UnitQuaternion::from_rotation_matrix(&frame_a).inverse()
        * UnitQuaternion::from_rotation_matrix(&frame_b);
    let (twist, swing) = swing_twist(&relative);
    let angles = [twist, swing[0], swing[1]];
    for (index, axis) in axes.angular.into_iter().enumerate() {
        let direction = frame_a * Vector3::ith(index, 1.0);
        builder.axis(
            axis,
            axis.motion,
            Jacobian {
                linear: Vector3::zeros(),
                angular_a: direction,
                angular_b: direction,
            },
            angles[index],
            12 + index as u8 * 4,
        );
    }

    if let Some(limit) = axes.cone {
        let size = sim_math::sqrt(swing[0] * swing[0] + swing[1] * swing[1]);
        if size > 1e-6 {
            let direction =
                frame_a * Vector3::new(0.0, swing[0] / size, swing[1] / size);
            builder.axis(
                JointAxis::FREE,
                AxisMotion::Limited {
                    min: f32::NEG_INFINITY,
                    max: limit,
                },
                Jacobian {
                    linear: Vector3::zeros(),
                    angular_a: direction,
                    angular_b: direction,
                },
                size,
                24,
            );
        }
    }

    if let Some((motion, spring)) = axes.distance {
        let length = offset.norm();
        let direction = if length > 1e-6 {
            offset / length
        } else {
            frame_a * Vector3::x()
        };
        let arm_a = anchor_a - a_position;
        builder.axis(
            JointAxis {
                motion,
                spring,
                motor: None,
            },
            motion,
            Jacobian {
                linear: direction,
                angular_a: arm_a.cross(&direction),
                angular_b: arm_b.cross(&direction),
            },
            length,
            28,
        );
    }
}

/// Twist angle about X and swing rotation vector (about Y, Z) of `q`,
/// where `q` is the swing applied after the twist.
pub(super) fn swing_twist(q: &UnitQuaternion<f32>) -> (f32, [f32; 2]) {
    // q and -q are the same rotation; pick w ≥ 0 so angles stay in ±π.
    let sign = if q.w < 0.0 { -1.0 } else { 1.0 };
    let (w, x) = (sign * q.w, sign * q.i);
    let length = sim_math::sqrt(w * w + x * x);
    let (tw, tx) = if length > 1e-6 {
        (w / length, x / length)
    } else {
        // A half-turn swing leaves the twist undefined; call it zero.
        (1.0, 0.0)
    };
    let twist = 2.0 * sim_math::atan2(tx, tw);
    let swing = q.quaternion() * sign * Quaternion::new(tw, -tx, 0.0, 0.0);
    let (sw, sy, sz) = if swing.w < 0.0 {
        (-swing.w, -swing.j, -swing.k)
    } else {
        (swing.w, swing.j, swing.k)
    };
    let size = sim_math::sqrt(sy * sy + sz * sz);
    if size < 1e-6 {
        return (twist, [2.0 * sy, 2.0 * sz]);
    }
    let angle = 2.0 * sim_math::atan2(size, sw);
    (twist, [sy / size * angle, sz / size * angle])
}

/// The row's bodies with the impulse each takes per unit row impulse.
fn sides(a: Option<usize>, b: usize, jacobian: &Jacobian) -> Vec<Side> {
    let mut sides = vec![(b, jacobian.angular_b, jacobian.linear)];
    if let Some(a) = a {
        sides.push((a, -jacobian.angular_a, -jacobian.linear));
    }
    sides
}

/// Applies `impulse` along `row` to its bodies.
fn apply(
    bodies: &mut [Body],
    inertia: &[Matrix3<f32>],
    articulations: &mut Articulations,
    row: &JointRow,
    impulse: f32,
) {
    if articulations.contains(row.b)
        || row.a.is_some_and(|a| articulations.contains(a))
    {
        let jacobian = Jacobian {
            linear: row.linear,
            angular_a: row.angular_a,
            angular_b: row.angular_b,
        };
        let sides = sides(row.a, row.b, &jacobian);
        articulations.apply(bodies, inertia, &sides, impulse);
        return;
    }
    if let Some(a) = row.a {
        let body = &mut bodies[a];
        body.velocity -= row.linear * (body.inverse_mass * impulse);
        body.angular_velocity -= inertia[a] * row.angular_a * impulse;
    }
    let body = &mut bodies[row.b];
    body.velocity += row.linear * (body.inverse_mass * impulse);
    body.angular_velocity += inertia[row.b] * row.angular_b * impulse;
}

/// Joints whose force or torque this step passed their break threshold,
/// as link index, force and torque, in link order.
pub(super) fn broken(
    links: &[JointLink],
    rows: &[(usize, Vec<JointRow>)],
    dt: f32,
) -> Vec<(usize, f32, f32)> {
    rows.iter()
        .filter_map(|(link, rows)| {
            let joint = &links[*link].joint;
            if joint.break_force <= 0.0 && joint.break_torque <= 0.0 {
                return None;
            }
            // Linear rows carry force; purely angular rows carry torque.
            let (force, torque) = rows.iter().fold(
                (Vector3::zeros(), Vector3::zeros()),
                |(force, torque), row| {
                    if row.linear == Vector3::zeros() {
                        (force, torque + row.angular_b * row.impulse)
                    } else {
                        (force + row.linear * row.impulse, torque)
                    }
                },
            );
            let (force, torque) = (force.norm() / dt, torque.norm() / dt);
            let over = |load: f32, limit: f32| limit > 0.0 && load > limit;
            (over(force, joint.break_force) || over(torque, joint.break_torque))
                .then_some((*link, force, torque))
        })
        .collect()
}

pub(super) fn warm_start(
    bodies: &mut [Body],
    inertia: &[Matrix3<f32>],
    articulations: &mut Articulations,
    rows: &[(usize, Vec<JointRow>)],
) {
    for row in rows.iter().flat_map(|(_, rows)| rows) {
        apply(bodies, inertia, articulations, row, row.impulse);
    }
}

/// One sequential-impulse pass over every row.
pub(super) fn solve(
    bodies: &mut [Body],
    inertia: &[Matrix3<f32>],
    articulations: &mut Articulations,
    rows: &mut [(usize, Vec<JointRow>)],
) {
    for row in rows.iter_mut().flat_map(|(_, rows)| rows) {
        let b = &bodies[row.b];
        let mut speed = row.linear.dot(&b.velocity)
            + row.angular_b.dot(&b.angular_velocity);
        if let Some(a) = row.a {
            let a = &bodies[a];
            speed -= row.linear.dot(&a.velocity)
                + row.angular_a.dot(&a.angular_velocity);
        }
        let change =
            -row.mass * (speed + row.bias + row.softness * row.impulse);
        let total = (row.impulse + change).clamp(row.lower, row.upper);
        let applied = total - row.impulse;
        row.impulse = total;
        apply(bodies, inertia, articulations, row, applied);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_twist_splits_rotations_about_each_axis() {
        let about = |axis: Vector3<f32>, angle: f32| {
            UnitQuaternion::from_axis_angle(
                &nalgebra::Unit::new_normalize(axis),
                angle,
            )
        };
        let (twist, swing) = swing_twist(&about(Vector3::x(), 0.7));
        assert!((twist - 0.7).abs() < 1e-5 && swing == [0.0, 0.0]);
        let (twist, swing) = swing_twist(&about(Vector3::y(), -0.4));
        assert!(twist.abs() < 1e-6, "{twist}");
        assert!((swing[0] + 0.4).abs() < 1e-5 && swing[1].abs() < 1e-6);
        // Twist first, then swing: both come back apart.
        let (twist, swing) = swing_twist(
            &(about(Vector3::z(), 0.5) * about(Vector3::x(), -1.2)),
        );
        assert!((twist + 1.2).abs() < 1e-5, "{twist}");
        assert!(swing[0].abs() < 1e-5 && (swing[1] - 0.5).abs() < 1e-5);
    }
}
