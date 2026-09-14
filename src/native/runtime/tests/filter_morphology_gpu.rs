use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{Result, compute::ComputeBatch, program::filter::morphology};
use crate::shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE};

#[test]
fn morphology_rejects_invalid_axis_operator_and_overflow() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([3, 5], vec![0; 60])?;
    let target = batch.texture_rgba8([3, 5], vec![0; 60])?;
    let c = FilterConfig {
        width: 3,
        height: 5,
        region_width: 3,
        region_height: 5,
        ..Default::default()
    };
    for invalid in [
        FilterConfig {
            morphology_axis: 2,
            ..c
        },
        FilterConfig {
            morphology_operator: 2,
            ..c
        },
        FilterConfig {
            morphology_radius: u32::MAX,
            ..c
        },
    ] {
        assert!(morphology::encode(&mut batch, invalid, None, source, target).is_err());
    }
    assert!(batch.passes().is_empty());
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_morphology_matches_rational_straight_channel_semantics() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    for [width, height] in [[1u32, 3u32], [5, 7], [33, 17]] {
        let source: Vec<u8> = (0..width * height)
            .flat_map(|i| {
                let a = (i * 43 % 256) as u8;
                let d = u32::from(a) + 1;
                [
                    (i * 11 % d) as u8,
                    (i * 37 % d) as u8,
                    (i * 71 % d) as u8,
                    a,
                ]
            })
            .collect();
        let mut batch = ComputeBatch::new();
        let input = batch.texture_rgba8([width, height], source.clone())?;
        let tiles: Vec<_> = (0..width.div_ceil(TILE_SIZE) * height.div_ceil(TILE_SIZE))
            .rev()
            .filter(|i| i % 2 == 0)
            .collect();
        let mut expected = Vec::new();
        for axis in [0, 1] {
            for operator in [0, 1] {
                for radius in [0, 1, 3, 40] {
                    for compact in [false, true] {
                        let c = FilterConfig {
                            width,
                            height,
                            region_x0: width / 3,
                            region_y0: 1,
                            region_width: width - width / 3,
                            region_height: height - 1,
                            dispatch_width: 2,
                            morphology_axis: axis,
                            morphology_operator: operator,
                            morphology_radius: radius,
                            ..Default::default()
                        };
                        let target =
                            batch.texture_rgba8([width, height], vec![57; source.len()])?;
                        morphology::encode(
                            &mut batch,
                            c,
                            compact.then_some(tiles.as_slice()),
                            input,
                            target,
                        )?;
                        batch.readback(target)?;
                        let mut out = vec![57; source.len()];
                        for y in c.region_y0..height {
                            for x in c.region_x0..width {
                                if compact
                                    && !tiles.contains(
                                        &(y / TILE_SIZE * width.div_ceil(TILE_SIZE)
                                            + x / TILE_SIZE),
                                    )
                                {
                                    continue;
                                }
                                let pos = if axis == 0 { x } else { y };
                                let len = if axis == 0 { width } else { height };
                                let ix = ((y * width + x) * 4) as usize;
                                if operator == 0 && (pos < radius || pos + radius >= len) {
                                    out[ix..ix + 4].fill(0);
                                    continue;
                                }
                                // Compare rational straight channels exactly before independently rounding the
                                // selected ratio multiplied by the selected output alpha.
                                let mut ratios = if operator == 0 {
                                    [(1u32, 1u32); 3]
                                } else {
                                    [(0, 1); 3]
                                };
                                let mut alpha = if operator == 0 { 255u32 } else { 0 };
                                for sample in
                                    pos.saturating_sub(radius)..=(pos + radius).min(len - 1)
                                {
                                    let (sx, sy) =
                                        if axis == 0 { (sample, y) } else { (x, sample) };
                                    let i = ((sy * width + sx) * 4) as usize;
                                    let a = u32::from(source[i + 3]);
                                    alpha = if operator == 0 {
                                        alpha.min(a)
                                    } else {
                                        alpha.max(a)
                                    };
                                    for lane in 0..3 {
                                        let ratio = if a == 0 {
                                            (0, 1)
                                        } else {
                                            (u32::from(source[i + lane]), a)
                                        };
                                        let old = ratios[lane];
                                        let less = ratio.0 * old.1 < old.0 * ratio.1;
                                        let greater = ratio.0 * old.1 > old.0 * ratio.1;
                                        if (operator == 0 && less) || (operator == 1 && greater) {
                                            ratios[lane] = ratio;
                                        }
                                    }
                                }
                                for lane in 0..3 {
                                    let (n, d) = ratios[lane];
                                    out[ix + lane] = ((n * alpha + d / 2) / d) as u8;
                                }
                                out[ix + 3] = alpha as u8;
                            }
                        }
                        expected.push(out);
                    }
                }
            }
        }
        for portable in [false, true] {
            for texture_table in [false, true] {
                routes.check_variant(
                    &batch,
                    &expected,
                    "morphology rational semantics",
                    Some(FilterVariant {
                        portable,
                        texture_table,
                    }),
                )?;
            }
        }
    }
    routes.validate()
}
