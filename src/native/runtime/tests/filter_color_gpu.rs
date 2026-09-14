use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    program::filter::{self, BasicFilter},
};
use crate::shared::filter_config::FilterConfig;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_color_filters_and_matrices_match_production_numeric_corpus() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let extent = [257, 3];
    let pixels: Vec<u8> = (0..extent[0] * extent[1])
        .flat_map(|i| {
            let alpha = (i % 256) as u8;
            [
                ((i * 37) % u32::from(alpha.max(1))) as u8,
                ((i * 71) % u32::from(alpha.max(1))) as u8,
                ((i * 113) % u32::from(alpha.max(1))) as u8,
                alpha,
            ]
        })
        .collect();
    let mut batch = ComputeBatch::new();
    let mut configs = Vec::new();
    for kind in 1..=8 {
        for amount in [-1.0, 0.0, 0.25, 0.5, 1.0, 2.0, 45.0, 90.0] {
            let config = FilterConfig {
                width: extent[0],
                height: extent[1],
                region_width: extent[0],
                region_height: extent[1],
                dispatch_width: 2,
                filter_kind: kind,
                amount,
                ..Default::default()
            };
            configs.push((BasicFilter::Color, config));
        }
    }
    for bias in [0.0, 0.125, -0.25] {
        let config = FilterConfig {
            width: extent[0],
            height: extent[1],
            region_width: extent[0],
            region_height: extent[1],
            dispatch_width: 2,
            matrix_r: [0.5, 0.25, 0.125, 0.0],
            matrix_g: [0.0, 0.75, 0.25, 0.0],
            matrix_b: [0.25, 0.0, 0.5, 0.0],
            matrix_a: [0.0, 0.0, 0.0, 1.0],
            matrix_bias: [bias; 4],
            ..Default::default()
        };
        configs.push((BasicFilter::ColorMatrix, config));
    }
    for &(kernel, config) in &configs {
        let target = batch.texture_rgba8(extent, pixels.clone())?;
        filter::encode(&mut batch, kernel, config, None, None, target)?;
        batch.readback(target)?;
    }
    let expected = routes.filter_reference_output(
        &batch,
        FilterVariant {
            portable: false,
            texture_table: false,
        },
    )?;
    // Independent exact semantic checks on opacity endpoints; no tolerance or skipped GPU comparisons.
    for (i, &(kernel, c)) in configs.iter().enumerate() {
        if kernel == BasicFilter::Color && c.filter_kind == 6 {
            if c.amount <= 0.0 {
                assert!(expected[i].iter().all(|b| *b == 0));
            }
            if c.amount >= 1.0 {
                assert_eq!(expected[i], pixels);
            }
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            let variant = FilterVariant {
                portable,
                texture_table,
            };
            routes.check_variant(
                &batch,
                &expected,
                &format!("color numeric {variant:?}"),
                Some(variant),
            )?;
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_color_half_channel_regressions() -> Result<()> {
    let routes = Routes::with_features(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)?;
    for (kernel, pixel, kind, amount, matrix_g) in [
        (BasicFilter::Color, [150, 6, 14, 158], 5, 0.25, [0.0; 4]),
        (BasicFilter::Color, [64, 144, 116, 196], 8, 1.0, [0.0; 4]),
        (
            BasicFilter::ColorMatrix,
            [10, 12, 10, 19],
            0,
            0.0,
            [0.0, 0.75, 0.25, 0.0],
        ),
    ] {
        let mut batch = ComputeBatch::new();
        let target = batch.texture_rgba8([1, 1], pixel.to_vec())?;
        let config = FilterConfig {
            width: 1,
            height: 1,
            region_width: 1,
            region_height: 1,
            filter_kind: kind,
            amount,
            matrix_r: [1.0, 0.0, 0.0, 0.0],
            matrix_g,
            matrix_b: [0.0, 0.0, 1.0, 0.0],
            matrix_a: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        };
        filter::encode(&mut batch, kernel, config, None, None, target)?;
        batch.readback(target)?;
        let variant = FilterVariant {
            portable: false,
            texture_table: false,
        };
        let expected = vec![match kernel {
            BasicFilter::Color if kind == 5 => vec![115, 43, 47, 158],
            BasicFilter::Color => vec![158, 141, 110, 196],
            _ => vec![10, 12, 10, 19],
        }];
        routes.check_variant(
            &batch,
            &expected,
            &format!("half channel {kernel:?} kind {kind}"),
            Some(variant),
        )?;
    }
    routes.validate()
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_color_matrix_alpha_cross_terms_and_clamping_match_cpu_semantics() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut expected = Vec::new();
    for pixel in [
        [0u8; 4],
        [64, 32, 16, 128],
        [255, 127, 63, 255],
        [1, 1, 0, 2],
    ] {
        for alpha_bias in [-1.0, 0.0, 0.5, 2.0] {
            let config = FilterConfig {
                width: 1,
                height: 1,
                region_width: 1,
                region_height: 1,
                matrix_r: [0.25, -0.25, 0.5, 0.25],
                matrix_g: [0.5, 0.5, 0.0, -0.25],
                matrix_b: [-1.0, 0.0, 2.0, 0.75],
                matrix_a: [0.5, 0.25, 0.0, 0.5],
                matrix_bias: [0.25, 0.5, 0.75, alpha_bias],
                ..Default::default()
            };
            let target = batch.texture_rgba8([1, 1], pixel.to_vec())?;
            filter::encode(
                &mut batch,
                BasicFilter::ColorMatrix,
                config,
                None,
                None,
                target,
            )?;
            batch.readback(target)?;
            // Independent straight-RGBA f64 definition, not the shader's premultiplied algebra.
            let a = f64::from(pixel[3]);
            let rgba = if a == 0.0 {
                [0.0; 4]
            } else {
                [
                    f64::from(pixel[0]) / a,
                    f64::from(pixel[1]) / a,
                    f64::from(pixel[2]) / a,
                    a / 255.0,
                ]
            };
            let transform = |row: [f32; 4], bias: f32| {
                row.iter()
                    .zip(rgba)
                    .map(|(coefficient, value)| f64::from(*coefficient) * value)
                    .sum::<f64>()
                    + f64::from(bias)
            };
            let output_alpha = transform(config.matrix_a, config.matrix_bias[3]).clamp(0.0, 1.0);
            let mut bytes = Vec::new();
            for (row, bias) in [config.matrix_r, config.matrix_g, config.matrix_b]
                .into_iter()
                .zip(config.matrix_bias)
            {
                bytes.push(
                    (transform(row, bias).clamp(0.0, 1.0) * output_alpha * 255.0 + 0.5) as u8,
                );
            }
            bytes.push((output_alpha * 255.0 + 0.5) as u8);
            expected.push(bytes);
        }
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "matrix alpha cross terms and clipping",
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
fn four_api_matrix_preserves_half_channel_when_alpha_is_unchanged() -> Result<()> {
    let routes = Routes::with_features(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)?;
    let mut batch = ComputeBatch::new();
    let target = batch.texture_rgba8([1, 1], vec![255, 127, 63, 255])?;
    let config = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        matrix_b: [-1.0, 0.0, 2.0, 0.75],
        matrix_bias: [0.0, 0.0, 0.75, 1.0],
        ..Default::default()
    };
    filter::encode(
        &mut batch,
        BasicFilter::ColorMatrix,
        config,
        None,
        None,
        target,
    )?;
    batch.readback(target)?;
    routes.check_variant(
        &batch,
        &[vec![0, 0, 254, 255]],
        "unchanged alpha preserves exact blue 253.5",
        Some(FilterVariant {
            portable: false,
            texture_table: false,
        }),
    )?;
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_matrix_opaque_input_and_saturated_output_preserve_rounding() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let pixels = (0..=255u8).flat_map(|r| [r, 0, 0, 255]).collect();
    let target = batch.texture_rgba8([256, 1], pixels)?;
    let config = FilterConfig {
        width: 256,
        height: 1,
        region_width: 256,
        region_height: 1,
        matrix_a: [0.3, 0.0, 0.0, 0.0],
        matrix_bias: [1.0, 0.0, 0.0, 0.0],
        ..Default::default()
    };
    filter::encode(
        &mut batch,
        BasicFilter::ColorMatrix,
        config,
        None,
        None,
        target,
    )?;
    batch.readback(target)?;
    // Opaque input has no unpremultiplication; saturated red equals output alpha.
    // An approximate reciprocal must not perturb either exact identity.
    let expected = (0..=255u8)
        .flat_map(|r| {
            let alpha = (f64::from(0.3f32) * f64::from(r) + 0.5) as u8;
            [alpha, 0, 0, alpha]
        })
        .collect();
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                std::slice::from_ref(&expected),
                "opaque matrix and saturated red",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
