//! Engine-owned CPU physics for `SimulationClass::Cpu` bodies.
//!
//! Every `FixedUpdate` step, [`step_cpu_physics`] gathers CPU bodies and
//! static colliders into the [`PhysicsWorld`] resource, integrates dynamic
//! and kinematic bodies, resolves contacts with sequential impulses, and
//! writes the result back to `Transform` and `RigidBody`. The ECS stays the
//! newest state for these bodies, so gameplay reads it on the same tick.
//! Queries ([`PhysicsWorld::raycast`], [`PhysicsWorld::overlap_sphere`])
//! answer immediately against the poses of the last step.
//!
//! Colliders scale with the entity's `Transform::scale`. Sensors report a
//! [`CollisionEvent`] but never push bodies apart. Two colliders interact
//! only when each one's `CollisionLayers::memberships` shares a bit with the
//! other's `filters`; a collider without `CollisionLayers` is on every layer.
//!
//! A dynamic body that stays nearly still for [`SLEEP_STEPS`] steps falls
//! asleep: it gets the [`Sleeping`] marker, its velocity is zeroed, and it
//! stops integrating until an awake moving body touches it, gameplay moves
//! it or sets its velocity, or gameplay removes the marker. Two sleeping
//! bodies (or a sleeping and a fixed one) are not tested against each
//! other, so resting sleepers send no `CollisionEvent`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{Component, Resource, World};
use nalgebra::{Matrix3, Matrix4, Rotation3, Vector3};

use crate::assets::{AssetServer, MeshAsset};
use crate::runtime::{
    Collider, ColliderShape, CollisionLayers, EventQueue, FrameTime,
    GlobalTransform, GpuProxyOf, MeshRenderer, Parent, PhysicsBody,
    PhysicsSettings, RigidBody, RigidBodyKind, SimulationClass,
};
use crate::Transform;

/// Fired once per touching pair of CPU colliders in each `FixedUpdate` step.
/// `sensor` is true when either collider is a sensor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionEvent {
    pub a: Entity,
    pub b: Entity,
    pub sensor: bool,
}

/// One touching pair found by the last step. `normal` points from `a` to
/// `b`; `depth` is how far they overlap along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub a: Entity,
    pub b: Entity,
    pub normal: [f32; 3],
    pub depth: f32,
    pub point: [f32; 3],
    pub sensor: bool,
}

/// First collider a ray reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub entity: Entity,
    pub distance: f32,
    pub point: [f32; 3],
    pub normal: [f32; 3],
}

/// Result of [`PhysicsWorld::move_character`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterMove {
    pub position: [f32; 3],
    /// True when the move touched a surface facing up (a floor).
    pub grounded: bool,
}

#[derive(Clone, Debug)]
enum Shape {
    Sphere(f32),
    Box(Vector3<f32>),
    /// Segment along local Y from `-half_height` to `half_height`, plus radius.
    Capsule {
        half_height: f32,
        radius: f32,
    },
    /// Convex hull of the mesh points (the support function never needs the
    /// hull itself).
    Hull(Arc<MeshData>),
    /// Static triangle soup, collided one triangle at a time.
    Triangles(Arc<MeshData>),
}

/// A mesh collider in the body's local frame, already scaled.
#[derive(Debug)]
struct MeshData {
    points: Vec<Vector3<f32>>,
    /// Hull triangles face outward; triangle-mesh ones keep their winding.
    triangles: Vec<[Vector3<f32>; 3]>,
    bounding_radius: f32,
    /// Distance from the local origin to the nearest face (hulls only).
    inner_radius: f32,
    /// Largest absolute coordinate on each axis.
    half_extents: Vector3<f32>,
}

impl MeshData {
    fn new(mesh: &MeshAsset, scale: [f32; 3], convex: bool) -> Self {
        let scale = Vector3::from(scale);
        let points = mesh
            .vertices
            .iter()
            .map(|vertex| Vector3::from(vertex.position).component_mul(&scale))
            .collect::<Vec<_>>();
        let indices = if mesh.indices.is_empty() {
            (0..points.len() as u32).collect()
        } else {
            mesh.indices.clone()
        };
        let center =
            points.iter().sum::<Vector3<f32>>() / points.len().max(1) as f32;
        let mut inner_radius = f32::INFINITY;
        let triangles = indices
            .chunks_exact(3)
            .filter(|chunk| chunk.iter().all(|&i| (i as usize) < points.len()))
            .map(|chunk| [0, 1, 2].map(|k| points[chunk[k] as usize]))
            .map(|[a, b, c]| {
                let normal = (b - a).cross(&(c - a));
                if !convex {
                    return [a, b, c];
                }
                let outward = if normal.dot(&(a - center)) < 0.0 {
                    [a, c, b]
                } else {
                    [a, b, c]
                };
                if let Some(normal) = normal.try_normalize(1e-12) {
                    inner_radius = inner_radius.min(normal.dot(&a).abs());
                }
                outward
            })
            .collect();
        let half_extents = points
            .iter()
            .fold(Vector3::zeros(), |most: Vector3<f32>, point| {
                most.sup(&point.abs())
            });
        Self {
            bounding_radius: points
                .iter()
                .map(|point| point.norm())
                .fold(0.0, f32::max),
            inner_radius: if convex && inner_radius.is_finite() {
                inner_radius.max(1e-3)
            } else {
                0.0
            },
            points,
            triangles,
            half_extents,
        }
    }
}

/// Mesh colliders built by the last step, keyed by mesh, revision, scale
/// and kind, so unchanged meshes are not rebuilt every step.
type MeshCache = HashMap<(u64, u64, [u32; 3], bool), Arc<MeshData>>;

#[derive(Clone, Debug)]
struct Body {
    entity: Entity,
    kind: RigidBodyKind,
    /// Children are placed by their parent, so the solver never moves them.
    movable: bool,
    position: Vector3<f32>,
    rotation: Rotation3<f32>,
    shape: Shape,
    sensor: bool,
    /// Stands in for a GPU body ([`GpuProxyOf`]); GPU bodies skip it.
    proxy: bool,
    layers: CollisionLayers,
    friction: f32,
    restitution: f32,
    inverse_mass: f32,
    /// Inverse principal moments of inertia in the body's local axes; zero
    /// for bodies the solver does not move.
    inverse_inertia: Vector3<f32>,
    asleep: bool,
    velocity: Vector3<f32>,
    angular_velocity: Vector3<f32>,
}

impl Shape {
    /// A primitive collider scaled by `scale`; `None` for mesh colliders,
    /// which need their mesh (see [`gather_bodies`]).
    fn scaled(shape: ColliderShape, scale: [f32; 3]) -> Option<Self> {
        let scale = Vector3::from(scale).abs();
        Some(match shape {
            ColliderShape::Sphere { radius } => {
                Self::Sphere(radius * scale.max())
            }
            ColliderShape::Box { half_extents } => {
                Self::Box(Vector3::from(half_extents).component_mul(&scale))
            }
            ColliderShape::Capsule {
                half_height,
                radius,
            } => Self::Capsule {
                half_height: half_height * scale.y,
                radius: radius * scale.x.max(scale.z),
            },
            ColliderShape::ConvexMesh | ColliderShape::TriangleMesh => {
                return None;
            }
        })
    }

    /// Inverse principal moments of a solid shape of `mass`.
    // ponytail: a capsule counts as its bounding box; close enough for
    // tumbling, exact formula if capsule spin ever looks wrong.
    // ponytail: hulls count as their bounding box too.
    fn inverse_inertia(&self, mass: f32) -> Vector3<f32> {
        let half = match *self {
            Self::Sphere(radius) => {
                return Vector3::repeat(1.0 / (0.4 * mass * radius * radius));
            }
            Self::Box(half) => half,
            Self::Hull(ref mesh) | Self::Triangles(ref mesh) => {
                mesh.half_extents
            }
            Self::Capsule {
                half_height,
                radius,
            } => Vector3::new(radius, half_height + radius, radius),
        };
        let squared = half.component_mul(&half);
        Vector3::new(
            squared.y + squared.z,
            squared.x + squared.z,
            squared.x + squared.y,
        )
        .map(|sum| 3.0 / (mass * sum).max(1e-9))
    }

    /// GPU encoding: x = kind (0 box, 1 sphere, 2 capsule), then the box
    /// half extents, the sphere radius, or the capsule half height and
    /// radius.
    // ponytail: GPU bodies see a mesh collider as its bounding box; send
    // triangles to the shader if GPU bodies must land on uneven meshes.
    fn gpu_words(&self) -> [f32; 4] {
        match *self {
            Self::Box(half) => [0.0, half.x, half.y, half.z],
            Self::Hull(ref mesh) | Self::Triangles(ref mesh) => {
                let half = mesh.half_extents;
                [0.0, half.x, half.y, half.z]
            }
            Self::Sphere(radius) => [1.0, radius, 0.0, 0.0],
            Self::Capsule {
                half_height,
                radius,
            } => [2.0, half_height, radius, 0.0],
        }
    }
}

/// [`GpuCollider::shape`] kind of a GPU body that has no collider.
pub(crate) const GPU_NO_SHAPE: [f32; 4] = [3.0, 0.0, 0.0, 0.0];

/// A GPU body's collider scaled by its transform, in the encoding of
/// [`GpuCollider::shape`], or [`GPU_NO_SHAPE`].
pub(crate) fn gpu_shape_words(
    collider: Option<&Collider>,
    scale: [f32; 3],
) -> [f32; 4] {
    // ponytail: a GPU body's own mesh is not at hand here, so a mesh
    // collider on a GPU body is its unit cube scaled by the transform.
    collider.map_or(GPU_NO_SHAPE, |collider| {
        Shape::scaled(collider.shape, scale)
            .unwrap_or(Shape::Box(Vector3::from(scale).abs() * 0.5))
            .gpu_words()
    })
}

/// A solid CPU or static collider that GPU bodies collide against, as of
/// the last CPU step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuCollider {
    /// Rotation and position, without scale (scale is in `shape`).
    pub model: [[f32; 4]; 4],
    /// x = kind (0 box, 1 sphere, 2 capsule along local Y); then the box
    /// half extents, the sphere radius, or the capsule half height and
    /// radius. Already scaled.
    pub shape: [f32; 4],
    /// Linear velocity of a kinematic or dynamic CPU body.
    pub velocity: [f32; 3],
    pub friction: f32,
    pub restitution: f32,
    pub layers: CollisionLayers,
}

impl Body {
    /// Query-only sensor body.
    fn probe(shape: Shape, position: Vector3<f32>) -> Self {
        Self {
            entity: Entity::PLACEHOLDER,
            kind: RigidBodyKind::Fixed,
            movable: false,
            position,
            rotation: Rotation3::identity(),
            shape,
            sensor: true,
            proxy: false,
            layers: CollisionLayers::default(),
            friction: 0.0,
            restitution: 0.0,
            inverse_mass: 0.0,
            inverse_inertia: Vector3::zeros(),
            asleep: false,
            velocity: Vector3::zeros(),
            angular_velocity: Vector3::zeros(),
        }
    }

    fn world_inverse_inertia(&self) -> Matrix3<f32> {
        let rotation = self.rotation.matrix();
        rotation
            * Matrix3::from_diagonal(&self.inverse_inertia)
            * rotation.transpose()
    }

    fn segment(&self) -> (Vector3<f32>, Vector3<f32>, f32) {
        let &Shape::Capsule {
            half_height,
            radius,
        } = &self.shape
        else {
            unreachable!("only capsules have a segment");
        };
        let axis = self.rotation * Vector3::y() * half_height;
        (self.position - axis, self.position + axis, radius)
    }

    /// Radius of the largest sphere around the center inside the shape.
    fn inner_radius(&self) -> f32 {
        match self.shape {
            Shape::Sphere(radius) | Shape::Capsule { radius, .. } => radius,
            Shape::Box(half) => half.min(),
            Shape::Hull(ref mesh) | Shape::Triangles(ref mesh) => {
                mesh.inner_radius
            }
        }
    }

    /// Radius around the core shape: a sphere is a rounded point and a
    /// capsule a rounded segment; the rest have no rounding.
    fn core_radius(&self) -> f32 {
        match self.shape {
            Shape::Sphere(radius) | Shape::Capsule { radius, .. } => radius,
            _ => 0.0,
        }
    }

    /// Farthest point of the core shape along `direction`, in world space.
    fn support(&self, direction: Vector3<f32>) -> Vector3<f32> {
        let local = self.rotation.inverse() * direction;
        let farthest = |points: &[Vector3<f32>]| {
            points
                .iter()
                .copied()
                .max_by(|a, b| a.dot(&local).total_cmp(&b.dot(&local)))
                .unwrap_or_default()
        };
        let point = match self.shape {
            Shape::Sphere(_) => Vector3::zeros(),
            Shape::Capsule { half_height, .. } => {
                Vector3::y() * half_height.copysign(local.y)
            }
            Shape::Box(half) => local.zip_map(&half, |d, h| h.copysign(d)),
            Shape::Hull(ref mesh) | Shape::Triangles(ref mesh) => {
                farthest(&mesh.points)
            }
        };
        self.position + self.rotation * point
    }

    /// Corners of a box or points of a hull in world space; empty for
    /// rounded shapes and triangle meshes.
    fn vertices(&self) -> Vec<Vector3<f32>> {
        let local = match self.shape {
            Shape::Box(half) => (0..8)
                .map(|corner| {
                    Vector3::from_fn(|axis, _| {
                        if corner >> axis & 1 == 1 {
                            half[axis]
                        } else {
                            -half[axis]
                        }
                    })
                })
                .collect(),
            Shape::Hull(ref mesh) => mesh.points.clone(),
            _ => Vec::new(),
        };
        local
            .into_iter()
            .map(|point| self.position + self.rotation * point)
            .collect()
    }

    fn interacts_with(&self, other: &Self) -> bool {
        self.layers.memberships & other.layers.filters != 0
            && other.layers.memberships & self.layers.filters != 0
    }

    fn bounding_radius(&self) -> f32 {
        match self.shape {
            Shape::Sphere(radius) => radius,
            Shape::Hull(ref mesh) | Shape::Triangles(ref mesh) => {
                mesh.bounding_radius
            }
            Shape::Box(half) => half.norm(),
            Shape::Capsule {
                half_height,
                radius,
            } => half_height + radius,
        }
    }
}

/// Marks a CPU body that fell asleep; see the module docs. Remove it to
/// wake the body.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sleeping;

/// CPU physics state after the last fixed step, with immediate queries.
#[derive(Resource, Debug, Default)]
pub struct PhysicsWorld {
    bodies: Vec<Body>,
    contacts: Vec<Contact>,
    /// Consecutive still steps of each awake dynamic body, and the position
    /// each sleeping body fell asleep at (to notice gameplay moving it).
    rest: HashMap<Entity, (u32, [f32; 3])>,
    /// Last step's impulses per contact pair, to warm-start the solver.
    warm: WarmStart,
    meshes: MeshCache,
}

/// Contact points of each pair in the first body's local frame, with the
/// normal and two tangent impulses they ended the step with.
type WarmStart = HashMap<(Entity, Entity), Vec<(Vector3<f32>, [f32; 3])>>;
/// Local distance within which a new manifold point inherits an old impulse.
const WARM_MATCH: f32 = 0.05;

/// Still steps before a body sleeps (half a second at 60 Hz).
pub const SLEEP_STEPS: u32 = 30;
/// Speeds below which a body counts as still, in m/s and rad/s.
const SLEEP_LINEAR: f32 = 0.05;
const SLEEP_ANGULAR: f32 = 0.05;

const SOLVER_ITERATIONS: usize = 8;
/// Overlap left alone so resting contacts do not jitter.
const PENETRATION_SLOP: f32 = 0.005;
/// Share of the remaining overlap removed each step.
const POSITION_CORRECTION: f32 = 0.8;
/// Closing speed below which contacts do not bounce, in m/s.
const RESTITUTION_THRESHOLD: f32 = 1.0;

impl PhysicsWorld {
    /// Touching pairs found by the last step, sensors included.
    #[must_use]
    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    /// Nearest collider hit by a ray within `max_distance`, among colliders
    /// whose `CollisionLayers::memberships` share a bit with `layer_mask`
    /// (`u32::MAX` for all). Sensors are included; a ray starting inside a
    /// collider does not hit it.
    #[must_use]
    pub fn raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
        layer_mask: u32,
    ) -> Option<RayHit> {
        let origin = Vector3::from(origin);
        let direction = Vector3::from(direction).try_normalize(1e-6)?;
        self.bodies
            .iter()
            .filter(|body| body.layers.memberships & layer_mask != 0)
            .filter_map(|body| {
                let (distance, normal) = ray_body(origin, direction, body)?;
                (distance <= max_distance).then(|| RayHit {
                    entity: body.entity,
                    distance,
                    point: (origin + direction * distance).into(),
                    normal: normal.into(),
                })
            })
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    /// Solid colliders of the last step, for GPU bodies to collide against.
    #[must_use]
    pub fn gpu_colliders(&self) -> Vec<GpuCollider> {
        self.bodies
            .iter()
            .filter(|body| !body.sensor && !body.proxy)
            .map(|body| {
                let mut model = body.rotation.to_homogeneous();
                model.fixed_view_mut::<3, 1>(0, 3).copy_from(&body.position);
                GpuCollider {
                    model: model.into(),
                    shape: body.shape.gpu_words(),
                    velocity: body.velocity.into(),
                    friction: body.friction,
                    restitution: body.restitution,
                    layers: body.layers,
                }
            })
            .collect()
    }

    /// Every collider on a layer in `layer_mask` that overlaps a sphere, in
    /// stable entity order.
    #[must_use]
    pub fn overlap_sphere(
        &self,
        center: [f32; 3],
        radius: f32,
        layer_mask: u32,
    ) -> Vec<Entity> {
        let probe = Body::probe(Shape::Sphere(radius), center.into());
        self.bodies
            .iter()
            .filter(|body| body.layers.memberships & layer_mask != 0)
            .filter(|body| collide(&probe, body).is_some())
            .map(|body| body.entity)
            .collect()
    }

    /// Nearest solid collider hit when `shape` (unrotated) moves from
    /// `origin` along `direction` up to `max_distance`. Uses the same layer
    /// mask as [`Self::raycast`]; sensors and `exclude` are skipped. A
    /// collider the shape already overlaps is hit at distance 0 only when
    /// the motion goes deeper into it, so a shape can always move out.
    /// `distance` is where the shape stops just before touching; `normal`
    /// points from the collider toward the shape.
    // ponytail: marches in steps of the shape's inner radius, then bisects;
    // a collider thinner than that gap can be skipped, and cost grows with
    // `max_distance`. Use exact swept tests if casts get long or hot.
    #[must_use]
    pub fn shape_cast(
        &self,
        shape: ColliderShape,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
        layer_mask: u32,
        exclude: Option<Entity>,
    ) -> Option<RayHit> {
        let shape = Shape::scaled(shape, [1.0; 3])?;
        let origin = Vector3::from(origin);
        let direction = Vector3::from(direction).try_normalize(1e-6)?;
        let probe = |distance: f32| {
            Body::probe(shape.clone(), origin + direction * distance)
        };
        let reach = probe(0.0).bounding_radius();
        let step = probe(0.0).inner_radius().max(1e-3);
        self.bodies
            .iter()
            .filter(|body| body.layers.memberships & layer_mask != 0)
            .filter(|body| !body.sensor && Some(body.entity) != exclude)
            .filter(|body| {
                // Skip colliders whose bounding sphere is far from the path.
                let along = (body.position - origin)
                    .dot(&direction)
                    .clamp(0.0, max_distance);
                let gap = (origin + direction * along - body.position).norm();
                gap <= reach + body.bounding_radius()
            })
            .filter_map(|body| {
                if let Some(start) = collide(&probe(0.0), body) {
                    let normal = -Vector3::from(start.normal);
                    return (normal.dot(&direction) < 0.0).then(|| RayHit {
                        entity: body.entity,
                        distance: 0.0,
                        point: start.point,
                        normal: normal.into(),
                    });
                }
                let (mut free, mut blocked) = (0.0_f32, None);
                while free < max_distance {
                    let next = (free + step).min(max_distance);
                    if collide(&probe(next), body).is_some() {
                        blocked = Some(next);
                        break;
                    }
                    free = next;
                }
                let mut blocked = blocked?;
                for _ in 0..16 {
                    let middle = 0.5 * (free + blocked);
                    if collide(&probe(middle), body).is_some() {
                        blocked = middle;
                    } else {
                        free = middle;
                    }
                }
                let contact = collide(&probe(blocked), body)?;
                Some(RayHit {
                    entity: body.entity,
                    distance: free,
                    point: contact.point,
                    normal: (-Vector3::from(contact.normal)).into(),
                })
            })
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    /// Moves an unrotated character `shape` (a box, sphere or capsule; a
    /// mesh shape does not move) from `position` by `motion`,
    /// sliding along solid colliders instead of passing through them (see
    /// [`Self::shape_cast`]). The caller writes the returned position to the
    /// character's `Transform`; pass the character's own entity as
    /// `exclude`. Uses the poses of the last fixed step.
    #[must_use]
    pub fn move_character(
        &self,
        shape: ColliderShape,
        position: [f32; 3],
        motion: [f32; 3],
        layer_mask: u32,
        exclude: Option<Entity>,
    ) -> CharacterMove {
        /// Gap kept to surfaces so the next move does not start inside them.
        const SKIN: f32 = 0.01;
        let mut position = Vector3::from(position);
        let mut remaining = Vector3::from(motion);
        let mut grounded = false;
        if Shape::scaled(shape, [1.0; 3]).is_none() {
            remaining = Vector3::zeros();
        }
        // Each slide removes motion into one surface; three covers a corner.
        for _ in 0..4 {
            let length = remaining.norm();
            if length < 1e-6 {
                break;
            }
            let direction = remaining / length;
            let Some(hit) = self.shape_cast(
                shape,
                position.into(),
                direction.into(),
                length + SKIN,
                layer_mask,
                exclude,
            ) else {
                position += remaining;
                break;
            };
            let normal = Vector3::from(hit.normal);
            grounded |= normal.y > 0.7;
            let travel = (hit.distance - SKIN).max(0.0);
            position += direction * travel;
            remaining = direction * (length - travel);
            remaining -= normal * remaining.dot(&normal).min(0.0);
        }
        CharacterMove {
            position: position.into(),
            grounded,
        }
    }
}

/// Runs one fixed CPU physics step; see the module docs.
pub(super) fn step_cpu_physics(world: &mut World) {
    let dt = world.resource::<FrameTime>().fixed_delta.as_secs_f32();
    let settings = world.resource::<PhysicsSettings>().clone();
    let mut rest =
        std::mem::take(&mut world.resource_mut::<PhysicsWorld>().rest);
    let mut meshes =
        std::mem::take(&mut world.resource_mut::<PhysicsWorld>().meshes);
    let mut bodies = gather_bodies(world, &mut meshes);
    world.resource_mut::<PhysicsWorld>().meshes = meshes;
    rest.retain(|entity, _| {
        bodies
            .binary_search_by_key(entity, |body| body.entity)
            .is_ok()
    });
    // Gameplay woke it: moved it, gave it velocity, or removed the marker.
    for body in bodies.iter_mut().filter(|body| body.asleep) {
        let position: [f32; 3] = body.position.into();
        if body.velocity != Vector3::zeros()
            || rest.get(&body.entity).is_none_or(|(_, at)| *at != position)
        {
            body.asleep = false;
        }
    }
    let contacts = find_contacts(&bodies);
    // ponytail: one pass, so a push wakes only direct neighbours this step
    // and a chain wakes over the next steps; add islands if that shows.
    for (a, b, contact) in &contacts {
        let moving = |body: &Body| {
            !body.asleep
                && body.kind != RigidBodyKind::Fixed
                && body.velocity.norm() > SLEEP_LINEAR
        };
        if contact.sensor {
            continue;
        }
        if moving(&bodies[*a]) {
            bodies[*b].asleep = false;
        }
        if moving(&bodies[*b]) {
            bodies[*a].asleep = false;
        }
    }
    for body in &mut bodies {
        if body.asleep {
            body.inverse_mass = 0.0;
            body.inverse_inertia = Vector3::zeros();
            body.velocity = Vector3::zeros();
            body.angular_velocity = Vector3::zeros();
        } else if world.get::<Sleeping>(body.entity).is_some() {
            world.entity_mut(body.entity).remove::<Sleeping>();
            rest.remove(&body.entity);
        }
    }
    let gravity = Vector3::from(settings.gravity);
    if settings.enabled {
        for body in bodies.iter_mut().filter(|body| body.inverse_mass > 0.0) {
            let scale = world
                .get::<RigidBody>(body.entity)
                .map_or(1.0, |rigid| rigid.gravity_scale);
            body.velocity += gravity * scale * dt;
        }
    }
    if settings.enabled {
        let mut physics = world.resource_mut::<PhysicsWorld>();
        let mut warm = std::mem::take(&mut physics.warm);
        solve_velocities(&mut bodies, &contacts, &mut warm);
        world.resource_mut::<PhysicsWorld>().warm = warm;
        sweep_fast_bodies(&mut bodies, dt);
        for body in bodies.iter_mut().filter(|body| {
            body.movable && body.kind != RigidBodyKind::Fixed && !body.asleep
        }) {
            body.position += body.velocity * dt;
            let spin = body.angular_velocity * dt;
            if spin != Vector3::zeros() {
                body.rotation =
                    Rotation3::from_scaled_axis(spin) * body.rotation;
            }
        }
        correct_positions(&mut bodies, &contacts, dt);
        write_back(world, &bodies);
        fall_asleep(world, &bodies, &mut rest);
    }

    let mut events = world.resource_mut::<EventQueue<CollisionEvent>>();
    for (a, b, contact) in &contacts {
        events.send(CollisionEvent {
            a: bodies[*a].entity,
            b: bodies[*b].entity,
            sensor: contact.sensor,
        });
    }
    let mut physics = world.resource_mut::<PhysicsWorld>();
    physics.contacts =
        contacts.into_iter().map(|(.., contact)| contact).collect();
    physics.bodies = bodies;
    physics.rest = rest;
}

/// Counts still steps for awake dynamic bodies and puts the ones that stayed
/// still long enough to sleep.
fn fall_asleep(
    world: &mut World,
    bodies: &[Body],
    rest: &mut HashMap<Entity, (u32, [f32; 3])>,
) {
    for body in bodies.iter().filter(|body| {
        !body.asleep
            && body.inverse_mass > 0.0
            && body.kind == RigidBodyKind::Dynamic
    }) {
        let still = body.velocity.norm() < SLEEP_LINEAR
            && body.angular_velocity.norm() < SLEEP_ANGULAR;
        let steps = rest.entry(body.entity).or_default();
        steps.0 = if still { steps.0 + 1 } else { 0 };
        if steps.0 < SLEEP_STEPS {
            continue;
        }
        steps.1 = body.position.into();
        let mut entity = world.entity_mut(body.entity);
        entity.insert(Sleeping);
        if let Some(mut rigid) = entity.get_mut::<RigidBody>() {
            rigid.linear_velocity = [0.0; 3];
            rigid.angular_velocity = [0.0; 3];
        }
    }
}

/// Collision bodies of every CPU and static collider, in entity order. A
/// mesh collider whose entity has no loaded mesh is left out.
fn gather_bodies(world: &mut World, meshes: &mut MeshCache) -> Vec<Body> {
    let mut query = world.query::<(
        Entity,
        &Transform,
        &Collider,
        &PhysicsBody,
        Option<&RigidBody>,
        Option<&Parent>,
        Option<&GlobalTransform>,
        Option<&CollisionLayers>,
        Option<&Sleeping>,
        Option<&MeshRenderer>,
        Option<&GpuProxyOf>,
    )>();
    let assets = world.get_resource::<AssetServer>();
    let mut used = MeshCache::new();
    let mut bodies = query
        .iter(world)
        .filter(|(_, _, _, physics, ..)| {
            matches!(
                physics.simulation,
                SimulationClass::Cpu | SimulationClass::Static
            )
        })
        .filter_map(
            |(
                entity,
                transform,
                collider,
                physics,
                rigid,
                parent,
                global,
                layers,
                sleeping,
                renderer,
                proxy,
            )| {
                let movable = parent.is_none();
                // ponytail: children read the pose propagated last frame, and
                // are treated as fixed; simulate them once joints exist.
                let pose = match (parent, global) {
                    (Some(_), Some(global)) => {
                        Transform::from_matrix(Matrix4::from(global.matrix))
                    }
                    _ => *transform,
                };
                let rigid = rigid.copied().unwrap_or(RigidBody {
                    kind: RigidBodyKind::Fixed,
                    ..RigidBody::default()
                });
                let kind = if physics.simulation == SimulationClass::Static {
                    RigidBodyKind::Fixed
                } else {
                    rigid.kind
                };
                let shape = match Shape::scaled(collider.shape, pose.scale) {
                    Some(shape) => shape,
                    None => {
                        let handle = renderer?.mesh;
                        let assets = assets?;
                        let convex =
                            collider.shape == ColliderShape::ConvexMesh;
                        let key = (
                            handle.key(),
                            assets.meshes.revision(handle)?,
                            pose.scale.map(f32::to_bits),
                            convex,
                        );
                        let mesh = match meshes.get(&key) {
                            Some(mesh) => mesh.clone(),
                            None => Arc::new(MeshData::new(
                                assets.meshes.get(handle)?,
                                pose.scale,
                                convex,
                            )),
                        };
                        used.insert(key, mesh.clone());
                        if convex {
                            Shape::Hull(mesh)
                        } else {
                            Shape::Triangles(mesh)
                        }
                    }
                };
                let inverse_mass = if kind == RigidBodyKind::Dynamic
                    && movable
                    && rigid.mass > 0.0
                    && !matches!(shape, Shape::Triangles(_))
                {
                    1.0 / rigid.mass
                } else {
                    0.0
                };
                let moving = kind != RigidBodyKind::Fixed;
                Some(Body {
                    entity,
                    kind,
                    movable,
                    position: pose.position.into(),
                    rotation: Rotation3::from_euler_angles(
                        pose.rotation[0],
                        pose.rotation[1],
                        pose.rotation[2],
                    ),
                    sensor: collider.sensor,
                    proxy: proxy.is_some(),
                    layers: layers.copied().unwrap_or_default(),
                    friction: collider.friction.max(0.0),
                    restitution: collider.restitution.clamp(0.0, 1.0),
                    inverse_mass,
                    inverse_inertia: if inverse_mass > 0.0 {
                        shape.inverse_inertia(rigid.mass)
                    } else {
                        Vector3::zeros()
                    },
                    shape,
                    asleep: sleeping.is_some() && inverse_mass > 0.0,
                    velocity: if moving {
                        rigid.linear_velocity.into()
                    } else {
                        Vector3::zeros()
                    },
                    angular_velocity: if moving {
                        rigid.angular_velocity.into()
                    } else {
                        Vector3::zeros()
                    },
                })
            },
        )
        .collect::<Vec<_>>();
    *meshes = used;
    // Query order follows archetypes; sort so results never depend on it.
    bodies.sort_by_key(|body| body.entity);
    bodies
}

/// Broad phase: sort-and-sweep of bounding spheres along X, then the exact
/// pair test. Contacts come out in (a, b) index order, the same order the
/// solver has always seen, so results do not depend on the sweep.
// ponytail: one axis only; bodies stacked in a tall column all overlap in X
// and fall back to O(n^2). Sweep the axis of largest spread, or add a grid,
// if that shows up.
fn find_contacts(bodies: &[Body]) -> Vec<(usize, usize, Contact)> {
    let span = |body: &Body| {
        let radius = body.bounding_radius();
        (body.position.x - radius, body.position.x + radius)
    };
    let mut order = (0..bodies.len()).collect::<Vec<_>>();
    order.sort_by(|&a, &b| span(&bodies[a]).0.total_cmp(&span(&bodies[b]).0));
    let mut contacts = Vec::new();
    for (rank, &a) in order.iter().enumerate() {
        let end = span(&bodies[a]).1;
        for &b in &order[rank + 1..] {
            if span(&bodies[b]).0 > end {
                break;
            }
            let (a, b) = (a.min(b), a.max(b));
            if let Some(contact) = narrow_phase(&bodies[a], &bodies[b]) {
                contacts.push((a, b, contact));
            }
        }
    }
    contacts.sort_by_key(|&(a, b, _)| (a, b));
    contacts
}

fn narrow_phase(first: &Body, second: &Body) -> Option<Contact> {
    let inert = |body: &Body| body.kind == RigidBodyKind::Fixed || body.asleep;
    if inert(first) && inert(second) || !first.interacts_with(second) {
        return None;
    }
    let reach = first.bounding_radius() + second.bounding_radius();
    if (first.position - second.position).norm_squared() > reach * reach {
        return None;
    }
    collide(first, second)
}

/// One manifold point of a solid contact, prepared for the solver.
struct SolverPoint {
    a: usize,
    b: usize,
    normal: Vector3<f32>,
    tangents: [Vector3<f32>; 2],
    /// Contact point relative to each body's center.
    ra: Vector3<f32>,
    rb: Vector3<f32>,
    /// Inverse effective masses along the normal and the two tangents.
    masses: [f32; 3],
    /// Normal speed the contact aims for (restitution bounce).
    target: f32,
    friction: f32,
    /// Accumulated normal and tangent impulses.
    impulses: [f32; 3],
}

/// Sequential impulses over every manifold point, with angular response and
/// accumulated friction clamped by each point's normal impulse.
fn solve_velocities(
    bodies: &mut [Body],
    contacts: &[(usize, usize, Contact)],
    warm: &mut WarmStart,
) {
    let inertia = bodies
        .iter()
        .map(Body::world_inverse_inertia)
        .collect::<Vec<_>>();
    let point_velocity = |bodies: &[Body], a: usize, b: usize, ra, rb| {
        let (first, second) = (&bodies[a], &bodies[b]);
        second.velocity + second.angular_velocity.cross(&rb)
            - first.velocity
            - first.angular_velocity.cross(&ra)
    };
    let mut points = Vec::new();
    for (a, b, contact) in contacts {
        let (a, b) = (*a, *b);
        let (first, second) = (&bodies[a], &bodies[b]);
        if contact.sensor || first.inverse_mass + second.inverse_mass == 0.0 {
            continue;
        }
        let normal = Vector3::from(contact.normal);
        let tangent = normal
            .cross(&Vector3::x())
            .try_normalize(1e-3)
            .unwrap_or_else(|| normal.cross(&Vector3::z()).normalize());
        let tangents = [tangent, normal.cross(&tangent)];
        for point in manifold(first, second, contact) {
            let (ra, rb) = (point - first.position, point - second.position);
            let mass = |direction: &Vector3<f32>| {
                let (ca, cb) = (ra.cross(direction), rb.cross(direction));
                let k = first.inverse_mass
                    + second.inverse_mass
                    + ca.dot(&(inertia[a] * ca))
                    + cb.dot(&(inertia[b] * cb));
                if k > 0.0 {
                    1.0 / k
                } else {
                    0.0
                }
            };
            // Bounce targets use the closing speed before any impulse.
            let closing = point_velocity(bodies, a, b, ra, rb).dot(&normal);
            let restitution = first.restitution.max(second.restitution);
            points.push(SolverPoint {
                a,
                b,
                normal,
                tangents,
                ra,
                rb,
                masses: [mass(&normal), mass(&tangents[0]), mass(&tangents[1])],
                target: if closing < -RESTITUTION_THRESHOLD {
                    -restitution * closing
                } else {
                    0.0
                },
                friction: (first.friction * second.friction).sqrt(),
                impulses: warm
                    .get(&(first.entity, second.entity))
                    .and_then(|old| {
                        let local = first.rotation.inverse() * ra;
                        old.iter()
                            .find(|(at, _)| (at - local).norm() < WARM_MATCH)
                    })
                    .map_or([0.0; 3], |(_, impulses)| *impulses),
            });
        }
    }
    let apply = |bodies: &mut [Body], point: &SolverPoint, impulse| {
        let (a, b) = (point.a, point.b);
        bodies[a].velocity -= impulse * bodies[a].inverse_mass;
        bodies[a].angular_velocity -= inertia[a] * point.ra.cross(&impulse);
        bodies[b].velocity += impulse * bodies[b].inverse_mass;
        bodies[b].angular_velocity += inertia[b] * point.rb.cross(&impulse);
    };
    for point in &points {
        let [normal, first, second] = point.impulses;
        let impulse = point.normal * normal
            + point.tangents[0] * first
            + point.tangents[1] * second;
        apply(bodies, point, impulse);
    }
    for _ in 0..SOLVER_ITERATIONS {
        for point in &mut points {
            let velocity =
                point_velocity(bodies, point.a, point.b, point.ra, point.rb);
            let change =
                (point.target - velocity.dot(&point.normal)) * point.masses[0];
            let total = (point.impulses[0] + change).max(0.0);
            let applied = total - point.impulses[0];
            point.impulses[0] = total;
            apply(bodies, point, point.normal * applied);

            // Coulomb friction on each tangent, limited by the normal impulse.
            let limit = point.friction * point.impulses[0];
            for axis in 0..2 {
                let tangent = point.tangents[axis];
                let velocity = point_velocity(
                    bodies, point.a, point.b, point.ra, point.rb,
                );
                let change = -velocity.dot(&tangent) * point.masses[axis + 1];
                let total =
                    (point.impulses[axis + 1] + change).clamp(-limit, limit);
                let applied = total - point.impulses[axis + 1];
                point.impulses[axis + 1] = total;
                apply(bodies, point, tangent * applied);
            }
        }
    }
    warm.clear();
    for point in &points {
        let first = &bodies[point.a];
        warm.entry((first.entity, bodies[point.b].entity))
            .or_default()
            .push((first.rotation.inverse() * point.ra, point.impulses));
    }
}

/// Contact points of a pair in world space. Two boxes touching face to face
/// get up to four points: the incident face clipped to the reference face.
/// Every other pair, and edge contacts, use the single contact point.
fn manifold(a: &Body, b: &Body, contact: &Contact) -> Vec<Vector3<f32>> {
    let single = vec![Vector3::from(contact.point)];
    let (&Shape::Box(half_a), &Shape::Box(half_b)) = (&a.shape, &b.shape)
    else {
        return mesh_manifold(a, b, contact).unwrap_or(single);
    };
    let normal = Vector3::from(contact.normal);
    // The body axis most parallel to `direction`: (axis, alignment, sign).
    let facing = |body: &Body, direction: Vector3<f32>| {
        (0..3)
            .map(|axis| {
                let dot =
                    (body.rotation * Vector3::ith(axis, 1.0)).dot(&direction);
                (axis, dot.abs(), dot.signum())
            })
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap()
    };
    let (face_a, face_b) = (facing(a, normal), facing(b, -normal));
    // The reference face is the one most parallel to the contact normal.
    let (reference, half, (axis, alignment, sign), incident, incident_half) =
        if face_a.1 >= face_b.1 {
            (a, half_a, face_a, b, half_b)
        } else {
            (b, half_b, face_b, a, half_a)
        };
    if alignment < 0.95 {
        return single;
    }
    let outward = reference.rotation * Vector3::ith(axis, sign);
    let (incident_axis, _, incident_sign) = facing(incident, -outward);
    let (u, v) = ((incident_axis + 1) % 3, (incident_axis + 2) % 3);
    let to_reference = reference.rotation.inverse();
    let mut polygon = [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)]
        .map(|(su, sv)| {
            let mut corner = Vector3::zeros();
            corner[incident_axis] =
                incident_sign * incident_half[incident_axis];
            corner[u] = su * incident_half[u];
            corner[v] = sv * incident_half[v];
            let world = incident.position + incident.rotation * corner;
            to_reference * (world - reference.position)
        })
        .to_vec();
    let sides = [(axis + 1) % 3, (axis + 2) % 3];
    for side in sides {
        for direction in [1.0, -1.0] {
            polygon = clip(&polygon, side, direction, half[side]);
        }
    }
    // Keep the clipped points that are below the reference face.
    polygon.retain(|point| half[axis] - sign * point[axis] >= 0.0);
    if polygon.is_empty() {
        return single;
    }
    if polygon.len() > 4 {
        // The extremes along both face diagonals are the corners of the
        // contact area; a same-size face clips to duplicated corners, where
        // per-axis extremes would keep only three.
        let (x, y) = (sides[0], sides[1]);
        let mut keep = [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)]
            .map(|(sx, sy)| {
                (0..polygon.len())
                    .max_by(|&i, &j| {
                        let score = |p: &Vector3<f32>| sx * p[x] + sy * p[y];
                        score(&polygon[i]).total_cmp(&score(&polygon[j]))
                    })
                    .unwrap()
            })
            .to_vec();
        keep.dedup();
        polygon = keep.into_iter().map(|index| polygon[index]).collect();
    }
    polygon
        .into_iter()
        .map(|point| reference.position + reference.rotation * point)
        .collect()
}

/// Contact points for a pair with a mesh collider: the vertices of the
/// polyhedral side that reach deepest along the normal, up to four.
// ponytail: those vertices are not clipped to the other collider, so a big
// box on one small triangle is supported at all four corners. Clip them as
// box pairs do if that shows.
fn mesh_manifold(
    a: &Body,
    b: &Body,
    contact: &Contact,
) -> Option<Vec<Vector3<f32>>> {
    /// Vertices within this distance of the deepest one count as touching.
    const TOLERANCE: f32 = 0.02;
    let is_mesh = |body: &Body| {
        matches!(body.shape, Shape::Hull(_) | Shape::Triangles(_))
    };
    if !is_mesh(a) && !is_mesh(b) {
        return None;
    }
    let normal = Vector3::from(contact.normal);
    // The smaller polyhedral body lies on the other; a triangle mesh has no
    // vertices here, so its partner is used.
    let (vertices, toward) = [(a.vertices(), normal), (b.vertices(), -normal)]
        .into_iter()
        .filter(|(vertices, _)| !vertices.is_empty())
        .min_by(|x, y| x.0.len().cmp(&y.0.len()))?;
    let deepest = vertices
        .iter()
        .map(|vertex| vertex.dot(&toward))
        .fold(f32::NEG_INFINITY, f32::max);
    let touching = vertices
        .into_iter()
        .filter(|vertex| vertex.dot(&toward) >= deepest - TOLERANCE)
        .collect::<Vec<_>>();
    if touching.len() < 2 {
        return None;
    }
    let tangent = normal
        .cross(&Vector3::x())
        .try_normalize(1e-3)
        .unwrap_or_else(|| normal.cross(&Vector3::z()).normalize());
    let bitangent = normal.cross(&tangent);
    let mut keep = [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)]
        .map(|(sx, sy)| {
            (0..touching.len())
                .max_by(|&i, &j| {
                    let score = |p: &Vector3<f32>| {
                        sx * p.dot(&tangent) + sy * p.dot(&bitangent)
                    };
                    score(&touching[i]).total_cmp(&score(&touching[j]))
                })
                .unwrap()
        })
        .to_vec();
    keep.sort_unstable();
    keep.dedup();
    Some(keep.into_iter().map(|index| touching[index]).collect())
}

/// Sutherland-Hodgman: keeps the part of a convex polygon where
/// `direction * point[side] <= limit`.
fn clip(
    polygon: &[Vector3<f32>],
    side: usize,
    direction: f32,
    limit: f32,
) -> Vec<Vector3<f32>> {
    let mut kept = Vec::with_capacity(polygon.len() + 1);
    for (index, &current) in polygon.iter().enumerate() {
        let next = polygon[(index + 1) % polygon.len()];
        let (here, there) = (
            direction * current[side] - limit,
            direction * next[side] - limit,
        );
        if here <= 0.0 {
            kept.push(current);
        }
        if (here <= 0.0) != (there <= 0.0) {
            kept.push(current + (next - current) * (here / (here - there)));
        }
    }
    kept
}

/// Continuous collision for bodies that would move farther than their inner
/// radius this step: a ray from the center along the motion stops the body
/// at the first solid collider and removes its velocity into that surface.
// ponytail: a center ray, not a full shape cast, so a fast body can still
// clip a thin edge it only grazes; the hit is inelastic. Add a swept-shape
// test if gameplay needs bouncing bullets.
fn sweep_fast_bodies(bodies: &mut [Body], dt: f32) {
    for index in 0..bodies.len() {
        let body = &bodies[index];
        let travel = body.velocity.norm() * dt;
        let reach = body.inner_radius();
        if body.inverse_mass == 0.0 || body.sensor || travel <= reach {
            continue;
        }
        let direction = body.velocity / body.velocity.norm();
        let hit = bodies
            .iter()
            .enumerate()
            .filter(|(other, target)| {
                *other != index && !target.sensor && body.interacts_with(target)
            })
            .filter_map(|(_, target)| {
                ray_body(body.position, direction, target)
            })
            .filter(|(distance, _)| *distance < travel + reach)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((distance, normal)) = hit else {
            continue;
        };
        let body = &mut bodies[index];
        body.position += direction * (distance - reach).max(0.0);
        let into = body.velocity.dot(&normal);
        if into < 0.0 {
            body.velocity -= normal * into;
        }
    }
}

/// Pushes overlapping bodies apart after integration. The contact depth is
/// corrected by how far the bodies already moved along the normal.
fn correct_positions(
    bodies: &mut [Body],
    contacts: &[(usize, usize, Contact)],
    dt: f32,
) {
    for (a, b, contact) in contacts {
        let (ima, imb) = (bodies[*a].inverse_mass, bodies[*b].inverse_mass);
        if contact.sensor || ima + imb == 0.0 {
            continue;
        }
        let normal = Vector3::from(contact.normal);
        let separation =
            (bodies[*b].velocity - bodies[*a].velocity).dot(&normal) * dt;
        let depth = contact.depth - separation - PENETRATION_SLOP;
        if depth <= 0.0 {
            continue;
        }
        let push = normal * (depth * POSITION_CORRECTION / (ima + imb));
        bodies[*a].position -= push * ima;
        bodies[*b].position += push * imb;
    }
}

fn write_back(world: &mut World, bodies: &[Body]) {
    for body in bodies
        .iter()
        .filter(|body| body.movable && body.kind != RigidBodyKind::Fixed)
    {
        let position: [f32; 3] = body.position.into();
        let (x, y, z) = body.rotation.euler_angles();
        let mut entity = world.entity_mut(body.entity);
        if let Some(mut transform) = entity.get_mut::<Transform>() {
            // Only touch changed fields so resting bodies do not trigger
            // change detection (and GPU or render re-extraction) every tick.
            if transform.position != position {
                transform.position = position;
            }
            if body.angular_velocity != Vector3::zeros() {
                transform.rotation = [x, y, z];
            }
        }
        if let Some(mut rigid) = entity.get_mut::<RigidBody>() {
            let velocity: [f32; 3] = body.velocity.into();
            if rigid.linear_velocity != velocity {
                rigid.linear_velocity = velocity;
            }
            let spin: [f32; 3] = body.angular_velocity.into();
            if rigid.angular_velocity != spin {
                rigid.angular_velocity = spin;
            }
        }
    }
}

/// Contact from `a` to `b`, or `None` when they do not touch.
fn collide(a: &Body, b: &Body) -> Option<Contact> {
    let sensor = a.sensor || b.sensor;
    let (normal, depth, point) = match (&a.shape, &b.shape) {
        (Shape::Triangles(_), Shape::Triangles(_)) => return None,
        (_, Shape::Triangles(mesh)) => triangles(a, b, mesh)?,
        (Shape::Triangles(mesh), _) => flip(triangles(b, a, mesh)?),
        (Shape::Hull(_), _) | (_, Shape::Hull(_)) => convex(
            |direction| a.support(direction),
            |direction| b.support(direction),
            a.core_radius(),
            b.core_radius(),
            b.position - a.position,
        )?,
        (&Shape::Sphere(ra), &Shape::Sphere(rb)) => {
            spheres(a.position, ra, b.position, rb)?
        }
        (&Shape::Sphere(radius), &Shape::Box(half)) => {
            sphere_box(a.position, radius, b, half)?
        }
        (&Shape::Box(half), &Shape::Sphere(radius)) => {
            flip(sphere_box(b.position, radius, a, half)?)
        }
        (&Shape::Box(ha), &Shape::Box(hb)) => boxes(a, ha, b, hb)?,
        (Shape::Capsule { .. }, _) => {
            let (center, radius) = capsule_proxy(a, b);
            let proxy = Body {
                position: center,
                shape: Shape::Sphere(radius),
                ..a.clone()
            };
            return collide(&proxy, b).map(|contact| Contact {
                a: a.entity,
                sensor,
                ..contact
            });
        }
        (_, Shape::Capsule { .. }) => {
            let contact = collide(b, a)?;
            flip((contact.normal.into(), contact.depth, contact.point.into()))
        }
    };
    Some(Contact {
        a: a.entity,
        b: b.entity,
        normal: normal.into(),
        depth,
        point: point.into(),
        sensor,
    })
}

type Hit = (Vector3<f32>, f32, Vector3<f32>);

fn flip((normal, depth, point): Hit) -> Hit {
    (-normal, depth, point)
}

/// The sphere on the capsule's segment nearest to `other`, which stands in
/// for the capsule in the pair test.
// ponytail: against a box the nearest segment point is refined twice, not
// solved exactly; a box edge crossing a long capsule can report shallow depth.
fn capsule_proxy(capsule: &Body, other: &Body) -> (Vector3<f32>, f32) {
    let (start, end, radius) = capsule.segment();
    let center = match other.shape {
        Shape::Hull(_) | Shape::Triangles(_) => {
            unreachable!("mesh pairs use the convex test")
        }
        Shape::Capsule { .. } => {
            let (other_start, other_end, _) = other.segment();
            segments_closest(start, end, other_start, other_end).0
        }
        Shape::Box(half) => {
            let mut point = closest_on_segment(start, end, other.position);
            for _ in 0..2 {
                point = closest_on_segment(
                    start,
                    end,
                    closest_on_box(point, other, half),
                );
            }
            point
        }
        Shape::Sphere(_) => closest_on_segment(start, end, other.position),
    };
    (center, radius)
}

fn spheres(a: Vector3<f32>, ra: f32, b: Vector3<f32>, rb: f32) -> Option<Hit> {
    let offset = b - a;
    let distance = offset.norm();
    if distance > ra + rb {
        return None;
    }
    let normal = offset.try_normalize(1e-6).unwrap_or_else(Vector3::y);
    Some((normal, ra + rb - distance, a + normal * ra))
}

fn closest_on_box(
    point: Vector3<f32>,
    body: &Body,
    half: Vector3<f32>,
) -> Vector3<f32> {
    let local = body.rotation.inverse() * (point - body.position);
    let clamped = local
        .zip_zip_map(&-half, &half, |value, low, high| value.clamp(low, high));
    body.position + body.rotation * clamped
}

fn sphere_box(
    center: Vector3<f32>,
    radius: f32,
    body: &Body,
    half: Vector3<f32>,
) -> Option<Hit> {
    let local = body.rotation.inverse() * (center - body.position);
    let inside = local.iter().zip(half.iter()).all(|(v, h)| v.abs() <= *h);
    if inside {
        // Push out through the nearest face.
        let (axis, gap) = (0..3)
            .map(|axis| (axis, half[axis] - local[axis].abs()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        let mut face = Vector3::zeros();
        face[axis] = local[axis].signum();
        // Normal from the sphere toward the box.
        let normal = -(body.rotation * face);
        return Some((normal, gap + radius, center));
    }
    let closest = closest_on_box(center, body, half);
    let offset = closest - center;
    let distance = offset.norm();
    if distance > radius {
        return None;
    }
    Some((offset / distance.max(1e-6), radius - distance, closest))
}

fn boxes(
    a: &Body,
    ha: Vector3<f32>,
    b: &Body,
    hb: Vector3<f32>,
) -> Option<Hit> {
    let axes_a = [0, 1, 2].map(|i| a.rotation * Vector3::ith(i, 1.0));
    let axes_b = [0, 1, 2].map(|i| b.rotation * Vector3::ith(i, 1.0));
    fn project(
        axes: &[Vector3<f32>; 3],
        half: Vector3<f32>,
        axis: &Vector3<f32>,
    ) -> f32 {
        (0..3).map(|i| half[i] * axes[i].dot(axis).abs()).sum()
    }
    let offset = b.position - a.position;
    let mut best: Option<(f32, Vector3<f32>)> = None;
    let candidates = axes_a.iter().chain(&axes_b).copied().chain(
        axes_a
            .iter()
            .flat_map(|x| axes_b.iter().map(move |y| x.cross(y))),
    );
    for axis in candidates {
        let Some(axis) = axis.try_normalize(1e-5) else {
            continue; // Parallel edges add nothing.
        };
        let overlap = project(&axes_a, ha, &axis) + project(&axes_b, hb, &axis)
            - offset.dot(&axis).abs();
        if overlap < 0.0 {
            return None;
        }
        // Face axes come first, so an equal edge axis never replaces one.
        if best.is_none_or(|(depth, _)| overlap < depth - 1e-5) {
            let normal = if offset.dot(&axis) < 0.0 { -axis } else { axis };
            best = Some((overlap, normal));
        }
    }
    let (depth, normal) = best?;
    // A representative point; the solver clips a full face manifold.
    let point = closest_on_box(b.position, a, ha)
        .lerp(&closest_on_box(a.position, b, hb), 0.5);
    Some((normal, depth, point))
}

/// `other` against each triangle of a static mesh `body`; the deepest
/// triangle contact wins. The normal points from `other` to the mesh.
// ponytail: every triangle is tested against a bounding sphere; add a BVH
// when meshes get large.
fn triangles(other: &Body, body: &Body, mesh: &MeshData) -> Option<Hit> {
    let reach = other.bounding_radius();
    mesh.triangles
        .iter()
        .map(|triangle| triangle.map(|v| body.position + body.rotation * v))
        .filter(|[a, b, c]| {
            let center = (a + b + c) / 3.0;
            let radius = [a, b, c]
                .iter()
                .map(|v| (*v - center).norm())
                .fold(0.0, f32::max);
            (other.position - center).norm() <= reach + radius
        })
        .filter_map(|triangle| {
            convex(
                |direction| other.support(direction),
                |direction| {
                    triangle
                        .iter()
                        .copied()
                        .max_by(|x, y| {
                            x.dot(&direction).total_cmp(&y.dot(&direction))
                        })
                        .unwrap()
                },
                other.core_radius(),
                0.0,
                triangle[0] - other.position,
            )
        })
        .max_by(|x, y| x.1.total_cmp(&y.1))
}

/// A point of the Minkowski difference `A - B` with the point of `A` it
/// came from (the point of `B` is `from_a - point`).
#[derive(Clone, Copy, Debug)]
struct SupportPoint {
    point: Vector3<f32>,
    from_a: Vector3<f32>,
}

/// Contact between two convex cores given by support functions, each
/// rounded by a radius (a sphere is a rounded point, a capsule a rounded
/// segment). GJK finds the core distance; when the cores overlap, EPA finds
/// the penetration. `guess` is any direction from A toward B.
fn convex(
    support_a: impl Fn(Vector3<f32>) -> Vector3<f32>,
    support_b: impl Fn(Vector3<f32>) -> Vector3<f32>,
    radius_a: f32,
    radius_b: f32,
    guess: Vector3<f32>,
) -> Option<Hit> {
    let support = |direction: Vector3<f32>| {
        let from_a = support_a(direction);
        SupportPoint {
            point: from_a - support_b(-direction),
            from_a,
        }
    };
    let radii = radius_a + radius_b;
    let start = guess.try_normalize(1e-9).unwrap_or_else(Vector3::x);
    let mut simplex = vec![support(-start)];
    let mut closest = simplex[0].point;
    let mut from_a = simplex[0].from_a;
    let mut overlap = false;
    for _ in 0..64 {
        let length = closest.norm_squared();
        if length < 1e-10 {
            overlap = true;
            break;
        }
        let next = support(-closest);
        if length - closest.dot(&next.point) <= 1e-6 * length.max(1e-4) {
            break;
        }
        simplex.push(next);
        let Some(kept) = closest_on_simplex(&simplex) else {
            overlap = true;
            break;
        };
        closest = kept
            .iter()
            .fold(Vector3::zeros(), |sum, (v, w)| sum + v.point * *w);
        from_a = kept
            .iter()
            .fold(Vector3::zeros(), |sum, (v, w)| sum + v.from_a * *w);
        simplex = kept.into_iter().map(|(vertex, _)| vertex).collect();
    }
    if !overlap {
        let distance = closest.norm();
        if distance > radii {
            return None;
        }
        if distance > 1e-4 {
            let normal = -closest / distance;
            let surface_a = from_a + normal * radius_a;
            let surface_b = from_a - closest - normal * radius_b;
            return Some((
                normal,
                radii - distance,
                (surface_a + surface_b) * 0.5,
            ));
        }
    }
    let (normal, distance, from_a) = expand(&support, simplex).unwrap_or((
        start,
        0.0,
        support(start).from_a,
    ));
    let surface_a = from_a + normal * radius_a;
    let surface_b = from_a - normal * (distance + radius_b);
    Some((normal, distance + radii, (surface_a + surface_b) * 0.5))
}

/// The point of a simplex (1 to 4 support points) nearest the origin, as
/// the vertices it lies on with their barycentric weights. `None` when a
/// tetrahedron contains the origin.
fn closest_on_simplex(
    simplex: &[SupportPoint],
) -> Option<Vec<(SupportPoint, f32)>> {
    let weighted = |indices: &[usize], weights: &[f32]| {
        indices
            .iter()
            .zip(weights)
            .filter(|(_, weight)| **weight > 0.0)
            .map(|(&index, &weight)| (simplex[index], weight))
            .collect::<Vec<_>>()
    };
    match simplex.len() {
        1 => Some(vec![(simplex[0], 1.0)]),
        2 => {
            let (a, b) = (simplex[0].point, simplex[1].point);
            let t = (-a.dot(&(b - a)) / (b - a).norm_squared().max(1e-12))
                .clamp(0.0, 1.0);
            Some(weighted(&[0, 1], &[1.0 - t, t]))
        }
        3 => {
            let [a, b, c] = [0, 1, 2].map(|i| simplex[i].point);
            Some(weighted(&[0, 1, 2], &triangle_weights(a, b, c)))
        }
        _ => {
            // Faces the origin sees from outside; a flat tetrahedron counts
            // every face, since none of them bounds a volume.
            let faces =
                [[0, 1, 2, 3], [0, 1, 3, 2], [0, 2, 3, 1], [1, 2, 3, 0]];
            let points = [0, 1, 2, 3].map(|i| simplex[i].point);
            let mut best: Option<(f32, Vec<(SupportPoint, f32)>)> = None;
            for [i, j, k, opposite] in faces {
                let normal =
                    (points[j] - points[i]).cross(&(points[k] - points[i]));
                let origin_side = -points[i].dot(&normal);
                let inside_side = (points[opposite] - points[i]).dot(&normal);
                if origin_side * inside_side > 0.0 && inside_side.abs() > 1e-9 {
                    continue;
                }
                let weights = triangle_weights(points[i], points[j], points[k]);
                let kept = weighted(&[i, j, k], &weights);
                let distance = kept
                    .iter()
                    .fold(Vector3::zeros(), |sum, (v, w)| sum + v.point * *w)
                    .norm_squared();
                if best.as_ref().is_none_or(|(least, _)| distance < *least) {
                    best = Some((distance, kept));
                }
            }
            best.map(|(_, kept)| kept)
        }
    }
}

/// Barycentric weights of the point of triangle `abc` nearest the origin
/// (Ericson, Real-Time Collision Detection 5.1.5).
fn triangle_weights(
    a: Vector3<f32>,
    b: Vector3<f32>,
    c: Vector3<f32>,
) -> [f32; 3] {
    let (ab, ac) = (b - a, c - a);
    let (d1, d2) = (-ab.dot(&a), -ac.dot(&a));
    if d1 <= 0.0 && d2 <= 0.0 {
        return [1.0, 0.0, 0.0];
    }
    let (d3, d4) = (-ab.dot(&b), -ac.dot(&b));
    if d3 >= 0.0 && d4 <= d3 {
        return [0.0, 1.0, 0.0];
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [1.0 - v, v, 0.0];
    }
    let (d5, d6) = (-ab.dot(&c), -ac.dot(&c));
    if d6 >= 0.0 && d5 <= d6 {
        return [0.0, 0.0, 1.0];
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [1.0 - w, 0.0, w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [0.0, 1.0 - w, w];
    }
    let denominator = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denominator, vc * denominator);
    [1.0 - v - w, v, w]
}

/// EPA: grows the simplex of overlapping cores into a polytope until its
/// face nearest the origin lies on the Minkowski difference's surface.
/// Returns that face's outward normal (from A toward B), its distance (the
/// core penetration) and the matching deepest point of A. `None` when the
/// cores are flat against each other and no volume can be built.
fn expand(
    support: &impl Fn(Vector3<f32>) -> SupportPoint,
    mut vertices: Vec<SupportPoint>,
) -> Option<(Vector3<f32>, f32, Vector3<f32>)> {
    // Grow a touching simplex into a tetrahedron around the origin.
    let directions = [
        Vector3::x(),
        Vector3::y(),
        Vector3::z(),
        -Vector3::x(),
        -Vector3::y(),
        -Vector3::z(),
    ];
    while vertices.len() < 4 {
        let points = vertices.iter().map(|v| v.point).collect::<Vec<_>>();
        let candidates: Vec<Vector3<f32>> = match points.len() {
            1 => directions.to_vec(),
            2 => directions
                .iter()
                .map(|axis| (points[1] - points[0]).cross(axis))
                .collect(),
            _ => {
                let normal =
                    (points[1] - points[0]).cross(&(points[2] - points[0]));
                vec![normal, -normal]
            }
        };
        let grows = |candidate: &SupportPoint| match points.len() {
            1 => (candidate.point - points[0]).norm() > 1e-5,
            2 => {
                (points[1] - points[0])
                    .cross(&(candidate.point - points[0]))
                    .norm()
                    > 1e-5
            }
            _ => {
                let normal =
                    (points[1] - points[0]).cross(&(points[2] - points[0]));
                (candidate.point - points[0]).dot(&normal).abs() > 1e-6
            }
        };
        let found = candidates
            .into_iter()
            .filter_map(|direction| direction.try_normalize(1e-9))
            .map(support)
            .find(grows)?;
        vertices.push(found);
    }
    vertices.truncate(4);
    let mut faces: Vec<[usize; 3]> = Vec::new();
    for [i, j, k, opposite] in
        [[0, 1, 2, 3], [0, 1, 3, 2], [0, 2, 3, 1], [1, 2, 3, 0]]
    {
        let [a, b, c, d] = [i, j, k, opposite].map(|n| vertices[n].point);
        faces.push(if (b - a).cross(&(c - a)).dot(&(d - a)) > 0.0 {
            [i, k, j]
        } else {
            [i, j, k]
        });
    }
    let plane = |vertices: &[SupportPoint], [i, j, k]: [usize; 3]| {
        let [a, b, c] = [i, j, k].map(|n| vertices[n].point);
        let normal = (b - a).cross(&(c - a)).try_normalize(1e-12)?;
        Some((normal, normal.dot(&a)))
    };
    for _ in 0..64 {
        let (index, normal, distance) = faces
            .iter()
            .enumerate()
            .filter_map(|(index, face)| {
                plane(&vertices, *face).map(|(n, d)| (index, n, d))
            })
            .min_by(|x, y| x.2.total_cmp(&y.2))?;
        let next = support(normal);
        if next.point.dot(&normal) - distance < 1e-4 {
            let [i, j, k] = faces[index];
            let [a, b, c] = [i, j, k].map(|n| vertices[n].point);
            let weights = triangle_weights(a, b, c);
            let from_a = [i, j, k]
                .iter()
                .zip(weights)
                .fold(Vector3::zeros(), |sum, (n, w)| {
                    sum + vertices[*n].from_a * w
                });
            return Some((normal, distance.max(0.0), from_a));
        }
        // Remove every face the new point sees and stitch the hole's rim
        // to it; rim edges are the ones only one removed face has.
        vertices.push(next);
        let new = vertices.len() - 1;
        let mut rim: Vec<[usize; 2]> = Vec::new();
        faces.retain(|&face| {
            let [a, ..] = face.map(|n| vertices[n].point);
            let visible = plane(&vertices, face)
                .is_some_and(|(n, _)| n.dot(&(next.point - a)) > 0.0);
            if visible {
                for edge in
                    [[face[0], face[1]], [face[1], face[2]], [face[2], face[0]]]
                {
                    if let Some(shared) =
                        rim.iter().position(|e| *e == [edge[1], edge[0]])
                    {
                        rim.swap_remove(shared);
                    } else {
                        rim.push(edge);
                    }
                }
            }
            !visible
        });
        faces.extend(rim.into_iter().map(|[a, b]| [a, b, new]));
    }
    None
}

fn closest_on_segment(
    start: Vector3<f32>,
    end: Vector3<f32>,
    point: Vector3<f32>,
) -> Vector3<f32> {
    let segment = end - start;
    let length = segment.norm_squared();
    if length < 1e-12 {
        return start;
    }
    let t = ((point - start).dot(&segment) / length).clamp(0.0, 1.0);
    start + segment * t
}

/// Closest points between two segments (Ericson, Real-Time Collision
/// Detection 5.1.9).
fn segments_closest(
    p1: Vector3<f32>,
    q1: Vector3<f32>,
    p2: Vector3<f32>,
    q2: Vector3<f32>,
) -> (Vector3<f32>, Vector3<f32>) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.norm_squared(), d2.norm_squared(), d2.dot(&r));
    let (s, t) = if a < 1e-12 && e < 1e-12 {
        (0.0, 0.0)
    } else if a < 1e-12 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(&r);
        if e < 1e-12 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(&d2);
            let denominator = a * e - b * b;
            let mut s = if denominator > 1e-12 {
                ((b * f - c * e) / denominator).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

/// Distance along a unit ray and the surface normal there.
fn ray_body(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    body: &Body,
) -> Option<(f32, Vector3<f32>)> {
    match body.shape {
        Shape::Sphere(radius) => {
            let t = ray_sphere(origin, direction, body.position, radius)?;
            let point = origin + direction * t;
            Some((t, (point - body.position) / radius))
        }
        Shape::Box(half) => {
            let inverse = body.rotation.inverse();
            let local_origin = inverse * (origin - body.position);
            let local_direction = inverse * direction;
            let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
            let mut normal = Vector3::zeros();
            for axis in 0..3 {
                let (o, d) = (local_origin[axis], local_direction[axis]);
                if d.abs() < 1e-8 {
                    if o.abs() > half[axis] {
                        return None;
                    }
                    continue;
                }
                let t1 = (-half[axis] - o) / d;
                let t2 = (half[axis] - o) / d;
                let (low, high) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
                if low > near {
                    near = low;
                    normal = Vector3::zeros();
                    normal[axis] = -d.signum();
                }
                far = far.min(high);
            }
            (near >= 0.0 && near <= far).then(|| (near, body.rotation * normal))
        }
        Shape::Hull(ref mesh) | Shape::Triangles(ref mesh) => {
            let inverse = body.rotation.inverse();
            let local_origin = inverse * (origin - body.position);
            let local_direction = inverse * direction;
            // A hull is solid: only faces seen from outside stop the ray.
            let one_sided = matches!(body.shape, Shape::Hull(_));
            let (t, normal) = mesh
                .triangles
                .iter()
                .filter_map(|triangle| {
                    ray_triangle(local_origin, local_direction, triangle)
                })
                .filter(|(_, normal)| {
                    !one_sided || normal.dot(&local_direction) < 0.0
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))?;
            let normal = if normal.dot(&local_direction) > 0.0 {
                -normal
            } else {
                normal
            };
            Some((t, body.rotation * normal))
        }
        Shape::Capsule { .. } => {
            let (start, end, radius) = body.segment();
            let t = ray_capsule(origin, direction, start, end, radius)?;
            let point = origin + direction * t;
            let axis_point = closest_on_segment(start, end, point);
            Some((t, (point - axis_point) / radius))
        }
    }
}

/// Moller-Trumbore: distance along the ray and the triangle's unit normal
/// (by winding).
fn ray_triangle(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    [a, b, c]: &[Vector3<f32>; 3],
) -> Option<(f32, Vector3<f32>)> {
    let (ab, ac) = (b - a, c - a);
    let p = direction.cross(&ac);
    let determinant = ab.dot(&p);
    if determinant.abs() < 1e-12 {
        return None;
    }
    let offset = origin - a;
    let u = offset.dot(&p) / determinant;
    let q = offset.cross(&ab);
    let v = direction.dot(&q) / determinant;
    let t = ac.dot(&q) / determinant;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t >= 0.0)
        .then(|| (t, ab.cross(&ac).normalize()))
}

fn ray_sphere(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    center: Vector3<f32>,
    radius: f32,
) -> Option<f32> {
    let offset = origin - center;
    let b = offset.dot(&direction);
    let h = b * b - (offset.norm_squared() - radius * radius);
    if h < 0.0 {
        return None;
    }
    let t = -b - h.sqrt();
    (t >= 0.0).then_some(t)
}

/// Ray against a capsule (after Inigo Quilez's `capIntersect`).
fn ray_capsule(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    start: Vector3<f32>,
    end: Vector3<f32>,
    radius: f32,
) -> Option<f32> {
    let (ba, oa) = (end - start, origin - start);
    let (baba, bard, baoa) =
        (ba.norm_squared(), ba.dot(&direction), ba.dot(&oa));
    let (rdoa, oaoa) = (direction.dot(&oa), oa.norm_squared());
    let a = baba - bard * bard;
    let b = baba * rdoa - baoa * bard;
    let c = baba * oaoa - baoa * baoa - radius * radius * baba;
    let h = b * b - a * c;
    if a > 1e-12 && h >= 0.0 {
        let t = (-b - h.sqrt()) / a;
        let y = baoa + t * bard;
        if y > 0.0 && y < baba {
            return (t >= 0.0).then_some(t);
        }
    }
    // Otherwise the ray can only enter through a cap.
    [start, end]
        .into_iter()
        .filter_map(|cap| ray_sphere(origin, direction, cap, radius))
        .min_by(f32::total_cmp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(shape: Shape, position: [f32; 3]) -> Body {
        Body {
            entity: Entity::PLACEHOLDER,
            kind: RigidBodyKind::Dynamic,
            movable: true,
            position: position.into(),
            rotation: Rotation3::identity(),
            shape,
            sensor: false,
            proxy: false,
            layers: CollisionLayers::default(),
            friction: 0.5,
            restitution: 0.0,
            inverse_mass: 1.0,
            inverse_inertia: Vector3::repeat(1.0),
            asleep: false,
            velocity: Vector3::zeros(),
            angular_velocity: Vector3::zeros(),
        }
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
    }

    #[test]
    fn pair_tests_report_normal_from_a_to_b_and_depth() {
        let unit_box = Shape::Box(Vector3::repeat(0.5));
        let cases = [
            (
                Shape::Sphere(1.0),
                [0.0; 3],
                Shape::Sphere(1.0),
                [1.5, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                0.5,
            ),
            (
                Shape::Sphere(0.5),
                [0.0, 0.9, 0.0],
                unit_box.clone(),
                [0.0; 3],
                [0.0, -1.0, 0.0],
                0.1,
            ),
            (
                unit_box.clone(),
                [0.0; 3],
                unit_box.clone(),
                [0.0, 0.0, 0.8],
                [0.0, 0.0, 1.0],
                0.2,
            ),
            (
                Shape::Capsule {
                    half_height: 1.0,
                    radius: 0.5,
                },
                [0.0; 3],
                Shape::Sphere(0.5),
                [0.9, 0.8, 0.0],
                [1.0, 0.0, 0.0],
                0.1,
            ),
            (
                unit_box.clone(),
                [0.0; 3],
                Shape::Capsule {
                    half_height: 1.0,
                    radius: 0.25,
                },
                [0.0, 1.5, 0.0],
                [0.0, 1.0, 0.0],
                0.25,
            ),
        ];
        for (shape_a, at_a, shape_b, at_b, normal, depth) in cases {
            let contact = collide(
                &body(shape_a.clone(), at_a),
                &body(shape_b.clone(), at_b),
            )
            .unwrap_or_else(|| panic!("{shape_a:?} and {shape_b:?} touch"));
            for (got, want) in contact.normal.iter().zip(normal) {
                assert_near(*got, want);
            }
            assert_near(contact.depth, depth);
        }
        assert!(collide(
            &body(unit_box.clone(), [0.0; 3]),
            &body(unit_box.clone(), [1.1, 0.0, 0.0])
        )
        .is_none());
    }

    #[test]
    fn broad_phase_finds_the_same_pairs_as_testing_every_pair() {
        // Deterministic scatter; some bodies overlap, most do not.
        let mut seed = 7_u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 20.0 - 10.0
        };
        let bodies = (0..200)
            .map(|index| {
                let shape = match index % 3 {
                    0 => Shape::Sphere(0.6),
                    1 => Shape::Box(Vector3::new(0.5, 0.7, 0.4)),
                    _ => Shape::Capsule {
                        half_height: 0.5,
                        radius: 0.3,
                    },
                };
                body(shape, [next(), next() * 0.2, next() * 0.2])
            })
            .collect::<Vec<_>>();
        let mut brute = Vec::new();
        for a in 0..bodies.len() {
            for b in a + 1..bodies.len() {
                if narrow_phase(&bodies[a], &bodies[b]).is_some() {
                    brute.push((a, b));
                }
            }
        }
        let swept = find_contacts(&bodies)
            .into_iter()
            .map(|(a, b, _)| (a, b))
            .collect::<Vec<_>>();
        assert!(brute.len() > 10, "scatter too sparse: {}", brute.len());
        assert_eq!(swept, brute);
    }

    #[test]
    fn rotated_boxes_separate_on_an_edge_axis() {
        let unit_box = Shape::Box(Vector3::repeat(0.5));
        let mut turned = body(unit_box.clone(), [1.2, 0.0, 0.0]);
        turned.rotation =
            Rotation3::from_euler_angles(0.0, 0.0, std::f32::consts::FRAC_PI_4);
        // The turned box reaches sqrt(0.5) = 0.707 along X: 0.5 + 0.707 > 1.2.
        let contact =
            collide(&body(unit_box.clone(), [0.0; 3]), &turned).unwrap();
        assert_near(contact.normal[0], 1.0);
        assert_near(contact.depth, 0.5 + 0.5_f32.sqrt() - 1.2);
        turned.position.x = 1.25;
        assert!(collide(&body(unit_box.clone(), [0.0; 3]), &turned).is_none());
    }

    #[test]
    fn rays_hit_each_shape_on_its_surface() {
        let world = PhysicsWorld {
            bodies: vec![
                body(Shape::Sphere(1.0), [0.0, 0.0, -5.0]),
                body(Shape::Box(Vector3::repeat(0.5)), [0.0, 0.0, -2.0]),
                body(
                    Shape::Capsule {
                        half_height: 1.0,
                        radius: 0.5,
                    },
                    [3.0, 0.0, 0.0],
                ),
            ],
            contacts: Vec::new(),
            rest: HashMap::new(),
            warm: HashMap::new(),
            meshes: HashMap::new(),
        };
        let hit = world
            .raycast([0.0; 3], [0.0, 0.0, -1.0], 100.0, u32::MAX)
            .unwrap();
        assert_near(hit.distance, 1.5);
        assert_eq!(hit.normal, [0.0, 0.0, 1.0]);
        let hit = world
            .raycast([3.0, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0, u32::MAX)
            .unwrap();
        assert_near(hit.distance, 3.5);
        assert_near(hit.normal[1], 1.0);
        let hit = world
            .raycast([0.0; 3], [1.0, 0.0, 0.0], 100.0, u32::MAX)
            .unwrap();
        assert_near(hit.distance, 2.5);
        assert!(world
            .raycast([0.0; 3], [1.0, 0.0, 0.0], 2.0, u32::MAX)
            .is_none());
        assert!(world
            .raycast([0.0; 3], [0.0, 1.0, 0.0], 100.0, u32::MAX)
            .is_none());
        assert_eq!(
            world.overlap_sphere([0.0, 0.0, -2.7], 0.3, u32::MAX).len(),
            1
        );
    }

    /// A closed cube mesh of half size `half`, with mixed winding (the
    /// hull must orient its faces itself).
    fn cube_mesh(half: f32) -> MeshAsset {
        let vertices = (0..8)
            .map(|corner| crate::assets::MeshVertex {
                position: [0, 1, 2].map(|axis| {
                    if corner >> axis & 1 == 1 {
                        half
                    } else {
                        -half
                    }
                }),
                ..Default::default()
            })
            .collect();
        let indices = vec![
            0, 1, 3, 0, 3, 2, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4, 2, 3, 7, 2,
            7, 6, 0, 2, 6, 0, 6, 4, 1, 3, 7, 1, 7, 5,
        ];
        MeshAsset { vertices, indices }
    }

    /// A 20 x 20 floor at y = 0 made of two triangles.
    fn floor_mesh() -> MeshAsset {
        let vertices =
            [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]]
                .map(|[x, z]| crate::assets::MeshVertex {
                    position: [x, 0.0, z],
                    ..Default::default()
                })
                .to_vec();
        MeshAsset {
            vertices,
            indices: vec![0, 2, 1, 0, 3, 2],
        }
    }

    #[test]
    fn mesh_colliders_match_primitives_and_report_normals_from_a_to_b() {
        let hull = || {
            Shape::Hull(Arc::new(MeshData::new(
                &cube_mesh(0.5),
                [1.0; 3],
                true,
            )))
        };
        let floor = Shape::Triangles(Arc::new(MeshData::new(
            &floor_mesh(),
            [1.0; 3],
            false,
        )));
        let unit_box = Shape::Box(Vector3::repeat(0.5));
        let capsule = Shape::Capsule {
            half_height: 0.5,
            radius: 0.25,
        };
        // (a, b, normal a to b, depth); each hull case matches its box case.
        let cases = [
            (
                hull(),
                Shape::Sphere(0.5),
                [0.0, 0.9, 0.0],
                [0.0, 1.0, 0.0],
                0.1,
            ),
            (
                unit_box.clone(),
                Shape::Sphere(0.5),
                [0.0, 0.9, 0.0],
                [0.0, 1.0, 0.0],
                0.1,
            ),
            (hull(), hull(), [0.8, 0.0, 0.0], [1.0, 0.0, 0.0], 0.2),
            (
                hull(),
                unit_box.clone(),
                [0.0, 0.0, -0.7],
                [0.0, 0.0, -1.0],
                0.3,
            ),
            (
                hull(),
                Shape::Sphere(0.5),
                [0.0, 0.3, 0.0],
                [0.0, 1.0, 0.0],
                0.7,
            ),
            (
                hull(),
                capsule.clone(),
                [0.0, 1.2, 0.0],
                [0.0, 1.0, 0.0],
                0.05,
            ),
            (
                Shape::Sphere(0.5),
                floor.clone(),
                [0.0, -0.4, 0.0],
                [0.0, -1.0, 0.0],
                0.1,
            ),
            (
                floor.clone(),
                hull(),
                [3.0, 0.45, 2.0],
                [0.0, 1.0, 0.0],
                0.05,
            ),
        ];
        for (a, b, offset, normal, depth) in cases {
            let label = format!("{a:?} vs {b:?}");
            let contact = collide(&body(a, [0.0; 3]), &body(b, offset))
                .unwrap_or_else(|| panic!("{label} misses"));
            assert!(
                (Vector3::from(contact.normal) - Vector3::from(normal)).norm()
                    < 1e-3,
                "{label}: normal {:?}",
                contact.normal
            );
            assert!(
                (contact.depth - depth).abs() < 1e-3,
                "{label}: depth {}",
                contact.depth
            );
        }
        assert!(collide(
            &body(hull(), [0.0; 3]),
            &body(hull(), [1.1, 0.0, 0.0])
        )
        .is_none());
        assert!(collide(
            &body(Shape::Sphere(0.5), [0.0, 0.6, 0.0]),
            &body(floor.clone(), [0.0; 3])
        )
        .is_none());
        assert!(collide(
            &body(floor.clone(), [0.0; 3]),
            &body(floor, [0.0; 3])
        )
        .is_none());
    }

    #[test]
    fn rays_hit_hulls_from_outside_and_triangle_meshes_from_either_side() {
        let world = PhysicsWorld {
            bodies: vec![
                body(
                    Shape::Hull(Arc::new(MeshData::new(
                        &cube_mesh(0.5),
                        [1.0; 3],
                        true,
                    ))),
                    [0.0, 0.0, -3.0],
                ),
                body(
                    Shape::Triangles(Arc::new(MeshData::new(
                        &floor_mesh(),
                        [1.0; 3],
                        false,
                    ))),
                    [0.0, -1.0, 0.0],
                ),
            ],
            ..PhysicsWorld::default()
        };
        let hit = world
            .raycast([0.0; 3], [0.0, 0.0, -1.0], 100.0, u32::MAX)
            .unwrap();
        assert_near(hit.distance, 2.5);
        assert_eq!(hit.normal, [0.0, 0.0, 1.0]);
        // A ray starting inside the hull does not hit it.
        assert!(world
            .raycast([0.0, 0.0, -3.0], [1.0, 0.0, 0.0], 100.0, u32::MAX)
            .is_none());
        for (origin, direction, normal) in [(3.0, -1.0, 1.0), (-3.0, 1.0, -1.0)]
        {
            let hit = world
                .raycast(
                    [1.0, origin, 1.0],
                    [0.0, direction, 0.0],
                    100.0,
                    u32::MAX,
                )
                .unwrap();
            assert_near(hit.distance, (origin + 1.0).abs());
            assert_near(hit.normal[1], normal);
        }
    }

    #[test]
    fn segment_closest_points_handle_crossing_and_parallel_segments() {
        let (a, b) = segments_closest(
            Vector3::new(-1.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 1.0),
            Vector3::new(0.0, 1.0, 1.0),
        );
        assert_eq!((a, b), (Vector3::zeros(), Vector3::new(0.0, 0.0, 1.0)));
        let (a, b) = segments_closest(
            Vector3::zeros(),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 2.0, 0.0),
            Vector3::new(1.0, 2.0, 0.0),
        );
        assert_near((a - b).norm(), 2.0);
    }
}
