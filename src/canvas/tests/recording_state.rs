use super::*;
use crate::shared::layer::mask::MaskKind;

fn draw(canvas: &mut Canvas) -> DrawId {
    canvas.push_rect(Rect::new(2.0, 2.0, 18.0, 18.0), Radius::ZERO, Color::WHITE)
}

fn draw_list(canvas: &Canvas, draw: DrawId) -> CommandListId {
    canvas
        .command_lists
        .iter()
        .position(|list| {
            list.commands
                .iter()
                .any(|command| matches!(command, Command::Draw(id) if *id == draw.index()))
        })
        .expect("draw must belong to a recorded command list")
}

fn mask() -> Mask {
    Mask {
        region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
        kind: MaskKind::Alpha,
    }
}

#[test]
fn every_layer_kind_restores_its_parent_after_clone() {
    let mut canvas = test_scene();
    let root = draw(&mut canvas);
    let mut parents = vec![draw_list(&canvas, root)];
    let kinds = [
        LayerKind::Clip,
        LayerKind::ClipSdf,
        LayerKind::Isolate,
        LayerKind::Opacity,
        LayerKind::Blend,
        LayerKind::Filter,
        LayerKind::Backdrop,
        LayerKind::Mask,
    ];
    for kind in kinds {
        match kind {
            LayerKind::Clip => {
                let mut path = BezPath::new();
                path.move_to((0.0, 0.0));
                path.line_to((32.0, 0.0));
                path.line_to((16.0, 32.0));
                path.close_path();
                canvas.push_clip_layer(path, Affine::IDENTITY, FillRule::NonZero, 0.1);
            }
            LayerKind::ClipSdf => {
                canvas.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO)
            }
            LayerKind::Isolate => {
                canvas.push_isolate_layer(rect_path(0.0, 0.0, 32.0, 32.0), Affine::IDENTITY, 0.1)
            }
            LayerKind::Opacity => canvas.push_opacity_layer(
                rect_path(0.0, 0.0, 32.0, 32.0),
                Affine::IDENTITY,
                0.1,
                0.5,
            ),
            LayerKind::Blend => canvas.push_blend_layer(
                rect_path(0.0, 0.0, 32.0, 32.0),
                Affine::IDENTITY,
                0.1,
                Mix::Multiply,
                Compose::SrcOver,
            ),
            LayerKind::Filter => canvas.push_filter_layer(Filter::Invert(1.0), mask().region),
            LayerKind::Backdrop => canvas.push_backdrop_layer(Filter::Invert(1.0), mask().region),
            LayerKind::Mask => {
                let mut source = test_scene();
                draw(&mut source);
                canvas.push_mask_layer(source, mask());
            }
        }
        let child = draw(&mut canvas);
        parents.push(draw_list(&canvas, child));
        assert!(!canvas.is_closed_for_append());
    }
    let mut clone = canvas.clone();
    for target in [&mut clone, &mut canvas] {
        for (index, kind) in kinds.into_iter().enumerate().rev() {
            assert_eq!(target.pop_layer(), Some(kind));
            let next = draw(target);
            assert_eq!(draw_list(target, next), parents[index]);
        }
        assert!(target.is_closed_for_append());
        assert_eq!(target.pop_layer(), None);
        assert_eq!(target.pop_layer(), None);
    }
}

#[test]
fn append_and_mask_imports_do_not_change_the_active_parent() {
    let mut child = test_scene();
    child.push_isolate_layer(rect_path(0.0, 0.0, 32.0, 32.0), Affine::IDENTITY, 0.1);
    draw(&mut child);
    child.pop_layer();
    draw(&mut child);

    let mut canvas = test_scene();
    canvas.push_opacity_layer(rect_path(0.0, 0.0, 32.0, 32.0), Affine::IDENTITY, 0.1, 0.5);
    let first = draw(&mut canvas);
    let parent = draw_list(&canvas, first);
    canvas.append(&child, Point::new(1.0, 1.0));
    let next = draw(&mut canvas);
    assert_eq!(draw_list(&canvas, next), parent);
    canvas.push_mask_layer(child, mask());
    let content = draw(&mut canvas);
    assert_ne!(draw_list(&canvas, content), parent);
    assert_eq!(canvas.pop_layer(), Some(LayerKind::Mask));
    let next = draw(&mut canvas);
    assert_eq!(draw_list(&canvas, next), parent);
    assert_eq!(canvas.pop_layer(), Some(LayerKind::Opacity));
    let next = draw(&mut canvas);
    assert_eq!(draw_list(&canvas, next), ROOT_COMMAND_LIST_ID);
    assert!(canvas.is_closed_for_append());
}

#[test]
fn reset_discards_deep_open_groups_and_rejects_stale_draw_handles() {
    let mut canvas = test_scene();
    let stale = draw(&mut canvas);
    for _ in 0..64 {
        canvas.push_opacity_layer(rect_path(0.0, 0.0, 32.0, 32.0), Affine::IDENTITY, 0.1, 0.5);
        draw(&mut canvas);
    }
    canvas.reset();
    assert!(canvas.is_closed_for_append());
    assert_eq!(canvas.pop_layer(), None);
    assert_eq!(canvas.command_lists.len(), 1);
    let current = draw(&mut canvas);
    assert_eq!(current.index(), stale.index());
    assert_eq!(draw_list(&canvas, current), ROOT_COMMAND_LIST_ID);
    assert!(canvas.draw_brush(stale).is_none());
    assert!(!canvas.set_draw_color(stale, Color::BLACK));
    assert!(canvas.draw_id_at(usize::MAX).is_none());
    assert_eq!(canvas.draw_solid_color(current), Some(Color::WHITE));
}
