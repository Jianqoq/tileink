use super::*;
use cosmic_text::{CacheKey, SubpixelBin};
use std::mem::ManuallyDrop;
use windows::{
    Win32::Graphics::DirectWrite::*,
    core::{BOOL, Interface, PCWSTR},
};

struct Reference {
    factory: IDWriteFactory2,
    face: IDWriteFontFace,
}
impl Reference {
    fn new() -> windows::core::Result<Self> {
        // SAFETY: The UTF-16 path is terminated and lives throughout the native call;
        // returned interfaces own their font-file references.
        unsafe {
            let factory: IDWriteFactory2 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let base: IDWriteFactory = factory.cast()?;
            let path = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/svg/fonts/NotoSans-Regular.ttf"
            )
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
            let file = base.CreateFontFileReference(PCWSTR(path.as_ptr()), None)?;
            let face = base.CreateFontFace(
                DWRITE_FONT_FACE_TYPE_TRUETYPE,
                &[Some(file)],
                0,
                DWRITE_FONT_SIMULATIONS_NONE,
            )?;
            Ok(Self { factory, face })
        }
    }
    fn image(
        &self,
        key: CacheKey,
        mode: TextSubpixelMode,
    ) -> windows::core::Result<GlyphRasterImage> {
        let mut run = DWRITE_GLYPH_RUN {
            fontFace: ManuallyDrop::new(Some(self.face.clone())),
            fontEmSize: f32::from_bits(key.font_size_bits),
            glyphCount: 1,
            glyphIndices: &raw const key.glyph_id,
            glyphAdvances: std::ptr::null(),
            glyphOffsets: std::ptr::null(),
            isSideways: BOOL(0),
            bidiLevel: 0,
        };
        let gray = mode == TextSubpixelMode::None;
        let flags = key.flags;
        let transform = flags
            .contains(cosmic_text::CacheKeyFlags::FAKE_ITALIC)
            .then(|| DWRITE_MATRIX {
                m11: 1.0,
                m12: 0.0,
                m21: -14.0f32.to_radians().tan(),
                m22: 1.0,
                dx: 0.0,
                dy: 0.0,
            });
        let phase = |bin: SubpixelBin| {
            if flags.contains(cosmic_text::CacheKeyFlags::PIXEL_FONT) {
                bin.as_float().round()
            } else {
                bin.as_float()
            }
        };
        // SAFETY: The glyph, font interface and run remain valid for this call;
        // DirectWrite retains the interfaces needed by its returned analysis.
        let analysis = unsafe {
            self.factory.CreateGlyphRunAnalysis(
                &raw const run,
                transform.as_ref().map(std::ptr::from_ref),
                DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                DWRITE_MEASURING_MODE_NATURAL,
                if flags.contains(cosmic_text::CacheKeyFlags::DISABLE_HINTING) {
                    DWRITE_GRID_FIT_MODE_DISABLED
                } else {
                    DWRITE_GRID_FIT_MODE_ENABLED
                },
                if gray {
                    DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE
                } else {
                    DWRITE_TEXT_ANTIALIAS_MODE_CLEARTYPE
                },
                phase(key.x_bin),
                phase(key.y_bin),
            )
        };
        // SAFETY: This drops exactly the owned clone placed in the ABI's ManuallyDrop field.
        unsafe { ManuallyDrop::drop(&mut run.fontFace) };
        let analysis = analysis?;
        let kind = if gray {
            DWRITE_TEXTURE_ALIASED_1x1
        } else {
            DWRITE_TEXTURE_CLEARTYPE_3x1
        };
        // SAFETY: The analysis and selected texture type remain valid throughout readback.
        let bounds = unsafe { analysis.GetAlphaTextureBounds(kind)? };
        let width = u32::try_from(bounds.right - bounds.left).unwrap();
        let height = u32::try_from(bounds.bottom - bounds.top).unwrap();
        let mut data = vec![0; width as usize * height as usize * if gray { 1 } else { 3 }];
        // SAFETY: The checked fixture dimensions size the writable slice for these exact bounds.
        unsafe { analysis.CreateAlphaTexture(kind, &raw const bounds, &mut data)? };
        if mode == TextSubpixelMode::Bgr {
            for pixel in data.as_chunks_mut::<3>().0 {
                pixel.swap(0, 2);
            }
        }
        Ok(GlyphRasterImage::new(
            if gray {
                GlyphContent::Mask
            } else {
                GlyphContent::SubpixelMask
            },
            Placement {
                left: bounds.left,
                top: -bounds.top,
                width,
                height,
            },
            data,
        ))
    }
}

#[test]
fn ordinary_windows_glyphs_preserve_native_coverage_through_the_real_cache() {
    // This compares the actual cache used by measurement and drawing with the native
    // rasterizer. Matching font metrics alone missed softer pre-composition coverage.
    let reference = Reference::new().unwrap();
    let mut fonts = font_system();
    let mut context = TextContext::new();
    for size in [14.0, 17.5, 28.0] {
        let layout = context.layout(&mut fonts, TextLayoutOptions::new("Hnev", size));
        assert_eq!(layout.glyphs().len(), 4);
        for glyph in layout.glyphs() {
            for (x_bin, y_bin) in [
                (SubpixelBin::Zero, SubpixelBin::Zero),
                (SubpixelBin::One, SubpixelBin::Two),
                (SubpixelBin::Three, SubpixelBin::One),
            ] {
                for flags in [
                    cosmic_text::CacheKeyFlags::empty(),
                    cosmic_text::CacheKeyFlags::FAKE_ITALIC,
                    cosmic_text::CacheKeyFlags::DISABLE_HINTING,
                    cosmic_text::CacheKeyFlags::PIXEL_FONT,
                    cosmic_text::CacheKeyFlags::FAKE_ITALIC
                        | cosmic_text::CacheKeyFlags::PIXEL_FONT
                        | cosmic_text::CacheKeyFlags::DISABLE_HINTING,
                ] {
                    let key = CacheKey {
                        x_bin,
                        y_bin,
                        flags: glyph.cache_key.flags | flags,
                        ..glyph.cache_key
                    };
                    for mode in [
                        TextSubpixelMode::None,
                        TextSubpixelMode::Rgb,
                        TextSubpixelMode::Bgr,
                    ] {
                        context
                            .set_raster_options(TextRasterOptions::new().with_subpixel_mode(mode));
                        let actual = context.glyph_image(&mut fonts, key).unwrap();
                        let expected = reference.image(key, mode).unwrap();
                        assert_eq!(
                            placement(actual.placement),
                            placement(expected.placement),
                            "native coverage footprint: {key:?} {mode:?}"
                        );
                        assert_eq!(actual.content, expected.content);
                        assert_eq!(
                            actual.data, expected.data,
                            "native antialiased samples: {key:?} {mode:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn explicit_expansion_preserves_the_portable_outline_contract() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new("H", 17.5));
    let key = layout.glyphs()[0].cache_key;
    for mode in [
        TextSubpixelMode::None,
        TextSubpixelMode::Rgb,
        TextSubpixelMode::Bgr,
    ] {
        context.set_raster_options(
            TextRasterOptions::new()
                .with_subpixel_mode(mode)
                .with_alpha_mask_embolden(0.125)
                .with_subpixel_mask_embolden(0.125),
        );
        let actual = context.glyph_image(&mut fonts, key).unwrap();
        let expected = crate::text::raster::raster_glyph_image(
            &mut fonts,
            &mut swash::scale::ScaleContext::new(),
            key,
            mode,
            0.125,
        )
        .unwrap();
        assert_eq!(actual.data, expected.data);
        assert_eq!(placement(actual.placement), placement(expected.placement));
    }
}

#[test]
fn native_empty_glyphs_have_no_raster_image() {
    let mut fonts = font_system();
    let mut context = TextContext::new();
    let layout = context.layout(&mut fonts, TextLayoutOptions::new(" ", 14.0));
    let key = layout.glyphs()[0].cache_key;
    for mode in [
        TextSubpixelMode::None,
        TextSubpixelMode::Rgb,
        TextSubpixelMode::Bgr,
    ] {
        context.set_raster_options(TextRasterOptions::new().with_subpixel_mode(mode));
        assert!(context.glyph_image(&mut fonts, key).is_none());
        assert!(context.glyph_image(&mut fonts, key).is_none());
    }
}

fn placement(p: Placement) -> (i32, i32, u32, u32) {
    (p.left, p.top, p.width, p.height)
}
