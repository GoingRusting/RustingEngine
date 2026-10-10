//! Deterministic XPBD soft bodies from tetrahedral meshes (Macklin, Müller
//! and Chentanez 2016) on the CPU.
//!
//! Each tetrahedron edge is a distance constraint and each tetrahedron a
//! volume constraint, both with XPBD compliance. Steps are split into small
//! substeps with one constraint pass each, so no Lagrange multipliers are
//! kept between passes. Constraints are solved Gauss-Seidel in ascending
//! index order, nothing is accumulated through atomics and no randomness is
//! used: the same mesh and settings give the same bits. It is independent of
//! the ECS; [`SoftBodyVolume`] puts it on an entity and attaches it to rigid
//! bodies and keeps it out of their sphere, box and capsule colliders.
//! Overstretched edges tear by removing the tetrahedra around them.

// The particle loops index several parallel arrays at once.
#![allow(clippy::needless_range_loop)]

use nalgebra::Vector3;

/// Most substeps one step runs, whatever the settings ask.
pub const MAX_SOFT_BODY_SUBSTEPS: u32 = 64;

/// Solver settings. Lengths are meters and times seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftBodySettings {
    pub gravity: [f32; 3],
    /// Substeps per step, up to [`MAX_SOFT_BODY_SUBSTEPS`]. More is stiffer
    /// and slower.
    pub substeps: u32,
    /// Edge compliance (inverse stiffness). 0 keeps edges rigid.
    pub edge_compliance: f32,
    /// Volume compliance. 0 keeps every tetrahedron at its rest volume.
    pub volume_compliance: f32,
    /// Height of a ground plane, if any. Particles below it are lifted onto
    /// it and lose their sideways motion (static friction).
    pub floor: Option<f32>,
    /// Fraction of velocity lost per second, so wobbles die out.
    pub damping: f32,
    /// Strain past which an edge tears: an edge stretched beyond
    /// `rest * (1 + tear_strain)` at the end of a step removes every
    /// tetrahedron that holds it. `None` never tears.
    pub tear_strain: Option<f32>,
}

impl Default for SoftBodySettings {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81, 0.0],
            substeps: 10,
            edge_compliance: 0.0,
            volume_compliance: 0.0,
            floor: Some(0.0),
            damping: 1.0,
            tear_strain: None,
        }
    }
}

/// A rigid body that soft-body particles hang on, for
/// [`SoftBody::step_coupled`]. The step moves and turns it with the pull of
/// its particles; inverse mass and inertia 0 make it immovable (kinematic).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorBody {
    pub position: [f32; 3],
    pub rotation: nalgebra::Rotation3<f32>,
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub inverse_mass: f32,
    /// World-space inverse inertia, held fixed through the step.
    pub inverse_inertia: nalgebra::Matrix3<f32>,
}

/// Holds one particle on a point fixed to an [`AnchorBody`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub particle: usize,
    /// Index into the bodies slice.
    pub body: usize,
    /// The point in the body's frame.
    pub local: [f32; 3],
}

/// A collider shape particles cannot enter, in its body's frame and already
/// scaled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObstacleShape {
    Sphere(f32),
    Box([f32; 3]),
    /// Along the body's Y axis, like capsule colliders.
    Capsule {
        half_height: f32,
        radius: f32,
    },
}

impl ObstacleShape {
    /// The shape of a sphere, box or capsule collider scaled like rigid
    /// physics scales it; `None` for mesh, heightfield and 2D colliders.
    pub fn from_collider(
        shape: super::ColliderShape,
        scale: [f32; 3],
    ) -> Option<Self> {
        let scale = vector(scale).abs();
        match shape {
            super::ColliderShape::Sphere { radius } => {
                Some(Self::Sphere(radius * scale.max()))
            }
            super::ColliderShape::Box { half_extents } => Some(Self::Box(
                vector(half_extents).component_mul(&scale).into(),
            )),
            super::ColliderShape::Capsule {
                half_height,
                radius,
            } => Some(Self::Capsule {
                half_height: half_height * scale.y,
                radius: radius * scale.x.max(scale.z),
            }),
            _ => None,
        }
    }

    /// Radius of a sphere around the body's center that holds the shape.
    pub fn bounding_radius(&self) -> f32 {
        match *self {
            Self::Sphere(radius) => radius,
            Self::Box(half) => vector(half).norm(),
            Self::Capsule {
                half_height,
                radius,
            } => half_height + radius,
        }
    }

    /// For a point inside the shape (body frame), the outward direction to
    /// the nearest surface and how far it is.
    fn push_out(&self, point: Vector3<f32>) -> Option<(Vector3<f32>, f32)> {
        let (from, radius) = match *self {
            Self::Box(half) => {
                let depths = vector(half) - point.abs();
                if depths.min() <= 0.0 {
                    return None;
                }
                let axis = depths.imin();
                let mut normal = Vector3::zeros();
                normal[axis] = if point[axis] < 0.0 { -1.0 } else { 1.0 };
                return Some((normal, depths[axis]));
            }
            Self::Sphere(radius) => (Vector3::zeros(), radius),
            Self::Capsule {
                half_height,
                radius,
            } => (
                Vector3::new(
                    0.0,
                    point.y.clamp(-half_height, half_height),
                    0.0,
                ),
                radius,
            ),
        };
        let offset = point - from;
        let distance = offset.norm();
        if distance >= radius {
            return None;
        }
        // A point right on the core goes up.
        let normal = if distance > 1e-9 {
            offset / distance
        } else {
            Vector3::y()
        };
        Some((normal, radius - distance))
    }
}

/// An [`ObstacleShape`] carried by one of the bodies of
/// [`SoftBody::step_coupled`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obstacle {
    /// Index into the bodies slice.
    pub body: usize,
    pub shape: ObstacleShape,
}

/// A tetrahedral soft body: particles, their tetrahedra and the edges
/// between them, with rest lengths and volumes taken at creation.
#[derive(Clone, Debug, PartialEq)]
pub struct SoftBody {
    pub positions: Vec<[f32; 3]>,
    pub velocities: Vec<[f32; 3]>,
    /// 0 for pinned particles, which never move.
    pub inverse_masses: Vec<f32>,
    pub tetrahedra: Vec<[u32; 4]>,
    pub edges: Vec<[u32; 2]>,
    rest_lengths: Vec<f32>,
    rest_volumes: Vec<f32>,
}

fn vector(value: [f32; 3]) -> Vector3<f32> {
    Vector3::from(value)
}

fn tetrahedron_volume(positions: &[[f32; 3]], tet: [u32; 4]) -> f32 {
    let [a, b, c, d] = tet.map(|index| vector(positions[index as usize]));
    (b - a).cross(&(c - a)).dot(&(d - a)) / 6.0
}

/// The six edges of a tetrahedron, each with its smaller index first.
fn tet_edges(tet: [u32; 4]) -> [[u32; 2]; 6] {
    [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
        .map(|(a, b)| [tet[a].min(tet[b]), tet[a].max(tet[b])])
}

impl SoftBody {
    /// Builds a body from particle positions and tetrahedra. Each
    /// tetrahedron's mass (`density` times its volume) is shared equally by
    /// its four particles. Inside-out tetrahedra are turned the right way.
    /// Refuses indices out of range and flat tetrahedra.
    pub fn new(
        positions: Vec<[f32; 3]>,
        mut tetrahedra: Vec<[u32; 4]>,
        density: f32,
    ) -> Result<Self, String> {
        let mut masses = vec![0.0_f32; positions.len()];
        let mut rest_volumes = Vec::with_capacity(tetrahedra.len());
        let mut edges = Vec::new();
        for (index, tet) in tetrahedra.iter_mut().enumerate() {
            if tet.iter().any(|&p| p as usize >= positions.len()) {
                return Err(format!(
                    "tetrahedron {index} has no such particle"
                ));
            }
            let mut volume = tetrahedron_volume(&positions, *tet);
            if volume < 0.0 {
                tet.swap(2, 3);
                volume = -volume;
            }
            if volume <= 1e-12 {
                return Err(format!("tetrahedron {index} is flat"));
            }
            rest_volumes.push(volume);
            for &particle in tet.iter() {
                masses[particle as usize] += density * volume / 4.0;
            }
            edges.extend(tet_edges(*tet));
        }
        // Shared edges appear once, in a fixed (sorted) order.
        edges.sort_unstable();
        edges.dedup();
        let rest_lengths = edges
            .iter()
            .map(|&[a, b]| {
                (vector(positions[a as usize]) - vector(positions[b as usize]))
                    .norm()
            })
            .collect();
        let inverse_masses = masses
            .iter()
            .map(|&mass| if mass > 0.0 { 1.0 / mass } else { 0.0 })
            .collect();
        Ok(Self {
            velocities: vec![[0.0; 3]; positions.len()],
            positions,
            inverse_masses,
            tetrahedra,
            edges,
            rest_lengths,
            rest_volumes,
        })
    }

    /// A box of `cells` cubes of side `spacing` with its minimum corner at
    /// `min`, each cube cut into six tetrahedra around its main diagonal
    /// (the Kuhn split, so neighbor cubes share faces exactly).
    pub fn block(
        min: [f32; 3],
        cells: [u32; 3],
        spacing: f32,
        density: f32,
    ) -> Result<Self, String> {
        let [nx, ny, nz] = cells.map(|count| count + 1);
        let mut positions = Vec::with_capacity((nx * ny * nz) as usize);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    positions.push([
                        min[0] + x as f32 * spacing,
                        min[1] + y as f32 * spacing,
                        min[2] + z as f32 * spacing,
                    ]);
                }
            }
        }
        let index = |x: u32, y: u32, z: u32| x + nx * (y + ny * z);
        let mut tetrahedra = Vec::new();
        for z in 0..cells[2] {
            for y in 0..cells[1] {
                for x in 0..cells[0] {
                    let corner =
                        |c: [u32; 3]| index(x + c[0], y + c[1], z + c[2]);
                    for order in [
                        [0, 1, 2],
                        [0, 2, 1],
                        [1, 0, 2],
                        [1, 2, 0],
                        [2, 0, 1],
                        [2, 1, 0],
                    ] {
                        let mut step = [0u32; 3];
                        let mut tet = [corner(step); 4];
                        for (slot, axis) in order.into_iter().enumerate() {
                            step[axis] = 1;
                            tet[slot + 1] = corner(step);
                        }
                        tetrahedra.push(tet);
                    }
                }
            }
        }
        Self::new(positions, tetrahedra, density)
    }

    /// Pins a particle where it is: it keeps its place whatever pulls on it.
    pub fn pin(&mut self, particle: usize) {
        self.inverse_masses[particle] = 0.0;
        self.velocities[particle] = [0.0; 3];
    }

    /// Total volume of all tetrahedra.
    pub fn volume(&self) -> f32 {
        self.tetrahedra
            .iter()
            .map(|&tet| tetrahedron_volume(&self.positions, tet))
            .sum()
    }

    /// Total volume at rest.
    pub fn rest_volume(&self) -> f32 {
        self.rest_volumes.iter().sum()
    }

    /// Removes every tetrahedron holding an edge stretched past
    /// `rest * (1 + strain)`, then the edges no tetrahedron holds any more.
    /// Particles keep their mass, so loose ones fly on as free points.
    /// Returns how many tetrahedra were removed.
    pub fn tear(&mut self, strain: f32) -> usize {
        let torn: Vec<[u32; 2]> = self
            .edges
            .iter()
            .zip(&self.rest_lengths)
            .filter(|&(&[a, b], &rest)| {
                let length = (vector(self.positions[a as usize])
                    - vector(self.positions[b as usize]))
                .norm();
                length > rest * (1.0 + strain)
            })
            .map(|(&edge, _)| edge)
            .collect();
        if torn.is_empty() {
            return 0;
        }
        let before = self.tetrahedra.len();
        let mut kept = 0;
        for index in 0..before {
            let tet = self.tetrahedra[index];
            let holds_torn = tet_edges(tet)
                .iter()
                .any(|edge| torn.binary_search(edge).is_ok());
            if !holds_torn {
                self.tetrahedra[kept] = tet;
                self.rest_volumes[kept] = self.rest_volumes[index];
                kept += 1;
            }
        }
        self.tetrahedra.truncate(kept);
        self.rest_volumes.truncate(kept);
        let mut held: Vec<[u32; 2]> = self
            .tetrahedra
            .iter()
            .flat_map(|&tet| tet_edges(tet))
            .collect();
        held.sort_unstable();
        held.dedup();
        let mut kept = 0;
        for index in 0..self.edges.len() {
            if held.binary_search(&self.edges[index]).is_ok() {
                self.edges[kept] = self.edges[index];
                self.rest_lengths[kept] = self.rest_lengths[index];
                kept += 1;
            }
        }
        self.edges.truncate(kept);
        self.rest_lengths.truncate(kept);
        before - self.tetrahedra.len()
    }

    /// Advances the body by `dt` seconds.
    pub fn step(&mut self, settings: &SoftBodySettings, dt: f32) {
        self.step_coupled(settings, dt, &mut [], &[], &[]);
    }

    /// Advances the body by `dt` seconds with each anchor's particle held on
    /// its point of a rigid body. Each substep solves the anchors as
    /// zero-length XPBD constraints between particle and body (Müller et
    /// al. 2020, "Detailed rigid body simulation with extended position
    /// based dynamics"), so the body is pulled and turned by the soft body
    /// as well as carrying it. Bodies move with their velocities through
    /// the step and come back with the pose and velocities they end it
    /// with. Anchors on pinned particles or missing bodies hold nothing.
    /// Particles inside an obstacle's shape are pushed out to its surface
    /// along the nearest way out, and its body is pushed back the same way
    /// (no friction).
    /// With [`SoftBodySettings::tear_strain`] set, the step ends with
    /// [`SoftBody::tear`].
    pub fn step_coupled(
        &mut self,
        settings: &SoftBodySettings,
        dt: f32,
        bodies: &mut [AnchorBody],
        anchors: &[Anchor],
        obstacles: &[Obstacle],
    ) {
        let substeps = settings.substeps.clamp(1, MAX_SOFT_BODY_SUBSTEPS);
        let h = dt / substeps as f32;
        if h <= 0.0 {
            return;
        }
        let gravity = vector(settings.gravity);
        let keep = (1.0 - settings.damping * h).clamp(0.0, 1.0);
        let mut previous = self.positions.clone();
        for _ in 0..substeps {
            for i in 0..self.positions.len() {
                previous[i] = self.positions[i];
                if self.inverse_masses[i] == 0.0 {
                    continue;
                }
                let velocity = vector(self.velocities[i]) * keep + gravity * h;
                self.velocities[i] = velocity.into();
                self.positions[i] =
                    (vector(self.positions[i]) + velocity * h).into();
            }
            for body in bodies.iter_mut() {
                body.position =
                    (vector(body.position) + vector(body.velocity) * h).into();
                body.rotation =
                    nalgebra::Rotation3::new(vector(body.angular_velocity) * h)
                        * body.rotation;
            }
            self.solve_edges(settings.edge_compliance / (h * h));
            self.solve_volumes(settings.volume_compliance / (h * h));
            for anchor in anchors {
                let weight = self.inverse_masses[anchor.particle];
                let Some(body) = bodies.get_mut(anchor.body) else {
                    continue;
                };
                if weight == 0.0 {
                    continue;
                }
                let arm = body.rotation * vector(anchor.local);
                let gap = vector(body.position) + arm
                    - vector(self.positions[anchor.particle]);
                self.close_gap(anchor.particle, body, arm, gap, h);
            }
            for obstacle in obstacles {
                let Some(body) = bodies.get_mut(obstacle.body) else {
                    continue;
                };
                for i in 0..self.positions.len() {
                    if self.inverse_masses[i] == 0.0 {
                        continue;
                    }
                    let arm = vector(self.positions[i]) - vector(body.position);
                    let local = body.rotation.inverse() * arm;
                    if let Some((normal, depth)) =
                        obstacle.shape.push_out(local)
                    {
                        let gap = body.rotation * normal * depth;
                        self.close_gap(i, body, arm, gap, h);
                    }
                }
            }
            for i in 0..self.positions.len() {
                if self.inverse_masses[i] == 0.0 {
                    continue;
                }
                if let Some(floor) = settings.floor {
                    if self.positions[i][1] < floor {
                        self.positions[i] =
                            [previous[i][0], floor, previous[i][2]];
                    }
                }
                self.velocities[i] =
                    ((vector(self.positions[i]) - vector(previous[i])) / h)
                        .into();
            }
        }
        if let Some(strain) = settings.tear_strain {
            self.tear(strain);
        }
    }

    /// Moves `particle` by `gap` (to where it should be), shared with
    /// `body` as one XPBD position constraint: the body is pushed and turned
    /// the other way at `arm` from its center, as much as its inverse mass
    /// and inertia allow.
    fn close_gap(
        &mut self,
        particle: usize,
        body: &mut AnchorBody,
        arm: Vector3<f32>,
        gap: Vector3<f32>,
        h: f32,
    ) {
        let weight = self.inverse_masses[particle];
        let length = gap.norm();
        if length < 1e-9 || weight == 0.0 {
            return;
        }
        let normal = gap / length;
        let turn = arm.cross(&normal);
        let body_weight =
            body.inverse_mass + turn.dot(&(body.inverse_inertia * turn));
        let lambda = length / (weight + body_weight);
        self.positions[particle] = (vector(self.positions[particle])
            + normal * (weight * lambda))
            .into();
        let push = -normal * lambda;
        body.position =
            (vector(body.position) + push * body.inverse_mass).into();
        body.velocity =
            (vector(body.velocity) + push * (body.inverse_mass / h)).into();
        let spin = body.inverse_inertia * arm.cross(&push);
        body.rotation = nalgebra::Rotation3::new(spin) * body.rotation;
        body.angular_velocity =
            (vector(body.angular_velocity) + spin / h).into();
    }

    fn solve_edges(&mut self, alpha: f32) {
        for (edge, &rest) in self.edges.iter().zip(&self.rest_lengths) {
            let [a, b] = edge.map(|index| index as usize);
            let weight = self.inverse_masses[a] + self.inverse_masses[b];
            let delta = vector(self.positions[a]) - vector(self.positions[b]);
            let length = delta.norm();
            if weight + alpha == 0.0 || length < 1e-9 {
                continue;
            }
            let normal = delta / length;
            let lambda = -(length - rest) / (weight + alpha);
            self.positions[a] = (vector(self.positions[a])
                + normal * (lambda * self.inverse_masses[a]))
                .into();
            self.positions[b] = (vector(self.positions[b])
                - normal * (lambda * self.inverse_masses[b]))
                .into();
        }
    }

    fn solve_volumes(&mut self, alpha: f32) {
        // For each corner, the two other corners whose cross product (from
        // a third) gives the outward volume gradient of that corner.
        const OPPOSITE: [[usize; 3]; 4] =
            [[1, 3, 2], [0, 2, 3], [0, 3, 1], [0, 1, 2]];
        for (tet, &rest) in self.tetrahedra.iter().zip(&self.rest_volumes) {
            let corners = tet.map(|index| index as usize);
            let points = corners.map(|index| vector(self.positions[index]));
            let mut gradients = [Vector3::zeros(); 4];
            let mut weight = 0.0;
            for (j, [p, q, r]) in OPPOSITE.into_iter().enumerate() {
                gradients[j] = (points[q] - points[p])
                    .cross(&(points[r] - points[p]))
                    / 6.0;
                weight += self.inverse_masses[corners[j]]
                    * gradients[j].norm_squared();
            }
            if weight + alpha == 0.0 {
                continue;
            }
            let volume = (points[1] - points[0])
                .cross(&(points[2] - points[0]))
                .dot(&(points[3] - points[0]))
                / 6.0;
            let lambda = -(volume - rest) / (weight + alpha);
            for j in 0..4 {
                let index = corners[j];
                self.positions[index] = (vector(self.positions[index])
                    + gradients[j] * (lambda * self.inverse_masses[index]))
                    .into();
            }
        }
    }

    /// FNV-1a over every position and velocity bit, for determinism checks.
    pub fn state_hash(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for value in self.positions.iter().chain(&self.velocities).flatten() {
            hash = (hash ^ u64::from(value.to_bits()))
                .wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
}

/// Holds a particle of a [`SoftBodyVolume`] on a point fixed to a rigid
/// body. The body carries the particle along, and a dynamic body feels the
/// soft body pull back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoftAttachment {
    pub particle: usize,
    pub body: bevy_ecs::prelude::Entity,
    /// The point in the body's frame, in meters (scale is not applied).
    pub local: [f32; 3],
}

/// A soft body on an entity, stepped once per fixed tick after rigid
/// physics. It is runtime state made by game code: not reflected or saved
/// in scene files yet, but kept in snapshots.
#[derive(bevy_ecs::prelude::Component, Clone, Debug)]
pub struct SoftBodyVolume {
    pub settings: SoftBodySettings,
    pub body: SoftBody,
    pub attachments: Vec<SoftAttachment>,
}

impl SoftBodyVolume {
    pub fn new(settings: SoftBodySettings, body: SoftBody) -> Self {
        Self {
            settings,
            body,
            attachments: Vec::new(),
        }
    }

    /// Attaches every particle within `radius` of `point` (world space) to
    /// `body`, whose transform is `transform`, where the particles are now.
    /// Returns how many were attached.
    pub fn attach_near(
        &mut self,
        point: [f32; 3],
        radius: f32,
        body: bevy_ecs::prelude::Entity,
        transform: &crate::Transform,
    ) -> usize {
        let rotation = body_rotation(transform);
        let origin = vector(transform.position);
        let before = self.attachments.len();
        for (particle, &position) in self.body.positions.iter().enumerate() {
            if (vector(position) - vector(point)).norm() <= radius {
                self.attachments.push(SoftAttachment {
                    particle,
                    body,
                    local: (rotation.inverse() * (vector(position) - origin))
                        .into(),
                });
            }
        }
        self.attachments.len() - before
    }
}

fn body_rotation(transform: &crate::Transform) -> nalgebra::Rotation3<f32> {
    let [roll, pitch, yaw] = transform.rotation;
    super::sim_math::rotation_from_euler(roll, pitch, yaw)
}

/// Per fixed step, after rigid physics: steps every [`SoftBodyVolume`] in
/// spawn order with its attached particles held on their bodies and its
/// particles kept out of nearby sphere, box and capsule colliders, then
/// writes back the pose and velocities of the dynamic bodies it moved.
/// Attachments to a missing or parented body hold nothing; a volume does
/// not collide with the bodies it is attached to.
// ponytail: every collider is checked against every volume each tick (a
// bounding-sphere test); use the broad phase if scenes get many of both.
#[allow(clippy::type_complexity)]
pub(super) fn step_soft_bodies(
    time: bevy_ecs::prelude::Res<super::FrameTime>,
    mut volumes: bevy_ecs::prelude::Query<(
        bevy_ecs::prelude::Entity,
        Option<&super::SpawnOrder>,
        &mut SoftBodyVolume,
    )>,
    mut bodies: bevy_ecs::prelude::Query<
        (
            bevy_ecs::prelude::Entity,
            Option<&super::SpawnOrder>,
            &mut crate::Transform,
            Option<&mut super::RigidBody>,
            Option<&super::Collider>,
            bevy_ecs::prelude::Has<super::PhysicsBody>,
        ),
        bevy_ecs::prelude::Without<super::Parent>,
    >,
) {
    let dt = time.fixed_delta.as_secs_f32();
    let mut volumes: Vec<_> = volumes.iter_mut().collect();
    volumes.sort_by_key(|(entity, order, _)| {
        super::fluid::visit_key(*order, *entity)
    });
    let mut colliders: Vec<_> = bodies
        .iter()
        .filter_map(|(entity, order, transform, _, collider, physics)| {
            let shape = ObstacleShape::from_collider(
                collider.filter(|_| physics)?.shape,
                transform.scale,
            )?;
            Some((
                super::fluid::visit_key(order, entity),
                entity,
                transform.position,
                shape,
            ))
        })
        .collect();
    colliders.sort_by_key(|&(key, ..)| key);
    for (_, _, mut volume) in volumes {
        let volume = &mut *volume;
        let mut entities = Vec::new();
        let mut held = Vec::new();
        let mut slot = |entity| {
            if let Some(index) = entities.iter().position(|&e| e == entity) {
                return Some(index);
            }
            let (_, _, transform, rigid, collider, _) =
                bodies.get(entity).ok()?;
            entities.push(entity);
            held.push(anchor_body(transform, rigid, collider, dt));
            Some(held.len() - 1)
        };
        let mut anchors = Vec::new();
        for attachment in &volume.attachments {
            if attachment.particle >= volume.body.positions.len() {
                continue;
            }
            if let Some(body) = slot(attachment.body) {
                anchors.push(Anchor {
                    particle: attachment.particle,
                    body,
                    local: attachment.local,
                });
            }
        }
        let (low, high) = bounds(&volume.body.positions);
        let center = (low + high) / 2.0;
        let reach = (high - low).norm() / 2.0;
        let mut obstacles = Vec::new();
        for &(_, entity, position, shape) in &colliders {
            if volume.attachments.iter().any(|a| a.body == entity) {
                continue;
            }
            // Room for a tick of travel at up to 60 m/s either way.
            let margin = 2.0 * 60.0 * dt;
            if (vector(position) - center).norm()
                > reach + shape.bounding_radius() + margin
            {
                continue;
            }
            if let Some(body) = slot(entity) {
                obstacles.push(Obstacle { body, shape });
            }
        }
        volume.body.step_coupled(
            &volume.settings,
            dt,
            &mut held,
            &anchors,
            &obstacles,
        );
        for (entity, state) in entities.into_iter().zip(held) {
            if state.inverse_mass == 0.0 {
                continue;
            }
            let Ok((_, _, mut transform, Some(mut rigid), ..)) =
                bodies.get_mut(entity)
            else {
                continue;
            };
            let (roll, pitch, yaw) = state.rotation.euler_angles();
            transform.position = state.position;
            transform.rotation = [roll, pitch, yaw];
            rigid.linear_velocity = state.velocity;
            rigid.angular_velocity = state.angular_velocity;
        }
    }
}

/// The smallest box around some points.
fn bounds(points: &[[f32; 3]]) -> (Vector3<f32>, Vector3<f32>) {
    let mut low = Vector3::repeat(f32::MAX);
    let mut high = Vector3::repeat(f32::MIN);
    for &point in points {
        low = low.inf(&vector(point));
        high = high.sup(&vector(point));
    }
    (low, high)
}

/// The start-of-tick state of a body for [`SoftBody::step_coupled`]. Only a
/// dynamic body with mass can be moved by the soft body.
fn anchor_body(
    transform: &crate::Transform,
    rigid: Option<&super::RigidBody>,
    collider: Option<&super::Collider>,
    dt: f32,
) -> AnchorBody {
    let rotation = body_rotation(transform);
    let (velocity, spin) =
        rigid.map_or((Vector3::zeros(), Vector3::zeros()), |rigid| {
            (
                vector(rigid.linear_velocity),
                vector(rigid.angular_velocity),
            )
        });
    let dynamic = rigid.is_some_and(|rigid| {
        rigid.kind == super::RigidBodyKind::Dynamic && rigid.mass > 0.0
    });
    let (inverse_mass, inverse_inertia) = match (rigid, collider) {
        (Some(rigid), Some(collider)) if dynamic => (
            1.0 / rigid.mass,
            super::cpu_physics::world_inverse_inertia(
                collider,
                transform.scale,
                &rotation,
                rigid.mass,
            ),
        ),
        (Some(rigid), None) if dynamic => {
            (1.0 / rigid.mass, nalgebra::Matrix3::zeros())
        }
        _ => (0.0, nalgebra::Matrix3::zeros()),
    };
    // Rigid physics already moved the body through this tick; the soft step
    // moves it through the tick again from where it was.
    AnchorBody {
        position: (vector(transform.position) - velocity * dt).into(),
        rotation: nalgebra::Rotation3::new(-spin * dt) * rotation,
        velocity: velocity.into(),
        angular_velocity: spin.into(),
        inverse_mass,
        inverse_inertia,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn run(body: &mut SoftBody, settings: &SoftBodySettings, steps: u32) {
        for _ in 0..steps {
            body.step(settings, DT);
        }
    }

    #[test]
    fn a_block_is_six_tetrahedra_per_cube_and_fills_its_box() {
        let body = SoftBody::block([0.0; 3], [2, 3, 4], 0.5, 1000.0).unwrap();
        assert_eq!(body.positions.len(), 3 * 4 * 5);
        assert_eq!(body.tetrahedra.len(), 6 * 24);
        let expected = 1.0 * 1.5 * 2.0;
        assert!((body.volume() - expected).abs() < 1e-4, "{}", body.volume());
        let mass: f32 = body.inverse_masses.iter().map(|w| 1.0 / w).sum();
        assert!((mass - expected * 1000.0).abs() < 0.5, "{mass}");
        assert!(
            SoftBody::new(vec![[0.0; 3]; 4], vec![[0, 1, 2, 3]], 1.0).is_err()
        );
        assert!(
            SoftBody::new(vec![[0.0; 3]; 3], vec![[0, 1, 2, 3]], 1.0).is_err()
        );
    }

    #[test]
    fn a_dropped_jelly_lands_on_the_floor_and_keeps_its_volume() {
        let settings = SoftBodySettings {
            edge_compliance: 1e-4,
            ..SoftBodySettings::default()
        };
        let mut body =
            SoftBody::block([-0.5, 1.0, -0.5], [4, 4, 4], 0.25, 1000.0)
                .unwrap();
        let mut lowest_volume = f32::MAX;
        for _ in 0..300 {
            body.step(&settings, DT);
            lowest_volume = lowest_volume.min(body.volume());
        }
        let bottom =
            body.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let top = body.positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
        assert!((0.0..0.01).contains(&bottom), "bottom {bottom}");
        assert!(top > 0.8, "collapsed to {top}");
        assert!(
            lowest_volume > 0.95 * body.rest_volume(),
            "volume fell to {lowest_volume}"
        );
        let speed = body
            .velocities
            .iter()
            .map(|v| vector(*v).norm())
            .fold(0.0, f32::max);
        assert!(speed < 0.2, "still moving at {speed}");
    }

    #[test]
    fn a_pinned_beam_holds_its_end_and_softer_beams_sag_more() {
        let sag = |edge_compliance: f32| {
            let settings = SoftBodySettings {
                edge_compliance,
                volume_compliance: edge_compliance,
                floor: None,
                ..SoftBodySettings::default()
            };
            let mut beam =
                SoftBody::block([0.0, 1.0, 0.0], [8, 1, 1], 0.1, 500.0)
                    .unwrap();
            let pinned: Vec<usize> = (0..beam.positions.len())
                .filter(|&i| beam.positions[i][0] == 0.0)
                .collect();
            for &i in &pinned {
                beam.pin(i);
            }
            let start = beam.positions.clone();
            run(&mut beam, &settings, 240);
            for &i in &pinned {
                assert_eq!(beam.positions[i], start[i]);
            }
            beam.positions
                .iter()
                .map(|p| 1.0 - p[1])
                .fold(0.0, f32::max)
        };
        let stiff = sag(1e-7);
        let soft = sag(1e-4);
        assert!(stiff < 0.1, "a stiff beam sagged {stiff}");
        assert!(soft > stiff * 4.0, "soft {soft} against stiff {stiff}");
    }

    #[test]
    fn the_same_start_gives_the_same_bits() {
        let settings = SoftBodySettings {
            edge_compliance: 1e-4,
            volume_compliance: 1e-5,
            ..SoftBodySettings::default()
        };
        let make = || {
            let mut body =
                SoftBody::block([0.0, 0.5, 0.0], [3, 2, 3], 0.2, 800.0)
                    .unwrap();
            body.velocities[5] = [1.0, 2.0, -0.5];
            body
        };
        let (mut a, mut b) = (make(), make());
        run(&mut a, &settings, 60);
        run(&mut b, &settings, 60);
        assert_eq!(a.state_hash(), b.state_hash());
        assert!(a.positions.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn an_overstretched_column_tears_and_drops_its_lower_half() {
        let pulled = |tear_strain: Option<f32>| {
            let settings = SoftBodySettings {
                edge_compliance: 1e-3,
                floor: None,
                tear_strain,
                ..SoftBodySettings::default()
            };
            let mut column =
                SoftBody::block([0.0, 0.0, 0.0], [1, 2, 1], 0.1, 500.0)
                    .unwrap();
            for i in 0..column.positions.len() {
                if column.positions[i][1] > 0.15 {
                    column.pin(i);
                } else if column.positions[i][1] < 0.05 {
                    column.velocities[i] = [0.0, -20.0, 0.0];
                }
            }
            let tetrahedra = column.tetrahedra.len();
            run(&mut column, &settings, 60);
            let lowest = column
                .positions
                .iter()
                .map(|p| p[1])
                .fold(f32::MAX, f32::min);
            (tetrahedra - column.tetrahedra.len(), lowest)
        };
        let (kept, held_at) = pulled(None);
        assert_eq!(kept, 0);
        assert!(held_at > -0.5, "the untearable column fell to {held_at}");
        let (removed, fell_to) = pulled(Some(0.5));
        assert!(removed >= 6, "only {removed} tetrahedra tore");
        assert!(fell_to < -2.0, "the torn half only fell to {fell_to}");
    }

    #[test]
    fn tearing_keeps_only_edges_of_remaining_tetrahedra() {
        let mut body =
            SoftBody::block([0.0, 0.0, 0.0], [2, 1, 1], 1.0, 1.0).unwrap();
        body.positions[0][0] -= 5.0;
        let removed = body.tear(0.5);
        assert!(removed > 0 && removed < 12, "removed {removed}");
        assert_eq!(body.tetrahedra.len() + removed, 12);
        assert!(body.tetrahedra.iter().all(|t| !t.contains(&0)));
        for edge in &body.edges {
            assert!(body
                .tetrahedra
                .iter()
                .any(|&t| tet_edges(t).contains(edge)));
        }
        assert_eq!(body.edges.len(), body.rest_lengths.len());
        assert_eq!(body.tear(0.5), 0);
    }

    fn still_body(position: [f32; 3], inverse_mass: f32) -> AnchorBody {
        AnchorBody {
            position,
            rotation: nalgebra::Rotation3::identity(),
            velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
            inverse_mass,
            inverse_inertia: nalgebra::Matrix3::identity() * inverse_mass,
        }
    }

    #[test]
    fn a_jelly_dropped_on_a_fixed_box_rests_on_its_top() {
        let settings = SoftBodySettings {
            edge_compliance: 1e-4,
            floor: None,
            ..SoftBodySettings::default()
        };
        let mut body =
            SoftBody::block([-0.2, 1.0, -0.2], [2, 2, 2], 0.2, 1000.0).unwrap();
        let mut bodies = [still_body([0.0, 0.0, 0.0], 0.0)];
        let obstacles = [Obstacle {
            body: 0,
            shape: ObstacleShape::Box([1.0, 0.5, 1.0]),
        }];
        for _ in 0..180 {
            body.step_coupled(&settings, DT, &mut bodies, &[], &obstacles);
        }
        let bottom =
            body.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!((0.49..0.52).contains(&bottom), "bottom {bottom}");
        assert_eq!(bodies[0], still_body([0.0, 0.0, 0.0], 0.0));
    }

    #[test]
    fn a_ball_thrown_into_a_floating_jelly_shares_its_momentum() {
        let settings = SoftBodySettings {
            gravity: [0.0; 3],
            edge_compliance: 1e-4,
            floor: None,
            damping: 0.0,
            ..SoftBodySettings::default()
        };
        let mut jelly =
            SoftBody::block([0.0, -0.2, -0.2], [2, 2, 2], 0.2, 250.0).unwrap();
        let jelly_mass: f32 =
            jelly.inverse_masses.iter().map(|w| 1.0 / w).sum();
        let ball_mass = 2.0;
        let mut bodies = [still_body([-0.5, 0.0, 0.0], 1.0 / ball_mass)];
        bodies[0].inverse_inertia =
            nalgebra::Matrix3::identity() / (0.4 * ball_mass * 0.15 * 0.15);
        bodies[0].velocity = [3.0, 0.0, 0.0];
        let obstacles = [Obstacle {
            body: 0,
            shape: ObstacleShape::Sphere(0.15),
        }];
        for _ in 0..60 {
            jelly.step_coupled(&settings, DT, &mut bodies, &[], &obstacles);
        }
        let jelly_momentum: f32 = jelly
            .velocities
            .iter()
            .zip(&jelly.inverse_masses)
            .map(|(v, w)| v[0] / w)
            .sum();
        let total = jelly_momentum + bodies[0].velocity[0] * ball_mass;
        assert!(
            (total - 6.0).abs() < 0.3,
            "momentum {total} (jelly {jelly_momentum} of {jelly_mass} kg)"
        );
        assert!(bodies[0].velocity[0] < 2.0, "ball kept {:?}", bodies[0]);
        assert!(jelly_momentum > 1.0, "jelly got {jelly_momentum}");
        let deepest = jelly
            .positions
            .iter()
            .map(|&p| (vector(p) - vector(bodies[0].position)).norm())
            .fold(f32::MAX, f32::min);
        assert!(deepest > 0.14, "a particle sits {deepest} inside the ball");
    }
}
