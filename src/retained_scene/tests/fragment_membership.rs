use super::*;
use crate::shared::execution::{ExecOp, ROOT_COMMAND_LIST_ID};

fn layered_leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 32, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 16.0, 16.0), Radius::ZERO, Color::WHITE);
    let region = Rect::new(16.0, 0.0, 32.0, 16.0);
    canvas.push_filter_layer(Filter::Opacity(1.0), Region::rect(region, Radius::ZERO));
    canvas.push_rect(region, Radius::ZERO, Color::BLACK);
    canvas.pop_layer();
    canvas.push_rect(Rect::new(32.0, 0.0, 48.0, 16.0), Radius::ZERO, Color::WHITE);
    Rc::new(canvas)
}

fn contains_draw(ops: &[ExecOp], draw: usize, batch: u32) -> bool {
    ops.iter().any(|op| match op {
        ExecOp::DrawBatch {
            draws, batch_id, ..
        } => *batch_id == batch && draws.contains(&draw),
        ExecOp::OffscreenLayer { children, .. } => contains_draw(children, draw, batch),
        ExecOp::OffscreenMaskLayer { content, mask, .. } => {
            contains_draw(content, draw, batch) || contains_draw(mask, draw, batch)
        }
    })
}

#[test]
fn root_fragment_reclassifies_a_reparented_leaf_that_now_spans_batches() {
    let id = RetainedNodeId::for_owner;
    let mut scene = RetainedScene::new(64, 32, 1.0, id(96_000)).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(id(96_000)),
            None,
            id(96_001),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    assert!(state.node_batches.contains_key(&id(96_001)));
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(id(96_000)),
            None,
            id(96_002),
            RetainedLayerDescriptor::ClipPath {
                path: Rect::new(0.0, 0.0, 64.0, 32.0).to_path(0.1),
                transform: Affine::IDENTITY,
                rule: FillRule::NonZero,
                tolerance: 0.1,
            },
        )
        .replace_scene(id(96_001), layered_leaf())
        .reparent(id(96_001), RetainedParent::content(id(96_002)), None)
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    assert!(
        !state.node_batches.contains_key(&id(96_001)),
        "the old scalar batch must be removed when a leaf spans batches"
    );
    let plan = state.canvas.compile(ROOT_COMMAND_LIST_ID);
    for draw in state.node_physical_draws(id(96_001)) {
        assert!(contains_draw(
            &plan.ops,
            draw,
            state.canvas.painter.batch_ids[draw]
        ));
    }
}

#[test]
fn appended_root_fragment_preserves_every_batch_of_a_layered_leaf() {
    let id = RetainedNodeId::for_owner;
    let root = id(94_000);
    let base = id(94_001);
    let layer = id(94_002);
    let child = id(94_003);
    let mut scene = RetainedScene::new(64, 32, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            base,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            layer,
            RetainedLayerDescriptor::ClipPath {
                path: Rect::new(0.0, 0.0, 64.0, 32.0).to_path(0.1),
                transform: Affine::IDENTITY,
                rule: FillRule::NonZero,
                tolerance: 0.1,
            },
        )
        .insert_scene(
            RetainedParent::content(layer),
            None,
            child,
            layered_leaf(),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    assert!(update_materializer(&mut state, &scene));
    assert!(
        !state.node_batches.contains_key(&child),
        "this leaf spans several execution batches"
    );
    let plan = state.canvas.compile(ROOT_COMMAND_LIST_ID);
    let draws = state.node_physical_draws(child).collect::<Vec<_>>();
    assert_eq!(draws.len(), 3);
    let mut batches = Vec::new();
    for draw in draws {
        let batch = state.canvas.painter.batch_ids[draw];
        assert_ne!(
            batch,
            u32::MAX,
            "visible draw {draw} has been silently made inactive"
        );
        assert!(
            contains_draw(&plan.ops, draw, batch),
            "draw {draw} has the wrong batch {batch}"
        );
        batches.push(batch);
    }
    batches.sort_unstable();
    batches.dedup();
    assert_eq!(batches.len(), 3);
}
