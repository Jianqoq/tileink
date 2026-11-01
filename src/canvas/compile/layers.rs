//! Resolve fusion while the source layer and its child topology are available.
//! Only geometric recording variants carry a draw; offscreen variants exclude clips.
use super::*;

#[expect(
    clippy::large_enum_variant,
    reason = "The transient compile result stays on the stack; boxing adds an allocation for each offscreen layer"
)]
pub(super) enum LayerExecution {
    Fused(LayerStackEntry),
    Offscreen(OffscreenLayer),
}

impl Canvas {
    pub(super) fn compile_layer(&self, layer: &Layer, children: CommandListId) -> LayerExecution {
        match layer {
            Layer::Clip { draw } | Layer::ClipSdf { draw } => {
                LayerExecution::Fused(LayerStackEntry::Clip { draw: *draw as u32 })
            }
            Layer::Opacity { draw, opacity } if self.can_fuse(layer, children) => {
                LayerExecution::Fused(LayerStackEntry::Opacity {
                    draw: *draw as u32,
                    opacity: *opacity,
                })
            }
            Layer::Blend { draw, mode } if self.can_fuse(layer, children) => {
                LayerExecution::Fused(LayerStackEntry::Blend {
                    draw: *draw as u32,
                    mode: *mode,
                })
            }
            Layer::Isolate { draw } => {
                LayerExecution::Offscreen(OffscreenLayer::Isolate { draw: *draw })
            }
            Layer::Opacity { draw, opacity } => {
                LayerExecution::Offscreen(OffscreenLayer::Opacity {
                    draw: *draw,
                    opacity: *opacity,
                })
            }
            Layer::Blend { draw, mode } => LayerExecution::Offscreen(OffscreenLayer::Blend {
                draw: *draw,
                mode: *mode,
            }),
            Layer::Filter {
                filter,
                sample_region,
            } => LayerExecution::Offscreen(OffscreenLayer::Filter {
                filter: filter.clone(),
                sample_region: sample_region.clone(),
            }),
            Layer::Backdrop {
                filter,
                sample_region,
                opacity,
            } => LayerExecution::Offscreen(OffscreenLayer::Backdrop {
                filter: filter.clone(),
                sample_region: sample_region.clone(),
                opacity: *opacity,
            }),
        }
    }
}
