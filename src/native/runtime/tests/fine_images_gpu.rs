use super::fine_fixture::{self, FineFixture};
use crate::native::runtime::Result;
use crate::shared::{
    gpu_coarse::{FineTileKind, PtclRecord},
    gpu_constants::TILE_SIZE,
};

pub(super) fn check(routes: &super::four_api::Routes) -> Result<()> {
    let tags = super::hlsl_constants::read_hlsl("shared/particle_tags.hlsli")?;
    let brush = super::hlsl_constants::read_hlsl("shared/brush/constants.hlsli")?;
    let pixels: Vec<u8> = (0..16u32)
        .flat_map(|i| {
            let alpha = 64 + i * 11;
            [(i * 3) as u8, (i * 2) as u8, i as u8, alpha as u8]
        })
        .collect();
    for kind in [
        FineTileKind::FullInterpreter,
        FineTileKind::MixedAnalyticSolidNoStack,
    ] {
        for repeated in [false, true] {
            let mut stored_reference: Option<Vec<u8>> = None;
            for variant in super::reference::FineVariant::ALL {
                let image = PtclRecord {
                    tag: tags["PTCL_IMAGE"],
                    ..Default::default()
                };
                let mut particles = vec![image];
                if repeated {
                    particles.push(image);
                }
                particles.push(PtclRecord {
                    tag: tags["PTCL_END"],
                    ..Default::default()
                });
                let mut scene = FineFixture::tile(&particles);
                scene.coarse[scene.config.fine_tile_kind_base as usize] = kind as u32;
                scene.image = ([4, 4], pixels.clone());
                let (mut draw, _) = fine_fixture::draw(0)?;
                draw.inverse_transform.e = -2.0;
                draw.inverse_transform.f = -3.0;
                scene.draws.push(draw);
                scene.config.paint_brush_base = 7;
                scene.paint = vec![0xdeadbeef; 7];
                scene.paint.extend([
                    brush["BRUSH_PATTERN_RESOURCE"],
                    0,
                    0,
                    0,
                    if variant.texture_table {
                        brush["BRUSH_TEXTURE_PLACEMENT_BIT"]
                    } else {
                        0
                    },
                    4,
                    4,
                    127,
                    0,
                ]);
                // A quarter-turn image transform follows the draw inverse.
                scene.paint.extend(
                    [
                        0.0f32, 0.25, -0.25, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                    ]
                    .map(f32::to_bits),
                );
                let mut expected = Vec::new();
                for y in 0..TILE_SIZE {
                    for x in 0..TILE_SIZE {
                        let ix = (6 - y as i32).clamp(0, 3) as usize;
                        let iy = (x as i32 - 2).clamp(0, 3) as usize;
                        let sample: [u32; 4] = std::array::from_fn(|c| {
                            (u32::from(pixels[(iy * 4 + ix) * 4 + c]) * 127 + 127) / 255
                        });
                        expected.extend(sample.map(|c| {
                            if repeated {
                                f64::from(c) * f64::from(510 - sample[3]) / 255.0
                            } else {
                                f64::from(c)
                            }
                        }));
                    }
                }
                let batch = scene.batch()?;
                let expected: Vec<u8> = if repeated {
                    if stored_reference.is_none() {
                        let result = routes.fine_reference(&batch, variant)?;
                        assert_eq!(result[0].len(), expected.len());
                        // Typed FLOAT->UNORM storage permits 0.6 integer ULP:
                        // D3D functional spec 3.2.3.6. A raw-float diagnostic verified
                        // 3.5372548 before storage and 3 afterward on this GPU.
                        // Correct the ideal-nearest oracle, not the rendering semantics.
                        for (index, (&stored, &ideal)) in
                            result[0].iter().zip(&expected).enumerate()
                        {
                            assert!(
                                (f64::from(stored) - ideal).abs() <= 0.6001,
                                "image composite channel {index}: {stored} vs {ideal}"
                            );
                        }
                        stored_reference = Some(result[0].clone());
                    }
                    // Every API AND texture variant must match these exact bytes;
                    // the independent conversion bound is never a parity tolerance.
                    stored_reference.as_ref().unwrap().clone()
                } else {
                    expected.into_iter().map(|value| value as u8).collect()
                };
                routes.check_fine(
                    &batch,
                    &[expected, bytemuck::cast_slice(&scene.spills).to_vec()],
                    &format!("fine transformed image {kind:?} repeated={repeated} {variant:?}"),
                    variant,
                )?;
            }
        }
    }
    Ok(())
}
