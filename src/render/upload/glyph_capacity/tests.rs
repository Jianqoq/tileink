use super::*;
use crate::{
    TextContext,
    shared::{
        affine::GpuAffine,
        bounds::PixelBounds,
        draw_record::{DrawTag, FillRuleWord},
    },
    text::{CanvasGlyph, TextRun},
};
use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem};

fn glyph_draw(run: u32) -> DrawRecord {
    DrawRecord {
        path_id: DrawRecord::NONE,
        glyph_run_id: run,
        sdf_offset: DrawRecord::NONE,
        sdf_len: 0,
        sdf_shadow_offset: DrawRecord::NONE,
        sdf_shadow_len: 0,
        brush_offset: DrawRecord::NONE,
        brush_len: 0,
        tag: DrawTag::Brush.into(),
        fill_rule: FillRuleWord::default(),
        pixel_bounds: PixelBounds {
            x0: 0,
            y0: 0,
            x1: 16,
            y1: 16,
        },
        local_pixel_bounds: PixelBounds {
            x0: 0,
            y0: 0,
            x1: 16,
            y1: 16,
        },
        solid_rect: 0,
        transform: GpuAffine::IDENTITY,
        inverse_transform: GpuAffine::IDENTITY,
    }
}

pub(crate) fn glyph_capacity_fixture() -> (GlyphCapacityCache, Canvas, PreparedTextData) {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.text_runs = vec![
        TextRun {
            glyph_start: 0,
            glyph_count: 2,
        },
        TextRun {
            glyph_start: 2,
            glyph_count: 2,
        },
    ];
    let (cache_key, x, y) = CacheKey::new(
        cosmic_text::fontdb::ID::dummy(),
        0,
        16.0,
        (0.0, 0.0),
        cosmic_text::fontdb::Weight::NORMAL,
        CacheKeyFlags::empty(),
    );
    canvas.text_glyphs = vec![CanvasGlyph { cache_key, x, y }; 4];
    canvas.draw_records = vec![glyph_draw(0), glyph_draw(0), glyph_draw(1)];
    let mut font_system = FontSystem::new();
    let mut context = TextContext::new();
    let text = PreparedTextData::new(&[], &[], &mut font_system, &mut context);
    let mut cache = GlyphCapacityCache::default();
    cache.rebuild(&canvas, &text);
    (cache, canvas, text)
}

#[test]
fn flat_text_changes_update_only_dependent_glyph_capacity_draws() {
    let (mut cache, canvas, text) = glyph_capacity_fixture();
    let changes = PreparedTextChanges::from_ranges(std::iter::once(0..1).collect(), Vec::new());

    cache.update(&canvas, Some(&text), None, Some(&changes));

    assert_eq!(cache.affected_draws.len(), 2);
    assert!(cache.affected_draws.contains(0));
    assert!(cache.affected_draws.contains(1));
    assert!(!cache.affected_draws.contains(2));
}

#[test]
fn flat_draw_diff_coalesces_changed_tail_with_appended_draws() {
    let (cache, mut canvas, _) = glyph_capacity_fixture();
    canvas.draw_records[2].pixel_bounds.x1 += 1;
    canvas.draw_records.push(glyph_draw(1));

    let changes = cache.flat_draw_changes(&canvas);

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0], 2..4);
}

#[test]
fn changed_glyphs_collect_each_dependent_draw_once() {
    let (mut cache, canvas, text) = glyph_capacity_fixture();
    cache.update_incremental(
        &canvas,
        &text,
        &SceneBufferChanges {
            glyphs: std::iter::once(0..2).collect(),
            ..Default::default()
        },
    );

    assert_eq!(cache.affected_draws.len(), 2);
    assert!(cache.affected_draws.contains(0));
    assert!(cache.affected_draws.contains(1));
}

#[test]
fn changed_draw_rebinds_future_glyph_damage_to_its_new_run() {
    let (mut cache, mut canvas, text) = glyph_capacity_fixture();
    canvas.draw_records[0].glyph_run_id = 1;
    cache.update_incremental(
        &canvas,
        &text,
        &SceneBufferChanges {
            draws: std::iter::once(0..1).collect(),
            ..Default::default()
        },
    );
    cache.update_incremental(
        &canvas,
        &text,
        &SceneBufferChanges {
            glyphs: std::iter::once(0..2).collect(),
            ..Default::default()
        },
    );

    assert_eq!(cache.affected_draws.len(), 1);
    assert!(!cache.affected_draws.contains(0));
    assert!(cache.affected_draws.contains(1));
}

#[test]
fn shrinking_text_state_removes_stale_run_and_draw_membership() {
    let (mut cache, mut canvas, text) = glyph_capacity_fixture();
    canvas.text_glyphs.truncate(2);
    canvas.text_runs.truncate(1);
    canvas.draw_records.truncate(2);

    cache.update_incremental(&canvas, &text, &SceneBufferChanges::default());

    assert_eq!(cache.glyph_runs.len(), 2);
    assert_eq!(cache.run_draws.len(), 1);
    assert_eq!(cache.draw_runs.len(), 2);
    assert!(cache.run_draws[0].iter().all(|&draw| draw < 2));
}
