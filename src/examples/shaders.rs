use rusting_engine::{
    CollisionType, ComputeShaderType, Engine, Material, Physics, ShaderType,
    Transform,
};

/// # Shaders Example
/// This example demonstrates how to assign various rendering configurations,
/// such as Unlit or Emissive materials, and static physics objects.
pub fn main() {
    let mut engine = Engine::new("RustingEngine - Shaders Demo");

    // Set a global directional light
    engine.set_light([30.0, 30.0, 30.0], [1.0, 0.95, 0.9], 450.0);

    // A fast-rendering Unlit material on a statically computed object
    // This removes heavy PBR calculations and avoids GPU-side physics checks.
    let unlit_mat = Material::standard()
        .color([0.2, 0.8, 0.2])
        .shader(ShaderType::Unlit)
        .build();

    // An emissive glowing object that calculates physics interactions
    let emissive_mat = Material::standard()
        .color([1.0, 1.0, 0.0])
        .shader(ShaderType::Emissive)
        .build();

    // 3. A standard PBR material
    let pbr_mat = Material::standard()
        .color([0.8, 0.2, 0.2])
        .shader(ShaderType::Pbr)
        .build();

    // Create a terrain/floor object that does NOT consume heavy physics simulation resources
    engine.add_cube(
        Transform::default(),
        &unlit_mat,
        &Physics::default()
            .compute_shader(ComputeShaderType::Static)
            .mass(0.0), // Does not move
    );

    // Create a moving, glowing bouncy ball
    engine.add_sphere(
        Transform {
            position: [0.0, 15.0, 0.0],
            scale: [2.0, 2.0, 2.0],
            ..Default::default()
        },
        &emissive_mat,
        &Physics::default()
            .mass(100.0)
            .collision_type(CollisionType::Sphere),
        2,
    );

    // A PBR cube falling
    engine.add_cube(
        Transform {
            position: [2.0, 20.0, 2.0],
            ..Default::default()
        },
        &pbr_mat,
        &Physics::default().mass(10.0),
    );

    engine.run();
}
