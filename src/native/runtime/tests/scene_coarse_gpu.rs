use super::{Result, fine_fixture::routes, reference::FineVariant};
use crate::{
    Canvas,
    native::runtime::{
        compute::ComputeBatch,
        program::scene::{SceneCache, SceneImages},
    },
    shared::execution::ExecOp,
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
    render_canvas(cache, &canvas, &Default::default(), chunked)
}

fn render_canvas(
    cache: &mut SceneCache,
    canvas: &Canvas,
    upload: &crate::shared::image_resource::GpuImageResourceUpload,
    chunked: bool,
) -> Result<ComputeBatch> {
    let mut batch = ComputeBatch::new();
    let images = crate::native::runtime::renderer::Images::record(&mut batch, upload)?;
    let target = encode_canvas(cache, &mut batch, canvas, &images, chunked)?;
    batch.readback(target)?;
    Ok(batch)
}

pub(super) fn encode_canvas(
    cache: &mut SceneCache,
    batch: &mut ComputeBatch,
    canvas: &Canvas,
    images: &crate::native::runtime::renderer::Images<'_>,
    chunked: bool,
) -> Result<crate::native::runtime::compute::ResourceId> {
    crate::native::runtime::renderer::Execution::record(
        cache,
        batch,
        canvas,
        images,
        None,
        crate::native::runtime::renderer::FrameOptions {
            chunked,
            clear_color: 0,
        },
        65535,
    )
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

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_canvas_image_resources_preserve_pixels() -> Result<()> {
    use crate::shared::{
        gpu_constants::NATIVE_TEXTURE_TABLE_CAPACITY,
        image_resource::{ImageKey, ImageResourceStore},
    };
    use crate::{Image, PatternSampling};
    use peniko::{Extend, kurbo::Rect};
    let routes = routes()?;
    let mut store = ImageResourceStore::default();
    store.insert(
        ImageKey(1),
        Image::from_rgba8(2, 1, [255, 0, 0, 255, 0, 255, 0, 255]),
    );
    let mut wide = [0, 0, 255, 255].repeat(1025);
    wide.extend([255, 255, 0, 255].repeat(1025));
    store.insert(ImageKey(2), Image::from_rgba8(2050, 1, wide));
    let upload = store.upload_merged(
        &ImageResourceStore::default(),
        4096,
        4,
        NATIVE_TEXTURE_TABLE_CAPACITY,
        None,
    );
    assert_eq!(upload.atlas_page_count(), 1);
    assert_eq!(upload.textures().len(), 1);
    let mut cache = SceneCache::default();
    for sampling in [PatternSampling::Nearest, PatternSampling::Bilinear] {
        let mut canvas = Canvas::new(2, 2, 1.0);
        canvas
            .push_image_key(
                Rect::new(0.0, 0.0, 2.0, 1.0),
                ImageKey(1),
                Extend::Pad,
                sampling,
            )
            .unwrap();
        canvas
            .push_image_key(
                Rect::new(0.0, 1.0, 2.0, 2.0),
                ImageKey(2),
                Extend::Pad,
                sampling,
            )
            .unwrap();
        let batch = render_canvas(&mut cache, &canvas, &upload, false)?;
        let expected = routes.fine_reference(&batch, FineVariant::ALL[1])?;
        assert_eq!(
            expected[0],
            [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255
            ]
        );
        for chunked in [false, true] {
            for variant in FineVariant::ALL {
                let upload = store.upload_merged(
                    &ImageResourceStore::default(),
                    4096,
                    4,
                    if variant.texture_table {
                        NATIVE_TEXTURE_TABLE_CAPACITY
                    } else {
                        0
                    },
                    None,
                );
                let batch = render_canvas(&mut cache, &canvas, &upload, chunked)?;
                routes.check_fine(&batch, &expected, "Canvas atlas and table", variant)?;
            }
        }
    }
    // Distinct pages exercise the placement layer index, not only array creation.
    let mut store = ImageResourceStore::default();
    store.insert(
        ImageKey(1),
        Image::from_rgba8(4, 4, [255, 0, 0, 255].repeat(16)),
    );
    store.insert(
        ImageKey(2),
        Image::from_rgba8(4, 4, [0, 255, 0, 255].repeat(16)),
    );
    let upload = store.upload_merged(&ImageResourceStore::default(), 8, 4, 0, None);
    assert_eq!(upload.atlas_page_count(), 2);
    let mut canvas = Canvas::new(2, 1, 1.0);
    canvas
        .push_image_key(
            Rect::new(0.0, 0.0, 1.0, 1.0),
            ImageKey(1),
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .unwrap();
    canvas
        .push_image_key(
            Rect::new(1.0, 0.0, 2.0, 1.0),
            ImageKey(2),
            Extend::Pad,
            PatternSampling::Nearest,
        )
        .unwrap();
    let expected = vec![vec![255, 0, 0, 255, 0, 255, 0, 255]];
    for chunked in [false, true] {
        let batch = render_canvas(&mut cache, &canvas, &upload, chunked)?;
        for variant in FineVariant::ALL {
            routes.check_fine(&batch, &expected, "Canvas atlas page selection", variant)?;
        }
    }
    routes.validate()
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_canvas_scan_geometry_feeds_layer_filters() -> Result<()> {
    use crate::native::runtime::program::filter::stack::{Composite, Textures};
    use crate::shared::filter_config::FilterConfig;
    use peniko::kurbo::{Affine, BezPath, Rect, Shape};
    let routes = routes()?;
    let mut canvas = Canvas::new(33, 29, 1.0);
    let mut path = BezPath::new();
    path.move_to((2.125, 3.5));
    path.line_to((30.375, 6.25));
    path.line_to((7.75, 25.875));
    path.close_path();
    canvas.push_clip_layer(path, Affine::IDENTITY, crate::FillRule::NonZero, 0.25);
    canvas.push_path(
        Rect::new(0.0, 0.0, 33.0, 29.0).to_path(0.25),
        crate::Brush::Solid(peniko::Color::from_rgb8(255, 0, 0)),
        Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.25,
    );
    canvas.pop_layer();
    let mut batch = ComputeBatch::new();
    let upload = Default::default();
    let images = SceneImages::record(&mut batch, &upload)?;
    let scene = SceneCache::default().record(&mut batch, &canvas, None, Some(&upload), 65535)?;
    let target = batch.texture_rgba8([33, 29], vec![0; 33 * 29 * 4])?;
    for index in scene.plan().all_direct_root_ops().unwrap() {
        let ExecOp::DrawBatch {
            batch_id,
            layer_stack,
            ..
        } = &scene.plan().ops[index]
        else {
            unreachable!()
        };
        scene.encode_coarse(
            &mut batch,
            *batch_id..batch_id.saturating_add(1),
            layer_stack.start as u32..layer_stack.end as u32,
            false,
            65535,
        )?;
        // SAFETY: associated images, scene and coarse precede fine in this batch.
        unsafe {
            scene.encode_fine(&mut batch, target, &images, 0, true, 65535)?;
        }
    }
    let mask = batch.texture_rgba8([33, 29], vec![0; 33 * 29 * 4])?;
    let config = FilterConfig {
        width: 33,
        height: 29,
        region_width: 33,
        region_height: 29,
        ..Default::default()
    };
    scene
        .filter_geometry()
        .mask(&mut batch, config, None, mask)?;
    let source = batch.texture_rgba8([33, 29], [0, 0, 255, 255].repeat(33 * 29))?;
    let composite = batch.texture_rgba8([33, 29], vec![0; 33 * 29 * 4])?;
    assert_eq!(scene.plan().layer_stack_data.len(), 1);
    scene.filter_stack().encode(
        &mut batch,
        Composite::Over,
        FilterConfig {
            layer_stack_end: 1,
            ..config
        },
        None,
        Textures {
            source,
            auxiliary: None,
            target: composite,
        },
    )?;
    assert!(
        batch.outputs().is_empty(),
        "geometry stays on GPU through mask/composite"
    );
    batch.readback(target)?;
    batch.readback(mask)?;
    batch.readback(composite)?;
    let expected = routes.render_reference(
        &batch,
        super::reference::FilterVariant {
            portable: false,
            texture_table: false,
        },
        FineVariant::ALL[0],
    )?;
    assert!(expected[0].chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
    for ((fine, mask), composite) in expected[0]
        .chunks_exact(4)
        .zip(expected[1].chunks_exact(4))
        .zip(expected[2].chunks_exact(4))
    {
        assert_eq!(mask, [fine[3]; 4]);
        assert_eq!(composite, [0, 0, fine[3], fine[3]]);
    }
    for variant in FineVariant::ALL {
        routes.check_render(
            &batch,
            &expected,
            "Canvas GPU geometry mask and stack",
            super::reference::FilterVariant {
                portable: variant.portable,
                texture_table: variant.texture_table,
            },
            variant,
        )?;
    }
    routes.validate()
}
