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
                    layer: old_layer, ..
                },
                Command::Layer {
                    layer: new_layer, ..
                },
            ) => {
                let entry = match (old_layer, new_layer) {
                    (
                        Layer::Clip { draw: old_draw } | Layer::ClipSdf { draw: old_draw },
                        Layer::Clip { draw: new_draw } | Layer::ClipSdf { draw: new_draw },
                    ) => Some((
                        old_draw,
                        new_draw,
                        LayerStackEntry::Clip {
                            draw: *new_draw as u32,
                        },
                    )),
                    (
                        Layer::Opacity { draw: old_draw, .. },
                        Layer::Opacity {
                            draw: new_draw,
                            opacity,
                        },
                    ) => Some((
                        old_draw,
                        new_draw,
                        LayerStackEntry::Opacity {
                            draw: *new_draw as u32,
                            opacity: *opacity,
                        },
                    )),
                    (
                        Layer::Blend { draw: old_draw, .. },
                        Layer::Blend {
                            draw: new_draw,
                            mode,
                        },
                    ) => Some((
                        old_draw,
                        new_draw,
                        LayerStackEntry::Blend {
                            draw: *new_draw as u32,
                            mode: *mode,
                        },
                    )),
                    _ => None,
                };
                // Only real fused entries own stack locations. Opacity/blend
                // with offscreen children patch their surface descriptor instead.
                if let Some((old_draw, new_draw, entry)) = entry
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
                    layer: replacement, ..
                },
            ) if retained_id.is_some_and(|surface| surface.node == id) => {
                match (layer, replacement) {
                    (OffscreenLayer::Isolate { draw }, Layer::Isolate { draw: new_draw }) => {
                        *draw = *new_draw;
                    }
                    (
                        OffscreenLayer::Opacity { draw, opacity },
                        Layer::Opacity {
                            draw: new_draw,
                            opacity: value,
                        },
                    ) => {
                        *draw = *new_draw;
                        *opacity = *value;
                    }
                    (
                        OffscreenLayer::Blend { draw, mode },
                        Layer::Blend {
                            draw: new_draw,
                            mode: value,
                        },
                    ) => {
                        *draw = *new_draw;
                        *mode = *value;
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
                    ) => {
                        *filter = replacement_filter.clone();
                        *sample_region = replacement_region.clone();
                    }
                    (
                        OffscreenLayer::Backdrop {
                            filter,
                            sample_region,
                            opacity,
                        },
                        Layer::Backdrop {
                            filter: replacement_filter,
                            sample_region: replacement_region,
                            opacity: replacement_opacity,
                        },
                    ) => {
                        if opacity.is_some() != replacement_opacity.is_some() {
                            return false;
                        }
                        *filter = replacement_filter.clone();
                        *sample_region = replacement_region.clone();
                        *opacity = *replacement_opacity;
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
