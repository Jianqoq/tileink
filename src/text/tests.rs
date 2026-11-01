use std::path::Path;

use cosmic_text::{
    Align, Attrs, CacheKeyFlags, Family, FontSystem, SwashContent, SwashImage, Weight, Wrap,
};
use peniko::{
    Color,
    kurbo::{Affine, Point, Rect, Shape},
};
use swash::zeno::Placement;

use crate::{
    Canvas,
    shared::{bounds::PixelBounds, draw_record::DrawTag},
};

use super::{
    raster::{
        FREETYPE_HARMONY_LCD_SHIFT, GlyphRasterImage, harmony_lcd_outline_shifts,
        merge_harmony_lcd_masks,
    },
    *,
};

pub(super) fn font_system() -> FontSystem {
    let mut fonts = FontSystem::new_with_locale_and_db(
        "en-US".into(),
        cosmic_text::fontdb::Database::default(),
    );
    for data in [
        include_bytes!("../svg/fonts/NotoSans-Regular.ttf").as_slice(),
        include_bytes!("../svg/fonts/NotoSans-Bold.ttf").as_slice(),
        include_bytes!("../svg/fonts/NotoColorEmojiCBDT.subset.ttf").as_slice(),
    ] {
        fonts.db_mut().load_font_data(data.to_vec());
    }
    fonts.db_mut().set_sans_serif_family("Noto Sans");
    fonts
}

fn canonical_glyph(content: GlyphContent, data: Vec<u8>) -> GlyphRasterImage {
    GlyphRasterImage::new(
        content,
        Placement {
            left: 0,
            top: 1,
            width: 1,
            height: 1,
        },
        data,
    )
}

fn raw_glyph(content: SwashContent, data: Vec<u8>) -> SwashImage {
    let mut image = SwashImage::new();
    image.content = content;
    image.placement = Placement {
        left: 0,
        top: 1,
        width: 1,
        height: 1,
    };
    image.data = data;
    image
}

mod bounds;
mod layout;
mod lcd;
mod prepared;
mod recording;

mod line_metrics;

mod hit;

#[cfg(target_os = "windows")]
mod platform_windows;
