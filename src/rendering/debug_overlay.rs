//! Small, editor-facing drawing data for the renderer's debug pass.
//!
//! This module deliberately contains only vertices and colours. The editor
//! decides which helpers are useful; the renderer only knows how to draw them.
//! That keeps grids and gizmos out of saved scenes and normal game rendering.

/// One coloured line in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugLine {
    /// Start position in world coordinates.
    pub start: [f32; 3],
    /// End position in world coordinates.
    pub end: [f32; 3],
    /// Linear RGBA colour.
    pub color: [f32; 4],
    /// Width in physical viewport pixels.
    pub thickness: f32,
    /// Draw after scene depth when this helper must remain readable.
    pub on_top: bool,
}

/// Editor-only geometry collected for one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderDebugOverlay {
    /// Lines drawn after normal opaque meshes.
    pub lines: Vec<DebugLine>,
}

impl RenderDebugOverlay {
    /// Adds one line without making the editor depend on Vulkan types.
    pub fn line(&mut self, start: [f32; 3], end: [f32; 3], color: [f32; 4]) {
        self.lines.push(DebugLine {
            start,
            end,
            color,
            thickness: 1.0,
            on_top: false,
        });
    }

    /// Adds a portable thick line that ignores scene depth.
    pub fn line_on_top(
        &mut self,
        start: [f32; 3],
        end: [f32; 3],
        color: [f32; 4],
        thickness: f32,
    ) {
        self.lines.push(DebugLine {
            start,
            end,
            color,
            thickness: thickness.max(1.0),
            on_top: true,
        });
    }
}
