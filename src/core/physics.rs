//! Physics describes how objects will interact with the physics world and other physic objects.
//!
//! Physics structure is always same for every object, but different shaders use physic in different ways.
//! To know more about 'how every shader use some physic param' you can look at compute shaders for more detailed describe.
//! [`ComputeShaderType`]
//!
//! # Complete physics example
//!
//! ```
//! use rusting_engine::{Physics, CollisionType, ComputeShaderType};
//!
//! // Bouncy basketball
//! let ball = Physics::default()
//!     .compute_shader(ComputeShaderType::FullPhysics)
//!     .collision_type(CollisionType::Sphere)
//!     .mass(0.6)      // 600g basketball
//!     .bounciness(0.8) // Very bouncy
//!     .friction(0.4)   // Some grip
//!     .gravity_scale(1.0);
//!
//! // Heavy sliding block (ice-like)
//! let slider = Physics::default()
//!     .mass(50.0)      // 50kg
//!     .friction(0.05)  // Very slippery
//!     .bounciness(0.1); // Almost no bounce
//! ```

use crate::core::collisions::CollisionType;
use crate::core::ComputeShaderType;

/// Core physic definition that describe all possible cases in physic world.
///
/// Physic can be very lightweight and very heavy in same time, its based on
/// which physic shader you use [`ComputeShaderType`]
/// and what is your physic tickrate for example Minecraft has 20 tickrate
/// this engine is using 60 tickrate.
///
/// # Example
/// ```
/// use rusting_engine::{ CollisionType, ComputeShaderType, Physics };
///
/// // Create floating cube that use maximal realistic shader
/// let floating_physic = &Physics::default()
///     .compute_shader(ComputeShaderType::FullPhysics) // FullPhysics has all possible physics cases as rotation, gravity, collisions, impulse and etc.
///     .mass(1.0) // equal to 1kg. More mass = more impulse when pushing other objects and etc.
///     .collision_type(CollisionType::Box) // Box collision
///     .gravity_scale(0.0); // Floating cube that is not pushed by gravity
///
/// // Create empty physic, no collision, no gravity, nothing, can be used to turn off physic or for debug
/// let empty_physic = &Physics::default()
///     .compute_shader(ComputeShaderType::Empty);
/// ```
#[derive(Clone, Debug, Copy)]
pub struct Physics {
    pub compute_shader: ComputeShaderType,
    pub collision_type: CollisionType,
    pub mass: f32,          // mass, 1.0 = 1kg
    pub bounciness: f32,    // 0.0 - rock, 1.0 - super-ball
    pub friction: f32,      // 0.0 - ice, 1.0 - gummy
    pub gravity_scale: f32, // 1.0 = normal gravity, -1.0 = antigravity
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
}

impl Default for Physics {
    fn default() -> Self {
        Self {
            compute_shader: ComputeShaderType::FullPhysics,
            collision_type: CollisionType::Box,
            mass: 1.0,
            bounciness: 0.3,
            friction: 0.5,
            gravity_scale: 1.0,
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        }
    }
}

impl Physics {
    /// Bounciness will describe how much speed object loses when bouncing.
    ///
    /// * `0.0` - No bounce (like a rock)
    /// * `0.5` - Medium bounce (like a basketball)
    /// * `1.0` - Perfect bounce (never stops)
    ///
    /// **Note:** Values outside 0.0-1.0 are clamped automatically.
    #[must_use]
    pub fn bounciness(mut self, val: f32) -> Self {
        self.bounciness = val.clamp(0.0, 1.0);
        self
    }

    /// Friction is used to describe how good can object slide on other object
    /// Range ( 0.0 - 1.0 )
    #[must_use]
    pub fn friction(mut self, val: f32) -> Self {
        self.friction = val;
        self
    }

    /// Gravity scale describe how strong does object effected by gravity, if 1, the gravity power will be same as World Gravity.
    ///
    /// Gravity = 0 => floating object
    ///
    /// Gravity < 0 => object will be pushed in opposite direction
    #[must_use]
    pub fn gravity_scale(mut self, val: f32) -> Self {
        self.gravity_scale = val;
        self
    }

    /// The speed of object rotating in **radians per second**.
    ///
    /// # Example
    /// ```
    /// use rusting_engine::Physics;
    ///
    /// // Rotate 180 degrees per second (π radians/sec)
    /// let spinning = Physics::default()
    ///     .angular_velocity([3.14159, 0.0, 0.0]);
    /// ```
    #[must_use]
    pub fn angular_velocity(mut self, val: [f32; 3]) -> Self {
        self.angular_velocity = val;
        self
    }

    /// The speed of object displace in some amount of time
    #[must_use]
    pub fn linear_velocity(mut self, v: [f32; 3]) -> Self {
        self.linear_velocity = v;
        self
    }

    /// With bigger mass, object can create bigger push impulse, object is more hard to displace.
    ///
    /// So mass is treated as real world mass.
    #[must_use]
    pub fn mass(mut self, m: f32) -> Self {
        self.mass = m;
        self
    }

    /// This will create invisible collision around object based on chosen shape.
    ///
    /// This Function is using [`CollisionType`]
    ///
    /// # Example
    /// ```
    /// use rusting_engine::{CollisionType, Physics};
    ///
    ///  // Invisible Box collision
    ///  let box_collision_physic = &Physics::default()
    ///     .collision_type(CollisionType::Box);
    ///
    /// // Invisible Sphere collision
    /// let sphere_collision_physic = &Physics::default()
    ///     .collision_type(CollisionType::Sphere);
    /// ```
    #[must_use]
    pub fn collision_type(mut self, c: CollisionType) -> Self {
        self.collision_type = c;
        self
    }

    /// Set shaders to describe how Object will interact with physical world.
    ///
    /// Highly recommended to read about [`ComputeShaderType`] to choose right shader for right object and never lose performance or realism.
    #[must_use]
    pub fn compute_shader(mut self, c: ComputeShaderType) -> Self {
        self.compute_shader = c;
        self
    }
}
