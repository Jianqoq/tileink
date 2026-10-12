use super::*;

#[test]
fn world_edge_bounds_translate_without_wrapping_in_local_surfaces() {
    let world = Bounds::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    for (origin, expected) in [
        (
            (-8, -16),
            Bounds::new(i32::MIN + 8, i32::MIN + 16, i32::MAX, i32::MAX),
        ),
        (
            (8, 16),
            Bounds::new(i32::MIN, i32::MIN, i32::MAX - 8, i32::MAX - 16),
        ),
    ] {
        let local = LocalSpace::new(Bounds::new(origin.0, origin.1, 64, 64));
        assert_eq!(local.bounds(world), expected);
        let pixels = local.pixel_bounds(PixelBounds {
            x0: world.x0,
            y0: world.y0,
            x1: world.x1,
            y1: world.y1,
        });
        assert_eq!(
            [pixels.x0, pixels.y0, pixels.x1, pixels.y1],
            [expected.x0, expected.y0, expected.x1, expected.y1]
        );
    }
}

#[test]
#[allow(
    clippy::lossy_float_literal,
    reason = "The expected value is 2^31, which is exactly representable in f32"
)]
fn path_points_translate_from_the_minimum_world_origin_in_float_space() {
    let local = LocalSpace::new(Bounds::new(
        i32::MIN,
        i32::MIN,
        i32::MIN + 32,
        i32::MIN + 32,
    ));
    let line = local.line(Line {
        path_id: 7,
        _pad: 0.0,
        p0: [i32::MIN as f32; 2],
        p1: [0.0; 2],
    });
    assert_eq!(line.p0, [0.0; 2]);
    assert_eq!(line.p1, [2_147_483_648.0; 2]);
    assert_eq!(line.path_id, 7);
}
use crate::shared::{
    execution::ROOT_COMMAND_LIST_ID, fill::FillRule, image::Image, image_resource::ImageKey,
};
use peniko::{Color, kurbo::Shape};

#[test]
fn local_scene_ignores_layer_stacks_outside_its_children() {
    let mut canvas = Canvas::new(128, 128, 1.0);
    for x in [0.0, 80.0] {
        canvas.push_clip_sdf_rect_layer(Rect::new(x, 0.0, x + 32.0, 32.0), crate::Radius::all(4.0));
        canvas.push_rect(
            Rect::new(x + 4.0, 4.0, x + 28.0, 28.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        canvas.pop_layer();
    }
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let second = plan
        .ops
        .iter()
        .rposition(|op| matches!(op, ExecOp::DrawBatch { .. }))
        .unwrap();
    let local = local_offscreen_scene(
        &canvas,
        &plan,
        &plan.ops[second..=second],
        Bounds::new(80, 0, 112, 32),
        &[3],
    );
    assert_eq!(local.canvas.draw_records.len(), 2);
    assert_eq!(local.plan.layer_stack_data.len(), 1);
    assert!(local.plan.layer_stack_data.iter().all(|entry| match entry {
        crate::shared::execution::LayerStackEntry::Clip { draw } =>
            (*draw as usize) < local.canvas.draw_records.len(),
        _ => false,
    }));
}

#[test]
fn local_selection_represents_every_requested_batch_even_when_empty() {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let selection = local_draw_selection(&canvas, &plan, &plan.ops, &[], Bounds::canvas(32, 32));
    assert!(selection.draws.is_empty());
    let ExecOp::DrawBatch { batch_id, .. } = &plan.ops[0] else {
        unreachable!()
    };
    assert_eq!(selection.batch_draws[batch_id], Vec::<usize>::new());
}

#[test]
#[should_panic(expected = "encoded offscreen brush")]
fn invalid_internal_brush_is_not_replaced_with_an_unpainted_draw() {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    canvas.draw_records[0].brush_len = 1;
    translated_scene_for_draws(&canvas, LocalSpace::new(Bounds::canvas(32, 32)), &[0]);
}

#[test]
fn local_space_shifts_geometry_to_surface_origin() {
    let local = LocalSpace::new(Bounds::new(10, 20, 30, 40));

    let line = local.line(Line {
        path_id: 3,
        _pad: 0.0,
        p0: [12.0, 23.0],
        p1: [18.0, 31.0],
    });
    assert_eq!(line.path_id, 3);
    assert_eq!(line.p0, [2.0, 3.0]);
    assert_eq!(line.p1, [8.0, 11.0]);
    assert_eq!(
        local.pixel_bounds(PixelBounds {
            x0: 11,
            y0: 22,
            x1: 29,
            y1: 39,
        }),
        PixelBounds {
            x0: 1,
            y0: 2,
            x1: 19,
            y1: 19,
        }
    );
    assert_eq!(
        local.bounds(Bounds::new(12, 24, 25, 36)),
        Bounds::new(2, 4, 15, 16)
    );
}

#[test]
fn local_space_keeps_brush_sampling_in_world_space() {
    let local = LocalSpace::new(Bounds::new(10, 20, 30, 40));

    assert_eq!(
        local.brush_transform([2.0, 3.0, 5.0, 7.0, 11.0, 13.0]),
        [2.0, 3.0, 5.0, 7.0, 131.0, 183.0]
    );
}

#[test]
fn translated_scene_preserves_scene_image_resources() {
    let key = ImageKey::new(42);
    let mut canvas = Canvas::new(8, 8, 1.0);
    assert!(
        canvas
            .scene_images
            .insert(key, Image::from_rgba8(1, 1, [255, 0, 0, 255]))
    );

    let (translated, _) =
        translated_scene_for_draws(&canvas, LocalSpace::new(Bounds::new(2, 2, 6, 6)), &[]);

    let image = translated
        .scene_image_resources()
        .get(key)
        .expect("translated scene should keep scene image resources");
    assert_eq!(image.rgba8_at(0, 0), [255, 0, 0, 255]);
}

#[test]
fn local_scene_compacts_draw_and_path_ids_to_the_filter_region() {
    let mut canvas = Canvas::new(128, 128, 1.0);
    canvas.push_path(
        Rect::new(12.0, 12.0, 28.0, 28.0).to_path(0.1),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    canvas.push_path(
        Rect::new(96.0, 96.0, 112.0, 112.0).to_path(0.1),
        Color::from_rgb8(0, 0, 255),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let local = local_offscreen_scene(
        &canvas,
        &plan,
        &plan.ops,
        Bounds::new(8, 8, 40, 40),
        &[0, 1],
    );

    assert_eq!(local.canvas.draw_records.len(), 1);
    assert_eq!(local.canvas.path_records.len(), 1);
    assert_eq!(local.canvas.draw_records[0].path_id, 0);
    assert_eq!(local.canvas.path_records[0].path_id, 0);
    assert!(local.canvas.lines.iter().all(|line| line.path_id == 0));
    assert_eq!(local.canvas.draw_records[0].pixel_bounds.x0, 4);
    let ExecOp::DrawBatch { draws, .. } = &local.children[0] else {
        panic!("plain paths should remain one local draw batch")
    };
    assert_eq!(draws.as_slice(), &[0]);
}

#[test]
fn empty_local_filter_has_no_hidden_geometry_record() {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_filter_layer(
        Filter::Opacity(0.5),
        Region::rect(Rect::new(8.0, 8.0, 24.0, 24.0), crate::Radius::ZERO),
    );
    canvas.pop_layer();
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let local = local_offscreen_scene(&canvas, &plan, &plan.ops, Bounds::new(8, 8, 24, 24), &[]);
    assert!(local.canvas.draw_records.is_empty());
    assert_eq!(local.children.len(), 1);
}
