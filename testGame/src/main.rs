use rusting_engine::prelude::*;
use std::f32::consts::PI;

fn update(scene: &mut GameScene<'_>, _time: &FrameTime) {
    scene.once("create_satellite_swarm", |scene| {
        let satellite = CubeSpawn::new().class("satellite");
        let planet = SphereSpawn::new().class("planet");

        let random_val =
            |seed: f32, magic: f32| -> f32 { (seed * magic).fract() };

        let planet_material = scene.create_material(MaterialAsset {
            model: MaterialModel::Pbr,
            base_color: [0.8, 0.4, 0.1, 1.0],
            metallic: 0.1,
            roughness: 0.9,
            ..MaterialAsset::default()
        });

        let satellite_material = scene.create_material(MaterialAsset {
            model: MaterialModel::Pbr,
            base_color: [0.9, 0.85, 0.7, 1.0],
            emissive: [0.1, 0.05, 0.0],
            ..MaterialAsset::default()
        });

        scene.spawn_sphere_with_material(
            "Planet",
            Transform {
                scale: [50.0, 50.0, 50.0],
                ..Default::default()
            },
            &planet,
            planet_material,
        );

        scene.set_background_color([0.0, 0.0, 0.0, 1.0]);

        for index in 0..10_000 {
            let fi = index as f32;

            let radius = 60.0 + random_val(fi, 123.456) * 140.0;
            let angle = random_val(fi, 789.123) * 2.0 * PI;
            let height = (random_val(fi, 345.678) - 0.5) * 40.0;

            let position = [radius * angle.cos(), height, radius * angle.sin()];

            let transform = Transform::new(position).with_rotation(
                random_val(fi, 111.111) * 2.0 * PI,
                random_val(fi, 222.222) * 2.0 * PI,
                random_val(fi, 333.333) * 2.0 * PI,
            );

            let name = format!("Satellite {index}");

            scene.spawn_cube_with_material(
                name.clone(),
                transform,
                &satellite,
                satellite_material,
            );
        }

        scene.apply_gpu_physics_to_class(
            "satellite",
            &GpuBodySettings {
                solver: PhysicsSolver::Space,
                ..Default::default()
            },
        );

        for index in 0..10_000 {
            let fi = index as f32;

            let radius = 60.0 + random_val(fi, 123.456) * 140.0;
            let angle = random_val(fi, 789.123) * 2.0 * PI;

            let orbital_speed = (500.0 / radius).sqrt();

            let velocity = [
                -angle.sin() * orbital_speed,
                0.0,
                angle.cos() * orbital_speed,
            ];

            scene.set_linear_velocity(&format!("Satellite {index}"), velocity);
        }
    });
}

rusting_game!(update);
