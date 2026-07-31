//! Direct draw-batch execution and fine/coarse dispatch orchestration.

use super::*;

enum PreparedDrawBatch {
    Skipped,
    Encoded,
    Unavailable,
}

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
        profile_cpu("plan.draw_batch", || {
            match self.prepare_draw_batch(commands, scene, draws, batch_id, layer_stack, target) {
                PreparedDrawBatch::Skipped => true,
                PreparedDrawBatch::Encoded => self.fine_batch_to_in(commands, target),
                PreparedDrawBatch::Unavailable => false,
            }
        })
    }

    fn prepare_draw_batch(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
        draws: &[usize],
        batch_id: u32,
        layer_stack: std::ops::Range<usize>,
        target: WgpuRenderTargetId,
    ) -> PreparedDrawBatch {
        let live = scene
            .stable_batch_counts
            .as_ref()
            .map_or(!draws.is_empty(), |counts| {
                counts.get(batch_id as usize).copied().unwrap_or(0) != 0
            });
        if !live {
            return PreparedDrawBatch::Skipped;
        }
        let stats = self.retained.stats_mut();
        stats.draw_batches = stats.draw_batches.saturating_add(1);
        if target == WgpuRenderTargetId::Main {
            stats.root_draw_batches = stats.root_draw_batches.saturating_add(1);
        }
        if self.encode_coarse_batch(
            commands,
            batch_id,
            batch_id.saturating_add(1),
            layer_stack.start as u32,
            layer_stack.end as u32,
        ) {
            PreparedDrawBatch::Encoded
        } else {
            PreparedDrawBatch::Unavailable
        }
    }

    pub(super) fn execute_direct_root_batches(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> bool {
        if self
            .fine
            .as_ref()
            .is_some_and(WgpuFinePipeline::uses_portable_textures)
        {
            return self.execute_direct_root_batches_portable(commands, canvas, plan, ops);
        }
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

    fn execute_direct_root_batches_portable(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> bool {
        if ops.is_empty() {
            return true;
        }
        let Some(root) = self
            .render_target_texture(WgpuRenderTargetId::Main)
            .cloned()
        else {
            return false;
        };
        self.fine_portable_source
            .resize(commands.device(), self.size.0, self.size.1);
        self.fine_portable_target
            .resize(commands.device(), self.size.0, self.size.1);
        copy_texture(
            commands.encoder(),
            &root,
            self.fine_portable_source.texture(),
            self.size,
        );
        self.retained.stats_mut().portable_texture_copies += 1;

        let partial = self.retained.active_tiles().is_some();
        if partial {
            copy_texture(
                commands.encoder(),
                &root,
                self.fine_portable_target.texture(),
                self.size,
            );
            self.retained.stats_mut().portable_texture_copies += 1;
        }

        let mut latest_is_source = true;
        let mut encoded_any = false;
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
            let prepared = profile_cpu("plan.draw_batch", || {
                self.prepare_draw_batch(
                    commands,
                    canvas,
                    draws,
                    *batch_id,
                    layer_stack.clone(),
                    WgpuRenderTargetId::Main,
                )
            });
            match prepared {
                PreparedDrawBatch::Skipped => continue,
                PreparedDrawBatch::Unavailable => return false,
                PreparedDrawBatch::Encoded => {}
            }
            let ok = if latest_is_source {
                self.fine_portable_batch_to_views_in(
                    commands,
                    self.fine_portable_source.view(),
                    self.fine_portable_target.view(),
                )
            } else {
                self.fine_portable_batch_to_views_in(
                    commands,
                    self.fine_portable_target.view(),
                    self.fine_portable_source.view(),
                )
            };
            if !ok {
                return false;
            }
            latest_is_source = !latest_is_source;
            encoded_any = true;
        }

        if encoded_any {
            let latest = if latest_is_source {
                self.fine_portable_source.texture()
            } else {
                self.fine_portable_target.texture()
            };
            copy_texture(commands.encoder(), latest, &root, self.size);
            self.retained.stats_mut().portable_texture_copies += 1;
        }
        true
    }

    #[cfg(test)]
    pub(super) fn coarse_and_fine_batch_to(
        &mut self,
        commands: &mut WgpuCommandBatch,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
        target: WgpuRenderTargetId,
    ) -> bool {
        self.encode_coarse_batch(
            commands,
            draw_start,
            draw_end,
            layer_stack_start,
            layer_stack_end,
        ) && self.fine_batch_to_in(commands, target)
    }

    fn encode_coarse_batch(
        &mut self,
        commands: &mut WgpuCommandBatch,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
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
        true
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
        let Some(target_texture) = self.render_target_texture(target).cloned() else {
            return false;
        };
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
        self.retained.stats_mut().portable_texture_copies += 1;
        let ok = self.fine_portable_batch_to_views_in(
            commands,
            self.fine_portable_source.view(),
            self.fine_portable_target.view(),
        );
        if ok {
            copy_texture(
                commands.encoder(),
                self.fine_portable_target.texture(),
                &target_texture,
                self.size,
            );
            self.retained.stats_mut().portable_texture_copies += 1;
        }
        ok
    }

    fn fine_portable_batch_to_views_in(
        &self,
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
    ) -> bool {
        let Some(fine) = &self.fine else {
            return false;
        };
        fine.render_tiles_to_views_in(
            commands,
            self.size.0,
            self.size.1,
            self.lengths,
            &self.scene_buffers,
            &self.scan,
            &self.coarse,
            &self.fine_spills,
            &self.fine_indirect_args,
            source,
            target,
            self.clear_color,
            true,
            self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
            self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
            self.retained.active_tiles().map(DamageTiles::len),
        )
    }
}
