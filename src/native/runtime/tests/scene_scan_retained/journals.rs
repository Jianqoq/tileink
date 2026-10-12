//! Consumed or skipped journals must never leave accepted GPU inputs stale.
use super::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_journal_gap_refreshes_draw_paint_and_tile_inputs() -> Result<()> {
    let context = gpu_support::context()?;
    let root = RetainedNodeId::for_owner(990);
    let first = RetainedNodeId::for_owner(991);
    let second = RetainedNodeId::for_owner(992);
    let solid = |color| {
        let mut canvas = Canvas::new(96, 96, 1.0);
        canvas.push_rect(
            peniko::kurbo::Rect::new(2.0, 3.0, 22.0, 23.0),
            crate::Radius::ZERO,
            color,
        );
        Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(96, 96, 1.0, root)?;
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            first,
            solid(Color::from_rgb8(240, 30, 20)),
            Affine::IDENTITY,
        )
        .insert_scene(
            RetainedParent::content(root),
            None,
            second,
            solid(Color::WHITE),
            Affine::translate((0.0, 32.0)),
        )
        .commit()?;
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut renderer = NativeRenderer::with_context(&context, 96, 96)?;
    renderer
        .render_to_image(&materializer.canvas())?
        .readback()?;
    scene
        .transaction()
        .replace_scene(first, solid(Color::from_rgb8(20, 30, 240)))
        .set_transform(first, Affine::translate((48.0, 0.0)))
        .commit()?;
    update(&mut materializer, &scene);
    // This journal describes only the second node. The renderer never accepted
    // the first node's new color, bounds or tile membership.
    scene
        .transaction()
        .set_transform(second, Affine::translate((32.0, 32.0)))
        .commit()?;
    update(&mut materializer, &scene);
    let canvas = materializer.canvas();
    let actual = renderer.render_to_image(&canvas)?.readback()?.pixels;
    let expected = NativeRenderer::with_context(&context, 96, 96)?
        .render_to_image(&canvas)?
        .readback()?
        .pixels;
    assert_eq!(
        actual, expected,
        "all scene inputs must cross the same version gap"
    );
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn skipped_retained_geometry_version_rebuilds_current_path_plans() -> Result<()> {
    let context = gpu_support::context()?;
    let mut scene = retained();
    let paint_node = RetainedNodeId::for_owner(902);
    let root = scene.root();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            paint_node,
            Rc::new(paint()),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut cache = SceneCache::default();
    geometry(&context, &mut cache, &materializer.canvas())?;
    scene
        .transaction()
        .set_transform(RetainedNodeId::for_owner(901), Affine::scale(0.5))
        .commit()?;
    update(&mut materializer, &scene);
    // The next CPU frame contains only this unrelated paint edit. Its dirty
    // ranges cannot describe the geometry version that the GPU never received.
    scene
        .transaction()
        .set_transform(paint_node, Affine::translate((2.0, 3.0)))
        .commit()?;
    update(&mut materializer, &scene);
    let canvas = materializer.canvas();
    assert!(canvas.buffer_changes.as_ref().unwrap().paths.is_empty());
    let (actual, config) = geometry(&context, &mut cache, &canvas)?;
    let (expected, _) = geometry(&context, &mut SceneCache::default(), &canvas)?;
    assert_eq!(config.unwrap()[6], 0);
    assert_eq!(actual, expected);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn reuse_after_removal_cannot_leave_stale_full_cumsum_metadata() -> Result<()> {
    let context = gpu_support::context()?;
    let root = RetainedNodeId::for_owner(980);
    let mut scene = RetainedScene::new(192, 192, 1.0, root)?;
    for i in 0..8 {
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(981 + i),
                Rc::new(leaf()),
                Affine::translate((i as f64, 0.0)),
            )
            .commit()?;
    }
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let initial = materializer.canvas();
    let owner = initial.path_records[0].transform.e as u64;
    let removed = RetainedNodeId::for_owner(981 + owner);
    let survivor = RetainedNodeId::for_owner(981 + (owner + 1) % 8);
    let mut cache = SceneCache::default();
    geometry(&context, &mut cache, &initial)?;
    scene.transaction().remove_subtree(removed).commit()?;
    update(&mut materializer, &scene);
    let (_, removal_config) = geometry(&context, &mut cache, &materializer.canvas())?;
    assert!(
        removal_config.is_none(),
        "removing path zero should leave no live geometry to scan"
    );
    scene
        .transaction()
        .set_transform(survivor, Affine::scale(2.0))
        .commit()?;
    update(&mut materializer, &scene);
    let canvas = materializer.canvas();
    let mut batch = gpu_support::batch(&context);
    cache.record(&mut batch, &canvas, None, None, 65535)?;
    assert_eq!(
        config(&batch).unwrap()[6],
        0,
        "larger segment allocation requires a full scan"
    );
    let prefix = batch
        .passes()
        .iter()
        .find(|pass| pass.shader.entry == "cumsum_prefix_chunks")
        .unwrap();
    let lengths = prefix
        .bindings
        .iter()
        .find(|(binding, _)| binding.slot == 2)
        .unwrap()
        .1;
    batch.readback(lengths)?;
    let expected_lengths =
        bytemuck::cast_slice::<_, u8>(&cache.staging.path_plans.cumsum_plan().chunk_lens).to_vec();
    let metadata = context
        .adapter
        .submit_compute(&batch)
        .map_err(|e| format!("{e:?}"))?
        .readback()?;
    assert_eq!(
        &metadata[0][..expected_lengths.len()],
        expected_lengths,
        "removed chunks must not remain live in the next full cumsum"
    );
    let (actual, _) = geometry(&context, &mut cache, &canvas)?;
    let (expected, _) = geometry(&context, &mut SceneCache::default(), &canvas)?;
    assert_eq!(actual, expected);
    context.check_validation()?;
    Ok(())
}
