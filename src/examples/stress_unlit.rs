use rusting_engine::{
    CollisionType, ComputeShaderType, Engine, Material, Physics, ShaderType,
    Transform,
};
/// # Stress Test: 10,000 Unlit Cubes
/// This example spawns 10,000 objects completely bypassing PBR shading and utilizing Unlit.
/// It emphasizes the GPU performance gains achievable when disabling
/// complex lighting logic for massive amounts of background objects / particles.
pub fn main() {
    let mut engine =
        Engine::new("RustingEngine - Stress Test Unlit (10k Objects)");
    engine.set_light([30.0, 50.0, 30.0], [1.0, 1.0, 1.0], 500.0);
    let red_unlit = Material::standard()
        .color([1.0, 0.2, 0.2])
        .shader(ShaderType::Unlit)
        .build();
    let green_unlit = Material::standard()
        .color([0.2, 1.0, 0.2])
        .shader(ShaderType::Unlit)
        .build();
    let grid_size = 20;

    for x in 0..grid_size {
        for y in 0..25 {
            for z in 0..grid_size {
                let pos = [
                    (x as f32 - grid_size as f32 / 2.0) * 1.5,
                    y as f32 * 1.5 + 5.0,
                    (z as f32 - grid_size as f32 / 2.0) * 1.5,
                ];
                // Alternate colors
                let mat = if (x + y + z) % 2 == 0 {
                    &red_unlit
                } else {
                    &green_unlit
                };
                engine.add_cube(
                    Transform {
                        position: pos,
                        ..Default::default()
                    },
                    mat,
                    &Physics::default()
                        .compute_shader(ComputeShaderType::Static)
                        .collision_type(CollisionType::Box),
                );
            }
        }
    }

    engine.run();
}
