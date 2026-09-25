use super::*;
use crate::render::incremental::{IncrementalRenderConfig, IncrementalState};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};

struct Fixture {
    scene: RetainedScene,
    materializer: PersistentSceneMaterializer,
    source_filter: RetainedNodeId,
    source: RetainedNodeId,
    backdrop: RetainedNodeId,
    outer: RetainedNodeId,
    root_backdrop: Option<RetainedNodeId>,
}

fn descriptor(amount: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: filter::Filter::Invert(amount),
        sample_region: Region::rect(Rect::new(-16.0, 16.0, 0.0, 32.0), crate::Radius::ZERO),
    }
}
fn child(rect: Rect, color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(rect, crate::Radius::ZERO, color);
    Rc::new(canvas)
}
fn fixture(cascade: bool) -> Fixture {
    let root = RetainedNodeId::for_owner(933_000);
    let outer = RetainedNodeId::for_owner(933_001);
    let source_filter = RetainedNodeId::for_owner(933_002);
    let source = RetainedNodeId::for_owner(933_003);
    let backdrop = RetainedNodeId::for_owner(933_004);
    let region = || Region::rect(Rect::new(-16.0, 16.0, 0.0, 32.0), crate::Radius::ZERO);
    let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_layer(
            RetainedParent::content(root),
            None,
            outer,
            RetainedLayerDescriptor::Filter {
                filter: filter::Filter::Offset { dx: 16.0, dy: 0.0 },
                sample_region: region(),
            },
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            source_filter,
            descriptor(0.0),
        )
        .insert_scene(
            RetainedParent::content(source_filter),
            None,
            source,
            child(Rect::new(-16.0, 16.0, 0.0, 32.0), Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_layer(
            RetainedParent::content(outer),
            None,
            backdrop,
            RetainedLayerDescriptor::Backdrop {
                filter: filter::Filter::Invert(1.0),
                sample_region: region(),
            },
        )
        .commit()
        .unwrap();
    let root_backdrop = cascade.then(|| {
        let id = RetainedNodeId::for_owner(933_005);
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(root),
                None,
                id,
                RetainedLayerDescriptor::Backdrop {
                    filter: filter::Filter::Offset { dx: 16.0, dy: 0.0 },
                    sample_region: Region::rect(
                        Rect::new(0.0, 16.0, 16.0, 32.0),
                        crate::Radius::ZERO,
                    ),
                },
            )
            .commit()
            .unwrap();
        id
    });
    let materializer = PersistentSceneMaterializer::new(&scene);
    Fixture {
        scene,
        materializer,
        source_filter,
        source,
        backdrop,
        outer,
        root_backdrop,
    }
}
impl Fixture {
    fn frame(&self) -> crate::canvas::RetainedFrame {
        self.materializer.canvas.persistent_frame.clone().unwrap()
    }
    fn update(&mut self) {
        let changes = self.scene.changes_since(self.materializer.version());
        assert!(self.materializer.update(&self.scene, changes));
    }
    fn change_filter(&mut self, amount: f32) {
        self.scene
            .transaction()
            .update_layer(self.source_filter, descriptor(amount))
            .commit()
            .unwrap();
        self.update();
    }
    fn assert_partial(&self, state: &mut IncrementalState) {
        let plan = state.plan(
            Some(self.frame()),
            (64, 64),
            IncrementalRenderConfig::default(),
            true,
        );
        assert!(
            !plan.stats.full_redraw,
            "{:?}",
            plan.stats.full_redraw_reason
        );
        assert!(
            plan.dirty_backdrops
                .as_ref()
                .unwrap()
                .contains(&self.backdrop)
        );
        assert_eq!(
            plan.changed_tiles.list(),
            if self.root_backdrop.is_some() {
                &[0, 4, 8, 1, 5, 9][..]
            } else {
                &[0, 4, 8][..]
            }
        );
        if let Some(id) = self.root_backdrop {
            assert!(plan.dirty_backdrops.as_ref().unwrap().contains(&id));
        }
    }
}

#[test]
fn skipped_same_node_updates_survive_state_delta_pruning() {
    let mut f = fixture(false);
    let mut state = IncrementalState::default();
    state.commit(Some(f.frame()));
    f.change_filter(1.0);
    f.change_filter(0.5);
    assert!(f.frame().delta.as_ref().unwrap().previous.is_none());
    f.assert_partial(&mut state);
    // Preparing again without successful commit must still deliver the same event.
    f.assert_partial(&mut state);
    state.commit(Some(f.frame()));
    let plan = state.plan(
        Some(f.frame()),
        (64, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert!(plan.changed_tiles.is_empty());
}

#[test]
fn leaf_bounds_updates_keep_partial_damage_when_frame_metadata_rebuilds() {
    let mut f = fixture(false);
    let mut state = IncrementalState::default();
    state.commit(Some(f.frame()));
    for rect in [
        Rect::new(-16.0, 16.0, -4.0, 32.0),
        Rect::new(-12.0, 16.0, 0.0, 32.0),
    ] {
        f.scene
            .transaction()
            .replace_scene(f.source, child(rect, Color::BLACK))
            .commit()
            .unwrap();
        f.update();
        assert!(f.frame().delta.is_none());
        f.assert_partial(&mut state);
    }
}

#[test]
fn nested_damage_reaches_later_root_backdrops_in_the_same_event() {
    let mut f = fixture(true);
    let mut state = IncrementalState::default();
    state.commit(Some(f.frame()));
    f.change_filter(1.0);
    f.assert_partial(&mut state);
}

#[test]
fn state_page_compaction_and_event_epochs_preserve_each_rendered_update() {
    let mut f = fixture(false);
    let ids = (0..260)
        .map(|n| RetainedNodeId::for_owner(934_000 + n))
        .collect::<Vec<_>>();
    for &id in &ids {
        f.scene
            .transaction()
            .insert_layer(
                RetainedParent::content(f.outer),
                Some(f.backdrop),
                id,
                descriptor(0.0),
            )
            .commit()
            .unwrap();
    }
    f.materializer = PersistentSceneMaterializer::new(&f.scene);
    let initial = f.frame();
    let mut state = IncrementalState::default();
    state.commit(Some(initial.clone()));
    for id in ids {
        f.scene
            .transaction()
            .update_layer(id, descriptor(1.0))
            .commit()
            .unwrap();
        f.update();
        f.assert_partial(&mut state);
        state.commit(Some(f.frame()));
    }
    assert!(!f.frame().state_pages.is_empty());
    let mut skipped = IncrementalState::default();
    skipped.commit(Some(initial));
    let plan = skipped.plan(
        Some(f.frame()),
        (64, 64),
        IncrementalRenderConfig::default(),
        true,
    );
    assert_eq!(
        plan.stats.full_redraw_reason,
        Some(crate::FullRedrawReason::DamageHistoryUnavailable)
    );
}

#[test]
fn scoped_leaf_removal_and_reinsertion_preserve_complete_damage_history() {
    for cascade in [false, true] {
        let mut f = fixture(cascade);
        let mut state = IncrementalState::default();
        state.commit(Some(f.frame()));
        f.scene
            .transaction()
            .remove_subtree(f.source)
            .commit()
            .unwrap();
        f.update();
        f.assert_partial(&mut state);
        // Keep the renderer on its old version: both structural events must
        // remain available independently of compacted node-state overlays.
        f.scene
            .transaction()
            .insert_scene(
                RetainedParent::content(f.source_filter),
                None,
                f.source,
                child(Rect::new(-16.0, 16.0, 0.0, 32.0), Color::BLACK),
                Affine::IDENTITY,
            )
            .commit()
            .unwrap();
        f.update();
        f.assert_partial(&mut state);
    }
}

#[test]
fn scoped_subtree_removal_keeps_old_dependency_domain_damage() {
    let mut f = fixture(true);
    let mut state = IncrementalState::default();
    state.commit(Some(f.frame()));
    // The source command and its containing filter both disappear. Resolve
    // their old local input before deleting the old command tree or chunks.
    f.scene
        .transaction()
        .remove_subtree(f.source_filter)
        .commit()
        .unwrap();
    f.update();
    f.assert_partial(&mut state);
}
