use std::ops::Range;

use crate::{
    cpu::{
        buffers::RasterBuffers,
        pipelines::{
            coarse::CoarseCpuPipeline, cumsum::CumsumCpuPipeline, fine::FineCpuPipeline,
            scan::ScanCpuPipeline,
        },
    },
    shared::{bounds::Bounds, draw_record::DrawRecord, execution::LayerStackEntry, image::Image},
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
    scene: &crate::scene::Scene,
    buffers: &mut RasterBuffers,
) {
    let Some(last_bd_record) = scene.bd_records.last().copied() else {
        buffers.clear_scan_outputs();
        return;
    };
    buffers.resize_scan_outputs(scene, last_bd_record);
    scan.prepare(
        &scene.lines,
        &scene.path_records,
        &scene.bd_records,
        &mut buffers.backdrops,
        &mut buffers.tile_segment_ranges,
        &mut buffers.segments,
        &mut buffers.segments_bump,
        &mut buffers.segment_tile_counts,
        &mut buffers.segment_tile_cursors,
        (scene.width_in_tiles(), scene.height_in_tiles()),
    )
    .run();
}

pub(in crate::cpu) fn run_cumsum(
    cumsum: &CumsumCpuPipeline,
    scene: &crate::scene::Scene,
    buffers: &mut RasterBuffers,
) {
    cumsum
        .prepare(&mut buffers.backdrops, &scene.bd_records)
        .run();
}

pub(in crate::cpu) fn run_coarse(
    coarse: &CoarseCpuPipeline,
    scene: &crate::scene::Scene,
    stage: CoarseStage<'_>,
    buffers: &mut RasterBuffers,
) {
    coarse
        .prepare(
            stage.draw_records,
            stage.draw_range,
            stage.layer_stack_data,
            stage.layer_stack_range,
            &scene.bd_records,
            &buffers.backdrops,
            &buffers.tile_segment_ranges,
            &mut buffers.tile_ptcl_ranges,
            &mut buffers.tile_ptcls,
            &mut buffers.tile_glyphs,
            (scene.width_in_tiles(), scene.height_in_tiles()),
            stage.text,
        )
        .run();
}

pub(in crate::cpu) fn run_fine(
    fine: &FineCpuPipeline,
    scene: &crate::scene::Scene,
    target: &mut Image,
    target_bounds: Bounds,
    buffers: &RasterBuffers,
    text: Option<&PreparedTextData>,
) {
    fine.prepare(
        &buffers.tile_ptcl_ranges,
        &buffers.tile_ptcls,
        &buffers.tile_glyphs,
        &buffers.segments,
        target,
        target_bounds,
        (scene.width_in_tiles(), scene.height_in_tiles()),
        text,
    )
    .run();
}
