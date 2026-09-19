use super::reference::{FilterVariant, FineVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    renderer::recording::{Limits, Recording},
};
use crate::shared::layer::{filter::Filter, region::Region};
use crate::{Canvas, Radius, TextContext, TextFontSystem, TextLayoutOptions};
use peniko::{
    Color,
    kurbo::{Point, Rect},
};

fn frame_fonts() -> TextFontSystem {
    let mut database = cosmic_text::fontdb::Database::new();
    database
        .load_font_data(include_bytes!("../../../svg/fonts/SourceSansPro-Regular.ttf").to_vec());
    database.load_font_data(
        include_bytes!("../../../svg/fonts/NotoColorEmojiCBDT.subset.ttf").to_vec(),
    );
    database.set_sans_serif_family("Source Sans Pro");
    TextFontSystem::new_with_locale_and_db("en-US".into(), database)
}

#[test]
fn frame_text_fixture_contains_color_and_coverage_glyphs() {
    let mut fonts = frame_fonts();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("Ag7\u{1f600}", 19.0));
    let mut canvas = Canvas::new(83, 61, 1.0);
    canvas.push_text_layout(&layout, Point::new(0.0, 0.0), Color::BLACK);
    let text = crate::text::PreparedTextData::new(
        &canvas.text_glyphs,
        &canvas.text_runs,
        &mut fonts,
        &mut context,
    );
    assert!(
        text.images()
            .iter()
            .any(|image| image.content == crate::text::PreparedGlyphContent::Color)
    );
    assert!(
        text.images()
            .iter()
            .any(|image| image.content == crate::text::PreparedGlyphContent::SubpixelMask)
    );
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_frame_text_preserves_root_and_local_glyphs() -> Result<()> {
    let routes = super::fine_fixture::routes()?;
    let mut fonts = frame_fonts();
    let mut context = TextContext::new();
    let mut recording = Recording::default();
    let resources = crate::shared::image_resource::ImageResourceStore::default();
    let limits = Limits {
        image_dimension: 4096,
        atlas_pages: 4,
        texture_table_len: 0,
        dispatch_dimension: 65535,
    };
    for (mode_index, mode) in [
        crate::TextSubpixelMode::None,
        crate::TextSubpixelMode::Rgb,
        crate::TextSubpixelMode::Bgr,
    ]
    .into_iter()
    .enumerate()
    {
        context.set_raster_options(
            crate::TextRasterOptions::new()
                .with_subpixel_mode(mode)
                .with_composite_mode(if mode_index == 0 {
                    crate::TextCompositeMode::Srgb
                } else {
                    crate::TextCompositeMode::Linear
                }),
        );
        let layout = context.layout(&mut fonts, TextLayoutOptions::new("Ag7\u{1f600}", 19.0));
        assert!(
            !layout.is_empty(),
            "the checked-in font must produce glyphs"
        );
        for filtered in [false, true] {
            let mut canvas = Canvas::new(83, 61, 1.0);
            canvas.push_rect(
                Rect::new(0.0, 0.0, 83.0, 61.0),
                Radius::ZERO,
                Color::from_rgb8(213, 227, 239),
            );
            canvas.push_text_layout(
                &layout,
                Point::new(1.25, 3.5),
                Color::from_rgb8(31, 67, 111),
            );
            if filtered {
                canvas.push_filter_layer(
                    Filter::Opacity(0.7),
                    Region::rect(Rect::new(13.0, 17.0, 78.0, 59.0), Radius::ZERO),
                );
            }
            canvas.push_text_layout_clipped(
                &layout,
                Point::new(17.5, 25.25),
                Rect::new(14.0, 18.0, 43.0, 58.0),
                Color::from_rgb8(173, 39, 81),
            );
            if filtered {
                canvas.pop_layer();
            }
            let expected = routes.text_reference(&canvas, &mut fonts, &mut context)?;
            assert!(
                expected[0]
                    .chunks_exact(4)
                    .any(|pixel| pixel != [213, 227, 239, 255])
            );
            let mut batch = ComputeBatch::new();
            let target = recording.record(
                &mut batch,
                &canvas,
                &resources,
                Some((&mut fonts, &mut context)),
                limits,
                mode_index == 1,
            )?;
            assert!(batch.outputs().is_empty());
            batch.readback(target)?;
            for fine in FineVariant::ALL {
                routes.check_render(
                    &batch,
                    &expected,
                    "complete frame text",
                    FilterVariant {
                        portable: fine.portable,
                        texture_table: fine.texture_table,
                    },
                    fine,
                )?;
            }
            // Reusing the same scene cache without prepared text must disable glyphs
            // and clear old text uploads, including inside localized filters.
            let expected = routes.canvas_reference(&canvas)?;
            let mut batch = ComputeBatch::new();
            let target = recording.record(
                &mut batch,
                &canvas,
                &resources,
                None,
                limits,
                mode_index == 1,
            )?;
            batch.readback(target)?;
            for fine in FineVariant::ALL {
                routes.check_render(
                    &batch,
                    &expected,
                    "text disabled after prepared frame",
                    FilterVariant {
                        portable: fine.portable,
                        texture_table: fine.texture_table,
                    },
                    fine,
                )?;
            }
        }
    }
    routes.validate()
}
