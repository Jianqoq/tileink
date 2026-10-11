//! Patch retained group descriptors without rebuilding batch topology.
use super::*;

impl ExecPlan {
    /// Patches one retained layer while preserving draw order and stable batch membership.
    /// Returns false when the replacement changes execution structure and must be recompiled.
    pub(crate) fn patch_retained_layer(
        &mut self,
        id: RetainedNodeId,
        old: &Command,
        new: &Command,
    ) -> bool {
        match (old, new) {
            (
                Command::Layer {
                    draw: old_draw,
                    layer: old_layer,
                    ..
                },
                Command::Layer {
                    draw: new_draw,
                    layer: new_layer,
                    ..
                },
            ) => {
                let entry = match (old_layer, new_layer) {
                    (Layer::Clip | Layer::ClipSdf { .. }, Layer::Clip | Layer::ClipSdf { .. }) => {
                        Some(LayerStackEntry::Clip {
                            draw: *new_draw as u32,
                        })
                    }
                    (Layer::Opacity(_), Layer::Opacity(value)) => Some(LayerStackEntry::Opacity {
                        draw: *new_draw as u32,
                        opacity: value.opacity,
                    }),
                    (Layer::Blend(_), Layer::Blend(value)) => Some(LayerStackEntry::Blend {
                        draw: *new_draw as u32,
                        mode: value.mode,
                    }),
                    _ => None,
                };
                // Opacity/blend with offscreen children have no fused entry.
                // Filters have no hidden draw at all: their placeholder must
                // never remove another layer's index. Index presence identifies
                // the actual compiled representation, including empty groups.
                if let Some(entry) = entry
                    && let Some(locations) = self.layer_stack_locations.remove(&(*old_draw as u32))
                {
                    for &index in &locations {
                        self.layer_stack_data[index] = entry;
                    }
                    self.layer_stack_locations
                        .insert(*new_draw as u32, locations);
                    return true;
                }
            }
            (Command::MaskLayer { .. }, Command::MaskLayer { .. }) => {}
            _ => return false,
        }
        patch_retained_layer_ops(&mut self.ops, id, new)
    }
}

fn patch_retained_layer_ops(ops: &mut [ExecOp], id: RetainedNodeId, new: &Command) -> bool {
    for op in ops {
        match (&mut *op, new) {
            (
                ExecOp::OffscreenLayer {
                    retained_id, layer, ..
                },
                Command::Layer {
                    draw: new_draw,
                    layer: replacement,
                    ..
                },
            ) if retained_id.is_some_and(|surface| surface.node == id) => {
                match (layer, replacement) {
                    (OffscreenLayer::Isolate { draw }, Layer::Isolate) => *draw = *new_draw,
                    (OffscreenLayer::Opacity { draw, opacity }, Layer::Opacity(value)) => {
                        *draw = *new_draw;
                        *opacity = value.opacity;
                    }
                    (OffscreenLayer::Blend { draw, mode }, Layer::Blend(value)) => {
                        *draw = *new_draw;
                        *mode = value.mode;
                    }
                    (
                        OffscreenLayer::Filter {
                            filter,
                            sample_region,
                        },
                        Layer::Filter {
                            filter: replacement_filter,
                            sample_region: replacement_region,
                        },
                    )
                    | (
                        OffscreenLayer::Backdrop {
                            filter,
                            sample_region,
                        },
                        Layer::Backdrop {
                            filter: replacement_filter,
                            sample_region: replacement_region,
                        },
                    ) => {
                        *filter = replacement_filter.clone();
                        *sample_region = replacement_region.clone();
                    }
                    _ => return false,
                }
                return true;
            }
            (
                ExecOp::OffscreenMaskLayer {
                    retained_id, layer, ..
                },
                Command::MaskLayer {
                    layer: replacement, ..
                },
            ) if retained_id.is_some_and(|surface| surface.node == id) => {
                *layer = replacement.clone();
                return true;
            }
            _ => {}
        }
        let patched = match op {
            ExecOp::OffscreenLayer { children, .. } => patch_retained_layer_ops(children, id, new),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                patch_retained_layer_ops(content, id, new)
                    || patch_retained_layer_ops(mask, id, new)
            }
            ExecOp::DrawBatch { .. } => false,
        };
        if patched {
            return true;
        }
    }
    false
}
