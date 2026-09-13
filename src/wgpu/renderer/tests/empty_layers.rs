use super::common::*;
use crate::render::{
    filter_resources::{cursors::FilterCursors, tables::FilterTransferUpload},
    output::RenderTargetId,
};
use crate::wgpu::commands::WgpuCommandBatch;

#[derive(Clone, Copy)]
enum EmptyLayer {
    Isolated,
    Mask,
    Filter,
    Backdrop,
}

fn transfer(marker: u32) -> Filter {
    Filter::ComponentTransfer(Box::new([marker; COMPONENT_TRANSFER_TABLE_LEN]))
}

fn filtered_child(canvas: &mut Canvas, marker: u32, region: Region) {
    canvas.push_filter_layer(transfer(marker), region);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    canvas.pop_layer();
}

fn check_empty_layer_resource_cursor(kind: EmptyLayer) {
    if !run_wgpu_tests() {
        return;
    }
    let full = Region::rect(Rect::new(0.0, 0.0, 16.0, 16.0), crate::Radius::ZERO);
    let outside = Rect::new(32.0, 32.0, 48.0, 48.0);
    let outside_region = Region::rect(outside, crate::Radius::ZERO);
    let mut canvas = Canvas::new(16, 16, 1.0);
    match kind {
        EmptyLayer::Isolated => {
            canvas.push_isolate_layer(outside.to_path(0.1), Affine::IDENTITY, 0.1)
        }
        EmptyLayer::Mask => {
            let mut mask_scene = Canvas::new(16, 16, 1.0);
            filtered_child(&mut mask_scene, 22, full.clone());
            canvas.push_mask_layer(
                mask_scene,
                Mask {
                    region: outside_region.clone(),
                    kind: MaskKind::Alpha,
                },
            );
        }
        EmptyLayer::Filter => canvas.push_filter_layer(transfer(33), outside_region.clone()),
        EmptyLayer::Backdrop => canvas.push_backdrop_layer(transfer(33), outside_region.clone()),
    }
    // A backdrop's children are not clipped to its sample region. For the cursor
    // probe this child's own filter is also out of bounds, so it needs no dispatch.
    // The separate pixel case below checks that visible backdrop children survive.
    let child_region = if matches!(kind, EmptyLayer::Backdrop) {
        outside_region
    } else {
        full.clone()
    };
    filtered_child(&mut canvas, 11, child_region);
    canvas.pop_layer();
    filtered_child(&mut canvas, 99, full);
    let plan = canvas.compile(canvas.root_commands);
    assert!(matches!(
        plan.ops[0],
        ExecOp::OffscreenLayer { .. } | ExecOp::OffscreenMaskLayer { .. }
    ));
    let upload = FilterTransferUpload::from_plan(&plan);
    let expected_markers: &[u32] = match kind {
        EmptyLayer::Isolated => &[11, 99],
        EmptyLayer::Mask => &[11, 22, 99],
        EmptyLayer::Filter => &[11, 33, 99],
        EmptyLayer::Backdrop => &[33, 11, 99],
    };
    assert_eq!(
        upload
            .tables
            .iter()
            .step_by(COMPONENT_TRANSFER_TABLE_LEN)
            .copied()
            .collect::<Vec<_>>(),
        expected_markers
    );
    let mut renderer = new_test_renderer(16, 16, Color::TRANSPARENT);
    let mut commands = WgpuCommandBatch::new(
        &renderer.device,
        &renderer.queue,
        "empty layer cursor regression",
    );
    let mut cursor = FilterCursors::default();
    assert!(renderer.execute_ops(
        &mut commands,
        &canvas,
        &plan,
        &plan.ops[..1],
        RenderTargetId::Main,
        &mut cursor,
        None
    ));
    commands.finish_with_status(true);
    let next = cursor.next_transfer_index() as usize;
    assert_eq!(
        upload.tables[next * COMPONENT_TRANSFER_TABLE_LEN],
        99,
        "skipping an empty layer must leave the next visible filter's actual table selected"
    );
}

#[test]
fn empty_isolated_group_advances_nested_filter_resources() {
    check_empty_layer_resource_cursor(EmptyLayer::Isolated);
}

#[test]
fn empty_mask_advances_content_and_mask_filter_resources() {
    check_empty_layer_resource_cursor(EmptyLayer::Mask);
}

#[test]
fn empty_filter_advances_children_and_parent_resources() {
    check_empty_layer_resource_cursor(EmptyLayer::Filter);
}

#[test]
fn empty_backdrop_advances_its_filter_and_visits_children() {
    check_empty_layer_resource_cursor(EmptyLayer::Backdrop);
}

#[test]
fn an_outside_backdrop_region_does_not_clip_visible_children() {
    if !run_wgpu_tests() {
        return;
    }
    let mut canvas = Canvas::new(16, 16, 1.0);
    canvas.push_backdrop_layer(
        Filter::Invert(1.0),
        Region::rect(Rect::new(32.0, 32.0, 48.0, 48.0), crate::Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 16.0, 16.0),
        crate::Radius::ZERO,
        Color::WHITE,
    );
    canvas.pop_layer();
    let image = render_native_wgpu(&canvas);
    assert_eq!(
        image.rgba8_at(8, 8),
        [255, 255, 255, 255],
        "Canvas backdrop semantics render children normally after the backdrop effect"
    );
}

// Empty backdrop effects still execute their foreground on Main. Verify that
// the actual adapter keeps the native overlap policy and portable single submit.
#[test]
fn empty_backdrop_foreground_preserves_root_submission_policy() {
    if !run_wgpu_tests() {
        return;
    }
    let mut canvas = Canvas::new(1024, 1024, 1.0);
    let outside = || {
        Region::rect(
            Rect::new(2048.0, 2048.0, 2064.0, 2064.0),
            crate::Radius::ZERO,
        )
    };
    canvas.push_backdrop_layer(Filter::Invert(1.0), outside());
    for batch in 0..16 {
        canvas.push_backdrop_layer(Filter::Invert(1.0), outside());
        let x = f64::from(batch) * 16.0;
        canvas.push_rect(
            Rect::new(x, 0.0, x + 16.0, 16.0),
            crate::Radius::ZERO,
            Color::WHITE,
        );
        canvas.pop_layer();
    }
    canvas.pop_layer();
    let mut renderer = new_test_renderer(1024, 1024, Color::TRANSPARENT);
    renderer.render(&canvas);
    let stats = renderer.incremental_render_stats();
    assert_eq!(
        stats.root_draw_batches, 16,
        "all backdrop foreground batches must actually execute on Main"
    );
    assert_eq!(
        stats.queue_submissions,
        if renderer.fine.as_ref().unwrap().uses_portable_textures() {
            1
        } else {
            2
        },
        "empty backdrop foreground must retain the native first-quarter overlap policy"
    );
    let image = renderer.image();
    assert_eq!(image.rgba8_at(8, 8), [255, 255, 255, 255]);
    assert_eq!(image.rgba8_at(248, 8), [255, 255, 255, 255]);
    assert_eq!(image.rgba8_at(512, 512), [0, 0, 0, 0]);
}
