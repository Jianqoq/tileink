//! Scene preparation and scan/cumsum staging before GPU execution.

use super::*;

impl Renderer {
    pub(super) fn scan_and_cumsum(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
    ) -> bool {
        let (Some(scan), Some(cumsum)) = (&self.scan_pipeline, &self.cumsum) else {
            return false;
        };
        self.coarse
            .encode_pending_tile_bin_copies(commands.encoder());
        let active = self
            .retained
            .active_tiles()
            .map(|damage| ActiveScanPlan::new(scene, damage, self.scene_upload.scan_ranges()));
        let stats = self.retained.stats_mut();
        if let Some(active) = &active {
            self.scan
                .upload_active_indices(&self.device, &self.queue, &active.indices);
            self.scan
                .upload_active_cumsum_plan(&self.device, &self.queue, &active.cumsum);
            stats.scanned_paths += active.path_count;
            stats.scanned_lines += active.line_count;
            stats.scan_chunks += active.chunk_count;
        } else {
            stats.scanned_paths += self.lengths.path_count as u32;
            stats.scanned_lines += self.lengths.line_count as u32;
            stats.scan_chunks += self.lengths.scan_chunk_count as u32;
        }
        scan.run_in(
            commands,
            &self.scene_buffers,
            &mut self.scan,
            self.lengths,
            active.as_ref(),
        );
        cumsum.run_in(
            commands,
            &self.scene_buffers,
            &mut self.scan,
            self.lengths,
            active.as_ref(),
        );
        true
    }

    #[cfg(test)]
    pub(super) fn coarse_batch(
        &mut self,
        _canvas: &Canvas,
        draw_start: u32,
        draw_end: u32,
        layer_stack_start: u32,
        layer_stack_end: u32,
    ) {
        if let Some(coarse) = &self.coarse_pipeline {
            coarse.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &self.scan,
                &mut self.coarse,
                self.lengths,
                WgpuCoarseBatch {
                    draw_start,
                    draw_end,
                    layer_stack_start,
                    layer_stack_end,
                    active_tile_count: None,
                },
            );
        }
    }

    #[cfg(test)]
    pub(super) fn scan_for_test(&mut self) {
        if let Some(scan) = &self.scan_pipeline {
            scan.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        }
    }

    #[cfg(test)]
    pub(super) fn cumsum_for_test(&mut self) {
        if let Some(cumsum) = &self.cumsum {
            cumsum.run(
                &self.device,
                &self.queue,
                &self.scene_buffers,
                &mut self.scan,
                self.lengths,
            );
        }
    }

    pub(super) fn render_prepared_tile_plan(&mut self, canvas: &Canvas) -> bool {
        self.render_prepared_tile_plan_with_history_copy(canvas, None)
    }

    pub(super) fn render_prepared_tile_plan_with_history_copy(
        &mut self,
        canvas: &Canvas,
        history_copy_dst: Option<&::wgpu::Texture>,
    ) -> bool {
        // The preceding render call submitted its frame command batch before returning. Its
        // quarantined local allocations can now safely receive queue writes ordered after it.
        self.begin_local_scene_resource_frame();
        if self
            .retained
            .active_tiles()
            .is_some_and(DamageTiles::is_empty)
        {
            if let Some(dst) = history_copy_dst {
                let mut commands =
                    WgpuCommandBatch::new(&self.device, &self.queue, "tileink wgpu frame");
                self.encode_history_copy(&mut commands, dst);
                self.retained.stats_mut().queue_submissions = commands.finish();
            }
            return true;
        }
        if self.fine.is_none() || self.coarse_pipeline.is_none() || self.filter.is_none() {
            return false;
        }
        let Some(plan) = self.plan.clone() else {
            return false;
        };

        if let Some(filter) = &self.filter {
            filter.reset_dispatch_counts();
        }
        self.filter_tile_work_arena.reset();
        self.prepare_active_tile_buffers();

        let mut commands = WgpuCommandBatch::new(&self.device, &self.queue, "tileink wgpu frame");
        // Start substantial native full frames before CPU encoding/completion reaches the
        // end of the plan. A first-quarter submission overlaps real coarse/fine GPU work;
        // scan alone is too small to amortize another submit. Keep small/partial frames and
        // the portable ping-pong path on their existing single-submission schedule.
        if self.retained.active_tiles().is_none()
            && !self.fine.as_ref().unwrap().uses_portable_textures()
            && u64::from(self.size.0) * u64::from(self.size.1) >= 1024 * 1024
        {
            let root_batches = execute::root_draw_batch_count(
                canvas,
                &plan.ops,
                Bounds::canvas(self.size.0, self.size.1),
            );
            if root_batches >= 16 {
                commands.set_initial_root_batch_budget(root_batches / 4);
            }
        }
        if !self.scan_and_cumsum(&mut commands, canvas) {
            self.retained.stats_mut().queue_submissions = commands.finish();
            return false;
        }
        if self.retained.active_tiles().is_some() {
            self.clear_render_region(
                &mut commands,
                WgpuRenderTargetId::Main,
                Bounds::canvas(self.size.0, self.size.1),
                self.clear_color,
            );
        } else {
            self.clear_render_target(&mut commands, WgpuRenderTargetId::Main, self.clear_color);
        }
        let draw_batch_ids = canvas
            .stable_batch_ids
            .as_deref()
            .unwrap_or(&plan.draw_batch_ids);
        let active_batches = profile_cpu("plan.active_batches", || {
            self.retained.active_tiles().map(|tiles| {
                self.scene_upload
                    .active_batch_ids(tiles.list(), draw_batch_ids)
            })
        });
        let mut filter_cursors = WgpuFilterCursors::default();
        let ok = profile_cpu("plan.execute", || {
            let direct_ops = active_batches.as_ref().map_or_else(
                || plan.all_direct_root_ops(),
                |active| plan.active_direct_root_ops(active),
            );
            if let Some(ops) = direct_ops {
                self.execute_direct_root_batches(&mut commands, canvas, &plan, &ops)
            } else {
                self.execute_ops(
                    &mut commands,
                    canvas,
                    &plan,
                    &plan.ops,
                    WgpuRenderTargetId::Main,
                    &mut filter_cursors,
                    active_batches.as_deref(),
                )
            }
        });
        if ok && let Some(dst) = history_copy_dst {
            self.encode_history_copy(&mut commands, dst);
        }
        self.retained.stats_mut().queue_submissions = commands.finish();
        if let Some(filter) = &self.filter {
            let (dispatches, compact_dispatches) = filter.dispatch_counts();
            let stats = self.retained.stats_mut();
            stats.filter_dispatches = dispatches;
            stats.compact_filter_dispatches = compact_dispatches;
        }
        ok
    }

    pub(super) fn encode_history_copy(
        &self,
        commands: &mut WgpuCommandBatch,
        dst: &::wgpu::Texture,
    ) {
        let _profile_scope = start_cpu_scope("history.copy");
        copy_texture(
            commands.encoder(),
            self.readback_target.texture(),
            dst,
            self.size,
        );
    }
}
