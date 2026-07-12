use std::sync::Arc;

use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use tileink::{
    Canvas, FillRule, Radius, Region, RetainedLayerDescriptor, RetainedNodeId, RetainedParent,
    RetainedScene,
};

use super::retained_bench::{HEIGHT, WIDTH};

#[derive(Clone, Copy, Debug)]
pub enum StressScenario {
    DeepHierarchyRevision,
    DeepHierarchyJournalGap,
    ManyBackdropsRevision,
    ManyRootLayersAddRemove,
    LargeChunkRevision,
    DeltaRotation,
}

impl StressScenario {
    pub const ALL: [Self; 6] = [
        Self::DeepHierarchyRevision,
        Self::DeepHierarchyJournalGap,
        Self::ManyBackdropsRevision,
        Self::ManyRootLayersAddRemove,
        Self::LargeChunkRevision,
        Self::DeltaRotation,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::DeepHierarchyRevision => "deep-hierarchy-revision",
            Self::DeepHierarchyJournalGap => "deep-hierarchy-journal-gap",
            Self::ManyBackdropsRevision => "many-backdrops-revision",
            Self::ManyRootLayersAddRemove => "many-root-layers-add-remove",
            Self::LargeChunkRevision => "large-chunk-revision",
            Self::DeltaRotation => "delta-rotation",
        }
    }

    pub const fn counts(self) -> &'static [usize] {
        match self {
            Self::DeepHierarchyRevision | Self::DeepHierarchyJournalGap => &[8, 32, 128, 256, 512],
            Self::ManyBackdropsRevision => &[1, 4, 16, 64, 256],
            Self::ManyRootLayersAddRemove => &[8, 32, 128, 512, 2_048],
            Self::LargeChunkRevision => &[100, 1_000, 5_000, 20_000, 100_000],
            Self::DeltaRotation => &[256, 1_000, 5_000, 20_000, 100_000],
        }
    }
}

pub struct StressWorkload {
    count: usize,
    scenario: StressScenario,
    first: Arc<Canvas>,
    second: Arc<Canvas>,
}

impl StressWorkload {
    pub fn new(count: usize, scenario: StressScenario) -> Self {
        let (first, second) = if matches!(scenario, StressScenario::LargeChunkRevision) {
            (
                large_scene(count, Color::from_rgb8(30, 130, 220)),
                large_scene(count, Color::from_rgb8(230, 90, 40)),
            )
        } else {
            (
                rect_scene(Color::from_rgb8(30, 130, 220)),
                rect_scene(Color::from_rgb8(230, 90, 40)),
            )
        };
        Self {
            count,
            scenario,
            first,
            second,
        }
    }

    pub fn build_scene(&self) -> RetainedScene {
        match self.scenario {
            StressScenario::DeepHierarchyRevision | StressScenario::DeepHierarchyJournalGap => {
                self.build_deep_hierarchy()
            }
            StressScenario::ManyBackdropsRevision => self.build_many_backdrops(),
            StressScenario::ManyRootLayersAddRemove => self.build_many_root_layers(),
            StressScenario::LargeChunkRevision => self.build_large_chunk(),
            StressScenario::DeltaRotation => self.build_delta_rotation(),
        }
    }

    pub fn mutate(&self, scene: &mut RetainedScene, frame: usize) {
        let canvas = if frame.is_multiple_of(2) {
            self.first.clone()
        } else {
            self.second.clone()
        };
        match self.scenario {
            StressScenario::DeepHierarchyRevision
            | StressScenario::ManyBackdropsRevision
            | StressScenario::LargeChunkRevision => {
                scene
                    .transaction()
                    .replace_scene(leaf_id(), canvas)
                    .commit()
                    .unwrap();
            }
            StressScenario::DeepHierarchyJournalGap => {
                for generation in 0..=256 {
                    let canvas = if (frame + generation).is_multiple_of(2) {
                        self.first.clone()
                    } else {
                        self.second.clone()
                    };
                    scene
                        .transaction()
                        .replace_scene(leaf_id(), canvas)
                        .commit()
                        .unwrap();
                }
            }
            StressScenario::ManyRootLayersAddRemove => {
                let extra = extra_layer_id();
                if frame.is_multiple_of(2) {
                    scene
                        .transaction()
                        .insert_layer(
                            RetainedParent::content(root_id()),
                            Some(layer_id(self.count / 2)),
                            extra,
                            clip_layer(),
                        )
                        .insert_scene(
                            RetainedParent::content(extra),
                            None,
                            extra_leaf_id(),
                            canvas,
                            (0.0, 0.0),
                        )
                        .commit()
                        .unwrap();
                } else {
                    scene.transaction().remove_subtree(extra).commit().unwrap();
                }
            }
            StressScenario::DeltaRotation => {
                let id = flat_leaf_id(frame % self.count);
                scene
                    .transaction()
                    .replace_scene(id, canvas)
                    .commit()
                    .unwrap();
            }
        }
    }

    fn build_deep_hierarchy(&self) -> RetainedScene {
        let root = root_id();
        // Keep affected pixels fixed so this series measures hierarchy depth rather than the
        // intentionally depth-proportional fine-stack spill surface.
        let mut scene = RetainedScene::new(64, 64, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        let mut parent = RetainedParent::content(root);
        for depth in 0..self.count {
            let layer = layer_id(depth);
            transaction.insert_layer(parent, None, layer, clip_layer());
            parent = RetainedParent::content(layer);
        }
        transaction
            .insert_scene(parent, None, leaf_id(), self.first.clone(), (0.0, 0.0))
            .commit()
            .unwrap();
        scene
    }

    fn build_many_backdrops(&self) -> RetainedScene {
        let root = root_id();
        let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        transaction.insert_scene(
            RetainedParent::content(root),
            None,
            leaf_id(),
            self.first.clone(),
            (0.0, 0.0),
        );
        for index in 0..self.count {
            let layer = layer_id(index);
            transaction
                .insert_layer(RetainedParent::content(root), None, layer, backdrop_layer())
                .insert_scene(
                    RetainedParent::content(layer),
                    None,
                    layer_leaf_id(index),
                    self.first.clone(),
                    (0.0, 0.0),
                );
        }
        transaction.commit().unwrap();
        scene
    }

    fn build_many_root_layers(&self) -> RetainedScene {
        let root = root_id();
        let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        for index in 0..self.count {
            let layer = layer_id(index);
            transaction
                .insert_layer(RetainedParent::content(root), None, layer, clip_layer())
                .insert_scene(
                    RetainedParent::content(layer),
                    None,
                    layer_leaf_id(index),
                    self.first.clone(),
                    (0.0, 0.0),
                );
        }
        transaction.commit().unwrap();
        scene
    }

    fn build_large_chunk(&self) -> RetainedScene {
        let root = root_id();
        let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                leaf_id(),
                self.first.clone(),
                (0.0, 0.0),
            )
            .commit()
            .unwrap();
        scene
    }

    fn build_delta_rotation(&self) -> RetainedScene {
        let root = root_id();
        let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        for index in 0..self.count {
            transaction.insert_scene(
                RetainedParent::content(root),
                None,
                flat_leaf_id(index),
                self.first.clone(),
                position(index, self.count),
            );
        }
        transaction.commit().unwrap();
        scene
    }
}

fn clip_layer() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::ClipPath {
        path: Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64).to_path(0.1),
        transform: Affine::IDENTITY,
        rule: FillRule::NonZero,
        tolerance: 0.1,
    }
}

fn backdrop_layer() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Backdrop {
        filter: tileink::Filter::Opacity(0.75),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
    }
}

fn rect_scene(color: Color) -> Arc<Canvas> {
    let mut canvas = Canvas::new(8, 8, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, color);
    Arc::new(canvas)
}

fn large_scene(count: usize, color: Color) -> Arc<Canvas> {
    let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
    let columns = (count as f64).sqrt().ceil().max(1.0) as usize;
    for index in 0..count {
        let x = (index % columns) as f64 * 2.0;
        let y = (index / columns) as f64 * 2.0;
        canvas.push_rect(Rect::new(x, y, x + 1.0, y + 1.0), Radius::ZERO, color);
    }
    Arc::new(canvas)
}

fn position(index: usize, count: usize) -> (f64, f64) {
    let columns = (count as f64).sqrt().ceil().max(1.0) as usize;
    (
        (index % columns) as f64 * 10.0,
        (index / columns) as f64 * 10.0,
    )
}

fn root_id() -> RetainedNodeId {
    RetainedNodeId::for_owner(1)
}

fn leaf_id() -> RetainedNodeId {
    RetainedNodeId::for_owner(2)
}

fn layer_id(index: usize) -> RetainedNodeId {
    RetainedNodeId::for_owner(10_000 + index as u64)
}

fn layer_leaf_id(index: usize) -> RetainedNodeId {
    RetainedNodeId::for_owner(1_000_000 + index as u64)
}

fn flat_leaf_id(index: usize) -> RetainedNodeId {
    RetainedNodeId::for_owner(10_000_000 + index as u64)
}

fn extra_layer_id() -> RetainedNodeId {
    RetainedNodeId::for_owner(u64::MAX - 1)
}

fn extra_leaf_id() -> RetainedNodeId {
    RetainedNodeId::for_owner(u64::MAX)
}
