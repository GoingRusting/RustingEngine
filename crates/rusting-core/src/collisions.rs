/// CollisionType used to make more user friendly interface and hide math
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CollisionType {
    /// The best for performance
    #[default]
    Sphere,
    /// Box collision. More heavy for GPU, if you can use sphere, I recommend to avoid using Box
    Box,
}

impl CollisionType {
    /// This function return number for collision type that will be used in shaders
    pub fn sort_key(&self) -> f32 {
        match self {
            CollisionType::Sphere => 0.8,
            CollisionType::Box => 0.2,
        }
    }
}
