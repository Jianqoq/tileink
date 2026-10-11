//! Resolve fusion while the source layer and its child topology are available.
//! The resulting offscreen type cannot carry clips or a filter's dummy draw.
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
    pub(super) fn compile_layer(
        &self,
        layer: &Layer,
        draw: usize,
        children: CommandListId,
    ) -> LayerExecution {
        match layer {
            Layer::Clip | Layer::ClipSdf { .. } => {
                LayerExecution::Fused(LayerStackEntry::Clip { draw: draw as u32 })
            }
            Layer::Opacity(value) if self.can_fuse(layer, children) => {
                LayerExecution::Fused(LayerStackEntry::Opacity {
                    draw: draw as u32,
                    opacity: value.opacity,
                })
            }
            Layer::Blend(value) if self.can_fuse(layer, children) => {
                LayerExecution::Fused(LayerStackEntry::Blend {
                    draw: draw as u32,
                    mode: value.mode,
                })
            }
            Layer::Isolate => LayerExecution::Offscreen(OffscreenLayer::Isolate { draw }),
            Layer::Opacity(value) => LayerExecution::Offscreen(OffscreenLayer::Opacity {
                draw,
                opacity: value.opacity,
            }),
            Layer::Blend(value) => LayerExecution::Offscreen(OffscreenLayer::Blend {
                draw,
                mode: value.mode,
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
            } => LayerExecution::Offscreen(OffscreenLayer::Backdrop {
                filter: filter.clone(),
                sample_region: sample_region.clone(),
            }),
        }
    }
}
