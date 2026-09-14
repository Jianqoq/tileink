use super::fine_fixture::{self, FineFixture};
use crate::native::runtime::Result;
use crate::shared::{
    gpu_coarse::{FineTileKind, PtclRecord},
    gpu_constants::TILE_SIZE,
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_fine_sdf_offsets_inverse_jacobian_and_sdf_clips() -> Result<()> {
    let routes = fine_fixture::routes()?;
    let tags = super::hlsl_constants::read_hlsl("shared/particle_tags.hlsli")?;
    let sdf = super::hlsl_constants::read_hlsl("shared/sdf/constants.hlsli")?;
    let particle = |name: &str, color| PtclRecord {
        tag: tags[name],
        color,
        ..Default::default()
    };
    for shadow in [false, true] {
        for scaled in [false, true] {
            for clip in [false, true] {
                let particles = if clip {
                    vec![
                        particle("PTCL_BEGIN_SDF_CLIP", 0),
                        particle("PTCL_COLOR", 0xffffffff),
                        particle("PTCL_END_CLIP", 0),
                        particle("PTCL_END", 0),
                    ]
                } else {
                    vec![particle("PTCL_SDF", 0), particle("PTCL_END", 0)]
                };
                let mut scene = FineFixture::tile(&particles);
                let (mut draw, brush) = fine_fixture::draw(0xffffffff)?;
                let mut geometry = [0u32; 17];
                geometry[0] = sdf[if shadow {
                    "SDF_KIND_RECT_SHADOW"
                } else {
                    "SDF_KIND_RECT"
                }];
                geometry[1] = (-100.0f32).to_bits();
                geometry[2] = (-100.0f32).to_bits();
                geometry[3] = (if scaled { 4.25f32 } else { 8.25f32 }).to_bits();
                geometry[4] = (100.0f32).to_bits();
                geometry[16] = (1.0f32).to_bits();
                scene.paint = vec![0xdeadbeef; 9];
                scene.paint.extend(geometry);
                scene.config.paint_brush_base = scene.paint.len() as u32;
                scene.paint.extend(brush);
                if shadow {
                    draw.sdf_shadow_offset = 4;
                    draw.sdf_shadow_len = geometry.len() as u32;
                    scene.config.paint_sdf_shadow_base = 5;
                } else {
                    draw.sdf_offset = 9;
                    draw.sdf_len = geometry.len() as u32;
                }
                if scaled {
                    draw.inverse_transform.a = 0.5;
                    draw.inverse_transform.d = 2.0;
                    draw.inverse_transform.e = 0.125;
                    draw.transform.a = 2.0;
                    draw.transform.d = 0.5;
                    draw.transform.e = -0.25;
                }
                scene.draws.push(draw);
                let expected: Vec<u8> = (0..TILE_SIZE * TILE_SIZE)
                    .flat_map(|p| {
                        let x = p % TILE_SIZE;
                        let coverage = if x < 8 {
                            255u8
                        } else if x == 8 {
                            64
                        } else {
                            0
                        };
                        [coverage; 4]
                    })
                    .collect();
                for kind in [
                    FineTileKind::FullInterpreter,
                    FineTileKind::PureSdfSolidNoStack,
                    FineTileKind::MixedAnalyticSolidNoStack,
                ] {
                    scene.coarse[scene.config.fine_tile_kind_base as usize] = kind as u32;
                    let batch = scene.batch()?;
                    for variant in super::reference::FineVariant::ALL {
                        routes.check_fine(&batch,&[expected.clone(),bytemuck::cast_slice(&scene.spills).to_vec()],
                    &format!("fine SDF shadow={shadow} scaled={scaled} clip={clip} {kind:?} {variant:?}"),variant)?;
                    }
                }
            }
        }
    }
    routes.assert_reference_pipeline_builds(super::reference::FineVariant::ALL.len());
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_fine_path_glyph_uses_linear_light_composition() -> Result<()> {
    let routes = fine_fixture::routes()?;
    let tags = super::hlsl_constants::read_hlsl("shared/particle_tags.hlsli")?;
    for name in ["PTCL_FILL", "PTCL_PATH_GLYPH"] {
        let mut scene = FineFixture::tile(&[
            PtclRecord {
                tag: tags[name],
                backdrop: 1,
                ..Default::default()
            },
            PtclRecord::default(),
        ]);
        let (draw, paint) = fine_fixture::draw(0x80404040)?;
        scene.draws.push(draw);
        scene.paint = paint;
        scene.config.clear_color = 0xff000000;
        // An independent sRGB transfer distinguishes text's linear-light over
        // from an ordinary half-transparent gray fill (stored channel 64).
        let linear = ((0.5 + 0.055f64) / 1.055).powf(2.4) * (128.0 / 255.0);
        let channel = if name == "PTCL_PATH_GLYPH" {
            ((1.055 * linear.powf(1.0 / 2.4) - 0.055) * 255.0).round() as u8
        } else {
            64
        };
        let batch = scene.batch()?;
        let expected = vec![
            [channel, channel, channel, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
            bytemuck::cast_slice(&scene.spills).to_vec(),
        ];
        for variant in super::reference::FineVariant::ALL {
            routes.check_fine(
                &batch,
                &expected,
                &format!("fine {name} {variant:?}"),
                variant,
            )?;
        }
    }
    routes.assert_reference_pipeline_builds(super::reference::FineVariant::ALL.len());
    routes.validate()
}
