use super::*;

impl Canvas {
    pub(crate) fn compile(&self, list_id: CommandListId) -> ExecPlan {
        if list_id == ROOT_COMMAND_LIST_ID
            && let Some(plan) = &self.compiled_plan
        {
            return (**plan).clone();
        }
        self.compile_uncached(list_id)
    }

    pub(crate) fn compile_shared(&self, list_id: CommandListId) -> SharedRc<ExecPlan> {
        if list_id == ROOT_COMMAND_LIST_ID
            && let Some(plan) = &self.compiled_plan
        {
            return plan.clone();
        }
        SharedRc::new(self.compile_uncached(list_id))
    }

    pub(super) fn compile_uncached(&self, list_id: CommandListId) -> ExecPlan {
        let mut ops = Vec::new();
        let mut plan = ExecPlan {
            ops: Vec::new(),
            layer_stack_data: Vec::new(),
            draw_order: SharedRc::new(Vec::new()),
            draw_batch_ids: SharedRc::new(Vec::new()),
            retained_batch_ids: std::collections::HashMap::new(),
            layer_stack_locations: std::collections::HashMap::new(),
            direct_root_batch_ops: None,
        };
        let mut layer_stack = Vec::new();
        let mut surface_slots = std::collections::HashMap::new();
        self.compile_into(
            list_id,
            &mut ops,
            &mut plan,
            &mut layer_stack,
            CompileOwners {
                retained: self.persistent_root,
                batch: None,
            },
            &mut surface_slots,
        );
        plan.ops = ops;
        plan.coalesce_draw_batches();
        plan.finalize_draw_batches(self.draw_records.len());
        plan
    }

    /// Hashes the command topology and layer parameters that determine an
    /// [`ExecPlan`]. Geometry, brushes, and text data are intentionally absent:
    /// they live in scene buffers and can change without rebuilding execution
    /// control flow.
    pub(crate) fn execution_plan_fingerprint(&self) -> u64 {
        use std::{fmt::Write as _, hash::Hasher as _};

        if let Some(key) = self.plan_cache_key {
            return key;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for (list_ix, list) in self.command_lists.iter().enumerate() {
            hasher.write_usize(list_ix);
            hasher.write_usize(list.commands.len());
            for command in &list.commands {
                match command {
                    Command::Draw(draw) => {
                        hasher.write_u8(0);
                        hasher.write_usize(*draw);
                    }
                    Command::MaterializedRetainedScene { id, children, .. } => {
                        hasher.write_u8(1);
                        hasher.write_u64(id.owner);
                        hasher.write_u32(id.slot);
                        hasher.write_usize(*children);
                    }
                    Command::Layer {
                        retained,
                        draw,
                        layer,
                        children,
                    } => {
                        hasher.write_u8(2);
                        hasher.write_usize(*draw);
                        hasher.write_usize(*children);
                        if let Some(retained) = retained {
                            hasher.write_u64(retained.id.owner);
                            hasher.write_u32(retained.id.slot);
                        }
                        let _ = write!(HasherWriter(&mut hasher), "{layer:?}");
                    }
                    Command::MaskLayer {
                        retained,
                        layer,
                        content,
                        mask,
                    } => {
                        hasher.write_u8(3);
                        hasher.write_usize(*content);
                        hasher.write_usize(*mask);
                        if let Some(retained) = retained {
                            hasher.write_u64(retained.id.owner);
                            hasher.write_u32(retained.id.slot);
                        }
                        let _ = write!(HasherWriter(&mut hasher), "{layer:?}");
                    }
                }
            }
        }
        // Persistent keys use the high bit, keeping the two identity domains disjoint.
        hasher.finish() & !(1 << 63)
    }

    fn compile_into(
        &self,
        list_id: CommandListId,
        ops: &mut Vec<ExecOp>,
        plan: &mut ExecPlan,
        layer_stack: &mut Vec<LayerStackEntry>,
        owners: CompileOwners,
        surface_slots: &mut std::collections::HashMap<RetainedNodeId, u32>,
    ) {
        let CompileOwners {
            retained: retained_owner,
            batch: batch_owner,
        } = owners;
        let op_start = ops.len();
        let mut pending_batch = Vec::<usize>::new();

        let flush_batch = |pending_batch: &mut Vec<usize>,
                           ops: &mut Vec<ExecOp>,
                           plan: &mut ExecPlan,
                           layer_stack: &[LayerStackEntry],
                           batch_owner: Option<RetainedBatchOwner>| {
            if pending_batch.is_empty() {
                return;
            }
            let layer_start = plan.layer_stack_data.len();
            plan.layer_stack_data.extend_from_slice(layer_stack);
            let layer_end = plan.layer_stack_data.len();

            ops.push(ExecOp::DrawBatch {
                draws: SharedRc::new(std::mem::take(pending_batch)),
                batch_id: u32::MAX,
                owners: SharedRc::new(batch_owner.into_iter().collect()),
                layer_stack: layer_start..layer_end,
            });
        };

        for command in &self.command_lists[list_id].commands {
            match command {
                Command::Draw(draw_ix) => pending_batch.push(*draw_ix),
                Command::MaterializedRetainedScene {
                    id,
                    revision: _,
                    children,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
                    self.compile_into(
                        *children,
                        ops,
                        plan,
                        layer_stack,
                        CompileOwners {
                            retained: Some(*id),
                            batch: batch_owner,
                        },
                        surface_slots,
                    );
                }
                Command::Layer {
                    retained,
                    draw,
                    layer,
                    children,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
                    let retained_owner = retained.map(|key| key.id).or(retained_owner);
                    let child_batch_owner = retained
                        .map(|key| RetainedBatchOwner {
                            node: key.id,
                            branch: RetainedBatchBranch::Content,
                        })
                        .or(batch_owner);
                    if self.can_fuse(layer, *children) {
                        match layer {
                            Layer::Clip | Layer::ClipSdf { .. } => {
                                // SDF clips stay analytic by using their hidden
                                // draw record as the same layer-stack entry as
                                // path clips, instead of materializing a mask.
                                ops.push(ExecOp::BeginClip);
                                layer_stack.push(LayerStackEntry::Clip { draw: *draw as u32 });
                                self.compile_into(
                                    *children,
                                    ops,
                                    plan,
                                    layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                layer_stack.pop();
                                ops.push(ExecOp::EndClip);
                            }
                            Layer::Opacity(opacity) => {
                                ops.push(ExecOp::BeginOpacity);
                                layer_stack.push(LayerStackEntry::Opacity {
                                    draw: *draw as u32,
                                    opacity: opacity.opacity,
                                });
                                self.compile_into(
                                    *children,
                                    ops,
                                    plan,
                                    layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                layer_stack.pop();
                                ops.push(ExecOp::EndOpacity);
                            }
                            Layer::Blend(blend) => {
                                ops.push(ExecOp::BeginBlend);
                                layer_stack.push(LayerStackEntry::Blend {
                                    draw: *draw as u32,
                                    mode: blend.mode,
                                });
                                self.compile_into(
                                    *children,
                                    ops,
                                    plan,
                                    layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                layer_stack.pop();
                                ops.push(ExecOp::EndBlend);
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        let stack_start = plan.layer_stack_data.len();
                        plan.layer_stack_data.extend_from_slice(layer_stack);
                        let stack_end = plan.layer_stack_data.len();
                        ops.push(ExecOp::OffscreenLayer {
                            retained_id: retained_owner.map(|owner| {
                                let slot = surface_slots.entry(owner).or_default();
                                let id = RetainedSurfaceId::new(owner, *slot);
                                *slot += 1;
                                id
                            }),
                            draw: *draw,
                            layer: layer.clone(),
                            outer_stack: stack_start..stack_end,
                            children: {
                                let mut child_ops = Vec::new();
                                let mut child_layer_stack =
                                    if matches!(layer, Layer::Backdrop { .. }) {
                                        layer_stack.clone()
                                    } else {
                                        Vec::new()
                                    };
                                self.compile_into(
                                    *children,
                                    &mut child_ops,
                                    plan,
                                    &mut child_layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                child_ops
                            },
                        });
                    }
                }
                Command::MaskLayer {
                    retained,
                    layer,
                    content,
                    mask,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
                    let retained_owner = retained.map(|key| key.id).or(retained_owner);
                    let content_batch_owner = retained
                        .map(|key| RetainedBatchOwner {
                            node: key.id,
                            branch: RetainedBatchBranch::Content,
                        })
                        .or(batch_owner);
                    let mask_batch_owner = retained
                        .map(|key| RetainedBatchOwner {
                            node: key.id,
                            branch: RetainedBatchBranch::Mask,
                        })
                        .or(batch_owner);
                    let stack_start = plan.layer_stack_data.len();
                    plan.layer_stack_data.extend_from_slice(layer_stack);
                    let stack_end = plan.layer_stack_data.len();
                    ops.push(ExecOp::OffscreenMaskLayer {
                        retained_id: retained_owner.map(|owner| {
                            let slot = surface_slots.entry(owner).or_default();
                            let id = RetainedSurfaceId::new(owner, *slot);
                            *slot += 1;
                            id
                        }),
                        layer: layer.clone(),
                        outer_stack: stack_start..stack_end,
                        content: {
                            let mut child_ops = Vec::new();
                            let mut child_layer_stack = Vec::new();
                            self.compile_into(
                                *content,
                                &mut child_ops,
                                plan,
                                &mut child_layer_stack,
                                CompileOwners {
                                    retained: retained_owner,
                                    batch: content_batch_owner,
                                },
                                surface_slots,
                            );
                            child_ops
                        },
                        mask: {
                            let mut mask_ops = Vec::new();
                            let mut mask_layer_stack = Vec::new();
                            self.compile_into(
                                *mask,
                                &mut mask_ops,
                                plan,
                                &mut mask_layer_stack,
                                CompileOwners {
                                    retained: retained_owner,
                                    batch: mask_batch_owner,
                                },
                                surface_slots,
                            );
                            mask_ops
                        },
                    });
                }
            }
        }

        flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
        if let Some(owner) = batch_owner
            && !exec_ops_contain_owner(&ops[op_start..], owner)
        {
            let layer_start = plan.layer_stack_data.len();
            plan.layer_stack_data.extend_from_slice(layer_stack);
            let layer_end = plan.layer_stack_data.len();
            ops.push(ExecOp::DrawBatch {
                draws: SharedRc::new(Vec::new()),
                batch_id: u32::MAX,
                owners: SharedRc::new(vec![owner]),
                layer_stack: layer_start..layer_end,
            });
        }
    }

    pub(super) fn can_fuse(&self, layer: &Layer, children: CommandListId) -> bool {
        match layer {
            Layer::Clip => true,
            Layer::ClipSdf { .. } => true,
            // Group opacity and blend must wrap the composited child subtree.
            // If a child opens its own offscreen layer, keeping the group fused
            // would apply it to separate fragments before they are combined.
            Layer::Opacity(_) | Layer::Blend(_) => !self.command_list_contains_offscreen(children),
            _ => false,
        }
    }

    pub(super) fn command_list_contains_offscreen(&self, list_id: CommandListId) -> bool {
        self.command_lists[list_id]
            .commands
            .iter()
            .any(|command| match command {
                Command::Draw(_) => false,
                Command::MaterializedRetainedScene { children, .. } => {
                    self.command_list_contains_offscreen(*children)
                }
                Command::Layer {
                    layer, children, ..
                } => {
                    !matches!(
                        layer,
                        Layer::Clip | Layer::ClipSdf { .. } | Layer::Opacity(_) | Layer::Blend(_)
                    ) || self.command_list_contains_offscreen(*children)
                }
                Command::MaskLayer { .. } => true,
            })
    }
}
#[derive(Clone, Copy)]
struct CompileOwners {
    retained: Option<RetainedNodeId>,
    batch: Option<RetainedBatchOwner>,
}

fn exec_ops_contain_owner(ops: &[ExecOp], owner: RetainedBatchOwner) -> bool {
    ops.iter().any(|op| match op {
        ExecOp::DrawBatch { owners, .. } => owners.contains(&owner),
        ExecOp::OffscreenLayer { children, .. } => exec_ops_contain_owner(children, owner),
        ExecOp::OffscreenMaskLayer { content, mask, .. } => {
            exec_ops_contain_owner(content, owner) || exec_ops_contain_owner(mask, owner)
        }
        _ => false,
    })
}
struct HasherWriter<'a, H>(&'a mut H);

impl<H: std::hash::Hasher> std::fmt::Write for HasherWriter<'_, H> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.write(value.as_bytes());
        Ok(())
    }
}
