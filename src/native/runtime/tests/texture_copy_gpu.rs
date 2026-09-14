use super::{Result, four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    compute::{ComputeBatch, TextureCopy},
    program::filter::{self, BasicFilter},
};
use crate::shared::filter_config::FilterConfig;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_texture_copy_preserves_regions_layers_and_raw_bytes() -> Result<()> {
    let routes = Routes::new()?;
    let mut batch = ComputeBatch::new();
    let pixels: Vec<u8> = (0..8 * 6 * 3 * 4).map(|i| (i * 17 + 11) as u8).collect();
    let source = batch.texture_array_rgba8([8, 6, 3], pixels.clone())?;
    let mut expected = vec![0xcd; 10 * 8 * 4 * 4];
    let destination = batch.texture_array_rgba8([10, 8, 4], expected.clone())?;
    batch.copy_texture(TextureCopy {
        source,
        destination,
        source_origin: [2, 1, 1],
        destination_origin: [4, 3, 0],
        extent: [3, 2, 2],
    })?;
    for layer in 0..2 {
        for y in 0..2 {
            let src = (((layer + 1) * 6 + y + 1) * 8 + 2) * 4;
            let dst = ((layer * 8 + y + 3) * 10 + 4) * 4;
            expected[dst..dst + 12].copy_from_slice(&pixels[src..src + 12]);
        }
    }
    let clone = batch.texture_array_rgba8([10, 8, 4], vec![0; expected.len()])?;
    batch.copy_texture(TextureCopy {
        source: destination,
        destination: clone,
        source_origin: [0; 3],
        destination_origin: [0; 3],
        extent: [10, 8, 4],
    })?;
    let selected = batch.texture_rgba8([3, 2], vec![0; 3 * 2 * 4])?;
    batch.copy_texture(TextureCopy {
        source: clone,
        destination: selected,
        source_origin: [4, 3, 1],
        destination_origin: [0; 3],
        extent: [3, 2, 1],
    })?;
    let selected_pixels: Vec<u8> = (0..2)
        .flat_map(|y| {
            let start = ((8 + 3 + y) * 10 + 4) * 4;
            expected[start..start + 12].iter().copied()
        })
        .collect();
    assert!(batch.passes().is_empty());
    batch.readback(clone)?;
    batch.readback(selected)?;
    routes.check(
        &batch,
        &[expected, selected_pixels],
        "pure GPU array subregion copies",
    )?;
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_texture_copy_orders_compute_and_transfer_accesses() -> Result<()> {
    let routes = Routes::with_features(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)?;
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([4, 3], vec![0; 4 * 3 * 4])?;
    let atlas = batch.texture_array_rgba8([6, 5, 2], vec![0xcd; 6 * 5 * 2 * 4])?;
    let config = FilterConfig {
        width: 4,
        height: 3,
        region_width: 4,
        region_height: 3,
        ..Default::default()
    };
    let colors = [[17, 33, 65, 127], [91, 12, 3, 191]];
    for (layer, color) in colors.iter().enumerate() {
        filter::encode(
            &mut batch,
            BasicFilter::Clear,
            FilterConfig {
                clear_color: u32::from_le_bytes(*color),
                ..config
            },
            None,
            None,
            source,
        )?;
        batch.copy_texture(TextureCopy {
            source,
            destination: atlas,
            source_origin: [0; 3],
            destination_origin: [1, 1, layer as u32],
            extent: [4, 3, 1],
        })?;
    }
    batch.copy_texture(TextureCopy {
        source: atlas,
        destination: source,
        source_origin: [1, 1, 0],
        destination_origin: [0; 3],
        extent: [4, 3, 1],
    })?;
    let result = batch.texture_rgba8([4, 3], vec![0; 4 * 3 * 4])?;
    filter::encode(
        &mut batch,
        BasicFilter::Copy,
        config,
        None,
        Some(source),
        result,
    )?;
    batch.readback(result)?;
    batch.readback(atlas)?;
    let mut expected_atlas = vec![0xcd; 6 * 5 * 2 * 4];
    for (layer, color) in colors.iter().enumerate() {
        for y in 1..4 {
            for x in 1..5 {
                let index = ((layer * 5 + y) * 6 + x) * 4;
                expected_atlas[index..index + 4].copy_from_slice(color);
            }
        }
    }
    for portable in [false, true] {
        routes.check_variant(
            &batch,
            &[colors[0].repeat(12), expected_atlas.clone()],
            "compute/transfer ordering",
            Some(FilterVariant {
                portable,
                texture_table: false,
            }),
        )?;
    }
    routes.validate()
}
