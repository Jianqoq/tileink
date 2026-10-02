//! Semantic oracles for attachment fetch, sparse coverage, and tile boundaries.
use super::*;
use crate::native::runtime::compute::SamplerFilter;
use crate::shared::{fine_config::FineConfig, gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY};
#[path = "render_tests/ordering.rs"]
mod ordering;

#[derive(Default)]
struct TileOptions<'a> {
    kind: u32,
    mixed: bool,
    patterned: bool,
    end_clip: bool,
    empty: &'a [u32],
    clear: [u8; 4],
}

fn solid_tiles(
    size: [u32; 2],
    viewport: [u32; 2],
    active: Option<&[u32]>,
    load: bool,
    options: TileOptions<'_>,
) -> Result<(ComputeBatch, Vec<u8>)> {
    let [width, height] = viewport;
    let tiles_width = width.div_ceil(16);
    let tiles_height = height.div_ceil(16);
    let tile_count = tiles_width * tiles_height;
    let all: Vec<_> = (0..tile_count).collect();
    let selected = active.unwrap_or(&all);
    let particles_per_tile = 1 + u32::from(options.end_clip);
    let kind_base = tile_count * 6 * (1 + particles_per_tile);
    let list_base = kind_base + tile_count;
    let mut coarse = vec![0u32; list_base as usize + selected.len()];
    for tile in 0..tile_count as usize {
        coarse[tile * 6 + 1] = tile as u32 * particles_per_tile;
        coarse[tile * 6 + 2] = (tile as u32 + 1) * particles_per_tile;
        let particle = tile_count as usize * 6 + tile * 6 * particles_per_tile as usize;
        coarse[particle] = 2; // solid premultiplied red at half opacity
        coarse[particle + 5] = 0x80000080;
        if options.end_clip {
            coarse[particle + 6] = if options.mixed && tile % 3 == 0 { 0 } else { 4 };
        }
        if options.empty.contains(&(tile as u32)) {
            coarse[tile * 6 + 2] = coarse[tile * 6 + 1];
            coarse[kind_base as usize + tile] = 1;
        } else {
            coarse[kind_base as usize + tile] = if options.mixed && tile % 3 == 1 {
                0
            } else {
                options.kind
            };
        }
    }
    coarse[list_base as usize..].copy_from_slice(selected);
    let config = FineConfig {
        width,
        height,
        tiles_width,
        tiles_height,
        tile_count,
        clear_color: u32::from_le_bytes(options.clear),
        load_target: u32::from(load),
        ptcl_capacity: tile_count * particles_per_tile,
        fine_tile_kind_base: kind_base,
        active_tile_list_base: list_base,
        active_tile_count: selected.len() as u32,
        dispatch_width: 1,
        incremental: u32::from(active.is_some()),
        ..Default::default()
    };
    let mut batch = ComputeBatch::new();
    let initial = [16, 32, 64, 255];
    let mut pixels = initial.repeat((size[0] * size[1]) as usize);
    if options.patterned {
        for y in 0..size[1] {
            for x in 0..size[0] {
                let i = ((y * size[0] + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[
                    (x * 13 % 128) as u8,
                    (y * 17 % 128) as u8,
                    64,
                    128,
                ]);
            }
        }
    }
    let mut expected = pixels.clone();
    for y in 0..height {
        for x in 0..width {
            if selected.contains(&(y / 16 * tiles_width + x / 16)) {
                let i = ((y * size[0] + x) * 4) as usize;
                let base: [u8; 4] = if load {
                    pixels[i..i + 4].try_into().unwrap()
                } else {
                    options.clear
                };
                let color = if options.empty.contains(&(y / 16 * tiles_width + x / 16)) {
                    base
                } else {
                    std::array::from_fn(|c| {
                        ([128u8, 0, 0, 128][c] as u32 + (base[c] as u32 * 127 + 127) / 255) as u8
                    })
                };
                expected[i..i + 4].copy_from_slice(&color);
            }
        }
    }
    let target = batch.texture_rgba8(size, pixels)?;
    let config = batch.buffer(bytemuck::bytes_of(&config).to_vec())?;
    let coarse = batch.buffer(bytemuck::cast_slice(&coarse).to_vec())?;
    let dummy = batch.buffer(vec![0; 4])?;
    let spills = batch.buffer(vec![0; 4])?;
    let atlas = batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?;
    let image = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let images = batch.texture_table(&vec![image; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
    let sampler = batch.sampler(SamplerFilter::Nearest)?;
    // SAFETY: tile IDs are unique/in bounds, each header addresses one complete
    // solid particle, and this stream never reads draw records or spills.
    unsafe {
        batch.dispatch(
            "fine_tile_main",
            &[
                (0, config),
                (1, target),
                (2, dummy),
                (3, dummy),
                (4, coarse),
                (5, dummy),
                (6, dummy),
                (7, spills),
                (12, atlas),
                (13, sampler),
                (30, images),
            ],
            [1, selected.len() as u32, 1],
        )?;
    }
    batch.readback(target)?;
    Ok((batch, expected))
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn hardware_tiles_preserve_sparse_pixels_edges_and_framebuffer_fetch() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for (size, viewport) in [
        ([1, 1], [1, 1]),
        ([17, 15], [17, 15]),
        ([33, 35], [33, 35]),
        ([65, 49], [33, 35]),
    ] {
        for load in [false, true] {
            let (batch, expected) =
                solid_tiles(size, viewport, None, load, TileOptions::default())?;
            let ticket = metal.submit_compute(&batch)?;
            assert_eq!(
                metal.readback_batch(&ticket)?,
                [expected],
                "{size:?}/{viewport:?} load={load}"
            );
        }
    }
    for load in [false, true] {
        let (batch, expected) = solid_tiles(
            [65, 49],
            [33, 35],
            Some(&[8, 0, 4]),
            load,
            TileOptions::default(),
        )?;
        let ticket = metal.submit_compute(&batch)?;
        assert_eq!(
            metal.readback_batch(&ticket)?,
            [expected],
            "sparse load={load}"
        );
    }
    assert!(matches!(
        metal.pipelines["fine_tile_main"].state,
        pipeline::State::Tile { .. }
    ));
    metal.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn drawing_rejects_import_without_attachment_usage_and_context_remains_usable() -> Result<()> {
    use crate::native::interop::metal::{ContextDescriptor, TextureDescriptor};
    let device = MTLCreateSystemDefaultDevice().ok_or("Metal device")?;
    let descriptor = MTLTextureDescriptor::new();
    descriptor.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
    descriptor.setTextureType(MTLTextureType::Type2D);
    descriptor.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite);
    descriptor.setStorageMode(MTLStorageMode::Private);
    // SAFETY: valid test-owned allocations and exclusive queue/texture ownership.
    let (context, texture) = unsafe {
        descriptor.setWidth(17);
        descriptor.setHeight(15);
        let context = crate::NativeContext::from_metal(ContextDescriptor {
            device: device.clone(),
            queue: device.newCommandQueue().ok_or("Metal queue")?,
        })?;
        let texture = context.import_metal_texture(TextureDescriptor {
            texture: device
                .newTextureWithDescriptor(&descriptor)
                .ok_or("Metal texture")?,
            initialized: false,
        })?;
        (context, texture)
    };
    let mut renderer = crate::NativeRenderer::with_context(&context, 17, 15)?;
    let mut canvas = crate::Canvas::new(17, 15, 1.0);
    canvas.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, 17.0, 15.0),
        crate::Radius::ZERO,
        peniko::Color::WHITE,
    );
    let error = renderer
        .render_to_texture(&canvas, &texture)
        .err()
        .ok_or("missing RenderTarget usage was accepted")?;
    assert!(error.to_string().contains("RenderTarget"), "{error}");
    let image = renderer.render_to_image(&canvas)?.readback()?;
    assert!(image.pixels.iter().all(|&pixel| pixel == u32::MAX));
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn simple_tiles_and_unexpected_tag_fallback_blend_destination_once() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for kind in [2, 3, 4] {
        for end_clip in [false, true] {
            for load in [false, true] {
                for active in [None, Some(&[8, 0, 4][..])] {
                    let (batch, expected) = solid_tiles(
                        [65, 49],
                        [33, 35],
                        active,
                        load,
                        TileOptions {
                            kind,
                            end_clip,
                            ..Default::default()
                        },
                    )?;
                    let ticket = metal.submit_compute(&batch)?;
                    assert_eq!(
                        metal.readback_batch(&ticket)?,
                        [expected],
                        "kind={kind} fallback={end_clip} load={load} sparse={}",
                        active.is_some()
                    );
                }
            }
        }
    }
    metal.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn empty_tiles_clear_or_preserve_pixels_without_touching_outside_viewport() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for (size, viewport, empty) in [
        ([1, 1], [1, 1], &[0][..]),
        ([17, 15], [17, 15], &[0][..]),
        ([65, 49], [33, 35], &[0, 3, 8][..]),
    ] {
        for load in [false, true] {
            for active in [None, Some(empty)] {
                let (batch, expected) = solid_tiles(
                    size,
                    viewport,
                    active,
                    load,
                    TileOptions {
                        empty,
                        clear: [8, 24, 40, 64],
                        ..Default::default()
                    },
                )?;
                let ticket = metal.submit_compute(&batch)?;
                assert_eq!(
                    metal.readback_batch(&ticket)?,
                    [expected],
                    "size={size:?} load={load} sparse={}",
                    active.is_some()
                );
            }
        }
    }
    metal.assert_valid()
}

// Mixed coarse kinds, terminators, and empty tiles must preserve blend order.
#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn mixed_tile_kinds_preserve_blend_order_and_terminate_before_unused_particles() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for load in [false, true] {
        for active in [None, Some(&[152, 0, 78, 2, 4, 64, 127][..])] {
            let (batch, expected) = solid_tiles(
                [273, 145],
                [257, 129],
                active,
                load,
                TileOptions {
                    kind: 2,
                    mixed: true,
                    patterned: false,
                    end_clip: true,
                    empty: &[4, 127],
                    clear: [8, 24, 40, 64],
                },
            )?;
            let ticket = metal.submit_compute(&batch)?;
            assert_eq!(
                metal.readback_batch(&ticket)?,
                [expected],
                "load={load} sparse={}",
                active.is_some()
            );
        }
    }
    metal.assert_valid()
}

#[test]
#[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
fn solid_tiles_preserve_uniform_and_varying_destinations_at_partial_tile_edges() -> Result<()> {
    let mut metal = Metal::with_options(&crate::NativeContextOptions {
        validation: true,
        ..Default::default()
    })?;
    for (size, viewport) in [
        ([17, 15], [17, 15]),
        ([33, 35], [33, 35]),
        ([65, 49], [33, 35]),
    ] {
        for load in [false, true] {
            for patterned in [false, true] {
                for end_clip in [false, true] {
                    let (batch, expected) = solid_tiles(
                        size,
                        viewport,
                        None,
                        load,
                        TileOptions {
                            kind: 2,
                            patterned,
                            end_clip,
                            clear: [8, 24, 40, 64],
                            ..Default::default()
                        },
                    )?;
                    let ticket = metal.submit_compute(&batch)?;
                    assert_eq!(
                        metal.readback_batch(&ticket)?,
                        [expected],
                        "size={size:?} load={load} patterned={patterned} fallback={end_clip}"
                    );
                }
            }
        }
    }
    metal.assert_valid()
}
