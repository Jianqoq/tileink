//! Direct draw-batch execution and fine/coarse dispatch orchestration.

use super::execution_adapter::WgpuExecutionAdapter;
use crate::render::draw_batches::{self as shared_draw_batches, RootBatchMode};

use super::*;

impl Renderer {
    pub(super) fn execute_ops(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        target: RenderTargetId,
        filter_cursors: &mut FilterCursors,
        active_batches: Option<&[u32]>,
    ) -> bool {
        crate::render::operations::execute_ops(
            &mut WgpuExecutionAdapter::new(self, commands),
            canvas,
            plan,
            ops,
            target,
            filter_cursors,
            active_batches,
        )
        .is_ok()
    }

    pub(super) fn execute_direct_root_batches(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[usize],
    ) -> bool {
        let mode = if self
            .fine
            .as_ref()
            .is_some_and(WgpuFinePipeline::uses_portable_textures)
        {
            RootBatchMode::Portable {
                partial: self.retained.active_tiles().is_some(),
            }
        } else {
            RootBatchMode::Direct
        };
        shared_draw_batches::execute_direct_root_batches(
            &mut WgpuExecutionAdapter::new(self, commands),
            canvas,
            &plan.ops,
            ops,
            mode,
        )
        .is_ok()
    }

    #[cfg(test)]
    pub(super) fn coarse_and_fine_batch_to(
        &mut self,
        commands: &mut WgpuCommandBatch,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
        target: RenderTargetId,
    ) -> bool {
        self.encode_coarse_batch(
            commands,
            draw_start,
            draw_end,
            layer_stack_start,
            layer_stack_end,
        ) && self.fine_batch_to_in(commands, target)
    }

    pub(super) fn encode_coarse_batch(
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
        target: RenderTargetId,
    ) -> bool {
        let Some(fine) = &self.fine else {
            return false;
        };
        let active_tile_count = self.retained.active_tiles().map(DamageTiles::len);
        if fine.uses_portable_textures() {
            return self.fine_portable_batch_to_in(commands, target);
        }
        match target {
            RenderTargetId::Main => {
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
                        &mut self.readback_target,
                        self.clear_color,
                        true,
                        self.max_clip_depth.saturating_sub(FINE_LOCAL_CLIP_DEPTH) as u32,
                        self.max_group_depth.saturating_sub(FINE_LOCAL_GROUP_DEPTH) as u32,
                        active_tile_count,
                    )
                }
            }
            RenderTargetId::Scratch(ix) => fine.render_tiles_in(
                commands,
                self.size.0,
                self.size.1,
                self.lengths,
                &self.scene_buffers,
                &self.scan,
                &self.coarse,
                &self.fine_spills,
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
        target: RenderTargetId,
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
            if let Some(active) = self.retained.active_tiles() {
                // Fine writes only active tiles. The remaining scratch pixels are
                // unspecified, so copying them back corrupts retained history.
                // Publish exactly the written regions, including clipped edge tiles.
                let regions = active.coalesced_rects(self.size);
                for &bounds in &regions {
                    copy_texture_region(
                        commands.encoder(),
                        self.fine_portable_target.texture(),
                        &target_texture,
                        self.size,
                        bounds,
                    );
                }
                self.retained.stats_mut().portable_texture_copies += regions.len() as u32;
            } else {
                copy_texture(
                    commands.encoder(),
                    self.fine_portable_target.texture(),
                    &target_texture,
                    self.size,
                );
                self.retained.stats_mut().portable_texture_copies += 1;
            }
        }
        ok
    }

    pub(super) fn fine_portable_batch_to_views_in(
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
