use rusting_engine::{
    CollisionType, ComputeShaderType, Engine, Material, Physics, ShaderType,
    Transform,
};
/// # Rotating test
/// This example spawns a sphere on the on the floor and cube that fall on it and roll
/// This test shows how accurate and 'good' cs_max( FullPhysic ) is
pub fn main() {
    let mut engine = Engine::new("RustingEngine - Rotation test");
    engine.set_light([30.0, 50.0, 30.0], [1.0, 1.0, 1.0], 500.0);

    // Cube is falling on sphere and rolling from it
    engine.add_cube(
        Transform {
            position: [0.5, 20.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            ..Default::default()
        },
        &Material::standard()
            .color([0.5, 0.5, 1.0])
            .shader(ShaderType::Pbr)
            .build(),
        &Physics::default().collision_type(CollisionType::Box),
    );

    engine.add_sphere(
        Transform {
            position: [0.0, 10.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            ..Default::default()
        },
        &Material::standard()
            .color([1.0, 0.1, 0.1])
            .shader(ShaderType::Pbr)
            .build(),
        &Physics::default()
            .compute_shader(ComputeShaderType::FullPhysics)
            .collision_type(CollisionType::Sphere),
        2,
    );

    // A large floor
    engine.add_cube(
        Transform {
            position: [0.0, 4.0, 0.0],
            scale: [40.0, 1.0, 40.0],
            ..Default::default()
        },
        &Material::standard()
            .color([0.5, 0.5, 0.5])
            .shader(ShaderType::Unlit)
            .build(),
        &Physics::default()
            .mass(1000000.0)
            .collision_type(CollisionType::Box),
    );
    engine.run();
}
