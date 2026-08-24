use rusting_engine::{
    CollisionType, ComputeShaderType, Engine, Material, Physics, ShaderType,
    Transform,
};

pub fn main() {
    let mut engine = Engine::new("RustingEngine - Native Textures");
    engine.set_light([10.0, 50.0, 20.0], [1.0, 1.0, 1.0], 500.0);

    // Load the texture into the global registry and get its ID
    let rust_tex = engine.load_texture("./testModels/RustSphere.png");

    // Spawn 10 textured native cubes
    for i in 0..10 {
        engine.add_cube(
            Transform {
                position: [
                    -10.0 + (i as f32) * 5.0,
                    10.0 + (i as f32) * 2.0,
                    -10.0,
                ],
                scale: [2.0, 2.0, 2.0],
                ..Default::default()
            },
            &Material::standard()
                .color([0.8, 0.8, 0.8]) // White base to show full texture color
                .roughness(0.4)
                .base_color_texture(rust_tex) // Attach our texture!
                .shader(ShaderType::Pbr)
                .build(),
            &Physics::default()
                .compute_shader(ComputeShaderType::Test)
                .mass(3.0)
                .gravity_scale(1.0)
                .collision_type(CollisionType::Box),
        );
    }

    // Spawn 10 textured native spheres
    for i in 0..10 {
        engine.add_sphere(
            Transform {
                position: [
                    -10.0 + (i as f32) * 5.0,
                    15.0 + (i as f32) * 2.0,
                    10.0,
                ],
                scale: [2.0, 2.0, 2.0],
                ..Default::default()
            },
            &Material::standard()
                .color([1.0, 1.0, 1.0])
                .metalness(0.2)
                .roughness(0.1)
                .base_color_texture(rust_tex) // Attach the exact SAME texture memory ID!
                .shader(ShaderType::Pbr)
                .build(),
            &Physics::default()
                .compute_shader(ComputeShaderType::Test)
                .mass(3.0)
                .gravity_scale(1.0)
                .collision_type(CollisionType::Sphere),
            2, // Subdivisions
        );
    }

    // Set shaders and run the engine loop
    engine.set_scene_shader(ShaderType::Heavy);
    engine.run();
}
