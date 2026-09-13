use super::*;
use crate::render::incremental::{IncrementalRenderConfig, IncrementalState};
use crate::{Filter, Mask, MaskKind, Radius, Region};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

fn region() -> Region {
    Region::rect(Rect::new(38.0, 38.0, 46.0, 46.0), Radius::ZERO)
}

fn backdrop() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Backdrop {
        filter: Filter::Invert(1.0),
        sample_region: region(),
    }
}

fn leaf() -> Rc<Canvas> {
    let mut canvas = Canvas::new(128, 128, 1.0);
    canvas.push_rect(
        Rect::new(40.0, 40.0, 44.0, 44.0),
        Radius::ZERO,
        Color::WHITE,
    );
    Rc::new(canvas)
}

fn assert_event(
    scene: &RetainedScene,
    materializer: &mut PersistentSceneMaterializer,
    state: &mut IncrementalState,
    tile: u32,
    dirty_backdrops: &[RetainedNodeId],
) {
    let changes = scene.changes_since(materializer.version());
    assert!(materializer.update(scene, changes));
    let frame = materializer.canvas.persistent_frame.clone();
    let plan = state.plan(
        frame.clone(),
        (128, 128),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(
        !plan.stats.full_redraw,
        "{:?}",
        plan.stats.full_redraw_reason
    );
    assert_eq!(plan.changed_tiles.list(), &[tile]);
    let actual = plan
        .dirty_backdrops
        .as_ref()
        .unwrap()
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    assert_eq!(actual, dirty_backdrops.iter().copied().collect());
    state.commit(frame);
}

#[test]
fn moving_a_group_across_a_backdrop_uses_both_painter_orders() {
    let [root, outer, group, source, backdrop_id] =
        std::array::from_fn(|i| RetainedNodeId::for_owner(938_000 + i as u64));
    // A pointwise outer filter isolates ordering from Offset's existing
    // conservative radius expansion; only the local source tile should change.
    let parent = RetainedParent::content(outer);
    let mut scene = RetainedScene::new(128, 128, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: Filter::Opacity(1.0),
                sample_region: Region::rect(Rect::new(0.0, 0.0, 96.0, 96.0), Radius::ZERO),
            },
        )
        .insert_group(parent, None, group)
        .insert_scene(
            RetainedParent::content(group),
            None,
            source,
            leaf(),
            Affine::IDENTITY,
        )
        .insert_layer(parent, None, backdrop_id, backdrop())
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut state = IncrementalState::default();
    state.commit(materializer.canvas.persistent_frame.clone());
    // The first move changes only OLD input; the second changes only NEW input.
    // A group has no own chunk. Its descendants must participate in both trees.
    scene
        .transaction()
        .reparent(group, parent, None)
        .commit()
        .unwrap();
    assert_event(&scene, &mut materializer, &mut state, 18, &[backdrop_id]);
    scene
        .transaction()
        .move_before(group, backdrop_id)
        .commit()
        .unwrap();
    assert_event(&scene, &mut materializer, &mut state, 18, &[backdrop_id]);
}

#[test]
fn reparenting_between_mask_branches_dirties_only_old_and_new_input_domains() {
    let [
        root,
        mask,
        group,
        source,
        content_backdrop,
        mask_backdrop,
        clean_backdrop,
    ] = std::array::from_fn(|i| RetainedNodeId::for_owner(939_000 + i as u64));
    let content = RetainedParent::content(mask);
    let mask_input = RetainedParent::mask(mask);
    let mut scene = RetainedScene::new(128, 128, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            mask,
            RetainedLayerDescriptor::Mask(Mask {
                kind: MaskKind::Alpha,
                region: Region::rect(Rect::new(0.0, 0.0, 128.0, 128.0), Radius::ZERO),
            }),
        )
        .insert_group(content, None, group)
        .insert_scene(
            RetainedParent::content(group),
            None,
            source,
            leaf(),
            Affine::IDENTITY,
        )
        .insert_layer(content, None, content_backdrop, backdrop())
        .insert_layer(mask_input, None, mask_backdrop, backdrop())
        .insert_layer(
            mask_input,
            None,
            clean_backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: Filter::Invert(1.0),
                sample_region: Region::rect(Rect::new(80.0, 80.0, 88.0, 88.0), Radius::ZERO),
            },
        )
        .commit()
        .unwrap();
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut state = IncrementalState::default();
    state.commit(materializer.canvas.persistent_frame.clone());
    for (parent, before) in [(mask_input, mask_backdrop), (content, content_backdrop)] {
        scene
            .transaction()
            .reparent(group, parent, Some(before))
            .commit()
            .unwrap();
        assert_event(
            &scene,
            &mut materializer,
            &mut state,
            18,
            &[content_backdrop, mask_backdrop],
        );
    }
}
