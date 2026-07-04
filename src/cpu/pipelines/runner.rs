use std::ops::Range;

use crate::{
    cpu::{
        buffers::RasterBuffers,
        pipelines::{
            coarse::CoarseCpuPipeline, cumsum::CumsumCpuPipeline, fine::FineCpuPipeline,
            scan::ScanCpuPipeline,
        },
    },
    shared::{
        bounds::Bounds, draw_record::DrawRecord, execution::LayerStackEntry, image::Image,
        image_resource::ImageResourceStore,
    },
    text::PreparedTextData,
};

pub(in crate::cpu) struct CoarseStage<'a> {
    pub(in crate::cpu) draw_records: &'a [DrawRecord],
    pub(in crate::cpu) draw_range: Range<usize>,
    pub(in crate::cpu) layer_stack_data: &'a [LayerStackEntry],
    pub(in crate::cpu) layer_stack_range: Range<usize>,
    pub(in crate::cpu) text: Option<&'a PreparedTextData>,
}

pub(in crate::cpu) fn run_scan(
    scan: &ScanCpuPipeline,
    canvas: &crate::canvas::Canvas,
    buffers: &mut RasterBuffers,
) {
    buffers.rebuild_tile_draw_bins(canvas);
    let Some(last_path_record) = canvas.path_records.last().copied() else {
        buffers.clear_scan_outputs();
        return;
    };
    buffers.resize_scan_outputs(canvas, last_path_record);
    scan.prepare(
        &canvas.lines,
        &canvas.path_records,
        &mut buffers.backdrops,
        &mut buffers.tile_segment_ranges,
        &mut buffers.segments,
        &mut buffers.segments_bump,
        &mut buffers.segment_tile_counts,
        &mut buffers.segment_tile_cursors,
        (canvas.width_in_tiles(), canvas.height_in_tiles()),
    )
    .run();
}

pub(in crate::cpu) fn run_cumsum(
    cumsum: &CumsumCpuPipeline,
    canvas: &crate::canvas::Canvas,
    buffers: &mut RasterBuffers,
) {
    cumsum
        .prepare(&mut buffers.backdrops, &canvas.path_records)
        .run();
}

pub(in crate::cpu) fn run_coarse(
    coarse: &CoarseCpuPipeline,
    canvas: &crate::canvas::Canvas,
    stage: CoarseStage<'_>,
    buffers: &mut RasterBuffers,
) {
    coarse
        .prepare(
            stage.draw_records,
            &canvas.brush_blob,
            &canvas.sdf_blob,
            &canvas.sdf_shadow_blob,
            stage.draw_range,
            stage.layer_stack_data,
            stage.layer_stack_range,
            &buffers.tile_draw_bins,
            &canvas.path_records,
            &buffers.backdrops,
            &buffers.tile_segment_ranges,
            &mut buffers.tile_ptcl_ranges,
            &mut buffers.tile_ptcls,
            &mut buffers.tile_glyphs,
            (canvas.width_in_tiles(), canvas.height_in_tiles()),
            stage.text,
        )
        .run();
}

pub(in crate::cpu) fn run_fine(
    fine: &FineCpuPipeline,
    canvas: &crate::canvas::Canvas,
    target: &mut Image,
    target_bounds: Bounds,
    buffers: &RasterBuffers,
    text: Option<&PreparedTextData>,
    image_resources: Option<&ImageResourceStore>,
) {
    fine.prepare(
        &buffers.tile_ptcl_ranges,
        &buffers.tile_ptcls,
        &buffers.tile_glyphs,
        &buffers.segments,
        target,
        target_bounds,
        (canvas.width_in_tiles(), canvas.height_in_tiles()),
        text,
        image_resources,
    )
    .run();
}
