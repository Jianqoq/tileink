use super::{Result, fine_fixture::routes, reference::FineVariant};
use crate::{
    Canvas,
    native::runtime::{
        compute::{ComputeBatch, SamplerFilter},
        program::scene::{SceneCache, SceneImages},
    },
    shared::{execution::ExecOp, gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY},
};

fn scene_batch(
    cache: &mut SceneCache,
    width: u32,
    height: u32,
    count: usize,
    triangle: bool,
    chunked: bool,
    depth: usize,
) -> Result<ComputeBatch> {
    let mut canvas = Canvas::new(width, height, 1.0);
    for _ in 0..depth {
        use peniko::kurbo::Shape;
        let path = peniko::kurbo::Rect::new(
            0.25,
            0.25,
            f64::from(width) - 0.25,
            f64::from(height) - 0.25,
        )
        .to_path(0.25);
        canvas.push_clip_layer(
            path.clone(),
            peniko::kurbo::Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.25,
        );
        canvas.push_opacity_layer(path, peniko::kurbo::Affine::IDENTITY, 0.25, 0.5);
    }
    for index in 0..count {
        let mut path = peniko::kurbo::BezPath::new();
        if triangle {
            let offset = (index % 7) as f64 / 8.0;
            path.move_to((8.0 + offset, 8.0));
            path.line_to((f64::from(width - 8), f64::from(height - 8) - offset));
            path.line_to((8.0 + offset, f64::from(height - 8) - offset));
        } else {
            path.move_to((0.0, 0.0));
            path.line_to((f64::from(width), 0.0));
            path.line_to((f64::from(width), f64::from(height)));
            path.line_to((0.0, f64::from(height)));
        }
        canvas.push_path(
            path,
            crate::Brush::Solid(peniko::Color::from_rgb8(255, 0, 0)),
            peniko::kurbo::Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.25,
        );
    }

    for _ in 0..depth * 2 {
        canvas.pop_layer();
    }
    let mut batch = ComputeBatch::new();
    let scene = cache.record(&mut batch, &canvas, None, None, 65535)?;
    let target = batch.texture_rgba8([width, height], vec![0; (width * height * 4) as usize])?;
    let atlas = batch.texture_array_rgba8([1, 1, 1], vec![0; 4])?;
    let image = batch.texture_rgba8([1, 1], vec![0; 4])?;
    let table = batch.texture_table(&vec![image; NATIVE_TEXTURE_TABLE_CAPACITY as usize])?;
    let sampler = batch.sampler(SamplerFilter::Linear)?;
    let images = SceneImages {
        atlas,
        table,
        sampler,
    };
    let indices = scene
        .plan
        .all_direct_root_ops()
        .ok_or("fixture must have direct root batches")?;
    for index in indices {
        let ExecOp::DrawBatch {
            batch_id,
            layer_stack,
            ..
        } = &scene.plan.ops[index]
        else {
            unreachable!()
        };
        scene.encode_coarse(
            &mut batch,
            *batch_id..batch_id.saturating_add(1),
            layer_stack.start as u32..layer_stack.end as u32,
            chunked,
            65535,
        )?;
        // SAFETY: this Canvas has no external images; coarse for this exact
        // prepared scene/batch precedes fine, and the initialized target is live.
        unsafe {
            scene.encode_fine(&mut batch, target, &images, 0, true, 65535)?;
        }
    }
    batch.readback(target)?;
    Ok(batch)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_canvas_scan_coarse_fine_preserves_pixels() -> Result<()> {
    let routes = routes()?;
    let mut cache = SceneCache::default();
    for (width, height, count, triangle, depth) in [
        (17, 1, 0, false, 0),
        (33, 19, 1, false, 0),
        (65, 49, 1, true, 0),
        (65, 49, 257, true, 0),
        (33, 19, 1, false, 6),
    ] {
        let dense = scene_batch(&mut cache, width, height, count, triangle, false, depth)?;
        let expected = routes.fine_reference(&dense, FineVariant::ALL[0])?;
        if count == 0 {
            assert_eq!(expected[0], vec![0; (width * height * 4) as usize]);
        } else if depth != 0 {
            assert!(expected[0].chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        } else if !triangle {
            assert_eq!(
                expected[0],
                [255, 0, 0, 255].repeat((width * height) as usize)
            );
        } else {
            assert!(expected[0].chunks_exact(4).any(|p| p == [255, 0, 0, 255]));
            assert!(expected[0].chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
        }
        for chunked in [false, true] {
            let batch = scene_batch(&mut cache, width, height, count, triangle, chunked, depth)?;
            for variant in FineVariant::ALL {
                routes.check_fine(
                    &batch,
                    &expected,
                    &format!("Canvas {count} depth {depth} chunked {chunked}"),
                    variant,
                )?;
            }
        }
    }
    routes.validate()
}
