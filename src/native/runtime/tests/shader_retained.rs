use super::*;
use crate::RetainedLayerDescriptor;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_shader_retained_filter_history_handles_local_source_and_uniform_edits() -> Result<()> {
    let context = context()?;
    let program = input_scale()?;
    let root = RetainedNodeId::for_owner(900);
    let layer = RetainedNodeId::for_owner(901);
    let child = RetainedNodeId::for_owner(902);
    let region = Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO);
    let descriptor = |amount| -> Result<RetainedLayerDescriptor> {
        Ok(RetainedLayerDescriptor::Filter {
            filter: program.filter(uniform(&[[amount, 1.0, 1.0, 1.0]]))?.into(),
            sample_region: region.clone(),
        })
    };
    let content = |color| {
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_rect(Rect::new(12.0, 10.0, 28.0, 26.0), Radius::ZERO, color);
        Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(64, 64, 1.0, root)?;
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, layer, descriptor(1.0)?)
        .insert_scene(
            RetainedParent::content(layer),
            None,
            child,
            content(Color::from_rgb8(180, 80, 120)),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 64)?;
    for phase in 0..4 {
        if phase == 1 {
            scene
                .transaction()
                .replace_scene(child, content(Color::from_rgb8(80, 180, 120)))
                .commit()?;
        }
        if phase == 2 {
            scene
                .transaction()
                .update_layer(layer, descriptor(0.25)?)
                .commit()?;
        }
        let actual = renderer
            .render_retained_to_image(&scene)?
            .readback()?
            .pixels;
        let mut fresh = NativeRenderer::with_context(&context, 64, 64)?;
        assert_eq!(
            actual,
            fresh.render_retained_to_image(&scene)?.readback()?.pixels,
            "phase={phase}"
        );
        let expected = if phase == 0 {
            0xff78_50b4
        } else if phase == 1 {
            0xff78_b450
        } else {
            0xff78_b414
        };
        assert_eq!(actual[18 * 64 + 20], expected);
        assert_eq!(actual[0], 0);
    }
    context.check_validation()?;
    Ok(())
}
