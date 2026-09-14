//! Scene preparation and scan/cumsum staging before GPU execution.

use super::*;

impl Renderer {
    pub(super) fn scan_and_cumsum(
        &mut self,
        commands: &mut WgpuCommandBatch,
        scene: &Canvas,
    ) -> bool {
        // Local offscreen resource sets have their own atlas allocations too.
        if !self.encode_vector_images(commands) {
            return false;
        }
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
                CoarseBatch {
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
        let mut commands = WgpuCommandBatch::new(&self.device, &self.queue, "tileink wgpu frame");
        let ok = self.encode_prepared_tile_plan(&mut commands, canvas, history_copy_dst, true);
        self.retained.stats_mut().queue_submissions = commands.finish_with_status(ok);
        ok
    }

    pub(super) fn encode_prepared_tile_plan(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        history_copy_dst: Option<&::wgpu::Texture>,
        allow_early_submit: bool,
    ) -> bool {
        let mut adapter =
            super::frame_adapter::WgpuFrameAdapter::new(self, commands, history_copy_dst);
        crate::render::frame::encode(
            &mut adapter,
            canvas,
            history_copy_dst.is_some(),
            allow_early_submit,
        )
        .is_ok()
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
