//! The declared pass schedule of one `SceneRenderer` frame.
//!
//! Each pass names the resources it reads and writes and the image layout it
//! needs them in. `SceneRenderer` records its passes in this order, labels
//! each one with [`FramePass::label`], and builds its render passes with the
//! attachment layouts this table asks for. Vulkano still inserts the
//! barriers; the table makes the order and the layout transitions explicit
//! and testable until a real render graph schedules them (Milestone 13).

use vulkano::image::ImageLayout;

/// One pass of a `SceneRenderer` frame, in submission order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePass {
    /// Staged material texture copies and physics state carry-over after a
    /// body buffer grows. Only frames that have either.
    Uploads,
    /// Fixed-tick GPU physics dispatches. Only frames with new ticks.
    Physics,
    /// GPU frustum culling into the visible list and indirect draws. Only
    /// frames that select the GPU culling path.
    Culling,
    /// Depth-only directional shadow map.
    Shadow,
    /// Lit scene into the HDR target (main render pass, subpass 0). On
    /// occlusion frames, only the opaque instances visible last frame, in
    /// an early render pass that keeps HDR and depth.
    Scene,
    /// Farthest-depth mip chain of the early scene depth. Occlusion frames
    /// only.
    DepthPyramid,
    /// Late cull against the depth pyramid into a second visible list and
    /// draw-command set. Occlusion frames only.
    OcclusionCulling,
    /// The rest of the scene over the early pass (late render pass,
    /// subpass 0). Occlusion frames only.
    LateScene,
    /// HDR target mapped into the output (main render pass, subpass 1).
    ToneMap,
    /// Editor debug lines over the output (main render pass, subpass 1).
    DebugOverlay,
}

/// A GPU resource that frame passes hand to each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameResource {
    MaterialTextures,
    PhysicsStates,
    /// Visible list and indirect draw commands written by GPU culling.
    DrawCommands,
    ShadowMap,
    DepthPyramid,
    HdrColor,
    SceneDepth,
    /// The caller's output image (swapchain image or offscreen target).
    Target,
}

/// How a pass uses one resource. `layout` is `None` for buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassAccess {
    pub resource: FrameResource,
    pub layout: Option<ImageLayout>,
    pub write: bool,
}

/// A layout change of `resource` between two passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutTransition {
    pub resource: FrameResource,
    pub from: ImageLayout,
    pub to: ImageLayout,
    pub before: FramePass,
}

impl FramePass {
    pub const ALL: [FramePass; 10] = [
        FramePass::Uploads,
        FramePass::Physics,
        FramePass::Culling,
        FramePass::Shadow,
        FramePass::Scene,
        FramePass::DepthPyramid,
        FramePass::OcclusionCulling,
        FramePass::LateScene,
        FramePass::ToneMap,
        FramePass::DebugOverlay,
    ];

    /// The debug-utils label the pass is recorded under.
    pub fn label(self) -> &'static str {
        match self {
            FramePass::Uploads => "Uploads",
            FramePass::Physics => "Physics",
            FramePass::Culling => "Culling",
            FramePass::Shadow => "Shadow",
            FramePass::Scene => "Scene",
            FramePass::DepthPyramid => "DepthPyramid",
            FramePass::OcclusionCulling => "OcclusionCulling",
            FramePass::LateScene => "LateScene",
            FramePass::ToneMap => "ToneMap",
            FramePass::DebugOverlay => "DebugOverlay",
        }
    }

    pub fn accesses(self) -> &'static [PassAccess] {
        use FrameResource::*;
        use ImageLayout::*;
        const fn read(
            resource: FrameResource,
            layout: Option<ImageLayout>,
        ) -> PassAccess {
            PassAccess {
                resource,
                layout,
                write: false,
            }
        }
        const fn write(
            resource: FrameResource,
            layout: Option<ImageLayout>,
        ) -> PassAccess {
            PassAccess {
                resource,
                layout,
                write: true,
            }
        }
        match self {
            FramePass::Uploads => {
                const {
                    &[
                        write(MaterialTextures, Some(TransferDstOptimal)),
                        write(PhysicsStates, None),
                    ]
                }
            }
            FramePass::Physics => {
                const { &[read(PhysicsStates, None), write(PhysicsStates, None)] }
            }
            FramePass::Culling => {
                const { &[read(PhysicsStates, None), write(DrawCommands, None)] }
            }
            FramePass::Shadow => {
                const { &[write(ShadowMap, Some(DepthStencilAttachmentOptimal))] }
            }
            FramePass::Scene | FramePass::LateScene => {
                const {
                    &[
                        read(MaterialTextures, Some(ShaderReadOnlyOptimal)),
                        read(PhysicsStates, None),
                        read(DrawCommands, None),
                        read(ShadowMap, Some(ShaderReadOnlyOptimal)),
                        write(HdrColor, Some(ColorAttachmentOptimal)),
                        write(SceneDepth, Some(DepthStencilAttachmentOptimal)),
                    ]
                }
            }
            FramePass::DepthPyramid => {
                const {
                    &[
                        read(SceneDepth, Some(ShaderReadOnlyOptimal)),
                        write(DepthPyramid, Some(General)),
                    ]
                }
            }
            FramePass::OcclusionCulling => {
                const {
                    &[
                        read(PhysicsStates, None),
                        read(DepthPyramid, Some(ShaderReadOnlyOptimal)),
                        write(DrawCommands, None),
                    ]
                }
            }
            FramePass::ToneMap => {
                const {
                    &[
                        read(HdrColor, Some(ShaderReadOnlyOptimal)),
                        write(Target, Some(ColorAttachmentOptimal)),
                    ]
                }
            }
            FramePass::DebugOverlay => {
                const {
                    &[
                        read(SceneDepth, Some(DepthStencilAttachmentOptimal)),
                        write(Target, Some(ColorAttachmentOptimal)),
                    ]
                }
            }
        }
    }

    /// The layout the first pass after `self` that touches `resource`
    /// expects, which is the layout `self` must leave it in.
    pub fn next_layout(self, resource: FrameResource) -> Option<ImageLayout> {
        FramePass::ALL
            .iter()
            .skip_while(|pass| **pass != self)
            .skip(1)
            .flat_map(|pass| pass.accesses())
            .find(|access| access.resource == resource)
            .and_then(|access| access.layout)
    }
}

/// Every layout change between consecutive users of an image, in order.
pub fn layout_transitions() -> Vec<LayoutTransition> {
    let mut last = Vec::<(FrameResource, ImageLayout)>::new();
    let mut transitions = Vec::new();
    for pass in FramePass::ALL {
        for access in pass.accesses() {
            let Some(layout) = access.layout else {
                continue;
            };
            match last.iter_mut().find(|(r, _)| *r == access.resource) {
                Some((_, from)) if *from != layout => {
                    transitions.push(LayoutTransition {
                        resource: access.resource,
                        from: *from,
                        to: layout,
                        before: pass,
                    });
                    *from = layout;
                }
                Some(_) => {}
                None => last.push((access.resource, layout)),
            }
        }
    }
    transitions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_read_follows_a_write_of_the_same_resource() {
        for (index, pass) in FramePass::ALL.iter().enumerate() {
            for access in pass.accesses().iter().filter(|a| !a.write) {
                // A pass may read what it writes itself (physics state).
                let written = FramePass::ALL[..=index]
                    .iter()
                    .flat_map(|p| p.accesses())
                    .any(|a| a.write && a.resource == access.resource);
                assert!(written, "{pass:?} reads unwritten {access:?}");
            }
        }
    }

    #[test]
    fn labels_are_unique() {
        let mut labels: Vec<_> =
            FramePass::ALL.iter().map(|p| p.label()).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), FramePass::ALL.len());
    }

    #[test]
    fn transitions_list_every_layout_change_between_passes() {
        use FrameResource::*;
        use ImageLayout::*;
        let expected = [
            (
                MaterialTextures,
                TransferDstOptimal,
                ShaderReadOnlyOptimal,
                FramePass::Scene,
            ),
            (
                ShadowMap,
                DepthStencilAttachmentOptimal,
                ShaderReadOnlyOptimal,
                FramePass::Scene,
            ),
            (
                SceneDepth,
                DepthStencilAttachmentOptimal,
                ShaderReadOnlyOptimal,
                FramePass::DepthPyramid,
            ),
            (
                DepthPyramid,
                General,
                ShaderReadOnlyOptimal,
                FramePass::OcclusionCulling,
            ),
            (
                SceneDepth,
                ShaderReadOnlyOptimal,
                DepthStencilAttachmentOptimal,
                FramePass::LateScene,
            ),
            (
                HdrColor,
                ColorAttachmentOptimal,
                ShaderReadOnlyOptimal,
                FramePass::ToneMap,
            ),
        ]
        .map(|(resource, from, to, before)| LayoutTransition {
            resource,
            from,
            to,
            before,
        });
        assert_eq!(layout_transitions(), expected);
        assert_eq!(
            FramePass::Shadow.next_layout(ShadowMap),
            Some(ShaderReadOnlyOptimal)
        );
        assert_eq!(FramePass::DebugOverlay.next_layout(Target), None);
    }
}
