use super::*;

// Independent fine-only oracle: every draw is interpreted in every tile. It
// bypasses coarse classification, so an incorrect full-tile proof changes pixels.
fn interpret_rectangles(context: &crate::NativeContext, canvas: &crate::Canvas) -> Result<Vec<u8>> {
    use crate::native::runtime::compute::SamplerFilter;
    use crate::shared::{fine_config::FineConfig, gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY};
    let (width, height) = canvas.physical_size();
    let tiles_width = width.div_ceil(16);
    let tiles_height = height.div_ceil(16);
    let tiles = tiles_width * tiles_height;
    let per_tile = canvas.draw_records.len() as u32 + 1;
    let kind_base = tiles * 6 * (1 + per_tile);
    let mut coarse = vec![0u32; (kind_base + tiles) as usize];
    for tile in 0..tiles as usize {
        coarse[tile * 6 + 1] = tile as u32 * per_tile;
        coarse[tile * 6 + 2] = (tile as u32 + 1) * per_tile;
        for draw in 0..canvas.draw_records.len() {
            let at = tiles as usize * 6 + (tile * per_tile as usize + draw) * 6;
            coarse[at] = 9;
            coarse[at + 5] = draw as u32;
        }
    }
    let mut paint = canvas.sdf_blob.clone();
    let shadow_base = paint.len() as u32;
    paint.extend_from_slice(&canvas.sdf_shadow_blob);
    let brush_base = paint.len() as u32;
    paint.extend_from_slice(&canvas.brush_blob);
    let config = FineConfig {
        width,
        height,
        tile_count: tiles,
        tiles_width,
        tiles_height,
        clear_color: 0xff000000,
        ptcl_capacity: tiles * per_tile,
        paint_sdf_shadow_base: shadow_base,
        paint_brush_base: brush_base,
        fine_tile_kind_base: kind_base,
        active_tile_count: tiles,
        dispatch_width: tiles,
        ..Default::default()
    };
    let mut batch = ComputeBatch::new();
    let config = batch.buffer(bytemuck::bytes_of(&config).to_vec())?;
    let target = batch.texture_rgba8(
        [width, height],
        [0, 0, 0, 255].repeat((width * height) as usize),
    )?;
    let draws = batch.buffer(bytemuck::cast_slice(&canvas.draw_records).to_vec())?;
    let paint = batch.buffer(bytes(&paint))?;
    let coarse = batch.buffer(bytes(&coarse))?;
    let dummy = batch.buffer(vec![0; 4])?;
    let spills = batch.buffer(vec![0; 4])?;
    let atlas = batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?;
    let image = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let images = batch.texture_table(&vec![image; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
    let sampler = batch.sampler(SamplerFilter::Nearest)?;
    // SAFETY: every tile owns a complete bounded stream of SDF draws plus END;
    // the full Canvas paint records and transforms stay paired, with no stacks.
    unsafe {
        batch.dispatch(
            "fine_tile_main",
            &[
                (0, config),
                (1, target),
                (2, draws),
                (3, paint),
                (4, coarse),
                (5, dummy),
                (6, dummy),
                (7, spills),
                (12, atlas),
                (13, sampler),
                (30, images),
            ],
            [tiles, 1, 1],
        )?;
    }
    batch.readback(target)?;
    let receipt = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?;
    Ok(receipt.readback()?.remove(0))
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_rect_tile_proofs_match_fine_only_pixels() -> Result<()> {
    use crate::{
        Canvas, NativeContext, NativeContextOptions, NativeRenderer, Radius, StrokeWidths,
    };
    use peniko::{Color, kurbo::Rect};
    #[cfg(feature = "dx12")]
    // SAFETY: serial tests enable native validation before device creation.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let backend = if cfg!(feature = "metal") {
        crate::NativeBackend::Metal
    } else if cfg!(feature = "dx12") {
        crate::NativeBackend::Dx12
    } else {
        crate::NativeBackend::Vulkan
    };
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            validation: true,
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        },
    )?;
    let mut renderer = NativeRenderer::with_context(&context, 129, 113)?;
    renderer.set_clear_color(Color::BLACK);
    for mode in 0..13 {
        let mut canvas = Canvas::new(129, 113, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, 129.0, 113.0),
            Radius::ZERO,
            Color::from_rgba8(90, 140, 210, 177),
        );
        let rect = if mode == 11 {
            Rect::new(1e7, 0.25, 1e7 + 128., 112.75)
        } else if mode == 12 {
            Rect::new(1.70e38, 0.25, 1.705e38, 112.75)
        } else if mode == 9 {
            Rect::new(65400.25, 0.25, 65528., 112.75)
        } else if mode == 10 {
            Rect::new(-65528., 0.25, -65400.25, 112.75)
        } else if mode == 7 {
            Rect::new(1e7 - 1., 0.25, 1e7 + 129., 112.75)
        } else if mode == 8 {
            Rect::new(1.69e38, 0.25, 1.705e38, 112.75)
        } else if mode == 3 {
            Rect::new(16.25, 16.25, 31.75, 31.75)
        } else {
            Rect::new(0.25, 0.25, 128.75, 112.75)
        };
        let radius = if matches!(mode, 7 | 8 | 11 | 12) {
            Radius::ZERO
        } else if mode == 1 {
            Radius::all(39.5)
        } else {
            Radius {
                top_left: 4.0,
                top_right: 16.0,
                bottom_left: 7.5,
                bottom_right: 26.0,
            }
        };
        let widths = if mode == 7 {
            StrokeWidths::all(2.)
        } else if mode == 8 {
            StrokeWidths {
                top: 0.,
                right: 0.,
                bottom: 0.,
                left: 2e36,
            }
        } else if mode == 2 {
            StrokeWidths::all(150.0)
        } else {
            StrokeWidths {
                top: 1.0,
                right: 7.5,
                bottom: 4.0,
                left: 2.25,
            }
        };
        let draw = if mode >= 11 {
            canvas.push_rect(rect, radius, Color::from_rgba8(240, 70, 30, 193))
        } else {
            canvas
                .push_rect_stroke_widths(rect, radius, widths, Color::from_rgba8(240, 70, 30, 193))
                .unwrap()
        };
        if mode >= 4 {
            use peniko::kurbo::Affine;
            let transform = match mode {
                4 => Affine::translate((0.375, -0.25)),
                5 => Affine::scale_non_uniform(0.8, 0.9),
                6 => Affine::rotate(0.15),
                7 => Affine::translate((-1e7, 0.)),
                8 => Affine::translate((-1.702e38, 0.)),
                9 => Affine::translate((-65408., 0.)),
                10 => Affine::translate((65512., 0.)),
                11 => Affine::translate((-1e7, 0.)),
                _ => Affine::translate((-1.702e38, 0.)),
            };
            let record = &mut canvas.draw_records[draw.index()];
            record.transform = crate::shared::affine::GpuAffine::from_logical(transform, 1.0);
            record.inverse_transform = record.transform.inverse().unwrap();
            record.pixel_bounds = crate::shared::bounds::PixelBounds {
                x0: 0,
                y0: 0,
                x1: 129,
                y1: 113,
            };
        }
        let expected = interpret_rectangles(&context, &canvas)?;
        let actual = renderer.render_to_image(&canvas)?.readback()?;
        assert!(
            bytemuck::cast_slice::<u32, u8>(&actual.pixels) == expected,
            "rectangle mode {mode}: first differing byte {:?}",
            bytemuck::cast_slice::<u32, u8>(&actual.pixels)
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
        );
    }
    context.check_validation()?;
    Ok(())
}
