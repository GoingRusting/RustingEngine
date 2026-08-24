use crate::scene::object::Transform;
use std::sync::Arc;

type AnimationFn =
    dyn Fn(&mut Transform, &mut [f32; 3], &mut [f32; 3], &mut [f32; 3], f32) + Send + Sync;

// ? I dont want to describe it, maybe I will delete it, bc I dont like it.
#[derive(Clone)]
pub enum AnimationType {
    Rotate,
    Pulse,
    Static,
    Custom(Arc<AnimationFn>),
}

impl std::fmt::Debug for AnimationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnimationType::Rotate => write!(f, "Rotate"),
            AnimationType::Pulse => write!(f, "Pulse"),
            AnimationType::Static => write!(f, "Static"),
            AnimationType::Custom(_) => write!(f, "Custom Logic"),
        }
    }
}
