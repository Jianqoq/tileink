use super::*;
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_retained_graph_edges_and_output_growth_match_fresh() -> Result<()> {
    let context = context()?;
    for graph in [false, true] {
        let root = RetainedNodeId::for_owner(1030);
        let layer = RetainedNodeId::for_owner(1031);
        let child = RetainedNodeId::for_owner(1032);
        let content = |phase| {
            let mut c = Canvas::new(96, 64, 2.0);
            let x = if phase == 0 { 8.0 } else { 10.0 };
            c.push_rect(
                Rect::new(x, 8.0, x + 8.0, 14.0),
                Radius::ZERO,
                Color::from_rgba8(180, 80, 31, if phase == 0 { 128 } else { 192 }),
            );
            Rc::new(c)
        };
        let descriptor = |phase| -> Result<_> {
            Ok(RetainedLayerDescriptor::Filter {
                filter: if graph {
                    graph_filter(channels_pipeline()?, phase >= 2)
                } else {
                    spatial_pipeline("offset_image", if phase >= 2 { 6.0 } else { 4.0 }, -2.0)?
                        .into()
                },
                sample_region: Region::rect(
                    if graph {
                        Rect::new(0.0, 0.0, 33.0, 19.0)
                    } else {
                        Rect::new(8.0, 8.0, 18.0, 14.0)
                    },
                    Radius::ZERO,
                ),
            })
        };
        let mut scene = RetainedScene::new(96, 64, 2.0, root)?;
        scene
            .transaction()
            .insert_layer(RetainedParent::content(root), None, layer, descriptor(0)?)
            .insert_scene(
                RetainedParent::content(layer),
                None,
                child,
                content(0),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 96, 64)?;
        for phase in 0..4 {
            if phase == 1 {
                scene
                    .transaction()
                    .replace_scene(child, content(1))
                    .commit()?;
            }
            if phase == 2 {
                scene
                    .transaction()
                    .update_layer(layer, descriptor(2)?)
                    .commit()?;
            }
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 96, 64)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            let differences: Vec<_> = actual
                .pixels
                .iter()
                .zip(&expected.pixels)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| (i % actual.width as usize, i / actual.width as usize, *a, *b))
                .collect();
            assert!(
                differences.is_empty(),
                "graph={graph},phase={phase}, count={}, first={:?}",
                differences.len(),
                &differences[..differences.len().min(16)]
            );
            if !graph {
                assert_ne!(
                    actual.pixels[20 * actual.width as usize + 38],
                    0,
                    "expanded output was clipped"
                );
            }
        }
    }
    context.check_validation()?;
    Ok(())
}
