use super::fine_fixture::{self, FineFixture};
use crate::native::runtime::Result;
use crate::native::runtime::compute::ComputeBatch;
use crate::shared::gpu_coarse::PtclRecord;
use crate::shared::{
    bounds::PixelBounds,
    gpu_constants::TILE_SIZE,
    gpu_text::{GLYPH_IMAGE_RECORD_WORDS, GLYPH_RECORD_WORDS, GlyphImageRecord, GlyphRecord},
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_fine_glyph_formats_transform_clip_and_empty_references() -> Result<()> {
    let routes = fine_fixture::routes()?;
    let constants = super::hlsl_constants::read_hlsl("fine/constants.hlsli")?;
    let tags = super::hlsl_constants::read_hlsl("shared/particle_tags.hlsli")?;
    let linear = intermediate_text_colors(&routes)?;
    for intermediate in [false, true] {
        for name in [
            "GLYPH_MASK",
            "GLYPH_COLOR",
            "GLYPH_SUBPIXEL_MASK",
            "GLYPH_LINEAR_MASK",
            "GLYPH_LINEAR_COLOR",
            "GLYPH_LINEAR_SUBPIXEL_MASK",
        ] {
            let content = constants[name];
            let mut scene = FineFixture::tile(&[
                PtclRecord {
                    tag: tags["PTCL_GLYPH"],
                    segment_end: 2,
                    ..Default::default()
                },
                PtclRecord {
                    tag: tags["PTCL_END"],
                    ..Default::default()
                },
            ]);
            // The glyph list contains an invalid image reference after the valid
            // glyph. It must skip that glyph without reading image index MAX.
            let kind_base = scene.config.fine_tile_kind_base as usize;
            scene.coarse.insert(kind_base, 1);
            scene.config.fine_tile_kind_base += 1;
            scene.config.clear_color = 0xff000000;
            let (mut draw, paint) = fine_fixture::draw(0xffffffff)?;
            draw.pixel_bounds = PixelBounds {
                x0: 3,
                y0: 4,
                x1: 6,
                y1: 6,
            };
            draw.inverse_transform.e = -4.0;
            draw.inverse_transform.f = -2.0;
            draw.transform.e = 4.0;
            draw.transform.f = 2.0;
            scene.draws.push(draw);
            scene.paint = paint;
            let glyphs = [
                GlyphRecord {
                    image_id: 0,
                    x: -2,
                    y: 5,
                },
                GlyphRecord {
                    image_id: u32::MAX,
                    x: 0,
                    y: 0,
                },
            ];
            let image = GlyphImageRecord {
                left: 0,
                top: 4,
                width: 5,
                height: 3,
                content,
                data_offset: 0,
            };
            scene.text = bytemuck::cast_slice(&glyphs).to_vec();
            scene.config.text_image_base = (2 * GLYPH_RECORD_WORDS) as u32;
            scene.text.extend(bytemuck::cast_slice::<_, u32>(&[image]));
            scene.config.text_image_data_base =
                (2 * GLYPH_RECORD_WORDS + GLYPH_IMAGE_RECORD_WORDS) as u32;
            // Binary channel coverage makes all transfer functions' endpoint values
            // exact, while format-specific colors distinguish RGB/color/mask paths.
            let data = if name.ends_with("SUBPIXEL_MASK") {
                0x00ff00ff
            } else if name.ends_with("COLOR") {
                0xff17539b
            } else {
                255
            };
            let data = if intermediate {
                if name.ends_with("SUBPIXEL_MASK") {
                    0x00808080
                } else if name.ends_with("COLOR") {
                    0x80404040
                } else {
                    128
                }
            } else {
                data
            };
            scene.text.extend([data; 15]);
            let mut expected = scene
                .config
                .clear_color
                .to_le_bytes()
                .repeat((TILE_SIZE * TILE_SIZE) as usize);
            let color = if name.ends_with("SUBPIXEL_MASK") {
                0xffff00ffu32
            } else if name.ends_with("COLOR") {
                data
            } else {
                0xffffffff
            };
            let color = if intermediate {
                match name {
                    "GLYPH_MASK" | "GLYPH_SUBPIXEL_MASK" => 0xff808080,
                    "GLYPH_COLOR" => 0xff404040,
                    "GLYPH_LINEAR_MASK" => linear[0],
                    "GLYPH_LINEAR_SUBPIXEL_MASK" => linear[1],
                    "GLYPH_LINEAR_COLOR" => linear[2],
                    _ => unreachable!(),
                }
            } else {
                color
            };
            for y in 4..6usize {
                for x in 3..6usize {
                    let p = (y * TILE_SIZE as usize + x) * 4;
                    expected[p..p + 4].copy_from_slice(&color.to_le_bytes());
                }
            }
            let batch = scene.batch()?;
            for variant in super::reference::FineVariant::ALL {
                routes.check_fine(
                    &batch,
                    &[
                        expected.clone(),
                        bytemuck::cast_slice(&scene.spills).to_vec(),
                    ],
                    &format!("glyph {name} intermediate={intermediate} {variant:?}"),
                    variant,
                )?;
            }
        }
    }
    routes.assert_reference_pipeline_builds(super::reference::FineVariant::ALL.len() + 1);
    routes.validate()
}

// The separately verified text probe supplies perceptual results without going
// through glyph format dispatch. Intermediate cases must distinguish every pair.
fn intermediate_text_colors(routes: &super::four_api::Routes) -> Result<[u32; 3]> {
    let mut batch = ComputeBatch::new();
    let records = [
        0xff000000u32,
        0xffffffff,
        0x808080,
        255,
        0xff000000,
        0x80404040,
        0xffffff,
        255,
    ];
    let input = batch.buffer(bytemuck::cast_slice(&records).to_vec())?;
    let count = batch.buffer(bytemuck::cast_slice(&[2u32, 0, 0, 0]).to_vec())?;
    let output = batch.buffer(vec![0; 10 * 4])?;
    // SAFETY: two four-word requests, two five-word results, and an explicit count.
    unsafe {
        batch.dispatch(
            "text_words",
            &[(9, input), (10, output), (11, count)],
            [1, 1, 1],
        )?;
    }
    batch.readback(output)?;
    let expected = routes.reference_output(&batch)?;
    routes.check(&batch, &expected, "glyph intermediate perceptual oracle")?;
    let word = |i: usize| u32::from_le_bytes(expected[0][i * 4..i * 4 + 4].try_into().unwrap());
    let colors = [word(2), word(4), word(7)];
    assert_ne!(colors[0], 0xff808080, "alpha glyph branches must differ");
    assert_ne!(colors[1], 0xff808080, "LCD glyph branches must differ");
    assert_ne!(colors[2], 0xff404040, "color glyph branches must differ");
    Ok(colors)
}
