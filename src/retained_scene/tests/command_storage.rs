//! Command-list storage belongs to live retained subtrees, not their edit history.
use super::*;

fn owned_lists(canvas: &Canvas) -> (HashSet<usize>, Vec<RetainedNodeId>) {
    fn visit(
        canvas: &Canvas,
        list: usize,
        owned: &mut HashSet<usize>,
        nodes: &mut Vec<RetainedNodeId>,
    ) {
        assert!(owned.insert(list), "two commands own list {list}");
        for command in &canvas.command_lists[list].commands {
            match command {
                Command::MaterializedRetainedScene { id, fragment, .. } => {
                    nodes.push(*id);
                    for list in fragment.clone() {
                        assert!(owned.insert(list), "two scene fragments own list {list}");
                    }
                }
                Command::Layer {
                    retained: Some(key),
                    children,
                    ..
                } => {
                    nodes.push(key.id);
                    visit(canvas, *children, owned, nodes);
                }
                Command::MaskLayer {
                    retained: Some(key),
                    content,
                    mask,
                    ..
                } => {
                    nodes.push(key.id);
                    visit(canvas, *content, owned, nodes);
                    visit(canvas, *mask, owned, nodes);
                }
                _ => panic!("retained containers own retained commands"),
            }
        }
    }
    let mut owned = HashSet::default();
    let mut nodes = Vec::new();
    visit(canvas, 0, &mut owned, &mut nodes);
    for (index, list) in canvas.command_lists.iter().enumerate() {
        assert!(
            owned.contains(&index) || list.commands.is_empty(),
            "unowned list {index} still has commands"
        );
    }
    (owned, nodes)
}

fn assert_ownership(state: &PersistentSceneMaterializer, scene: &RetainedScene) {
    let (owned, nodes) = owned_lists(&state.canvas);
    let fresh = PersistentSceneMaterializer::new(scene);
    let (expected, order) = owned_lists(&fresh.canvas);
    assert_eq!(owned.len(), expected.len());
    assert_eq!(nodes, order);
}

fn uncached_plan(canvas: &Canvas) -> String {
    let mut copy = canvas.clone();
    copy.compiled_plan = None;
    format!("{:?}", copy.compile(0))
}

fn id(value: u64) -> RetainedNodeId {
    RetainedNodeId::for_owner(200_000 + value)
}

fn filter() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(0.5),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    }
}

fn scene() -> RetainedScene {
    let mut scene = RetainedScene::new(32, 32, 1.0, id(0)).unwrap();
    let mut tx = scene.transaction();
    for index in 10..18 {
        tx.insert_scene(
            RetainedParent::content(id(0)),
            None,
            id(index),
            leaf(Color::BLACK),
            Affine::IDENTITY,
        );
    }
    tx.insert_layer(RetainedParent::content(id(0)), None, id(1), filter())
        .insert_scene(
            RetainedParent::content(id(1)),
            None,
            id(2),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    scene
}

#[test]
fn repeated_nested_subtree_edits_reclaim_command_lists() {
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let initial = state.canvas.command_lists.len();
    let old = std::rc::Rc::clone(&state.canvas);
    let old_plan = uncached_plan(&old);
    let crate::shared::execution::ExecOp::DrawBatch { draws: outside, .. } =
        &old.compiled_plan.as_ref().unwrap().ops[0]
    else {
        panic!("root plain run")
    };
    let mut peak = 0;
    for iteration in 0..24 {
        scene
            .transaction()
            .insert_layer(RetainedParent::content(id(1)), None, id(3), filter())
            .insert_scene(
                RetainedParent::content(id(3)),
                None,
                id(4),
                leaf(Color::WHITE),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        let crate::shared::execution::ExecOp::DrawBatch { draws, .. } =
            &state.canvas.compiled_plan.as_ref().unwrap().ops[0]
        else {
            panic!("root plain run")
        };
        assert!(
            Rc::ptr_eq(outside, draws),
            "local edits must preserve unrelated batches"
        );
        if iteration == 0 {
            peak = state.canvas.command_lists.len();
        }
        assert!(
            state.canvas.command_lists.len() <= peak,
            "nested insertion {iteration} leaked command lists"
        );
        scene.transaction().remove_subtree(id(3)).commit().unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        assert!(
            state.canvas.command_lists.len() <= peak.max(initial),
            "nested removal {iteration} leaked command lists"
        );
    }
    assert_eq!(old.command_lists.len(), initial);
    assert_eq!(uncached_plan(&old), old_plan);
}

#[test]
fn repeated_non_tail_root_removal_reclaims_command_lists() {
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let mut peak = 0;
    for iteration in 0..24 {
        scene
            .transaction()
            .insert_layer(RetainedParent::content(id(0)), Some(id(1)), id(3), filter())
            .insert_scene(
                RetainedParent::content(id(3)),
                None,
                id(4),
                leaf(Color::WHITE),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        if iteration == 0 {
            peak = state.canvas.command_lists.len();
        }
        scene.transaction().remove_subtree(id(3)).commit().unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        assert!(
            state.canvas.command_lists.len() <= peak,
            "root removal {iteration} leaked command lists"
        );
    }
}

#[test]
fn repeated_mask_kind_changes_reclaim_the_retired_branch() {
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let mut peak = 0;
    for iteration in 0..24 {
        scene
            .transaction()
            .update_layer(
                id(1),
                RetainedLayerDescriptor::Mask(crate::Mask {
                    region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
                    kind: crate::MaskKind::Alpha,
                }),
            )
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        if iteration == 0 {
            peak = state.canvas.command_lists.len();
        }
        scene
            .transaction()
            .update_layer(id(1), filter())
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        assert!(
            state.canvas.command_lists.len() <= peak,
            "mask transition {iteration} leaked its retired branch"
        );
    }
}

#[test]
fn repeated_reparenting_reclaims_both_previous_offscreen_trees() {
    let mut scene = scene();
    scene
        .transaction()
        .insert_layer(RetainedParent::content(id(0)), None, id(5), filter())
        .insert_layer(RetainedParent::content(id(1)), None, id(3), filter())
        .insert_scene(
            RetainedParent::content(id(3)),
            None,
            id(4),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let mut peak = 0;
    for iteration in 0..24 {
        let parent = if iteration % 2 == 0 { id(5) } else { id(1) };
        scene
            .transaction()
            .reparent(id(3), RetainedParent::content(parent), None)
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        if iteration == 0 {
            peak = state.canvas.command_lists.len();
        }
        assert!(
            state.canvas.command_lists.len() <= peak,
            "reparent {iteration} leaked old command trees"
        );
    }
}

#[test]
fn swapping_subtrees_recycles_old_ownership_after_new_locations_are_published() {
    let mut scene = scene();
    let mut tx = scene.transaction();
    tx.insert_layer(RetainedParent::content(id(0)), None, id(5), filter());
    for (layer, child, parent) in [(3, 4, 1), (6, 7, 5)] {
        tx.insert_layer(
            RetainedParent::content(id(parent)),
            None,
            id(layer),
            filter(),
        )
        .insert_scene(
            RetainedParent::content(id(layer)),
            None,
            id(child),
            leaf(Color::WHITE),
            Affine::IDENTITY,
        );
    }
    tx.commit().unwrap();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let mut peak = 0;
    for iteration in 0..24 {
        let (a, b) = if iteration % 2 == 0 { (5, 1) } else { (1, 5) };
        scene
            .transaction()
            .reparent(id(3), RetainedParent::content(id(a)), None)
            .reparent(id(6), RetainedParent::content(id(b)), None)
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        if iteration < 2 {
            peak = peak.max(state.canvas.command_lists.len());
        }
        assert!(
            state.canvas.command_lists.len() <= peak,
            "swapped ownership must remain bounded"
        );
    }
}

#[test]
fn scene_fragments_release_all_internal_clip_filter_and_mask_lists() {
    let mut source = Canvas::new(32, 32, 1.0);
    let mut mask = Canvas::new(32, 32, 1.0);
    mask.push_rect(Rect::new(0.0, 0.0, 16.0, 32.0), Radius::ZERO, Color::WHITE);
    source.push_mask_layer(
        mask,
        crate::Mask {
            region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
            kind: crate::MaskKind::Alpha,
        },
    );
    source.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, 24.0, 32.0), Radius::ZERO);
    source.push_filter_layer(
        Filter::Opacity(0.5),
        Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    );
    source.push_rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO, Color::WHITE);
    for _ in 0..3 {
        source.pop_layer();
    }
    let source = Rc::new(source);
    let mut scene = scene();
    let mut state = PersistentSceneMaterializer::new(&scene);
    let mut peak = 0;
    for iteration in 0..24 {
        scene
            .transaction()
            .insert_layer(RetainedParent::content(id(1)), None, id(3), filter())
            .insert_scene(
                RetainedParent::content(id(3)),
                None,
                id(4),
                std::rc::Rc::clone(&source),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        if iteration == 0 {
            peak = state.canvas.command_lists.len();
        }
        assert!(state.canvas.command_lists.len() <= peak);
        scene.transaction().remove_subtree(id(3)).commit().unwrap();
        assert!(update_materializer(&mut state, &scene));
        assert_ownership(&state, &scene);
        assert!(state.canvas.command_lists.len() <= peak);
    }
}
