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
            ) if same_layer_plan_shape(old_layer, new_layer) => {
                let entry = match new_layer {
                    Layer::Clip | Layer::ClipSdf { .. } => Some(LayerStackEntry::Clip {
                        draw: *new_draw as u32,
                    }),
                    Layer::Opacity(value) => Some(LayerStackEntry::Opacity {
                        draw: *new_draw as u32,
                        opacity: value.opacity,
                    }),
                    Layer::Blend(value) => Some(LayerStackEntry::Blend {
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
        patch_retained_layer_ops(&mut self.ops, id, old, new)
    }
}

fn same_layer_plan_shape(old: &Layer, new: &Layer) -> bool {
    matches!(
        (old, new),
        (
            Layer::Clip | Layer::ClipSdf { .. },
            Layer::Clip | Layer::ClipSdf { .. }
        ) | (Layer::Isolate, Layer::Isolate)
            | (Layer::Opacity(_), Layer::Opacity(_))
            | (Layer::Blend(_), Layer::Blend(_))
            | (Layer::Filter { .. }, Layer::Filter { .. })
            | (Layer::Backdrop { .. }, Layer::Backdrop { .. })
    )
}

fn patch_retained_layer_ops(
    ops: &mut [ExecOp],
    id: RetainedNodeId,
    old: &Command,
    new: &Command,
) -> bool {
    for op in ops {
        match (&mut *op, old, new) {
            (
                ExecOp::OffscreenLayer {
                    retained_id,
                    draw,
                    layer,
                    ..
                },
                Command::Layer { draw: old_draw, .. },
                Command::Layer {
                    draw: new_draw,
                    layer: replacement,
                    ..
                },
            ) if retained_id.is_some_and(|surface| surface.node == id) && *draw == *old_draw => {
                *draw = *new_draw;
                *layer = replacement.clone();
                return true;
            }
            (
                ExecOp::OffscreenMaskLayer {
                    retained_id, layer, ..
                },
                Command::MaskLayer { .. },
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
            ExecOp::OffscreenLayer { children, .. } => {
                patch_retained_layer_ops(children, id, old, new)
            }
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                patch_retained_layer_ops(content, id, old, new)
                    || patch_retained_layer_ops(mask, id, old, new)
            }
            ExecOp::DrawBatch { .. } => false,
        };
        if patched {
            return true;
        }
    }
    false
}
