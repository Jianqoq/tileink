use super::*;
mod layers;
use crate::shared::execution::OffscreenLayer;
use layers::LayerExecution;

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
            return std::rc::Rc::clone(plan);
        }
        SharedRc::new(self.compile_uncached(list_id))
    }

    pub(super) fn compile_uncached(&self, list_id: CommandListId) -> ExecPlan {
        self.compile_commands(&self.command_lists[list_id].commands, Vec::new())
    }

    /// Compile a retained fragment directly, preserving its ancestor clip stack without
    /// allocating a synthetic root list in persistent command storage.
    pub(crate) fn compile_command(
        &self,
        command: &Command,
        layer_stack: Vec<LayerStackEntry>,
    ) -> ExecPlan {
        self.compile_commands(std::slice::from_ref(command), layer_stack)
    }

    fn compile_commands(
        &self,
        commands: &[Command],
        mut layer_stack: Vec<LayerStackEntry>,
    ) -> ExecPlan {
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
        let mut surface_slots = std::collections::HashMap::new();
        self.compile_into(
            commands,
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
                    Command::MaterializedRetainedScene { id, fragment, .. } => {
                        hasher.write_u8(1);
                        hasher.write_u64(id.owner);
                        hasher.write_u32(id.slot);
                        hasher.write_usize(fragment.start);
                        hasher.write_usize(fragment.end);
                    }
                    Command::Layer {
                        retained,
                        layer,
                        children,
                    } => {
                        hasher.write_u8(2);
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
        commands: &[Command],
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

        for command in commands {
            match command {
                Command::Draw(draw_ix) => pending_batch.push(*draw_ix),
                Command::MaterializedRetainedScene { id, fragment, .. } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
                    self.compile_into(
                        &self.command_lists[fragment.start].commands,
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
                    match self.compile_layer(layer, *children) {
                        LayerExecution::Fused(entry) => {
                            // Every fused group has the same traversal. Hidden SDF/path clip
                            // draws remain analytic through the batch's stack snapshot.
                            layer_stack.push(entry);
                            self.compile_into(
                                &self.command_lists[*children].commands,
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
                        }
                        LayerExecution::Offscreen(layer) => {
                            let stack_start = plan.layer_stack_data.len();
                            plan.layer_stack_data.extend_from_slice(layer_stack);
                            let stack_end = plan.layer_stack_data.len();
                            let inherit_stack = matches!(layer, OffscreenLayer::Backdrop { .. });
                            ops.push(ExecOp::OffscreenLayer {
                                retained_id: retained_owner.map(|owner| {
                                    let slot = surface_slots.entry(owner).or_default();
                                    let id = RetainedSurfaceId::new(owner, *slot);
                                    *slot += 1;
                                    id
                                }),
                                layer,
                                outer_stack: stack_start..stack_end,
                                children: {
                                    let mut child_ops = Vec::new();
                                    let mut child_layer_stack = if inherit_stack {
                                        layer_stack.clone()
                                    } else {
                                        Vec::new()
                                    };
                                    self.compile_into(
                                        &self.command_lists[*children].commands,
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
                                &self.command_lists[*content].commands,
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
                                &self.command_lists[*mask].commands,
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

    pub(crate) fn can_fuse(&self, layer: &Layer, children: CommandListId) -> bool {
        match layer {
            Layer::Clip { .. } | Layer::ClipSdf { .. } => true,
            // Fine rasterization opens/closes a fused group per draw batch. A nested layer
            // changes the stack and splits batches, so the group must isolate the entire
            // composited subtree instead of applying alpha/blend to each fragment separately.
            Layer::Opacity { .. } | Layer::Blend { .. } => {
                self.command_list_has_single_stack(children, children, &mut None)
            }
            _ => false,
        }
    }

    fn command_list_has_single_stack(
        &self,
        list: CommandListId,
        stack: CommandListId,
        first: &mut Option<CommandListId>,
    ) -> bool {
        self.command_lists[list]
            .commands
            .iter()
            .all(|command| match command {
                Command::Draw(_) => *first.get_or_insert(stack) == stack,
                Command::MaterializedRetainedScene { fragment, .. } => {
                    self.command_list_has_single_stack(fragment.start, stack, first)
                }
                Command::Layer {
                    layer:
                        Layer::Clip { .. }
                        | Layer::ClipSdf { .. }
                        | Layer::Opacity { .. }
                        | Layer::Blend { .. },
                    children,
                    ..
                } => self.command_list_has_single_stack(*children, *children, first),
                Command::Layer { .. } | Command::MaskLayer { .. } => false,
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
    })
}
struct HasherWriter<'a, H>(&'a mut H);

impl<H: std::hash::Hasher> std::fmt::Write for HasherWriter<'_, H> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.write(value.as_bytes());
        Ok(())
    }
}
