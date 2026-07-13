//! Direct draw-batch execution and fine/coarse dispatch orchestration.

use super::*;

impl Renderer {
    pub(super) fn execute_ops(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
        active_batches: Option<&[u32]>,
    ) -> bool {
        for op in ops {
            let ok = match op {
                ExecOp::DrawBatch {
                    draws,
                    batch_id,
                    layer_stack,
                    ..
                } => {
                    active_batches.is_some_and(|active| active.binary_search(batch_id).is_err())
                        || self.execute_draw_batch(
                            commands,
                            canvas,
                            draws,
                            *batch_id,
                            layer_stack.clone(),
                            target,
                        )
                }
                ExecOp::BeginClip
                | ExecOp::EndClip
                | ExecOp::BeginOpacity
                | ExecOp::EndOpacity
                | ExecOp::BeginBlend
                | ExecOp::EndBlend => true,
                ExecOp::OffscreenLayer {
                    retained_id,
                    draw,
                    layer,
                    outer_stack,
                    children,
                } => self.execute_offscreen_layer(
                    commands,
                    *retained_id,
                    canvas,
                    plan,
                    *draw,
                    layer,
                    outer_stack.clone(),
                    children,
                    target,
                    filter_cursors,
                ),
                ExecOp::OffscreenMaskLayer {
                    retained_id,
                    layer,
                    outer_stack,
                    content,
                    mask,
                } => self.execute_mask_layer(
                    commands,
                    *retained_id,
                    canvas,
                    plan,
                    layer,
                    outer_stack.clone(),
                    content,
                    mask,
                    target,
                    filter_cursors,
                ),
            };
            if !ok {
                return false;
            }
        }
        true
    }

    pub(super) fn execute_draw_batch(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
        draws: &[usize],
        batch_id: u32,
        layer_stack: std::ops::Range<usize>,
        target: WgpuRenderTargetId,
    ) -> bool {
        let live = scene
            .stable_batch_counts
            .as_ref()
            .map_or(!draws.is_empty(), |counts| {
                counts.get(batch_id as usize).copied().unwrap_or(0) != 0
            });
        if !live {
            return true;
        }
        let stats = self.retained.stats_mut();
        stats.draw_batches = stats.draw_batches.saturating_add(1);
        if target == WgpuRenderTargetId::Main {
            stats.root_draw_batches = stats.root_draw_batches.saturating_add(1);
        }
        profile_cpu("plan.draw_batch", || {
            self.coarse_and_fine_batch_to(
                commands,
                batch_id,
                batch_id.saturating_add(1),
                layer_stack.start as u32,
                layer_stack.end as u32,
                target,
            )
        })
    }

    pub(super) fn execute_direct_root_batches(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> bool {
        for &index in ops {
            let ExecOp::DrawBatch {
                draws,
                batch_id,
                layer_stack,
                ..
            } = &plan.ops[index]
            else {
                unreachable!("direct root index points to a draw batch")
            };
            if !self.execute_draw_batch(
                commands,
                canvas,
                draws,
                *batch_id,
                layer_stack.clone(),
                WgpuRenderTargetId::Main,
            ) {
                return false;
            }
        }
        true
    }

    pub(super) fn coarse_and_fine_batch_to(
        &mut self,
        commands: &mut WgpuCommandBatch,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
        target: WgpuRenderTargetId,
    ) -> bool {
        if self.coarse_pipeline.is_none() || self.fine.is_none() {
            return false;
        }
        let binning_stats = self
            .retained
            .active_tiles()
            .map(|active| self.scene_upload.coarse_binning_stats(active.list()));
        let active_tile_count = binning_stats.map(|stats| stats.active_tiles);
        let dense =
            binning_stats.is_some_and(|stats| match self.retained.config().coarse_binning {
                CoarseBinningMode::Auto => prefer_dense_binning(self.lengths, stats),
                CoarseBinningMode::ForceCompact => false,
                CoarseBinningMode::ForceDense => stats.active_tiles != 0,
            });
        if dense {
            self.retained.stats_mut().dense_coarse_batches += 1;
        }
        let batch = WgpuCoarseBatch {
            draw_start,
            draw_end,
            layer_stack_start,
            layer_stack_end,
            active_tile_count: if dense { None } else { active_tile_count },
        };
        self.coarse_pipeline.as_ref().unwrap().encode_in(
            commands,
            &self.scene_buffers,
            &self.scan,
            &mut self.coarse,
            self.lengths,
            batch,
        );
        self.fine_batch_to_in(commands, target)
    }

    pub(super) fn fine_batch_to_in(
        &mut self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
    ) -> bool {
        let Some(fine) = &self.fine else {
            return false;
        };
        let active_tile_count = self.retained.active_tiles().map(DamageTiles::len);
        if fine.uses_portable_textures() {
            return self.fine_portable_batch_to_in(commands, target);
        }
        match target {
            WgpuRenderTargetId::Main => {
                if let Some(target) = &self.root_target_view {
                    fine.render_tiles_to_view_in(
                        commands,
                        self.size.0,
                        self.size.1,
                        self.lengths,
                        &self.scene_buffers,
                        &self.scan,
                        &self.coarse,
                        &self.fine_spills,
                        &self.fine_indirect_args,
                        target,
                        self.clear_color,
                        true,
                        self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                        self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
                        active_tile_count,
                    )
                } else {
                    fine.render_tiles_in(
                        commands,
                        self.size.0,
                        self.size.1,
                        self.lengths,
                        &self.scene_buffers,
                        &self.scan,
                        &self.coarse,
                        &self.fine_spills,
                        &self.fine_indirect_args,
                        &mut self.readback_target,
                        self.clear_color,
                        true,
                        self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                        self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
                        active_tile_count,
                    )
                }
            }
            WgpuRenderTargetId::Scratch(ix) => fine.render_tiles_in(
                commands,
                self.size.0,
                self.size.1,
                self.lengths,
                &self.scene_buffers,
                &self.scan,
                &self.coarse,
                &self.fine_spills,
                &self.fine_indirect_args,
                &mut self.scratch[ix],
                self.clear_color,
                true,
                self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
                active_tile_count,
            ),
        }
    }

    pub(super) fn fine_portable_batch_to_in(
        &mut self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
    ) -> bool {
        let Some(fine) = &self.fine else {
            return false;
        };
        let Some(target_texture) = self.render_target_texture(target).cloned() else {
            return false;
        };
        let active_tile_count = self.retained.active_tiles().map(DamageTiles::len);
        self.fine_portable_source
            .resize(commands.device(), self.size.0, self.size.1);
        self.fine_portable_target
            .resize(commands.device(), self.size.0, self.size.1);
        copy_texture(
            commands.encoder(),
            &target_texture,
            self.fine_portable_source.texture(),
            self.size,
        );
        let ok = fine.render_tiles_to_views_in(
            commands,
            self.size.0,
            self.size.1,
            self.lengths,
            &self.scene_buffers,
            &self.scan,
            &self.coarse,
            &self.fine_spills,
            &self.fine_indirect_args,
            self.fine_portable_source.view(),
            self.fine_portable_target.view(),
            self.clear_color,
            true,
            self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
            self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
            active_tile_count,
        );
        if ok {
            copy_texture(
                commands.encoder(),
                self.fine_portable_target.texture(),
                &target_texture,
                self.size,
            );
        }
        ok
    }
}
