use super::{four_api::Routes, reference::FilterVariant};
use crate::native::runtime::{
    Result,
    compute::ComputeBatch,
    program::filter::{self, BasicFilter},
};
use crate::shared::{filter_config::FilterConfig, gpu_constants::TILE_SIZE};

fn config(width: u32, height: u32) -> FilterConfig {
    FilterConfig {
        width,
        height,
        region_width: width,
        region_height: height,
        dispatch_width: 2,
        clear_color: 0x7f372511,
        rect_x0: 0.0,
        rect_y0: 0.0,
        rect_x1: width.min(3) as f32,
        rect_y1: height.min(5) as f32,
        ..Default::default()
    }
}

#[test]
fn basic_filter_encoder_rejects_invalid_regions_tiles_aliases_and_coordinates() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let source = batch.texture_rgba8([17, 19], vec![0; 17 * 19 * 4])?;
    let target = batch.texture_rgba8([17, 19], vec![0; 17 * 19 * 4])?;
    let valid = config(17, 19);
    for c in [
        FilterConfig {
            region_width: 18,
            ..valid
        },
        FilterConfig {
            region_x0: u32::MAX,
            ..valid
        },
        FilterConfig {
            offset_x: i32::MIN,
            ..valid
        },
        FilterConfig {
            offset_y: i32::MAX,
            ..valid
        },
        FilterConfig {
            rect_x1: f32::NAN,
            ..valid
        },
        FilterConfig {
            rect_y1: 20.0,
            ..valid
        },
        FilterConfig {
            rect_x0: 4.0,
            ..valid
        },
    ] {
        assert!(filter::encode(&mut batch, BasicFilter::Tile, c, None, source, target).is_err());
    }
    for tiles in [&[0, 0][..], &[4][..], &[u32::MAX][..]] {
        assert!(
            filter::encode(
                &mut batch,
                BasicFilter::Copy,
                valid,
                Some(tiles),
                source,
                target
            )
            .is_err()
        );
    }
    assert!(filter::encode(&mut batch, BasicFilter::Copy, valid, None, source, source).is_err());
    let wrong = batch.buffer(vec![0; 16])?;
    assert!(filter::encode(&mut batch, BasicFilter::Copy, valid, None, wrong, target).is_err());
    filter::encode(
        &mut batch,
        BasicFilter::Copy,
        valid,
        Some(&[]),
        source,
        target,
    )?;
    filter::encode(
        &mut batch,
        BasicFilter::Copy,
        FilterConfig {
            region_width: 0,
            ..valid
        },
        None,
        source,
        target,
    )?;
    assert!(batch.passes().is_empty());
    Ok(())
}

fn expected(
    kernel: BasicFilter,
    c: FilterConfig,
    tiles: Option<&[u32]>,
    source: &[u8],
    initial: &[u8],
) -> Vec<u8> {
    let mut pixels = initial.to_vec();
    for y in c.region_y0..c.region_y0 + c.region_height {
        for x in c.region_x0..c.region_x0 + c.region_width {
            if tiles.is_some_and(|t| {
                !t.contains(&(y / TILE_SIZE * c.width.div_ceil(TILE_SIZE) + x / TILE_SIZE))
            }) {
                continue;
            }
            let mut destination = [x as i64, y as i64];
            let index = ((y * c.width + x) * 4) as usize;
            let mut pixel: [u8; 4] = source[index..index + 4].try_into().unwrap();
            match kernel {
                BasicFilter::Clear => pixel = c.clear_color.to_le_bytes(),
                BasicFilter::Copy => (),
                BasicFilter::SourceAlpha => pixel = [0, 0, 0, pixel[3]],
                BasicFilter::Tile => {
                    let w = c.rect_x1 as i64 - c.rect_x0 as i64;
                    let h = c.rect_y1 as i64 - c.rect_y0 as i64;
                    if w == 0 || h == 0 {
                        continue;
                    }
                    let sx = c.rect_x0 as i64 + (x as i64 - c.rect_x0 as i64).rem_euclid(w);
                    let sy = c.rect_y0 as i64 + (y as i64 - c.rect_y0 as i64).rem_euclid(h);
                    let i = ((sy * c.width as i64 + sx) * 4) as usize;
                    pixel.copy_from_slice(&source[i..i + 4]);
                }
                BasicFilter::Offset => {
                    let sx = x as i64 - c.offset_x as i64;
                    let sy = y as i64 - c.offset_y as i64;
                    pixel = if inside(c, [sx, sy]) {
                        let i = ((sy * c.width as i64 + sx) * 4) as usize;
                        source[i..i + 4].try_into().unwrap()
                    } else {
                        [0; 4]
                    };
                }
                BasicFilter::DropShadowMask => {
                    if pixel[3] == 0 {
                        continue;
                    }
                    destination[0] += c.offset_x as i64;
                    destination[1] += c.offset_y as i64;
                    if !inside(c, destination) {
                        continue;
                    }
                    pixel = [pixel[3]; 4];
                }
            }
            let i = ((destination[1] * c.width as i64 + destination[0]) * 4) as usize;
            pixels[i..i + 4].copy_from_slice(&pixel);
        }
    }
    pixels
}
fn inside(c: FilterConfig, p: [i64; 2]) -> bool {
    p[0] >= c.region_x0 as i64
        && p[1] >= c.region_y0 as i64
        && p[0] < (c.region_x0 + c.region_width) as i64
        && p[1] < (c.region_y0 + c.region_height) as i64
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_basic_filters_match_all_production_variants_and_cpu_pixels() -> Result<()> {
    let features = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    let routes = Routes::with_features(features)?;
    for [width, height] in [[1, 3], [3, 7], [65, 35]] {
        let source: Vec<u8> = (0..width * height)
            .flat_map(|i| {
                [
                    (i * 37) as u8,
                    (i * 11) as u8,
                    (i * 73) as u8,
                    if i % 3 == 0 { 0 } else { (i * 29) as u8 },
                ]
            })
            .collect();
        let initial = vec![0x39; source.len()];
        let mut batch = ComputeBatch::new();
        let input = batch.texture_rgba8([width, height], source.clone())?;
        let tiles: Vec<_> = (0..width.div_ceil(TILE_SIZE) * height.div_ceil(TILE_SIZE))
            .rev()
            .filter(|t| t % 2 == 0)
            .collect();
        let mut outputs = Vec::new();
        for compact in [false, true] {
            for offset in [[0, 0], [-2, 3], [3, -2]] {
                let mut c = config(width, height);
                c.offset_x = offset[0];
                c.offset_y = offset[1];
                if offset != [0, 0] {
                    c.region_x0 = width / 3;
                    c.region_y0 = 1;
                    c.region_width = width - c.region_x0;
                    c.region_height = height - 2;
                }
                if width > 5 && offset == [-2, 3] {
                    c.rect_x0 = 2.75;
                    c.rect_y0 = 3.25;
                    c.rect_x1 = 5.75;
                    c.rect_y1 = 8.5;
                    c.region_x0 = 0;
                    c.region_y0 = 0;
                    c.region_width = width;
                    c.region_height = height;
                }
                if offset == [3, -2] {
                    c.rect_x1 = c.rect_x0;
                }
                for kernel in [
                    BasicFilter::Clear,
                    BasicFilter::Copy,
                    BasicFilter::SourceAlpha,
                    BasicFilter::Tile,
                    BasicFilter::Offset,
                    BasicFilter::DropShadowMask,
                ] {
                    let target = batch.texture_rgba8([width, height], initial.clone())?;
                    let active = compact.then_some(tiles.as_slice());
                    filter::encode(&mut batch, kernel, c, active, input, target)?;
                    batch.readback(target)?;
                    outputs.push(expected(kernel, c, active, &source, &initial));
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
                    &outputs,
                    &format!("basic filters {width}x{height} {variant:?}"),
                    Some(variant),
                )?;
            }
        }
    }
    routes.validate()
}
