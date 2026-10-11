use super::*;
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_renderer_backdrop_and_retained_updates_match_fresh_frames() -> Result<()>
{
    let context = context()?;
    let pipeline = pipeline(ShaderTextureFormat::Rgba16Float)?;
    let region = Region::rect(Rect::new(0.0, 0.0, 33.0, 19.0), Radius::ZERO);
    let mut renderer = NativeRenderer::with_context(&context, 33, 19)?;
    let root = RetainedNodeId::for_owner(1000);
    let layer = RetainedNodeId::for_owner(1001);
    let child = RetainedNodeId::for_owner(1002);
    let content = |color| {
        let mut c = Canvas::new(33, 19, 1.0);
        c.push_rect(Rect::new(0.0, 0.0, 33.0, 19.0), Radius::ZERO, color);
        Rc::new(c)
    };
    let descriptor = |p: ShaderPipeline| RetainedLayerDescriptor::Filter {
        filter: p.into(),
        sample_region: region.clone(),
    };
    let mut scene = RetainedScene::new(33, 19, 1.0, root)?;
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            descriptor(pipeline.clone()),
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            child,
            content(Color::from_rgb8(180, 80, 31)),
            Affine::IDENTITY,
        )
        .commit()?;
    for phase in 0..4 {
        if phase == 1 {
            scene
                .transaction()
                .replace_scene(child, content(Color::from_rgb8(90, 160, 31)))
                .commit()?;
        }
        if phase == 2 {
            scene
                .transaction()
                .update_layer(
                    layer,
                    descriptor(pipeline.with_resource(
                        8,
                        ShaderResource::Image {
                            size: [1, 1],
                            format: ShaderTextureFormat::Rgba8Unorm,
                            bytes: Rc::from([64, 255, 0, 255]),
                        },
                    )?),
                )
                .commit()?;
        }
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, 33, 19)?;
        assert_eq!(
            actual.pixels,
            fresh.render_retained_to_image(&scene)?.readback()?.pixels
        );
        let pixel = if phase == 0 {
            0xff1f50b4u32
        } else {
            0xff1fa05au32
        };
        check(
            &actual.pixels,
            &vec![pixel; 33 * 19],
            [33, 19],
            if phase >= 2 { 64 } else { 128 },
        );
    }
    let mut backdrop = Canvas::new(33, 19, 1.0);
    backdrop.push_rect(
        Rect::new(0.0, 0.0, 33.0, 19.0),
        Radius::ZERO,
        Color::from_rgb8(180, 80, 31),
    );
    backdrop.push_backdrop_layer(pipeline.into(), region);
    backdrop.pop_layer();
    check(
        &renderer.render_to_image(&backdrop)?.readback()?.pixels,
        &vec![0xff1f50b4; 33 * 19],
        [33, 19],
        128,
    );
    context.check_validation()?;
    Ok(())
}
#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_pipeline_retained_backdrop_tracks_reads_outside_output_region() -> Result<()> {
    use ShaderBindingKind as K;
    let context = context()?;
    let kernel = ShaderKernel::new(
        ShaderSources {
            metal: Some(include_str!("shaders/pipeline.metal").into()),
            hlsl: Some(include_str!("shaders/pipeline.hlsl").into()),
        },
        "global_color",
        [8, 8, 1],
        vec![
            ShaderBinding {
                name: "config".into(),
                slot: 0,
                kind: K::Config,
            },
            ShaderBinding {
                name: "input_image".into(),
                slot: 1,
                kind: K::Texture,
            },
            ShaderBinding {
                name: "output_image".into(),
                slot: 3,
                kind: K::WriteTexture(ShaderTextureFormat::Rgba8Unorm),
            },
        ],
    )?;
    let pipeline = ShaderPipeline::new(
        vec![
            ShaderResource::Input(0),
            ShaderResource::Output,
            ShaderResource::config(ShaderExtent::FULL),
        ],
        vec![ShaderPass {
            kernel,
            bindings: vec![(0, 2), (1, 0), (3, 1)],
            dispatch: ShaderDispatch::Image(ShaderExtent::FULL),
        }],
    )?;
    let root = RetainedNodeId::for_owner(1010);
    let background = RetainedNodeId::for_owner(1011);
    let layer = RetainedNodeId::for_owner(1012);
    let content = |color| {
        let mut c = Canvas::new(64, 64, 1.0);
        c.push_rect(Rect::new(0.0, 0.0, 2.0, 2.0), Radius::ZERO, color);
        Rc::new(c)
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, root)?;
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            background,
            content(Color::from_rgb8(180, 80, 31)),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::Backdrop {
                filter: pipeline.into(),
                sample_region: Region::rect(Rect::new(32.0, 32.0, 48.0, 48.0), Radius::ZERO),
            },
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 64)?;
    for phase in 0..3 {
        if phase == 1 {
            scene
                .transaction()
                .replace_scene(background, content(Color::from_rgb8(90, 160, 31)))
                .commit()?;
        }
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, 64, 64)?;
        assert_eq!(
            actual.pixels,
            fresh.render_retained_to_image(&scene)?.readback()?.pixels,
            "phase={phase}"
        );
        assert_eq!(
            actual.pixels[40 * 64 + 40],
            if phase == 0 { 0xff1f50b4 } else { 0xff1fa05a }
        );
    }
    context.check_validation()?;
    Ok(())
}
