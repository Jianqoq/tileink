use super::fine_fixture::{self, FineFixture};
use crate::native::runtime::Result;
use crate::shared::{
    gpu_coarse::{PtclRecord, TileCoarseRecord},
    gpu_constants::{FINE_WORKGROUP_SIZE, TILE_SIZE},
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_fine_spills_preserve_tile_lane_and_inactive_regions() -> Result<()> {
    let routes = fine_fixture::routes()?;
    let tags = super::hlsl_constants::read_hlsl("shared/particle_tags.hlsli")?;
    let constants = super::hlsl_constants::read_hlsl("fine/constants.hlsli")?;
    let sdf = super::hlsl_constants::read_hlsl("shared/sdf/constants.hlsli")?;
    let lanes = FINE_WORKGROUP_SIZE as usize;
    for groups in [false, true] {
        let mut scene = FineFixture::tile(&[]);
        scene.config.width = 3 * TILE_SIZE;
        scene.config.tiles_width = 3;
        scene.config.tile_count = 3;
        scene.config.active_tile_count = 2;
        scene.config.incremental = 1;
        let depth = constants[if groups {
            "FINE_LOCAL_GROUP_DEPTH"
        } else {
            "FINE_LOCAL_CLIP_DEPTH"
        }] + 2;
        let fields = if groups {
            constants["FINE_GROUP_SPILL_FIELDS"] as usize
        } else {
            1
        };
        let prefix = if groups { 7 } else { 0 };
        if groups {
            scene.config.group_spill_base = prefix;
            scene.config.group_spill_depth = 3;
        } else {
            scene.config.clip_spill_depth = 3;
        }
        let mut particles = Vec::new();
        let mut tiles = Vec::new();
        scene.paint.clear();
        for tile in 0..3u32 {
            let (mut draw, _) = fine_fixture::draw(0xffffffff)?;
            draw.sdf_offset = scene.paint.len() as u32;
            draw.sdf_len = 17;
            draw.inverse_transform.e = -((tile * TILE_SIZE) as f32);
            let mut geometry = [0u32; 17];
            geometry[0] = sdf["SDF_KIND_RECT"];
            geometry[1] = (-100.0f32).to_bits();
            geometry[2] = (-100.0f32).to_bits();
            geometry[3] = (4.25 + tile as f32 * 2.0).to_bits();
            geometry[4] = 100.0f32.to_bits();
            scene.paint.extend(geometry);
            scene.draws.push(draw);
            let start = particles.len() as u32;
            let record = |name: &str, backdrop, color| PtclRecord {
                tag: tags[name],
                backdrop,
                color,
                ..Default::default()
            };
            if !groups {
                particles.push(record("PTCL_BEGIN_SDF_CLIP", 0, tile));
            }
            for _ in 0..depth {
                if groups {
                    particles.push(record("PTCL_SDF", 0, tile));
                }
                particles.push(record(
                    if groups {
                        "PTCL_BEGIN_OPACITY"
                    } else {
                        "PTCL_BEGIN_CLIP"
                    },
                    1,
                    255,
                ));
            }
            // Stop with occupied stacks to inspect each written slot directly;
            // the separate nested-stack suite verifies all matching pop behavior.
            particles.push(record("PTCL_COLOR", 0, 0xffffffff));
            particles.push(record("PTCL_END", 0, 0));
            let end = particles.len() as u32;
            tiles.push(TileCoarseRecord {
                ptcl_start: start,
                ptcl_end: end,
                ptcl_count: end - start,
                ..Default::default()
            });
        }
        scene.config.paint_brush_base = scene.paint.len() as u32;
        scene.paint.extend(fine_fixture::draw(0xffffffff)?.1);
        scene.config.ptcl_capacity = particles.len() as u32;
        scene.coarse = bytemuck::cast_slice(&tiles).to_vec();
        scene
            .coarse
            .extend(bytemuck::cast_slice::<_, u32>(&particles));
        scene.config.fine_tile_kind_base = scene.coarse.len() as u32;
        scene.coarse.extend([0; 3]);
        scene.config.active_tile_list_base = scene.coarse.len() as u32;
        scene.coarse.extend([2, 0]);
        scene.spills = vec![0x37373737; prefix as usize + 3 * 3 * lanes * fields + 7];
        let mut expected_spills = scene.spills.clone();
        let mut pixels = vec![0u8; (scene.config.width * scene.config.height * 4) as usize];
        for tile in [0usize, 2] {
            for lane in 0..lanes {
                let x = lane % TILE_SIZE as usize;
                let y = lane / TILE_SIZE as usize;
                let edge = 4 + tile * 2;
                let alpha = if x < edge {
                    255u32
                } else if x == edge {
                    64
                } else {
                    0
                };
                let parent = alpha * 0x01010101;
                let pixel = if groups { 0xffffffffu32 } else { parent };
                let offset = (y * scene.config.width as usize + tile * TILE_SIZE as usize + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&pixel.to_le_bytes());
                let written = if groups { 2 } else { 3 };
                for d in 0..written {
                    let base = prefix as usize + ((tile * 3 + d) * lanes + lane) * fields;
                    if groups {
                        expected_spills[base..base + fields].copy_from_slice(&[
                            tags["PTCL_BEGIN_OPACITY"],
                            parent,
                            255,
                            255,
                            255,
                        ]);
                    } else {
                        expected_spills[base] = alpha;
                    }
                }
            }
        }
        let batch = scene.batch()?;
        for variant in super::reference::FineVariant::ALL {
            routes.check_fine(
                &batch,
                &[
                    pixels.clone(),
                    bytemuck::cast_slice(&expected_spills).to_vec(),
                ],
                &format!("fine tile/lane spill groups={groups} {variant:?}"),
                variant,
            )?;
        }
    }
    routes.assert_reference_pipeline_builds(super::reference::FineVariant::ALL.len());
    routes.validate()
}
