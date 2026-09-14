use super::{Result, fine_fixture::routes, reference::FineVariant};
use crate::native::runtime::program::fine::{self, FineBindings};
use crate::render::fine::{FineParams, FinePlan};
use crate::{
    Canvas,
    native::runtime::{
        compute::{ComputeBatch, ResourceId, SamplerFilter},
        program::{
            coarse::{self, CoarseBindings},
            scene_scan::{self, PreparedScan},
        },
    },
    render::{
        coarse::{CoarseBatch, CoarsePlan},
        upload::{paint::PaintData, scene::SceneUploadStaging},
    },
    shared::{
        execution::ROOT_COMMAND_LIST_ID, gpu_coarse::*,
        gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
    },
};

fn upload<T: bytemuck::Pod>(batch: &mut ComputeBatch, records: &[T]) -> Result<ResourceId> {
    batch.buffer(if records.is_empty() {
        vec![0; size_of::<T>()]
    } else {
        bytemuck::cast_slice(records).to_vec()
    })
}

// Build inputs from production Canvas/planning instead of hand-authored shader records.
// This remains an integration harness until NativeRenderer owns full frame execution.
fn scene_batch(
    width: u32,
    height: u32,
    count: usize,
    triangle: bool,
    chunked: bool,
) -> Result<ComputeBatch> {
    let mut canvas = Canvas::new(width, height, 1.0);
    for index in 0..count {
        let mut path = peniko::kurbo::BezPath::new();
        if triangle {
            let offset = (index % 7) as f64 / 8.0;
            path.move_to((8.0 + offset, 8.0));
            path.line_to((f64::from(width - 8), f64::from(height - 8) - offset));
            path.line_to((8.0 + offset, f64::from(height - 8) - offset));
        } else {
            path.move_to((0.0, 0.0));
            path.line_to((f64::from(width), 0.0));
            path.line_to((f64::from(width), f64::from(height)));
            path.line_to((0.0, f64::from(height)));
        }
        canvas.push_path(
            path,
            crate::Brush::Solid(peniko::Color::from_rgb8(255, 0, 0)),
            peniko::kurbo::Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.25,
        );
    }
    let execution = canvas.compile_shared(ROOT_COMMAND_LIST_ID);
    let mut staging = SceneUploadStaging::default();
    let lengths = staging.build_lengths(&canvas, None, &execution, false, None, None);
    let mut batch = ComputeBatch::new();
    let scan = scene_scan::encode_scene(
        &mut batch,
        &PreparedScan::new(&canvas, &mut staging.path_plans),
        65535,
    )?;
    let paint = staging.paint.prepare(&canvas, None);
    let (shadow_base, brush_base) = (paint.shadow_base, paint.brush_base);
    let paint_words = match paint.data {
        PaintData::Immediate {
            sdfs,
            shadows,
            brushes,
        } => [sdfs, shadows, brushes].concat(),
        PaintData::Retained { .. } => unreachable!(),
    };
    let work_len = coarse_work_word_len(
        lengths.tile_count,
        lengths.coarse_ptcl_capacity,
        lengths.coarse_glyph_capacity,
        lengths.tile_draw_index_count,
        lengths.tile_draw_chunk_count,
    );
    let mut work = vec![0u32; work_len.max(1)];
    let records: &[u32] = bytemuck::cast_slice(staging.tile_draw_bins.upload_records());
    let record_base = coarse_work_tile_draw_record_word_offset(
        lengths.tile_count,
        lengths.coarse_ptcl_capacity,
        lengths.coarse_glyph_capacity,
    );
    work[record_base..record_base + records.len()].copy_from_slice(records);
    let indices = staging.tile_draw_bins.upload_indices();
    let index_base = coarse_work_tile_draw_index_word_offset(
        lengths.tile_count,
        lengths.coarse_ptcl_capacity,
        lengths.coarse_glyph_capacity,
    );
    work[index_base..index_base + indices.len()].copy_from_slice(indices);
    let draws = upload(&mut batch, &canvas.draw_records)?;
    let text = upload(&mut batch, &[0u32])?;
    let paint = upload(&mut batch, &paint_words)?;
    let work = upload(&mut batch, &work)?;
    let layers = upload(&mut batch, &[LayerStackRecord::default()])?;
    let chunks = upload(
        &mut batch,
        &vec![CoarseChunkRecord::default(); lengths.coarse_chunk_count.max(1)],
    )?;
    let batches = upload(&mut batch, &execution.draw_batch_ids)?;
    let coarse = CoarsePlan::new(
        lengths,
        CoarseBatch {
            draw_end: count as u32,
            ..Default::default()
        },
        brush_base,
        chunked,
        65535,
    )?;
    // SAFETY: production Canvas and shared binning create valid records; all work
    // allocations use shared capacities, scan is in this same ordered batch.
    unsafe {
        coarse::encode(
            &mut batch,
            &coarse,
            &CoarseBindings {
                draws,
                text,
                paint,
                paths: scan.paths,
                backdrops: scan.backdrops,
                ranges: scan.tile_segment_ranges,
                layers,
                work,
                chunks,
                batches,
            },
        )?;
    }
    let fine_plan = FinePlan::new(
        lengths,
        FineParams {
            width,
            height,
            paint_sdf_shadow_base: shadow_base,
            paint_brush_base: brush_base,
            ..Default::default()
        },
        65535,
    )?;
    let target = batch.texture_rgba8([width, height], vec![0; (width * height * 4) as usize])?;
    let spills = upload(&mut batch, &vec![0u32; fine_plan.spill_words().max(1)])?;
    let atlas = batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?;
    let image = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let images = batch.texture_table(&vec![image; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
    let sampler = batch.sampler(SamplerFilter::Linear)?;
    // SAFETY: this harness has one root draw batch, no text/images/layers/spills;
    // fine consumes the complete coarse stream and scan segment allocation.
    unsafe {
        fine::encode(
            &mut batch,
            &fine_plan,
            &FineBindings {
                target,
                draws,
                paint,
                coarse: work,
                segments: scan.segments,
                text,
                spills,
                atlas,
                images,
                sampler,
            },
        )?;
    }
    batch.readback(target)?;
    Ok(batch)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_canvas_scan_coarse_fine_preserves_pixels() -> Result<()> {
    let routes = routes()?;
    for (width, height, count, triangle) in
        [(33, 19, 1, false), (65, 49, 1, true), (65, 49, 257, true)]
    {
        let dense = scene_batch(width, height, count, triangle, false)?;
        let expected = routes.fine_reference(&dense, FineVariant::ALL[0])?;
        if !triangle {
            assert_eq!(
                expected[0],
                [255, 0, 0, 255].repeat((width * height) as usize)
            );
        } else {
            assert!(expected[0].chunks_exact(4).any(|p| p == [255, 0, 0, 255]));
            assert!(expected[0].chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        }
        for chunked in [false, true] {
            let batch = scene_batch(width, height, count, triangle, chunked)?;
            for variant in FineVariant::ALL {
                routes.check_fine(
                    &batch,
                    &expected,
                    &format!("Canvas {count} chunked {chunked}"),
                    variant,
                )?;
            }
        }
    }
    routes.validate()
}
