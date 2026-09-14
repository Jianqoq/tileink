use super::four_api::Routes;
use super::reference::FilterVariant;
use crate::native::runtime::Result;
use crate::native::runtime::compute::ComputeBatch;
use crate::native::runtime::program::filter::path_mask;
use crate::shared::filter_config::FilterConfig;

// Exact rational ray crossing in fixed-point units; no floating shader arithmetic.
fn covered(lines: &[[i32; 4]], x: u32, y: u32) -> bool {
    let px = i128::from(x) * 256 + 128;
    let py = i128::from(y) * 256 + 128;
    let mut winding = 0;
    for &[x0, y0, x1, y1] in lines {
        let (x0, y0, x1, y1) = (
            i128::from(x0),
            i128::from(y0),
            i128::from(x1),
            i128::from(y1),
        );
        let delta = if y0 <= py && py < y1 {
            1
        } else if y1 <= py && py < y0 {
            -1
        } else {
            continue;
        };
        let numerator = (x0 - px) * (y1 - y0) + (x1 - x0) * (py - y0);
        if (y1 > y0 && numerator > 0) || (y1 < y0 && numerator < 0) {
            winding += delta;
        }
    }
    winding != 0
}
#[test]
fn path_mask_rejects_invalid_ranges_indices_and_foreign_geometry() -> Result<()> {
    let mut batch = ComputeBatch::new();
    assert!(path_mask::upload(&mut batch, &[], &[]).is_err());
    assert!(
        path_mask::upload(
            &mut batch,
            std::slice::from_ref(&std::ops::Range { start: 1, end: 0 }),
            &[]
        )
        .is_err()
    );
    assert!(
        path_mask::upload(
            &mut batch,
            std::slice::from_ref(&std::ops::Range { start: 0, end: 1 }),
            &[]
        )
        .is_err()
    );
    let paths = path_mask::upload(
        &mut batch,
        std::slice::from_ref(&std::ops::Range { start: 0, end: 0 }),
        &[],
    )?;
    let target = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let c = FilterConfig {
        width: 1,
        height: 1,
        region_width: 1,
        region_height: 1,
        ..Default::default()
    };
    assert!(
        path_mask::encode(
            &mut batch,
            FilterConfig {
                table_index: 1,
                ..c
            },
            None,
            paths,
            target
        )
        .is_err()
    );
    let too_wide = i32::MAX as u32 / crate::shared::gpu_constants::PATH_MASK_COORDINATE_SCALE + 2;
    let error = path_mask::encode(
        &mut batch,
        FilterConfig {
            width: too_wide,
            ..c
        },
        None,
        paths,
        target,
    )
    .unwrap_err();
    assert!(error.to_string().contains("signed fixed-point"));
    let mut foreign = ComputeBatch::new();
    let paths = path_mask::upload(
        &mut foreign,
        std::slice::from_ref(&std::ops::Range { start: 0, end: 0 }),
        &[],
    )?;
    assert!(path_mask::encode(&mut batch, c, None, paths, target).is_err());
    assert!(batch.passes().is_empty());
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_path_masks_match_rational_winding() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut lines = Vec::new();
    let mut ranges = Vec::new();
    for vertices in [
        vec![],
        vec![[1, 1], [29, 1], [29, 15], [1, 15]],
        vec![[1, 1], [1, 15], [29, 15], [29, 1]],
        vec![[-3, 2], [31, 8], [2, 17]],
        vec![[2, 2], [30, 14], [2, 14], [30, 2]],
    ] {
        let start = lines.len() as u32;
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            lines.push([a[0] * 256, a[1] * 256, b[0] * 256, b[1] * 256]);
        }
        ranges.push(start..lines.len() as u32);
    }
    // Opposite loops cancel; the same orientation remains nonzero with winding two.
    ranges.push(ranges[1].start..ranges[2].end);
    let start = lines.len() as u32;
    let loop_lines = lines[ranges[1].start as usize..ranges[1].end as usize].to_vec();
    lines.extend_from_slice(&loop_lines);
    lines.extend_from_slice(&loop_lines);
    ranges.push(start..lines.len() as u32);
    let paths = path_mask::upload(&mut batch, &ranges, &lines)?;
    let mut expected = Vec::new();
    for (index, range) in ranges.iter().enumerate() {
        let c = FilterConfig {
            width: 33,
            height: 17,
            region_width: 33,
            region_height: 17,
            table_index: index as u32,
            ..Default::default()
        };
        let target = batch.texture_rgba8([33, 17], vec![57; 33 * 17 * 4])?;
        path_mask::encode(&mut batch, c, None, paths, target)?;
        batch.readback(target)?;
        let mut pixels = Vec::new();
        for y in 0..17 {
            for x in 0..33 {
                let alpha = if covered(&lines[range.start as usize..range.end as usize], x, y) {
                    255
                } else {
                    0
                };
                pixels.extend_from_slice(&[alpha; 4]);
            }
        }
        expected.push(pixels);
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "path mask rational winding",
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
fn four_api_path_mask_large_fixed_point_crossing() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let mut lines = vec![
        [i32::MAX, 0, -2147483389, 256],
        [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
        [i32::MAX, i32::MIN, i32::MIN, i32::MAX],
        [128, i32::MIN, 128, i32::MAX],
        [129, i32::MIN, 129, i32::MAX],
    ];
    let mut random = 0x12345678u32;
    for _ in 0..64 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let x0 = random as i32;
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let x1 = random as i32;
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let y0 = (random | 0x80000000) as i32;
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let y1 = (random & 0x7fffffff) as i32;
        lines.push([x0, y0, x1, y1]);
        lines.push([x1, y1, x0, y0]);
    }
    let ranges: Vec<_> = (0..lines.len() as u32).map(|i| i..i + 1).collect();
    let paths = path_mask::upload(&mut batch, &ranges, &lines)?;
    let mut expected = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let target = batch.texture_rgba8([1, 1], vec![57; 4])?;
        path_mask::encode(
            &mut batch,
            FilterConfig {
                width: 1,
                height: 1,
                region_width: 1,
                region_height: 1,
                table_index: index as u32,
                ..Default::default()
            },
            None,
            paths,
            target,
        )?;
        batch.readback(target)?;
        expected.push(vec![if covered(&[*line], 0, 0) { 255 } else { 0 }; 4]);
    }
    assert_eq!(expected[0], [255; 4]);
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "large fixed point path crossings and product carries",
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
fn four_api_path_mask_fractional_edges_compact_region() -> Result<()> {
    let routes = Routes::with_features(
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING,
    )?;
    let mut batch = ComputeBatch::new();
    let lines = [
        [128, 128, 8320, 128],
        [8320, 128, 8320, 4224],
        [8320, 4224, 128, 4224],
        [128, 4224, 128, 128],
        [0, 640, 8192, 640],
        [128, 128, 128, 128],
    ];
    let paths = path_mask::upload(
        &mut batch,
        std::slice::from_ref(&std::ops::Range { start: 0, end: 6 }),
        &lines,
    )?;
    let tiles = [5, 0];
    let mut expected = Vec::new();
    for compact in [false, true] {
        let c = FilterConfig {
            width: 33,
            height: 17,
            region_x0: 1,
            region_y0: 2,
            region_width: 32,
            region_height: 15,
            dispatch_width: 2,
            ..Default::default()
        };
        let target = batch.texture_rgba8([33, 17], vec![57; 33 * 17 * 4])?;
        path_mask::encode(
            &mut batch,
            c,
            compact.then_some(tiles.as_slice()),
            paths,
            target,
        )?;
        batch.readback(target)?;
        let mut pixels = vec![57; 33 * 17 * 4];
        let tile_size = crate::shared::gpu_constants::TILE_SIZE;
        for y in 2..17 {
            for x in 1..33 {
                if compact
                    && !tiles.contains(&(y / tile_size * 33u32.div_ceil(tile_size) + x / tile_size))
                {
                    continue;
                }
                let alpha = if covered(&lines, x, y) { 255 } else { 0 };
                let i = ((y * 33 + x) * 4) as usize;
                pixels[i..i + 4].fill(alpha);
            }
        }
        expected.push(pixels);
    }
    for portable in [false, true] {
        for texture_table in [false, true] {
            routes.check_variant(
                &batch,
                &expected,
                "fractional path edges and compact clipping",
                Some(FilterVariant {
                    portable,
                    texture_table,
                }),
            )?;
        }
    }
    routes.validate()
}
