//! Engine integration for the reusable runtime input resource.

use bevy_ecs::prelude::World;

pub use rusting_core::input::{
    KeyCode, MouseButton, PadButton, RuntimeInput, Stick,
};

pub(super) fn install(world: &mut World) {
    world.insert_resource(RuntimeInput::default());
}

/// Connected gamepads, read through gilrs. The window runner polls it once
/// per frame before the update, the same way it records window events.
#[cfg(feature = "window")]
pub struct Gamepads(Option<gilrs::Gilrs>);

#[cfg(feature = "window")]
impl Gamepads {
    /// Opens the gamepad backend. With none (no permission to the input
    /// devices, an unsupported platform) the game runs without pads.
    #[must_use]
    pub fn new() -> Self {
        Self(
            gilrs::Gilrs::new()
                .map_err(|error| eprintln!("gamepads unavailable: {error}"))
                .ok(),
        )
    }

    /// Records every pad event since the last poll into `input`.
    pub fn poll(&mut self, input: &mut RuntimeInput) {
        use gilrs::{Axis, EventType};
        let Some(gilrs) = &mut self.0 else {
            return;
        };
        while let Some(event) = gilrs.next_event() {
            match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(button) = pad_button(button) {
                        input.record_pad_button(button, true);
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(button) = pad_button(button) {
                        input.record_pad_button(button, false);
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    let (stick, index) = match axis {
                        Axis::LeftStickX => (Stick::Left, 0),
                        Axis::LeftStickY => (Stick::Left, 1),
                        Axis::RightStickX => (Stick::Right, 0),
                        Axis::RightStickY => (Stick::Right, 1),
                        _ => continue,
                    };
                    let mut tilt = input.stick(stick);
                    tilt[index] = value;
                    input.record_stick(stick, tilt);
                }
                EventType::Disconnected => input.release_all(),
                _ => {}
            }
        }
    }
}

#[cfg(feature = "window")]
impl Default for Gamepads {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "window")]
fn pad_button(button: gilrs::Button) -> Option<PadButton> {
    use gilrs::Button as B;
    Some(match button {
        B::South => PadButton::South,
        B::East => PadButton::East,
        B::West => PadButton::West,
        B::North => PadButton::North,
        B::LeftTrigger => PadButton::LeftBumper,
        B::RightTrigger => PadButton::RightBumper,
        B::LeftTrigger2 => PadButton::LeftTrigger,
        B::RightTrigger2 => PadButton::RightTrigger,
        B::Select => PadButton::Select,
        B::Start => PadButton::Start,
        B::LeftThumb => PadButton::LeftStick,
        B::RightThumb => PadButton::RightStick,
        B::DPadUp => PadButton::DpadUp,
        B::DPadDown => PadButton::DpadDown,
        B::DPadLeft => PadButton::DpadLeft,
        B::DPadRight => PadButton::DpadRight,
        _ => return None,
    })
}
