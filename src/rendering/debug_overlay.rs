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
        self.lines.push(DebugLine { start, end, color });
    }
}
