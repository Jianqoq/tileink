use super::*;
use crate::{TextContext, TextFontSystem, TextLayoutOptions};

fn fonts() -> TextFontSystem {
    let mut fonts = TextFontSystem::new_with_locale_and_db(
        "en-US".into(),
        cosmic_text::fontdb::Database::default(),
    );
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../../svg/fonts/NotoSans-Regular.ttf").to_vec());
    fonts
}

fn text_leaf(fonts: &mut TextFontSystem, text: &mut TextContext, runs: usize) -> Rc<Canvas> {
    let layout = text.layout(
        fonts,
        TextLayoutOptions::new("Retained text", 12.0)
            .with_attrs(cosmic_text::Attrs::new().family(cosmic_text::Family::Name("Noto Sans"))),
    );
    assert!(!layout.is_empty());
    let mut canvas = Canvas::new(640, 480, 1.0);
    for index in 0..runs {
        canvas.push_text_layout(
            &layout,
            Point::new(4.0, 4.0 + index as f64 * 17.0),
            Color::WHITE,
        );
    }
    assert_eq!(canvas.text_runs.len(), runs);
    Rc::new(canvas)
}

fn assert_materialized_text(materializer: &PersistentSceneMaterializer) {
    let canvas = &materializer.canvas;
    for chunk in materializer.chunks.values() {
        let draws = &canvas.draw_records[materializer.arenas.draws.range(chunk.draws)];
        for (local, physical) in chunk.canvas.draw_records.iter().zip(draws) {
            if local.glyph_run_id == DrawRecord::NONE {
                continue;
            }
            let local_run = chunk.canvas.text_runs[local.glyph_run_id as usize];
            let physical_run = canvas.text_runs[physical.glyph_run_id as usize];
            assert_eq!(physical_run.glyph_count, local_run.glyph_count);
            let count = local_run.glyph_count as usize;
            let local_start = local_run.glyph_start as usize;
            let physical_start = physical_run.glyph_start as usize;
            assert_eq!(
                &canvas.text_glyphs[physical_start..physical_start + count],
                &chunk.canvas.text_glyphs[local_start..local_start + count],
            );
        }
    }
}

#[test]
fn text_storage_preserves_runs_and_glyphs_through_empty_growth_and_compaction() {
    let root = RetainedNodeId::for_owner(84_000);
    let ids = [84_001, 84_002, 84_003, 84_004].map(RetainedNodeId::for_owner);
    let mut fonts = fonts();
    let mut text = TextContext::new();
    let mut scene = RetainedScene::new(640, 480, 1.0, root).unwrap();
    let mut tx = scene.transaction();
    for (id, runs) in ids.into_iter().zip([2, 3, 1]) {
        tx.insert_scene(
            RetainedParent::content(root),
            None,
            id,
            text_leaf(&mut fonts, &mut text, runs),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    assert_materialized_text(&materializer);
    for runs in [0, 1, 8, 2, 0, 3] {
        scene
            .transaction()
            .replace_scene(ids[0], text_leaf(&mut fonts, &mut text, runs))
            .commit()
            .unwrap();
        assert!(update_materializer(&mut materializer, &scene));
        assert_materialized_text(&materializer);
    }
    let compactions = materializer.arenas.runs.compactions();
    scene
        .transaction()
        .remove_subtree(ids[0])
        .remove_subtree(ids[1])
        .insert_scene(
            RetainedParent::content(root),
            None,
            ids[3],
            text_leaf(&mut fonts, &mut text, 24),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert!(materializer.arenas.runs.compactions() > compactions);
    assert_materialized_text(&materializer);
    scene
        .transaction()
        .remove_subtree(ids[2])
        .remove_subtree(ids[3])
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_eq!(materializer.arenas.runs.live_len(), 0);
    assert_eq!(materializer.arenas.glyphs.as_ref().unwrap().live_len(), 0);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            ids[0],
            text_leaf(&mut fonts, &mut text, 2),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut materializer, &scene));
    assert_materialized_text(&materializer);
}

#[cfg(any(feature = "metal", feature = "dx12", feature = "vulkan"))]
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_text_mutations_match_fresh_immediate_rendering()
-> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "metal")]
    let backend = crate::NativeBackend::Metal;
    #[cfg(feature = "dx12")]
    let backend = crate::NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = crate::NativeBackend::Vulkan;
    let context = crate::NativeContext::new(
        backend,
        &crate::NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let root = RetainedNodeId::for_owner(84_100);
    let child = RetainedNodeId::for_owner(84_101);
    let mut fonts = fonts();
    let mut text = TextContext::new();
    let mut renderer = crate::NativeRenderer::with_context(&context, 640, 480)?;
    let mut scene = RetainedScene::new(640, 480, 1.0, root)?;
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            text_leaf(&mut fonts, &mut text, 0),
            Affine::translate((6.0, 10.0)),
        )
        .commit()?;
    for (runs, offset) in [0.0, 24.0]
        .into_iter()
        .flat_map(|offset| [0, 1, 8, 2, 0, 3].map(|runs| (runs, offset)))
    {
        let leaf = text_leaf(&mut fonts, &mut text, runs);
        let mut source = Canvas::new(640, 480, 1.0);
        source.append(&leaf, Point::new(offset, offset));
        let source = Rc::new(source);
        scene
            .transaction()
            .replace_scene(child, std::rc::Rc::clone(&source))
            .commit()?;
        let actual = renderer
            .render_retained_to_image_with_text(&scene, &mut fonts, &mut text)?
            .readback()?;
        let mut immediate = Canvas::new(640, 480, 1.0);
        immediate.append(&leaf, Point::new(6.0 + offset, 10.0 + offset));
        let mut fresh = crate::NativeRenderer::with_context(&context, 640, 480)?;
        let expected = fresh
            .render_to_image_with_text(&immediate, &mut fonts, &mut text)?
            .readback()?;
        assert!(
            actual.pixels == expected.pixels,
            "runs={runs}, offset={offset}, mismatches={}, source draw={:?}, appended draw={:?}",
            actual
                .pixels
                .iter()
                .zip(&expected.pixels)
                .filter(|(a, b)| a != b)
                .count(),
            source.draw_records.first(),
            immediate.draw_records.first(),
        );
        assert_eq!(actual.pixels.iter().any(|pixel| *pixel != 0), runs != 0);
    }
    Ok(())
}
