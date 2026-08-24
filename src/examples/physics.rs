use rusting_engine::{CollisionType, Engine, Material, Physics, Transform};

/// # Physics Gravity & Collision Example
/// This example demonstrates how hundreds of instanced objects can dynamically interact
/// via compute shader acceleration without taxing the CPU.
pub fn main() {
    let mut engine = Engine::new("RustingEngine - Physics Demo");
    engine.set_light([30.0, 30.0, 30.0], [1.0, 0.95, 0.9], 450.0);

    let red = Material::standard().color([1.0, 0.0, 0.0]).build();
    let green = Material::standard().color([0.0, 1.0, 0.0]).build();

    // Generate a massive multi-layered block pile
    for i in 0..10 {
        for j in 0..3 {
            for k in 0..10 {
                let pos =
                    [i as f32 * 1.5, j as f32 * 2.0 + 7.0, k as f32 * 1.5];
                engine.add_cube(
                    Transform {
                        position: pos,
                        ..Default::default()
                    },
                    &red,
                    &Physics::default()
                        .collision_type(CollisionType::Box)
                        .gravity_scale(0.0), // Starts with zero gravity
                );
            }
        }
    }

    // Add an enormous heavy boulder falling from the sky to crash into the blocks!
    engine.add_sphere(
        Transform {
            position: [7.0, 100.0, 7.0],
            scale: [7.5, 7.5, 7.5],
            ..Default::default()
        },
        &green,
        &Physics::default()
            .mass(100000.0) // Very heavy
            .collision_type(CollisionType::Sphere),
        2,
    );

    // Call run to begin simulation!
    engine.run();
}
