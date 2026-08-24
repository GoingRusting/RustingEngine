use rusting_engine::{
    CollisionType, ComputeShaderType, Engine, Material, Physics, ShaderType, Transform,
};
/// # Stress Test: 10,000 PBR Cubes
/// This example spawns 10,000 objects utilizing standard PBR shading.
/// It highlights frame-rate impact relative to the Unlit version, as PBR
/// calculates complex lighting, metallic/roughness values, and environment interactions.
/// also good example of usage `set_scene_shader` and `set_scene_physic`
pub fn main() {
    let mut engine = Engine::new("RustingEngine - Stress Test PBR (10k Objects)");
    engine.set_light([300.0, 500.0, 300.0], [1.0, 1.0, 1.0], 50000.0);
    let red_pbr = Material::standard()
        .color([1.0, 0.2, 0.2])
        .shader(ShaderType::Pbr)
        .build();
    let green_pbr = Material::standard()
        .color([0.2, 1.0, 0.2])
        .shader(ShaderType::Pbr)
        .build();
    let grid_size = 20;

    for x in 0..grid_size {
        for y in 0..grid_size {
            for z in 0..grid_size {
                let pos = [
                    (x as f32 - grid_size as f32 / 2.0) * 5.0,
                    y as f32 * 5.0 + 7.0,
                    (z as f32 - grid_size as f32 / 2.0) * 5.0,
                ];
                let cube_size = 4.0;
                // Alternate colors
                let mat = if (x + y + z) % 2 == 0 {
                    &red_pbr
                } else {
                    &green_pbr
                };
                engine.add_cube(
                    Transform {
                        position: pos,
                        scale: [cube_size, cube_size, cube_size],
                        ..Default::default()
                    },
                    mat,
                    &Physics::default()
                        .compute_shader(ComputeShaderType::Empty)
                        .mass(1.0)
                        .bounciness(1.0)
                        .collision_type(CollisionType::Box)
                        .gravity_scale(0.0),
                );
            }
        }
    }

    // for x in 0..grid_size {
    //     for y in 0..25 {
    //         for z in 0..grid_size {
    //             let pos = [
    //                 (x as f32 - grid_size as f32 / 2.0) * 3.0,
    //                 y as f32 * 3.0 + 20.0,
    //                 (z as f32 - grid_size as f32 / 2.0) * 3.0,
    //             ];
    //             let mat = if (x + y + z) % 2 == 0 { &red_pbr } else { &green_pbr };
    //             engine.add_sphere(
    //                 Transform {
    //                     position: pos,
    //                     scale: [1.0, 1.0, 1.0],
    //                     ..Default::default()
    //                 },
    //                 mat,
    //                 &Physics::default()
    //                     .compute_shader(ComputeShaderType::Test)
    //                     .mass(1.0)
    //                     .collision_type(CollisionType::Sphere)
    //                     .gravity_scale(1.0),
    //                 2
    //             );
    //         }
    //     }
    // }
    engine.add_sphere(
        Transform {
            position: [0.0, 3000.0, 0.0],
            scale: [10.0, 10.0, 10.0],
            ..Default::default()
        },
        &Material::standard()
            .color([0.1, 1.0, 0.1])
            .shader(ShaderType::Pbr)
            .build(),
        &Physics::default()
            .compute_shader(ComputeShaderType::Test)
            .mass(10000.0)
            .gravity_scale(10.0)
            .collision_type(CollisionType::Sphere),
        2,
    );

    engine.set_scene_shader(ShaderType::Pbr);
    engine.set_scene_physic(ComputeShaderType::Test);
    engine.run();
}
