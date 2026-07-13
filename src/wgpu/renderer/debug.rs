//! Debug rendering entry points and scan-buffer readback.

use super::*;

impl Renderer {
    pub fn render_with_text(
        &mut self,
        canvas: &Canvas,
        font_system: &mut TextFontSystem,
        text_context: &mut TextContext,
    ) {
        assert!(
            self.render_native_with_text(canvas, font_system, text_context),
            "wgpu renderer could not render text scene natively"
        );
    }

    pub fn render_with_options(
        &mut self,
        canvas: &Canvas,
        options: &RenderOptions,
    ) -> RenderDebugCapture {
        self.retained.set_history_owner(HistoryOwner::Internal);
        self.retained.reset_transient_output();
        let mode = self
            .retained
            .replace_mode(super::super::incremental::IncrementalRenderMode::ForceFull);
        let selected = SelectedScene::Borrowed(canvas);
        let frame = selected.frame();
        let materialization = selected.materialization();
        let materialized_reused = selected.materialized_reused();
        let scene = selected.scene();
        let plan = self
            .retained
            .begin_frame(frame, scene, self.profiler.is_active());
        self.retained.stats_mut().materialized_scene_reused = materialized_reused;
        self.prepare_scene(scene);
        self.retained.mark_scene_prepared(materialization, false);
        let rendered_native = self.render_prepared_tile_plan(scene);
        self.retained.finish_frame(plan, rendered_native, true);
        self.retained.replace_mode(mode);
        if rendered_native {
            self.size = scene.physical_size();
            let image = self.image();
            let debug = self.read_debug_scan_buffers();
            return capture_render_debug(
                "wgpu",
                scene,
                &image,
                DebugScanBuffers {
                    backdrops: &debug.backdrops,
                    tile_segment_ranges: &debug.tile_segment_ranges,
                    segments: &debug.segments,
                },
                options,
            );
        }

        panic!("wgpu renderer could not render debug scene natively")
    }

    pub(super) fn read_debug_scan_buffers(&self) -> WgpuDebugScanReadback {
        let backdrops =
            self.scan
                .backdrops
                .read::<i32>(&self.device, &self.queue, self.lengths.backdrop_len);
        let tile_segment_ranges = self.scan.tile_segment_ranges.read::<TileSegmentRange>(
            &self.device,
            &self.queue,
            self.lengths.backdrop_len,
        );
        let segments = self.scan.segments.read::<LineSegment>(
            &self.device,
            &self.queue,
            self.lengths.segment_capacity,
        );
        WgpuDebugScanReadback {
            backdrops,
            tile_segment_ranges,
            segments,
        }
    }
}
