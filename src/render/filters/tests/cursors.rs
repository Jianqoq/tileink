use super::*;
use crate::render::filter_resources::{paths::FilterPathUpload, tables::FilterTransferUpload};
use crate::shared::layer::Layer;
use crate::shared::layer::filter::COMPONENT_TRANSFER_TABLE_LEN;
use peniko::kurbo::{Affine, BezPath, Shape};

fn execute_op(
    adapter: &mut Adapter,
    canvas: &Canvas,
    plan: &ExecPlan,
    index: usize,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
) {
    let ExecOp::OffscreenLayer {
        layer: Layer::Filter {
            filter,
            sample_region,
        },
        children,
        ..
    } = &plan.ops[index]
    else {
        panic!("filter fixture")
    };
    execute(
        adapter,
        canvas,
        plan,
        FilterLayer {
            retained_id: None,
            filter,
            region: sample_region,
            stack: 2..4,
            children,
        },
        target,
        cursors,
    )
    .unwrap();
}

#[test]
fn full_canvas_filter_on_scratch_preserves_prefix_resource_indices() {
    let (canvas, mut plan) = fixture();
    let mut second = plan.ops[1].clone();
    let ExecOp::OffscreenLayer {
        layer: Layer::Filter { filter, .. },
        ..
    } = &mut second
    else {
        unreachable!()
    };
    *filter = Filter::ComponentTransfer(Box::new([22; COMPONENT_TRANSFER_TABLE_LEN]));
    let upload = FilterTransferUpload::from_ops_and_filter(&plan.ops, filter);
    assert_eq!(upload.tables[0], 11);
    assert_eq!(upload.tables[COMPONENT_TRANSFER_TABLE_LEN], 22);
    plan.ops.push(second);
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    adapter.targets.push(Some(Allocation(900)));
    let mut cursors = FilterCursors::default();
    cursors.advance_ops(&plan.ops[..2]);
    execute_op(
        &mut adapter,
        &canvas,
        &plan,
        2,
        RenderTargetId::Scratch(0),
        &mut cursors,
    );
    assert_eq!(adapter.filter_transfer_indices, [1]);
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(adapter.targets[0].as_ref().unwrap().0, 900);
}

#[test]
fn cropped_filter_after_prefix_uses_compacted_resource_indices() {
    let (canvas, mut plan) = fixture();
    let mut second = plan.ops[1].clone();
    let ExecOp::OffscreenLayer {
        layer: Layer::Filter {
            filter,
            sample_region,
        },
        ..
    } = &mut second
    else {
        unreachable!()
    };
    *filter = Filter::ComponentTransfer(Box::new([22; COMPONENT_TRANSFER_TABLE_LEN]));
    *sample_region = Region::rect(Rect::new(8.0, 8.0, 24.0, 24.0), Radius::ZERO);
    plan.ops.push(second);
    let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
    let mut cursors = FilterCursors::default();
    cursors.advance_ops(&plan.ops[..2]);
    execute_op(
        &mut adapter,
        &canvas,
        &plan,
        2,
        RenderTargetId::Main,
        &mut cursors,
    );
    assert_eq!(adapter.filter_transfer_indices, [0]);
    assert_eq!(cursors.next_transfer_index(), 2);
    assert_eq!(adapter.candidate_queries.get(), 1);
}

#[test]
fn filter_child_paths_follow_full_or_compacted_plan_layout() {
    for (full, target) in [
        (true, RenderTargetId::Main),
        (true, RenderTargetId::Scratch(0)),
        (false, RenderTargetId::Main),
        (false, RenderTargetId::Scratch(0)),
    ] {
        let (canvas, mut plan) = fixture();
        let mut child = plan.ops[1].clone();
        let mut triangle = BezPath::new();
        triangle.move_to((0.0, 0.0));
        triangle.line_to((32.0, 0.0));
        triangle.line_to((0.0, 32.0));
        triangle.close_path();
        let ExecOp::OffscreenLayer {
            layer: Layer::Filter { sample_region, .. },
            ..
        } = &mut child
        else {
            unreachable!()
        };
        *sample_region = Region::path(triangle, Affine::IDENTITY, 0.1);
        let mut parent = plan.ops[1].clone();
        let ExecOp::OffscreenLayer {
            layer: Layer::Filter { sample_region, .. },
            children,
            ..
        } = &mut parent
        else {
            unreachable!()
        };
        *sample_region = Region::path(
            if full {
                Rect::new(0.0, 0.0, 32.0, 32.0)
            } else {
                Rect::new(8.0, 8.0, 24.0, 24.0)
            }
            .to_path(0.1),
            Affine::IDENTITY,
            0.1,
        );
        *children = vec![child];
        plan.ops = vec![parent];
        assert_eq!(FilterPathUpload::from_plan(&plan).range_starts.len(), 2);
        let mut adapter = Adapter::new(&canvas, RetainedSurfaceKind::Filter);
        let mut cursors = FilterCursors::default();
        execute_op(&mut adapter, &canvas, &plan, 0, target, &mut cursors);
        assert_eq!(
            adapter.filter_path_indices,
            [Some(u32::from(full))],
            "full={full} {target:?}"
        );
        let ExecOp::OffscreenLayer {
            layer: Layer::Filter { sample_region, .. },
            ..
        } = &plan.ops[0]
        else {
            unreachable!()
        };
        assert_eq!(cursors.next_path_index(sample_region), Some(2));
    }
}
