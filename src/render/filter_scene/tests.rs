use super::PreparedFilterScene;
use crate::{
    FillRule,
    canvas::Canvas,
    shared::{bounds::Bounds, execution::ROOT_COMMAND_LIST_ID, layer::filter::Filter},
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::cell::Cell;

#[test]
fn full_surface_borrows_scene_plan_and_children_without_querying_draws() {
    let canvas = Canvas::new(128, 96, 1.0);
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let prepared = PreparedFilterScene::new(
        &canvas,
        &plan,
        &plan.ops,
        &Filter::Opacity(0.5),
        Bounds::canvas(128, 96),
        || panic!("full surface must not query or clone draw arenas"),
    );
    let (local_canvas, local_plan, local_children) = prepared.scene();
    assert!(std::ptr::eq(local_canvas, &canvas));
    assert!(std::ptr::eq(local_plan, &plan));
    assert_eq!(local_children.as_ptr(), plan.ops.as_ptr());
    assert!(prepared.is_root_scene());
    assert!(matches!(prepared.filter(), Filter::Opacity(value) if *value == 0.5));
}

#[test]
fn cropped_surface_queries_once_and_compacts_geometry_in_local_coordinates() {
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
    let calls = Cell::new(0);
    let prepared = PreparedFilterScene::new(
        &canvas,
        &plan,
        &plan.ops,
        &Filter::Opacity(1.0),
        Bounds::new(8, 8, 40, 40),
        || {
            calls.set(calls.get() + 1);
            vec![0, 1]
        },
    );
    let (local_canvas, local_plan, local_children) = prepared.scene();
    assert_eq!(calls.get(), 1);
    assert!(!prepared.is_root_scene());
    assert!(!std::ptr::eq(local_canvas, &canvas));
    assert!(!std::ptr::eq(local_plan, &plan));
    assert_eq!(
        (
            local_canvas.physical_width(),
            local_canvas.physical_height()
        ),
        (32, 32)
    );
    assert_eq!(local_canvas.draw_records.len(), 1);
    assert_eq!(local_canvas.draw_records[0].pixel_bounds.x0, 4);
    assert_eq!(local_children.len(), 1);
    assert_eq!(canvas.draw_records.len(), 2);
}

#[test]
fn full_surface_detection_uses_physical_size_and_includes_the_origin() {
    let canvas = Canvas::new(64, 64, 2.0);
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    for (bounds, root) in [
        (Bounds::canvas(128, 128), true),
        (Bounds::canvas(64, 64), false),
        (Bounds::new(16, 0, 144, 128), false),
    ] {
        let calls = Cell::new(0);
        let prepared =
            PreparedFilterScene::new(&canvas, &plan, &[], &Filter::Opacity(1.0), bounds, || {
                calls.set(calls.get() + 1);
                vec![]
            });
        assert_eq!(prepared.is_root_scene(), root);
        assert_eq!(calls.get(), usize::from(!root));
        assert!(prepared.scene().2.is_empty());
    }
}

#[test]
fn scratch_capacity_includes_filter_temporaries_and_both_history_slots() {
    let canvas = Canvas::new(32, 32, 1.0);
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    for (filter, extra) in [
        (Filter::Opacity(1.0), 0),
        (Filter::Offset { dx: 1.0, dy: -2.0 }, 1),
    ] {
        let prepared = PreparedFilterScene::new(
            &canvas,
            &plan,
            &plan.ops,
            &filter,
            Bounds::canvas(32, 32),
            Vec::new,
        );
        assert_eq!(prepared.scratch_count(false), 1 + extra);
        assert_eq!(prepared.scratch_count(true), 3 + extra);
    }
}

#[test]
fn borrowing_the_root_plan_does_not_expand_the_requested_child_list() {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_path(
        Rect::new(0.0, 0.0, 16.0, 16.0).to_path(0.1),
        Color::from_rgb8(255, 0, 0),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert!(!plan.ops.is_empty());
    let prepared = PreparedFilterScene::new(
        &canvas,
        &plan,
        &[],
        &Filter::Opacity(1.0),
        Bounds::canvas(32, 32),
        Vec::new,
    );
    assert!(std::ptr::eq(prepared.scene().1, &plan));
    assert!(prepared.scene().2.is_empty());
}

#[test]
fn off_canvas_filter_sources_survive_the_root_spatial_index_clip() {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(
        Rect::new(-16.0, 0.0, -15.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    assert_eq!(canvas.draw_records.len(), 1);
    let prepared = PreparedFilterScene::new(
        &canvas,
        &plan,
        &plan.ops,
        &Filter::Opacity(1.0),
        Bounds::new(-16, 0, 48, 64),
        Vec::new,
    );
    // Root tile bins cannot enumerate an entirely off-canvas source. Local
    // preparation must retain it before translation makes it visible locally.
    assert_eq!(prepared.scene().0.draw_records.len(), 1);
    assert_eq!(prepared.scene().0.draw_records[0].pixel_bounds.x0, 0);
}

#[test]
fn nested_wrap_retains_input_outside_the_outer_surface_in_painter_order() {
    use crate::Region;
    use crate::shared::layer::filter::{ConvolveEdgeMode, ConvolveMatrix};
    let mut canvas = Canvas::new(128, 128, 1.0);
    canvas.push_filter_layer(
        Filter::ConvolveMatrix(ConvolveMatrix {
            columns: 3,
            rows: 1,
            target_x: 1,
            target_y: 0,
            data: vec![1.0, 0.0, 0.0],
            divisor: 1.0,
            bias: 0.0,
            edge_mode: ConvolveEdgeMode::Wrap,
            preserve_alpha: false,
        }),
        Region::Rect {
            rect: Rect::new(16.0, 32.0, 64.0, 64.0),
            radius: crate::Radius::ZERO,
        },
    );
    canvas.push_rect(
        Rect::new(16.0, 32.0, 17.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.push_rect(
        Rect::new(40.0, 32.0, 41.0, 64.0),
        crate::Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    let plan = canvas.compile(ROOT_COMMAND_LIST_ID);
    let prepared = PreparedFilterScene::new(
        &canvas,
        &plan,
        &plan.ops,
        &Filter::Opacity(1.0),
        Bounds::new(32, 32, 64, 64),
        || vec![2],
    );
    let (local, _, ops) = prepared.scene();
    let crate::shared::execution::ExecOp::OffscreenLayer { children, .. } = &ops[0] else {
        panic!("nested filter missing")
    };
    let crate::shared::execution::ExecOp::DrawBatch { draws, .. } = &children[0] else {
        panic!("child batch missing")
    };
    assert_eq!(
        draws.len(),
        2,
        "outer compaction must preserve the nested filter's input domain"
    );
    let origins = draws
        .iter()
        .map(|draw| local.draw_records[*draw].pixel_bounds.x0)
        .collect::<Vec<_>>();
    assert_eq!(
        origins,
        [-16, 8],
        "index candidates and supplemental sources retain painter order"
    );
}
