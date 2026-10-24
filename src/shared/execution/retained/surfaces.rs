//! Move retained offscreen descriptors while preserving stable draw slots.
use super::*;

impl ExecPlan {
    /// Updates only translated offscreen descriptors belonging to one retained scene leaf.
    ///
    /// Position changes preserve command topology, physical draw slots, and `BatchIds`. Replacing
    /// these descriptors avoids recompiling unrelated scene fragments while still moving filter,
    /// backdrop, group, and mask surface geometry to the new coordinates.
    pub(crate) fn patch_retained_scene_position(
        &mut self,
        id: RetainedNodeId,
        fragment: &Self,
    ) -> bool {
        let mut replacements = HashMap::new();
        collect_surface_patches(&fragment.ops, id, &mut replacements);
        if replacements.is_empty() {
            return false;
        }
        let mut patched = 0;
        patch_surface_ops(&mut self.ops, &replacements, &mut patched);
        patched == replacements.len()
    }
}

#[derive(Clone)]
enum SurfacePatch {
    Layer { layer: OffscreenLayer },
    Mask { layer: Mask },
}

fn collect_surface_patches(
    ops: &[ExecOp],
    owner: RetainedNodeId,
    patches: &mut HashMap<RetainedSurfaceId, SurfacePatch>,
) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                retained_id,
                layer,
                children,
                ..
            } => {
                if let Some(id) = retained_id.filter(|id| id.node == owner) {
                    patches.insert(
                        id,
                        SurfacePatch::Layer {
                            layer: layer.clone(),
                        },
                    );
                }
                collect_surface_patches(children, owner, patches);
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                content,
                mask,
                ..
            } => {
                if let Some(id) = retained_id.filter(|id| id.node == owner) {
                    patches.insert(
                        id,
                        SurfacePatch::Mask {
                            layer: layer.clone(),
                        },
                    );
                }
                collect_surface_patches(content, owner, patches);
                collect_surface_patches(mask, owner, patches);
            }
            ExecOp::DrawBatch { .. } => {}
        }
    }
}

fn patch_surface_ops(
    ops: &mut [ExecOp],
    patches: &HashMap<RetainedSurfaceId, SurfacePatch>,
    patched: &mut usize,
) {
    for op in ops {
        match op {
            ExecOp::OffscreenLayer {
                retained_id,
                layer,
                children,
                ..
            } => {
                if let Some(SurfacePatch::Layer {
                    layer: replacement_layer,
                }) = retained_id.and_then(|id| patches.get(&id))
                {
                    *layer = replacement_layer.clone();
                    *patched += 1;
                }
                patch_surface_ops(children, patches, patched);
            }
            ExecOp::OffscreenMaskLayer {
                retained_id,
                layer,
                content,
                mask,
                ..
            } => {
                if let Some(SurfacePatch::Mask { layer: replacement }) =
                    retained_id.and_then(|id| patches.get(&id))
                {
                    *layer = replacement.clone();
                    *patched += 1;
                }
                patch_surface_ops(content, patches, patched);
                patch_surface_ops(mask, patches, patched);
            }
            ExecOp::DrawBatch { .. } => {}
        }
    }
}
