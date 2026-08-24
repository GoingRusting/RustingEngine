use rusting_engine::{
    CollisionType, ComputeShaderType, Engine, Material, Physics, ShaderType, Transform,
};

pub fn main() {
    let mut engine = Engine::new("RustingEngine - Stress Test PBR (10k Objects)");
    engine.set_light([300.0, 500.0, 300.0], [1.0, 1.0, 1.0], 50000.0);

    // engine.add_gltf(Transform {
    //         position: [0.0, 0.0, 0.0],
    //         scale: [10.0, 10.0, 10.0],
    //         ..Default::default()
    //     },
    //     &Material::standard()
    //         .color([0.1, 1.0, 0.1])
    //         .shader(ShaderType::Pbr)
    //         .build(),
    //     &Physics::default()
    //         .compute_shader(ComputeShaderType::FullPhysics)
    //         .mass(10.0)
    //         .gravity_scale(1.0)
    //         .collision_type(CollisionType::Sphere),
    //     "./testModels/1kRustingSphere.gltf"
    // );

    engine.add_gltf(
        Transform {
            position: [5.0, 100.0, 5.0],
            scale: [10.0, 10.0, 10.0],
            ..Default::default()
        },
        &Material::standard()
            .color([1.0, 1.0, 1.0])
            .shader(ShaderType::Pbr)
            .build(),
        &Physics::default()
            .compute_shader(ComputeShaderType::Test)
            .mass(10.0)
            .gravity_scale(1.0)
            .collision_type(CollisionType::Sphere),
        "./testModels/1kRustingSphere.gltf",
    );

    for i in 0..10 {
        for j in 0..10 {
            for k in 0..10 {
                engine.add_gltf(
                    Transform {
                        position: [
                            0.0 + (1.0 * i as f32),
                            0.0 + (1.0 * j as f32),
                            0.0 + (1.0 * k as f32),
                        ],
                        scale: [1.0, 1.0, 1.0],
                        ..Default::default()
                    },
                    &Material::standard()
                        .color([1.0, 1.0, 1.0])
                        .shader(ShaderType::Pbr)
                        .build(),
                    &Physics::default()
                        .compute_shader(ComputeShaderType::Test)
                        .mass(10.0)
                        .gravity_scale(1.0)
                        .collision_type(CollisionType::Box),
                    "./testModels/cube.gltf",
                );
            }
        }
    }

    engine.set_scene_shader(ShaderType::Heavy);
    engine.set_scene_physic(ComputeShaderType::Test);
    engine.run();
}
