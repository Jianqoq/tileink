use super::*;
use crate::{
    Radius,
    shared::gpu_plan::{GpuBufferLengths, GpuLengthOverrides},
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};

fn scene(alpha: u8, radius: Radius) -> Canvas {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO, Color::BLACK);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 64.0, 64.0),
        radius,
        Color::from_rgba8(23, 47, 93, alpha),
    );
    canvas
}

fn transient(canvas: &Canvas) -> TileDrawBins {
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
        canvas,
        None,
        &plan,
        &mut bins,
        &mut Vec::new(),
        false,
        GpuLengthOverrides::default(),
    );
    bins
}

#[test]
fn early_occlusion_removes_hidden_gpu_refs_but_keeps_filter_source_queries() {
    for count in [1, 255, 256, 257, 513] {
        let mut canvas = scene(120, Radius::ZERO);
        for _ in 0..count {
            canvas.push_rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO, Color::WHITE);
        }
        let bins = transient(&canvas);
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        assert_eq!(bins.upload_index_count(), 16);
        for tile in 0..16 {
            assert_eq!(bins.tile_draws(tile), [count + 1]);
        }
        assert_eq!(
            bins.draws_in_bounds(Bounds::new(0, 0, 16, 16), &plan.draw_order, None),
            *plan.draw_order
        );
        assert_eq!(
            bins.draws_in_bounds(Bounds::new(-16, 0, 0, 16), &plan.draw_order, None),
            *plan.draw_order
        );
    }
}

#[test]
fn early_occlusion_preserves_alpha_aa_corners_and_uncertain_transforms() {
    for alpha in [0, 120, 254] {
        let bins = transient(&scene(alpha, Radius::ZERO));
        assert!(!bins.occlusion.active);
        assert_eq!(bins.tile_draws(0), [0, 1]);
    }
    let bins = transient(&scene(255, Radius::all(16.0)));
    assert_eq!(bins.tile_draws(0), [0, 1]);
    assert_eq!(bins.tile_draws(5), [1]);
    for transform in [
        Affine::translate((0.375, 0.375)),
        Affine::scale(0.9),
        Affine::rotate(0.1),
    ] {
        let mut canvas = scene(255, Radius::ZERO);
        let draw = &mut canvas.draw_records[1];
        draw.transform = crate::shared::affine::GpuAffine::from_logical(transform, 1.0);
        let bins = transient(&canvas);
        assert_eq!(bins.tile_draws(0), [0, 1]);
    }
}

#[test]
fn early_occlusion_preserves_every_draw_at_partial_clip_edges() {
    let mut canvas = Canvas::new(64, 64, 1.0);
    let full = Rect::new(0.0, 0.0, 64.0, 64.0);
    canvas.push_clip_sdf_rect_layer(full.inset(4.375), Radius::all(8.0));
    canvas.push_rect(full, Radius::ZERO, Color::from_rgba8(200, 40, 90, 119));
    canvas.push_rect(full, Radius::ZERO, Color::WHITE);
    canvas.pop_layer();
    let bins = transient(&canvas);
    // Clip coverage multiplies every draw, so an opaque brush is translucent at AA edges.
    assert_eq!(bins.tile_draws(0), [1, 2]);
    assert_eq!(bins.tile_draws(5), [2]);
}

#[test]
fn early_occlusion_stays_inside_each_batch_and_keeps_fused_wrappers() {
    for kind in 0..3 {
        let mut canvas = scene(120, Radius::ZERO);
        let full = Rect::new(0.0, 0.0, 64.0, 64.0);
        match kind {
            0 => canvas.push_clip_sdf_rect_layer(full.inset(4.0), Radius::all(8.0)),
            1 => canvas.push_opacity_layer(full.to_path(0.1), Affine::IDENTITY, 0.1, 0.5),
            _ => canvas.push_blend_layer(
                full.to_path(0.1),
                Affine::IDENTITY,
                0.1,
                peniko::Mix::Multiply,
                peniko::Compose::SrcOver,
            ),
        }
        canvas.push_rect(full, Radius::ZERO, Color::BLACK);
        canvas.push_rect(full, Radius::ZERO, Color::WHITE);
        canvas.pop_layer();
        canvas.push_rect(full, Radius::ZERO, Color::from_rgba8(90, 80, 70, 120));
        let bins = transient(&canvas);
        let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
        let draws = bins.tile_draws(5);
        assert_eq!(draws, [0, 1, 4, 5], "kind={kind}; plan={plan:?}");
        assert!(bins.coarse_ptcl_capacity(&plan, None) >= 16 * 7);
    }
}

#[test]
fn early_occlusion_full_rebuild_uses_one_gpu_arena_across_spatial_edits() {
    let mut canvas = scene(255, Radius::ZERO);
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    bins.update_occlusion(&canvas, &plan);
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut Vec::new());
    assert_eq!(bins.records, bins.upload_records());
    assert_eq!(bins.draw_indices, bins.upload_indices());
    assert!(bins.tile_pages.iter().all(Vec::is_empty));
    let _ = bins.take_dirty();
    canvas.brush_blob[canvas.draw_records[1].brush_offset as usize + 4] = 0xfeff_ffff;
    bins.update_occlusion(&canvas, &plan);
    assert!(bins.update_changed(&canvas.draw_records, &plan.draw_order, None, (4, 4), &[]));
    assert_eq!(bins.tile_draws(0), [0, 1]);
    assert!(bins.tile_pages.iter().all(Vec::is_empty));
    canvas.draw_records[1].transform.e = 32.0;
    canvas.draw_records[1].pixel_bounds.x0 += 32;
    canvas.draw_records[1].pixel_bounds.x1 += 32;
    bins.update_occlusion(&canvas, &plan);
    assert!(bins.update_changed(
        &canvas.draw_records,
        &plan.draw_order,
        None,
        (4, 4),
        std::slice::from_ref(&(1..2))
    ));
    assert_eq!(bins.tile_draws(0), [0]);
    assert_eq!(bins.tile_draws(2), [0, 1]);
    assert_eq!(bins.index_format, IndexFormat::FlatPrefix);
    assert!(bins.take_dirty().0);
}

#[test]
fn early_occlusion_does_not_dirty_unculled_indices_for_batch_only_changes() {
    let canvas = scene(120, Radius::ZERO);
    let mut plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    bins.update_occlusion(&canvas, &plan);
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut Vec::new());
    let _ = bins.take_dirty();
    std::rc::Rc::make_mut(&mut plan.draw_batch_ids).fill(7);
    for op in &mut plan.ops {
        if let crate::shared::execution::ExecOp::DrawBatch { batch_id, .. } = op {
            *batch_id = 7;
        }
    }
    bins.update_occlusion(&canvas, &plan);
    assert!(!bins.occlusion.active);
    assert!(bins.update_changed(&canvas.draw_records, &plan.draw_order, None, (4, 4), &[]));
    assert_eq!(bins.tile_draws(0), [0, 1]);
    let (full, records, pages) = bins.take_dirty();
    assert!(!full && records.is_empty() && pages.is_empty());
}

#[test]
fn early_occlusion_only_replaces_tile_lists_when_a_predecessor_is_hidden() {
    let full = Rect::new(0.0, 0.0, 64.0, 64.0);
    let mut canvas = scene(120, Radius::ZERO);
    canvas.push_opacity_layer(full.to_path(0.1), Affine::IDENTITY, 0.1, 0.65);
    canvas.push_rect(full, Radius::ZERO, Color::BLACK);
    canvas.push_rect(full, Radius::all(16.0), Color::WHITE);
    canvas.pop_layer();
    canvas.push_rect(full, Radius::ZERO, Color::from_rgba8(90, 80, 70, 50));
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    bins.update_occlusion(&canvas, &plan);
    let refs = [0, 1, 3, 4, 5];
    let mut visible = Vec::new();
    assert!(!bins.occlusion.filter_tile(0, 4, &refs, &mut visible));
    assert!(bins.occlusion.filter_tile(5, 4, &refs, &mut visible));
    assert_eq!(visible, [0, 1, 4, 5]);
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut Vec::new());
    assert_eq!(bins.tile_draws(0), refs);
    assert_eq!(bins.tile_draws(5), [0, 1, 4, 5]);
}

#[test]
fn early_occlusion_selects_each_tiles_last_local_occluder() {
    let mut canvas = Canvas::new(64, 32, 1.0);
    let full = Rect::new(0.0, 0.0, 64.0, 32.0);
    canvas.push_rect(full, Radius::ZERO, Color::BLACK);
    canvas.push_rect(full, Radius::ZERO, Color::from_rgba8(90, 80, 70, 120));
    canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO, Color::WHITE);
    canvas.push_rect(full, Radius::ZERO, Color::from_rgba8(90, 80, 70, 100));
    canvas.push_rect(Rect::new(32.0, 0.0, 64.0, 32.0), Radius::ZERO, Color::WHITE);
    canvas.push_rect(full, Radius::ZERO, Color::from_rgba8(90, 80, 70, 50));
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    bins.update_occlusion(&canvas, &plan);
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut Vec::new());
    assert_eq!(bins.tile_draws(0), [2, 3, 5]);
    assert_eq!(bins.tile_draws(2), [4, 5]);
    let _ = bins.take_dirty();
    canvas.brush_blob[canvas.draw_records[2].brush_offset as usize + 4] = 0xfeff_ffff;
    bins.update_occlusion(&canvas, &plan);
    assert!(bins.update_changed(&canvas.draw_records, &plan.draw_order, None, (4, 2), &[]));
    assert_eq!(bins.tile_draws(0), [0, 1, 2, 3, 5]);
    assert_eq!(bins.tile_draws(2), [4, 5]);
    let (full, records, pages) = bins.take_dirty();
    assert!(!full && records.len() == 4 && pages.is_empty());
}

#[test]
fn early_occlusion_refreshes_persistent_paint_without_geometry_or_rank_changes() {
    let mut canvas = scene(255, Radius::ZERO);
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    bins.update_occlusion(&canvas, &plan);
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut Vec::new());
    assert_eq!(bins.tile_draws(0), [1]);
    assert_eq!(
        bins.draws_in_bounds(Bounds::new(0, 0, 16, 16), &plan.draw_order, None),
        [0, 1]
    );
    let _ = bins.take_dirty();
    let resident_indices = bins.upload_indices().to_vec();
    for alpha in [254, 255, 0, 255] {
        let offset = canvas.draw_records[1].brush_offset as usize;
        canvas.brush_blob[offset + 4] = (alpha << 24) | 0x005d_2f17;
        bins.occlusion.update(&canvas, &plan);
        assert!(bins.update_changed(&canvas.draw_records, &plan.draw_order, None, (4, 4), &[]));
        assert_eq!(
            bins.tile_draws(0),
            if alpha == 255 { vec![1] } else { vec![0, 1] }
        );
        let (full, records, pages) = bins.take_dirty();
        assert!(!full && records.len() == 16 && pages.is_empty());
        assert_eq!(bins.upload_indices(), resident_indices);
    }
    bins.occlusion.update(&canvas, &plan);
    assert!(bins.update_changed(&canvas.draw_records, &plan.draw_order, None, (4, 4), &[]));
    let (full, records, pages) = bins.take_dirty();
    assert!(!full && records.is_empty() && pages.is_empty());
}

#[test]
fn early_occlusion_rejects_unknown_brush_and_clip_coverage() {
    let full = Rect::new(0.0, 0.0, 64.0, 64.0);
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(full, Radius::ZERO, Color::BLACK);
    canvas.push_rect(
        full,
        Radius::ZERO,
        crate::Brush::four_corner(full, [Color::WHITE; 4]),
    );
    assert!(!transient(&canvas).occlusion.active);

    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_clip_layer(
        full.to_path(0.1),
        Affine::translate((0.25, 0.25)),
        crate::FillRule::NonZero,
        0.1,
    );
    canvas.push_rect(full, Radius::ZERO, Color::BLACK);
    canvas.push_rect(full, Radius::ZERO, Color::WHITE);
    canvas.pop_layer();
    assert!(!transient(&canvas).occlusion.active);
    assert_eq!(transient(&Canvas::new(0, 0, 1.0)).upload_index_count(), 0);
}

#[test]
fn early_occlusion_retains_order_on_rebuild_and_restores_revealed_geometry() {
    let mut canvas = scene(255, Radius::ZERO);
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let mut bins = TileDrawBins::default();
    bins.update_occlusion(&canvas, &plan);
    build_tile_draw_bins_into(&canvas, &plan, &mut bins, &mut Vec::new());
    let _ = bins.take_dirty();
    canvas.draw_records[1].transform.e = 32.0;
    canvas.draw_records[1].pixel_bounds.x0 += 32;
    canvas.draw_records[1].pixel_bounds.x1 += 32;
    bins.occlusion.update(&canvas, &plan);
    assert!(bins.update_changed(
        &canvas.draw_records,
        &plan.draw_order,
        None,
        (4, 4),
        std::slice::from_ref(&(1..2))
    ));
    assert_eq!(bins.tile_draws(0), [0]);
    assert_eq!(bins.tile_draws(2), [1]);
    assert_eq!(
        bins.draws_in_bounds(Bounds::new(32, 0, 48, 16), &plan.draw_order, None),
        [0, 1]
    );
}
