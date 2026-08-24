use crate::input::InputState;

pub struct Camera {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
}

pub fn create_look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let f = {
        let v = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / len, v[1] / len, v[2] / len]
    };
    let s = {
        let v = [
            f[1] * up[2] - f[2] * up[1],
            f[2] * up[0] - f[0] * up[2],
            f[0] * up[1] - f[1] * up[0],
        ];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / len, v[1] / len, v[2] / len]
    };
    let u = [
        s[1] * f[2] - s[2] * f[1],
        s[2] * f[0] - s[0] * f[2],
        s[0] * f[1] - s[1] * f[0],
    ];

    [
        [s[0], u[0], -f[0], 0.0],
        [s[1], u[1], -f[1], 0.0],
        [s[2], u[2], -f[2], 0.0],
        [
            -(s[0] * eye[0] + s[1] * eye[1] + s[2] * eye[2]),
            -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
            (f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2]),
            1.0,
        ],
    ]
    // ? Why is here so much math, I am tired of math a bit
}

pub fn create_projection_matrix(aspect: f32, fov: f32, z_near: f32, z_far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, -f, 0.0, 0.0],
        [0.0, 0.0, z_far / (z_far - z_near), 1.0],
        [0.0, 0.0, -(z_far * z_near) / (z_far - z_near), 0.0],
    ]
}

pub fn camera_rotate(camera: &mut Camera, inputs: &InputState) -> [[f32; 4]; 4] {
    let (yaw_sin, yaw_cos) = camera.yaw.sin_cos();
    let (pitch_sin, pitch_cos) = camera.pitch.sin_cos();

    let forward = [camera.yaw.cos(), 0.0, camera.yaw.sin()];

    let len = (forward[0] * forward[0] + forward[2] * forward[2]).sqrt();
    let forward = [-forward[0] / len, 0.0, -forward[2] / len];
    let view_forward = [yaw_cos * pitch_cos, pitch_sin, yaw_sin * pitch_cos];
    let right = [-camera.yaw.sin(), 0.0, camera.yaw.cos()];
    let world_up = [0.0, 1.0, 0.0];
    if inputs.mouse_captured {
        let speed_mult = inputs.speed * inputs.sprint;

        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::W)
        {
            for (position, direction) in camera.position.iter_mut().zip(forward) {
                *position += direction * speed_mult;
            }
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::S)
        {
            for (position, direction) in camera.position.iter_mut().zip(forward) {
                *position -= direction * speed_mult;
            }
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::A)
        {
            for (position, direction) in camera.position.iter_mut().zip(right) {
                *position -= direction * speed_mult;
            }
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::D)
        {
            for (position, direction) in camera.position.iter_mut().zip(right) {
                *position += direction * speed_mult;
            }
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::Space)
        {
            camera.position[1] += speed_mult;
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::LControl)
        {
            camera.position[1] -= speed_mult;
        }

        let target = [
            camera.position[0] + view_forward[0],
            camera.position[1] + view_forward[1],
            camera.position[2] + view_forward[2],
        ];

        create_look_at(camera.position, target, world_up)
    } else {
        let speed_step = inputs.speed * inputs.sprint;

        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::W)
        {
            camera.position[0] += forward[0] * speed_step;
            camera.position[1] += forward[1] * speed_step;
            camera.position[2] += forward[2] * speed_step;
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::S)
        {
            camera.position[0] -= forward[0] * speed_step;
            camera.position[1] -= forward[1] * speed_step;
            camera.position[2] -= forward[2] * speed_step;
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::A)
        {
            camera.position[0] -= right[0] * speed_step;
            camera.position[2] -= right[2] * speed_step;
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::D)
        {
            camera.position[0] += right[0] * speed_step;
            camera.position[2] += right[2] * speed_step;
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::Space)
        {
            camera.position[1] += speed_step;
        }
        if inputs
            .keys_pressed
            .contains(&winit::event::VirtualKeyCode::LControl)
        {
            camera.position[1] -= speed_step;
        }
        let target = [
            camera.position[0] + view_forward[0],
            camera.position[1] + view_forward[1],
            camera.position[2] + view_forward[2],
        ];
        create_look_at(camera.position, target, [0.0, 1.0, 0.0])
    }
}
