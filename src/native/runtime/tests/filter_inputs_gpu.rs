use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    program::filter::inputs::{self, InputFilter},
};
use crate::shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE};

fn config(width: u32, height: u32) -> FilterConfig {
    FilterConfig {
        width,
        height,
        region_width: width,
        region_height: height,
        dispatch_width: 2,
        ..Default::default()
    }
}

#[test]
fn input_filters_validate_resources_and_allow_read_only_aliases() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let input = batch.texture_rgba8([3, 5], vec![0; 60])?;
    let target = batch.texture_rgba8([3, 5], vec![0; 60])?;
    let wrong = batch.buffer(vec![0; 60])?;
    let size_mismatch = batch.texture_rgba8([5, 3], vec![0; 60])?;
    let mut foreign_batch = ComputeBatch::new();
    let foreign = foreign_batch.texture_rgba8([3, 5], vec![0; 60])?;
    let c = config(3, 5);
    for kernel in [
        InputFilter::Blend {
            source: target,
            backdrop: input,
        },
        InputFilter::Composite {
            source: input,
            backdrop: target,
        },
        InputFilter::Mask { mask: target },
        InputFilter::Mask { mask: wrong },
    ] {
        assert!(inputs::encode(&mut batch, kernel, c, None, target).is_err());
    }
    assert!(
        inputs::encode(
            &mut batch,
            InputFilter::Mask { mask: input },
            c,
            Some(&[0, 0]),
            target
        )
        .is_err()
    );
    assert!(
        inputs::encode(
            &mut batch,
            InputFilter::Composite {
                source: input,
                backdrop: input
            },
            FilterConfig {
                matrix_bias: [f32::NAN; 4],
                ..c
            },
            None,
            target
        )
        .is_err()
    );
    for mask in [size_mismatch, foreign] {
        assert!(inputs::encode(&mut batch, InputFilter::Mask { mask }, c, None, target).is_err());
    }
    assert!(batch.passes().is_empty());
    inputs::encode(
        &mut batch,
        InputFilter::Blend {
            source: input,
            backdrop: input,
        },
        c,
        None,
        target,
    )?;
    inputs::encode(
        &mut batch,
        InputFilter::Mask { mask: input },
        c,
        None,
        target,
    )?;
    assert_eq!(batch.passes().len(), 2);
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_input_filters_match_all_modes_and_independent_mask() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [33u32, 17];
    let pixels = |seed: u32| {
        (0..extent[0] * extent[1])
            .flat_map(|i| {
                let alpha = (i * seed % 256) as u8;
                [
                    (i * 37 % u32::from(alpha.max(1))) as u8,
                    (i * 71 % u32::from(alpha.max(1))) as u8,
                    (i * 113 % u32::from(alpha.max(1))) as u8,
                    alpha,
                ]
            })
            .collect::<Vec<_>>()
    };
    let source = pixels(13);
    let backdrop = pixels(29);
    let mut batch = ComputeBatch::new();
    let a = batch.texture_rgba8(extent, source.clone())?;
    let b = batch.texture_rgba8(extent, backdrop.clone())?;
    let tiles = [5, 2, 0];
    let mut masks = Vec::new();
    let mut output_count = 0;
    for compact in [false, true] {
        let active = compact.then_some(tiles.as_slice());
        let c = FilterConfig {
            region_x0: 1,
            region_y0: 1,
            region_width: 31,
            region_height: 15,
            ..config(extent[0], extent[1])
        };
        for mix in 0..16 {
            for compose in 0..14 {
                let target = batch.texture_rgba8(extent, backdrop.clone())?;
                inputs::encode(
                    &mut batch,
                    InputFilter::Blend {
                        source: a,
                        backdrop: b,
                    },
                    FilterConfig {
                        blend_mode: mix | (compose << 8),
                        ..c
                    },
                    active,
                    target,
                )?;
                batch.readback(target)?;
                output_count += 1;
            }
        }
        for operator in 0..6 {
            for coefficients in [
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [1.0, -0.5, 0.5, 0.25],
            ] {
                let target = batch.texture_rgba8(extent, backdrop.clone())?;
                inputs::encode(
                    &mut batch,
                    InputFilter::Composite {
                        source: a,
                        backdrop: b,
                    },
                    FilterConfig {
                        filter_kind: operator,
                        matrix_bias: coefficients,
                        ..c
                    },
                    active,
                    target,
                )?;
                batch.readback(target)?;
                let mut cpu = backdrop.clone();
                for y in c.region_y0..c.region_y0 + c.region_height {
                    for x in c.region_x0..c.region_x0 + c.region_width {
                        let tile = y / TILE_SIZE * extent[0].div_ceil(TILE_SIZE) + x / TILE_SIZE;
                        if compact && !tiles.contains(&tile) {
                            continue;
                        }
                        let i = ((y * extent[0] + x) * 4) as usize;
                        let sa = u32::from(source[i + 3]);
                        let da = u32::from(backdrop[i + 3]);
                        for lane in 0..4 {
                            let a = u32::from(source[i + lane]);
                            let b = u32::from(backdrop[i + lane]);
                            cpu[i + lane] = if operator == 5 {
                                let a = f64::from(a) / 255.0;
                                let b = f64::from(b) / 255.0;
                                let k = coefficients.map(f64::from);
                                ((k[0] * a * b + k[1] * a + k[2] * b + k[3]).clamp(0.0, 1.0)
                                    * 255.0
                                    + 0.5) as u8
                            } else {
                                let (sf, df) = match operator {
                                    1 => (da, 0),
                                    2 => (255 - da, 0),
                                    3 => (da, 255 - sa),
                                    4 => (255 - da, 255 - sa),
                                    _ => (255, 255 - sa),
                                };
                                ((a * sf + b * df + 127) / 255).min(255) as u8
                            };
                        }
                    }
                }
                masks.push((output_count, cpu));
                output_count += 1;
            }
        }
        let target = batch.texture_rgba8(extent, backdrop.clone())?;
        inputs::encode(&mut batch, InputFilter::Mask { mask: a }, c, active, target)?;
        // Repeat to require a fresh portable snapshot of the preceding GPU result.
        inputs::encode(&mut batch, InputFilter::Mask { mask: a }, c, active, target)?;
        batch.readback(target)?;
        let mut expected = backdrop.clone();
        for y in c.region_y0..c.region_y0 + c.region_height {
            for x in c.region_x0..c.region_x0 + c.region_width {
                let tile = y / TILE_SIZE * extent[0].div_ceil(TILE_SIZE) + x / TILE_SIZE;
                if compact && !tiles.contains(&tile) {
                    continue;
                }
                let i = ((y * extent[0] + x) * 4) as usize;
                let mask = u32::from(source[i + 3]);
                let alpha = u32::from(backdrop[i + 3]);
                let once = (alpha * mask + 127) / 255;
                let twice = ((once * mask + 127) / 255) as u8;
                expected[i..i + 4].fill(twice);
            }
        }
        masks.push((output_count, expected));
        output_count += 1;
    }
    let expected = routes.filter_reference_output(
        &batch,
        FilterVariant {
            portable: false,
            texture_table: false,
        },
    )?;
    for (index, pixels) in masks {
        assert_eq!(
            expected[index], pixels,
            "independent composite/mask semantics"
        );
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "input filters",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_hue_source_out_half_channel_regression() -> Result<()> {
    let routes = Routes::with_features(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)?;
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([1, 1], vec![6, 60, 78, 138])?;
    let backdrop = batch.texture_rgba8([1, 1], vec![102, 136, 68, 170])?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    inputs::encode(
        &mut batch,
        InputFilter::Blend { source, backdrop },
        FilterConfig {
            blend_mode: 12 | (7 << 8),
            ..config(1, 1)
        },
        None,
        target,
    )?;
    batch.readback(target)?;
    // Decimal hue luminance yields blue 35.5; backend contraction must not choose different bytes.
    routes.check_variant(
        &batch,
        &[vec![15, 30, 36, 46]],
        "hue source-out half blue",
        Some(FilterVariant {
            portable: false,
            texture_table: false,
        }),
    )?;
    routes.validate()
}
