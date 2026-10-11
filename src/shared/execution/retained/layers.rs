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
        let (old_draw, new_draw, old_layer, new_layer) = match (old, new) {
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
            ) if same_layer_plan_shape(old_layer, new_layer) => (
                Some(*old_draw),
                Some(*new_draw),
                Some(old_layer),
                Some(new_layer),
            ),
            (Command::MaskLayer { .. }, Command::MaskLayer { .. }) => (None, None, None, None),
            _ => return false,
        };

        if let (Some(old_draw), Some(new_draw), Some(new_layer)) = (old_draw, new_draw, new_layer) {
            let locations = self
                .layer_stack_locations
                .remove(&(old_draw as u32))
                .unwrap_or_default();
            for &index in &locations {
                let entry = &mut self.layer_stack_data[index];
                match (entry, new_layer) {
                    (LayerStackEntry::Clip { draw }, Layer::Clip | Layer::ClipSdf { .. })
                        if *draw == old_draw as u32 =>
                    {
                        *draw = new_draw as u32;
                    }
                    (LayerStackEntry::Opacity { draw, opacity }, Layer::Opacity(replacement))
                        if *draw == old_draw as u32 =>
                    {
                        *draw = new_draw as u32;
                        *opacity = replacement.opacity;
                    }
                    (LayerStackEntry::Blend { draw, mode }, Layer::Blend(replacement))
                        if *draw == old_draw as u32 =>
                    {
                        *draw = new_draw as u32;
                        *mode = replacement.mode;
                    }
                    _ => {}
                }
            }
            if matches!(
                new_layer,
                Layer::Clip | Layer::ClipSdf { .. } | Layer::Opacity(_) | Layer::Blend(_)
            ) {
                self.layer_stack_locations
                    .entry(new_draw as u32)
                    .or_default()
                    .extend(locations);
                return true;
            }
        }
        patch_retained_layer_ops(&mut self.ops, id, old_draw, old_layer, new)
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
    old_draw: Option<usize>,
    old_layer: Option<&Layer>,
    new: &Command,
) -> bool {
    let mut matched = false;
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                retained_id,
                draw,
                layer,
                children,
                ..
            } => {
                if retained_id.is_some_and(|surface| surface.node == id)
                    && old_draw == Some(*draw)
                    && old_layer.is_some_and(|old| same_layer_plan_shape(old, layer))
                {
                    let Command::Layer {
                        draw: replacement_draw,
                        layer: replacement,
                        ..
                    } = new
                    else {
                        return false;
                    };
                    *draw = *replacement_draw;
                    *layer = replacement.clone();
                    matched = true;
                }
                matched |= patch_retained_layer_ops(children, id, old_draw, old_layer, new);
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                content,
                mask,
                ..
            } => {
                if retained_id.is_some_and(|surface| surface.node == id) {
                    let Command::MaskLayer {
                        layer: replacement, ..
                    } = new
                    else {
                        return false;
                    };
                    *layer = replacement.clone();
                    matched = true;
                }
                matched |= patch_retained_layer_ops(content, id, old_draw, old_layer, new);
                matched |= patch_retained_layer_ops(mask, id, old_draw, old_layer, new);
            }
            _ => {}
        }
    }
    // Fused layers have no retained-id-bearing ExecOp; their stack entry was patched above.
    matched
        || matches!(
            new,
            Command::Layer {
                layer: Layer::Clip | Layer::ClipSdf { .. } | Layer::Opacity(_) | Layer::Blend(_),
                ..
            }
        )
}
