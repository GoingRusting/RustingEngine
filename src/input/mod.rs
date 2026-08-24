use std::collections::HashSet;
use winit::keyboard::KeyCode;
use winit::window::CursorGrabMode;

pub struct InputState {
    pub keys_pressed: HashSet<KeyCode>,
    pub mouse_delta: [f32; 2],
    pub mouse_captured: bool,
    pub last_mouse_pos: [f32; 2],
    pub is_mouse_dragging: bool,
    pub speed: f32,
    pub sprint: f32,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            keys_pressed: std::collections::HashSet::new(),
            mouse_delta: [0.0, 0.0],
            mouse_captured: false,
            last_mouse_pos: [0.0, 0.0],
            is_mouse_dragging: false,
            sprint: 2.0,
            speed: 0.02,
        }
    }
}

pub fn set_mouse_capture(window: &winit::window::Window, captured: bool) {
    if captured {
        let _ = window.set_cursor_grab(CursorGrabMode::Locked);
        window.set_cursor_visible(false);
    } else {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
        window.set_cursor_visible(true);
    }
}
