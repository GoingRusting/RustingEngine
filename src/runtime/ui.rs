//! Runtime UI: immediate-mode egui drawn over the game view.
//!
//! Every `App` built with the `ui` feature has a [`RuntimeUi`] resource.
//! `App::update` opens one egui pass around the `Update` and `PostUpdate`
//! schedules, so any system there can draw a HUD or menu:
//!
//! ```ignore
//! fn hud(ui: Res<RuntimeUi>, time: Res<FrameTime>) {
//!     egui::Area::new("hud".into()).show(ui.context(), |ui| {
//!         ui.label(format!("{:.0} FPS", 1.0 / time.delta.as_secs_f32()));
//!     });
//! }
//! ```
//!
//! The window runner feeds window input in and paints the finished pass over
//! the rendered scene. Clicks and keys that egui uses do not reach
//! `RuntimeInput`. Without a window the pass still runs with empty input, so
//! UI systems behave the same in headless runs.

use bevy_ecs::prelude::Resource;

#[derive(Resource, Default)]
pub struct RuntimeUi {
    context: egui::Context,
    /// Window input for the next pass; taken when the pass begins.
    input: egui::RawInput,
    /// Result of the last finished pass, until the runner takes it.
    output: Option<egui::FullOutput>,
}

impl RuntimeUi {
    /// Context to draw with during `Update` and `PostUpdate`.
    #[must_use]
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// Sets the input the next pass reads.
    pub fn set_input(&mut self, input: egui::RawInput) {
        self.input = input;
    }

    /// Takes the last finished pass: shapes, texture changes, and platform
    /// output such as the cursor icon.
    pub fn take_output(&mut self) -> Option<egui::FullOutput> {
        self.output.take()
    }

    pub(super) fn begin_pass(&mut self) {
        self.context.begin_pass(std::mem::take(&mut self.input));
    }

    pub(super) fn end_pass(&mut self) {
        let mut output = self.context.end_pass();
        // Texture changes are cumulative; keep ones the runner has not
        // uploaded yet (a headless run never takes them).
        if let Some(previous) = self.output.take() {
            let mut textures = previous.textures_delta;
            textures.append(output.textures_delta);
            output.textures_delta = textures;
        }
        self.output = Some(output);
    }
}
