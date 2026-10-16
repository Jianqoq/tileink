use super::*;
use crate::canvas::SceneBufferChanges;
use crate::text::TextRun;
use peniko::{Color, kurbo::Rect};

fn canvas() -> Canvas {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(
        Rect::new(2.0, 3.0, 12.0, 14.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    canvas
}

#[test]
fn first_preparation_and_a_missing_cached_plan_require_complete_metadata() {
    let mut canvas = canvas();
    canvas.buffer_changes = Some(SceneBufferChanges {
        plan_structure_reused: true,
        plan_values_patched: true,
        ..Default::default()
    });
    let mut state = ScenePreparation::default();
    for _ in 0..2 {
        let prepared = state.prepare_plan(&canvas, &mut None);
        assert!(!prepared.reused_metadata);
        assert_eq!(prepared.stack_depths, (0, 0));
    }
}

#[test]
fn an_installed_plan_does_not_imply_initialized_preparation_metadata() {
    for (plan_structure_reused, plan_values_patched) in [(true, false), (false, true)] {
        let mut canvas = Canvas::new(32, 32, 1.0);
        for _ in 0..3 {
            canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 32.0, 32.0), crate::Radius::ZERO);
        }
        canvas.push_rect(
            Rect::new(2.0, 3.0, 12.0, 14.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        for _ in 0..3 {
            canvas.pop_layer();
        }
        canvas.buffer_changes = Some(SceneBufferChanges {
            plan_structure_reused,
            plan_values_patched,
            ..Default::default()
        });
        let mut installed = Some(canvas.compile_shared(ROOT_COMMAND_LIST_ID));
        let prepared = ScenePreparation::default().prepare_plan(&canvas, &mut installed);
        assert!(!prepared.reused_metadata);
        assert_eq!(prepared.stack_depths, (3, 0));
    }
}

#[test]
fn unchanged_plan_reuses_cached_plan_and_metadata() {
    let canvas = canvas();
    let mut state = ScenePreparation::default();
    let first = state.prepare_plan(&canvas, &mut None);
    let reference = std::rc::Rc::clone(&first.plan);
    state.metadata.as_mut().unwrap().stack_depths = (7, 9);
    let prepared = state.prepare_plan(&canvas, &mut Some(first.plan));
    assert!(Rc::ptr_eq(&prepared.plan, &reference));
    assert!(prepared.reused_metadata);
    assert_eq!(prepared.stack_depths, (7, 9));
}

#[test]
fn structural_reuse_refreshes_values_when_the_fingerprint_changes() {
    let mut canvas = canvas();
    let mut state = ScenePreparation::default();
    let first = state.prepare_plan(&canvas, &mut None);
    let reference = canvas.compile_shared(ROOT_COMMAND_LIST_ID);
    canvas.compiled_plan = Some(std::rc::Rc::clone(&reference));
    // The retained materializer can certify unchanged structure independently of its key.
    let metadata = state.metadata.as_mut().unwrap();
    metadata.fingerprint = metadata.fingerprint.wrapping_add(1);
    canvas.buffer_changes = Some(SceneBufferChanges {
        plan_structure_reused: true,
        ..Default::default()
    });
    let prepared = state.prepare_plan(&canvas, &mut Some(first.plan));
    assert!(Rc::ptr_eq(&prepared.plan, &reference));
    assert!(prepared.reused_metadata);
}

#[test]
fn patched_values_consume_the_new_plan_but_keep_its_size_metadata() {
    let original = canvas();
    let mut state = ScenePreparation::default();
    let first = state.prepare_plan(&original, &mut None);
    let mut changed = Canvas::new(32, 32, 1.0);
    changed.push_rect(
        Rect::new(5.0, 6.0, 18.0, 21.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    changed.buffer_changes = Some(SceneBufferChanges {
        plan_values_patched: true,
        ..Default::default()
    });
    let expected = changed.compile_shared(ROOT_COMMAND_LIST_ID);
    changed.compiled_plan = Some(std::rc::Rc::clone(&expected));
    let metadata = state.metadata.as_mut().unwrap();
    metadata.fingerprint = metadata.fingerprint.wrapping_add(1);
    state.metadata.as_mut().unwrap().stack_depths = (7, 9);
    let prepared = state.prepare_plan(&changed, &mut Some(first.plan));
    assert!(Rc::ptr_eq(&prepared.plan, &expected));
    assert!(prepared.reused_metadata);
    assert_eq!(prepared.stack_depths, (7, 9));
}

#[test]
fn a_new_topology_rebuilds_metadata_and_does_not_share_another_renderers_cache() {
    let original = canvas();
    let mut state = ScenePreparation::default();
    let first = state.prepare_plan(&original, &mut None);
    state.metadata.as_mut().unwrap().stack_depths = (7, 9);
    let empty = Canvas::new(32, 32, 1.0);
    let prepared = state.prepare_plan(&empty, &mut Some(first.plan));
    assert!(!prepared.reused_metadata);
    assert_eq!(prepared.stack_depths, (0, 0));
    let independent = ScenePreparation::default().prepare_plan(&empty, &mut None);
    assert!(!independent.reused_metadata);
}

#[test]
fn draw_membership_changes_refresh_program_detection_without_reencoding_brushes() {
    let program = crate::ShaderProgram::new(
        crate::ShaderSources {
            metal: Some(include_str!("../../../examples/shaders/coordinates.metal").into()),
            hlsl: Some(include_str!("../../../examples/shaders/coordinates.hlsl").into()),
        },
        "coordinates",
        crate::ShaderInput::None,
        4,
    )
    .unwrap();
    let mut canvas = canvas();
    canvas.push_rect(
        Rect::new(14.0, 3.0, 24.0, 14.0),
        crate::Radius::ZERO,
        program.brush([0; 4]).unwrap(),
    );
    let mut state = ScenePreparation::default();
    let mut prepared = state.prepare_plan(&canvas, &mut None);
    assert!(prepared.has_programs);
    let program_offset = canvas.draw_records[1].brush_offset;
    let program_len = canvas.draw_records[1].brush_len;
    canvas.buffer_changes = Some(SceneBufferChanges {
        draws: std::iter::once(1..2).collect(),
        ..Default::default()
    });
    for enabled in [false, true, false] {
        canvas.draw_records[1].brush_offset = if enabled {
            program_offset
        } else {
            canvas.draw_records[0].brush_offset
        };
        canvas.draw_records[1].brush_len = if enabled {
            program_len
        } else {
            canvas.draw_records[0].brush_len
        };
        prepared = state.prepare_plan(&canvas, &mut Some(prepared.plan));
        assert!(prepared.reused_metadata);
        assert_eq!(prepared.has_programs, enabled);
    }
}

#[test]
fn text_journal_gap_reconciles_skipped_glyph_positions() {
    use crate::retained_scene::PersistentSceneMaterializer;
    use crate::text::CanvasGlyph;
    use crate::{RetainedNodeId, RetainedScene};
    let scene = RetainedScene::new(64, 64, 1.0, RetainedNodeId::for_owner(1)).unwrap();
    let mut canvas = (*PersistentSceneMaterializer::new(&scene).canvas()).clone();
    let (cache_key, x, y) = cosmic_text::CacheKey::new(
        cosmic_text::fontdb::ID::dummy(),
        0,
        16.0,
        (0.0, 0.0),
        cosmic_text::fontdb::Weight::NORMAL,
        cosmic_text::CacheKeyFlags::empty(),
    );
    canvas.text_glyphs.push(CanvasGlyph { cache_key, x, y });
    canvas.text_runs.push(TextRun {
        glyph_start: 0,
        glyph_count: 1,
    });
    let mut fonts = TextFontSystem::new();
    let mut context = TextContext::new();
    let mut prepared = None;
    prepare_text(&mut prepared, &canvas, &mut fonts, &mut context);
    let version = canvas.persistent_frame.as_ref().unwrap().version;
    canvas.persistent_frame.as_mut().unwrap().version += 2;
    canvas.text_glyphs[0].x = 17;
    // The position changed in the skipped version; this later journal is empty.
    canvas.buffer_changes = Some(SceneBufferChanges {
        from_version: Some(version + 1),
        ..Default::default()
    });
    prepare_text(&mut prepared, &canvas, &mut fonts, &mut context);
    assert_eq!(prepared.as_ref().unwrap().glyph(0).unwrap().x, 17);
    canvas.persistent_frame.as_mut().unwrap().version += 1;
    canvas.text_glyphs[0].x = 23;
    canvas.buffer_changes = Some(SceneBufferChanges {
        from_version: Some(version + 2),
        glyphs: std::iter::once(0..1).collect(),
        ..Default::default()
    });
    assert!(prepare_text(&mut prepared, &canvas, &mut fonts, &mut context).is_none());
    assert_eq!(prepared.as_ref().unwrap().glyph(0).unwrap().x, 23);
}

#[test]
fn text_preparation_reconciles_immediate_and_unproven_journals() {
    let mut font_system = TextFontSystem::new();
    let mut context = TextContext::new();
    let mut data = None;
    let mut canvas = Canvas::new(16, 16, 1.0);
    assert!(prepare_text(&mut data, &canvas, &mut font_system, &mut context).is_none());
    assert!(data.is_some());
    canvas.text_runs.push(TextRun {
        glyph_start: 0,
        glyph_count: 0,
    });
    let changes = prepare_text(&mut data, &canvas, &mut font_system, &mut context).unwrap();
    assert_eq!(changes.runs(), std::slice::from_ref(&(0..1)));
    let unchanged = prepare_text(&mut data, &canvas, &mut font_system, &mut context).unwrap();
    assert!(unchanged.runs().is_empty());
    canvas.text_runs.push(TextRun {
        glyph_start: 0,
        glyph_count: 0,
    });
    canvas.buffer_changes = Some(SceneBufferChanges {
        text_runs: std::iter::once(1..2).collect(),
        ..Default::default()
    });
    // Without a materialization stamp, supplied ranges cannot prove continuity.
    let changes = prepare_text(&mut data, &canvas, &mut font_system, &mut context).unwrap();
    assert_eq!(changes.runs(), std::slice::from_ref(&(1..2)));
    // Some(empty) means the flat frame needs no patch; None requests full text upload.
    // Reconciliation must have installed the second run already.
    canvas.buffer_changes = None;
    let unchanged = prepare_text(&mut data, &canvas, &mut font_system, &mut context).unwrap();
    assert!(unchanged.runs().is_empty());
}
