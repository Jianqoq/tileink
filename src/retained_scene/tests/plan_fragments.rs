use super::*;
use crate::shared::execution::{ExecOp, ROOT_COMMAND_LIST_ID};

fn filter() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(1.0),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
    }
}

fn group(opacity: bool) -> RetainedLayerDescriptor {
    let path = Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1);
    if opacity {
        RetainedLayerDescriptor::Opacity {
            path,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
            opacity: 0.5,
        }
    } else {
        RetainedLayerDescriptor::ClipPath {
            path,
            transform: Affine::IDENTITY,
            rule: FillRule::NonZero,
            tolerance: 0.1,
        }
    }
}

fn contains_draw(ops: &[ExecOp], draw: usize) -> bool {
    ops.iter().any(|op| match op {
        ExecOp::DrawBatch { draws, .. } => draws.contains(&draw),
        ExecOp::OffscreenLayer { children, .. } => contains_draw(children, draw),
        ExecOp::OffscreenMaskLayer { content, mask, .. } => {
            contains_draw(content, draw) || contains_draw(mask, draw)
        }
    })
}

#[test]
fn removing_a_root_fragment_after_a_layer_kind_change_keeps_its_sibling() {
    let id = RetainedNodeId::for_owner;
    let root = id(97_000);
    let changed = id(97_001);
    let nested = id(97_002);
    let sibling = id(97_003);
    let survivor = id(97_004);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(root), None, changed, group(false))
        .insert_layer(RetainedParent::content(changed), None, nested, filter())
        .insert_layer(RetainedParent::content(root), None, sibling, filter())
        .insert_scene(
            RetainedParent::content(sibling),
            None,
            survivor,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .update_layer(changed, group(true))
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    scene
        .transaction()
        .remove_subtree(changed)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    let plan = state.canvas.compile(ROOT_COMMAND_LIST_ID);
    for draw in state.node_physical_draws(survivor) {
        assert!(
            contains_draw(&plan.ops, draw),
            "removing the changed fragment removed an unrelated live sibling"
        );
    }
    assert_eq!(root_retained_commands(&state), [sibling]);
}
