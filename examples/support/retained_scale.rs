use std::sync::Arc;

use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use tileink::{
    Canvas, Filter, Radius, Region, RetainedLayerDescriptor, RetainedNodeId, RetainedParent,
    RetainedScene,
};

use super::retained_bench::{HEIGHT, WIDTH};

#[derive(Clone, Copy, Debug)]
pub enum Scenario {
    Static,
    OneRevision,
    VariableLength,
    AllRevisions,
    OneMove,
    AddRemove,
    LayerAddRemove,
    MiddleLayerAddRemove,
    NestedLayerAddRemove,
    Reparent,
    Reorder,
    LayerUpdate,
    ManyLayerUpdate,
    FilterChildRevision,
    CroppedFilterChildRevision,
    BackdropBackgroundRevision,
    ArenaFragmentation,
    ManualInvalidation,
}

impl Scenario {
    pub const ALL: [Self; 18] = [
        Self::Static,
        Self::OneRevision,
        Self::VariableLength,
        Self::AllRevisions,
        Self::OneMove,
        Self::AddRemove,
        Self::LayerAddRemove,
        Self::MiddleLayerAddRemove,
        Self::NestedLayerAddRemove,
        Self::Reparent,
        Self::Reorder,
        Self::LayerUpdate,
        Self::ManyLayerUpdate,
        Self::FilterChildRevision,
        Self::CroppedFilterChildRevision,
        Self::BackdropBackgroundRevision,
        Self::ArenaFragmentation,
        Self::ManualInvalidation,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::OneRevision => "one-revision",
            Self::VariableLength => "variable-length",
            Self::AllRevisions => "all-revisions",
            Self::OneMove => "one-move",
            Self::AddRemove => "add-remove",
            Self::LayerAddRemove => "layer-add-remove",
            Self::MiddleLayerAddRemove => "middle-layer-add-remove",
            Self::NestedLayerAddRemove => "nested-layer-add-remove",
            Self::Reparent => "reparent",
            Self::Reorder => "reorder",
            Self::LayerUpdate => "layer-update",
            Self::ManyLayerUpdate => "many-layer-update",
            Self::FilterChildRevision => "filter-child-revision",
            Self::CroppedFilterChildRevision => "cropped-filter-child-revision",
            Self::BackdropBackgroundRevision => "backdrop-background-revision",
            Self::ArenaFragmentation => "arena-fragmentation",
            Self::ManualInvalidation => "manual-invalidation",
        }
    }

    #[allow(dead_code)]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|scenario| scenario.name() == name)
    }
}

pub struct Workload {
    count: usize,
    scenario: Scenario,
    first: Arc<Canvas>,
    second: Arc<Canvas>,
    longer: Arc<Canvas>,
}

impl Workload {
    pub fn new(count: usize, scenario: Scenario) -> Self {
        Self {
            count,
            scenario,
            first: rect_scene(Color::from_rgb8(30, 130, 220)),
            second: rect_scene(Color::from_rgb8(230, 90, 40)),
            longer: two_rect_scene(),
        }
    }

    pub fn build_scene(&self) -> RetainedScene {
        let root = node_id(0);
        if matches!(self.scenario, Scenario::ManyLayerUpdate) {
            let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
            let mut transaction = scene.transaction();
            for index in 0..self.count {
                let layer = many_layer_id(index);
                let position = position(index, self.count);
                transaction
                    .insert_layer(
                        RetainedParent::content(root),
                        None,
                        layer,
                        opacity_layer_at(0.75, position),
                    )
                    .insert_scene(
                        RetainedParent::content(layer),
                        None,
                        many_layer_leaf_id(self.count, index),
                        self.first.clone(),
                        position,
                    );
            }
            transaction.commit().unwrap();
            return scene;
        }
        let group = node_id(self.count + 1);
        let layer = node_id(self.count + 3);
        let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
        let mut transaction = scene.transaction();
        transaction.insert_group(RetainedParent::content(root), None, group);
        if matches!(
            self.scenario,
            Scenario::LayerUpdate
                | Scenario::FilterChildRevision
                | Scenario::CroppedFilterChildRevision
        ) {
            transaction.insert_layer(
                RetainedParent::content(root),
                None,
                layer,
                match self.scenario {
                    Scenario::FilterChildRevision => filter_layer(),
                    Scenario::CroppedFilterChildRevision => cropped_filter_layer(),
                    _ => opacity_layer(0.75),
                },
            );
        }
        let parent = if matches!(
            self.scenario,
            Scenario::LayerUpdate
                | Scenario::FilterChildRevision
                | Scenario::CroppedFilterChildRevision
        ) {
            RetainedParent::content(layer)
        } else {
            RetainedParent::content(root)
        };
        for index in 0..self.count {
            transaction.insert_scene(
                parent,
                None,
                node_id(index + 1),
                self.first.clone(),
                position(index, self.count),
            );
        }
        if matches!(self.scenario, Scenario::NestedLayerAddRemove) {
            let outer_layer = node_id(self.count + 6);
            transaction
                .insert_layer(
                    RetainedParent::content(root),
                    None,
                    outer_layer,
                    isolate_layer_at((0.0, 0.0)),
                )
                .insert_scene(
                    RetainedParent::content(outer_layer),
                    None,
                    node_id(self.count + 7),
                    self.first.clone(),
                    (0.0, 0.0),
                );
        }
        if matches!(self.scenario, Scenario::BackdropBackgroundRevision) {
            transaction
                .insert_layer(RetainedParent::content(root), None, layer, backdrop_layer())
                .insert_scene(
                    RetainedParent::content(layer),
                    None,
                    node_id(self.count + 8),
                    self.second.clone(),
                    (16.0, 16.0),
                );
        }
        transaction.commit().unwrap();
        scene
    }

    pub fn mutate(&self, scene: &mut RetainedScene, frame: usize) {
        let even = frame.is_multiple_of(2);
        let root = node_id(0);
        let first_node = node_id(1);
        let second_node = node_id(2.min(self.count));
        let group = node_id(self.count + 1);
        let extra = node_id(self.count + 2);
        let layer = node_id(self.count + 3);
        let extra_layer = node_id(self.count + 4);
        let extra_layer_leaf = node_id(self.count + 5);
        let outer_layer = node_id(self.count + 6);
        let mut transaction = scene.transaction();
        match self.scenario {
            Scenario::Static => return,
            Scenario::OneRevision => {
                transaction.replace_scene(
                    first_node,
                    if even {
                        self.first.clone()
                    } else {
                        self.second.clone()
                    },
                );
            }
            Scenario::VariableLength => {
                transaction.replace_scene(
                    first_node,
                    if even {
                        self.first.clone()
                    } else {
                        self.longer.clone()
                    },
                );
            }
            Scenario::AllRevisions => {
                let child = if even { &self.first } else { &self.second };
                for index in 0..self.count {
                    transaction.replace_scene(node_id(index + 1), child.clone());
                }
            }
            Scenario::OneMove => {
                let mut position = position(0, self.count);
                if !even {
                    position.0 += 16.0;
                }
                transaction.set_position(first_node, position);
            }
            Scenario::AddRemove => {
                if even {
                    transaction.insert_scene(
                        RetainedParent::content(root),
                        None,
                        extra,
                        self.second.clone(),
                        (16.0, 16.0),
                    );
                } else {
                    transaction.remove_subtree(extra);
                }
            }
            Scenario::LayerAddRemove | Scenario::MiddleLayerAddRemove => {
                let before = matches!(self.scenario, Scenario::MiddleLayerAddRemove)
                    .then(|| node_id(self.count / 2 + 1));
                if even {
                    transaction
                        .insert_layer(
                            RetainedParent::content(root),
                            before,
                            extra_layer,
                            opacity_layer_at(0.5, (16.0, 16.0)),
                        )
                        .insert_scene(
                            RetainedParent::content(extra_layer),
                            None,
                            extra_layer_leaf,
                            self.second.clone(),
                            (16.0, 16.0),
                        );
                } else {
                    transaction.remove_subtree(extra_layer);
                }
            }
            Scenario::NestedLayerAddRemove => {
                if even {
                    transaction
                        .insert_layer(
                            RetainedParent::content(outer_layer),
                            None,
                            extra_layer,
                            opacity_layer_at(0.5, (16.0, 16.0)),
                        )
                        .insert_scene(
                            RetainedParent::content(extra_layer),
                            None,
                            extra_layer_leaf,
                            self.second.clone(),
                            (16.0, 16.0),
                        );
                } else {
                    transaction.remove_subtree(extra_layer);
                }
            }
            Scenario::Reparent => {
                transaction.reparent(
                    first_node,
                    if even {
                        RetainedParent::content(root)
                    } else {
                        RetainedParent::content(group)
                    },
                    None,
                );
            }
            Scenario::Reorder if self.count > 1 => {
                if even {
                    transaction.move_before(first_node, second_node);
                } else {
                    transaction.move_before(second_node, first_node);
                }
            }
            Scenario::Reorder => return,
            Scenario::LayerUpdate => {
                transaction.update_layer(layer, opacity_layer(if even { 0.75 } else { 0.25 }));
            }
            Scenario::ManyLayerUpdate => {
                transaction.update_layer(
                    many_layer_id(self.count / 2),
                    opacity_layer_at(
                        if even { 0.75 } else { 0.25 },
                        position(self.count / 2, self.count),
                    ),
                );
            }
            Scenario::FilterChildRevision
            | Scenario::CroppedFilterChildRevision
            | Scenario::BackdropBackgroundRevision => {
                transaction.replace_scene(
                    first_node,
                    if even {
                        self.first.clone()
                    } else {
                        self.second.clone()
                    },
                );
            }
            Scenario::ArenaFragmentation => {
                for index in (0..self.count).step_by(2) {
                    let id = node_id(index + 1);
                    if even {
                        transaction.remove_subtree(id);
                    } else {
                        transaction.insert_scene(
                            RetainedParent::content(root),
                            (index + 1 < self.count).then(|| node_id(index + 2)),
                            id,
                            self.first.clone(),
                            position(index, self.count),
                        );
                    }
                }
            }
            Scenario::ManualInvalidation => {
                transaction.invalidate_rect(Rect::new(0.0, 0.0, 8.0, 8.0));
            }
        }
        transaction.commit().unwrap();
    }
}

fn opacity_layer(opacity: f32) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Opacity {
        path: Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity,
    }
}

fn opacity_layer_at(opacity: f32, position: (f64, f64)) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Opacity {
        path: Rect::new(position.0, position.1, position.0 + 8.0, position.1 + 8.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
        opacity,
    }
}

fn isolate_layer_at(position: (f64, f64)) -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Isolate {
        path: Rect::new(position.0, position.1, position.0 + 48.0, position.1 + 48.0).to_path(0.1),
        transform: Affine::IDENTITY,
        tolerance: 0.1,
    }
}

fn filter_layer() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(0.75),
        sample_region: Region::rect(
            Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
            Radius::ZERO,
        ),
    }
}

fn cropped_filter_layer() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Filter {
        filter: Filter::Opacity(0.75),
        // The non-zero origin forces the general local-surface path. Keeping the region fixed
        // while node count scales exposes accidental full-scene translation or upload work.
        sample_region: Region::rect(Rect::new(1.0, 1.0, 257.0, 257.0), Radius::ZERO),
    }
}

fn backdrop_layer() -> RetainedLayerDescriptor {
    RetainedLayerDescriptor::Backdrop {
        filter: Filter::Opacity(0.75),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
    }
}

fn node_id(index: usize) -> RetainedNodeId {
    RetainedNodeId::for_owner(index as u64 + 1)
}

fn many_layer_id(index: usize) -> RetainedNodeId {
    node_id(index + 1)
}

fn many_layer_leaf_id(count: usize, index: usize) -> RetainedNodeId {
    node_id(count + index + 1)
}

fn rect_scene(color: Color) -> Arc<Canvas> {
    let mut scene = Canvas::new(8, 8, 1.0);
    scene.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, color);
    Arc::new(scene)
}

fn two_rect_scene() -> Arc<Canvas> {
    let mut scene = Canvas::new(8, 8, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, 4.0, 8.0),
        Radius::ZERO,
        Color::from_rgb8(40, 210, 90),
    );
    scene.push_rect(
        Rect::new(4.0, 0.0, 8.0, 8.0),
        Radius::ZERO,
        Color::from_rgb8(240, 210, 40),
    );
    Arc::new(scene)
}

fn position(index: usize, count: usize) -> (f64, f64) {
    let columns = (count as f64).sqrt().ceil().max(1.0) as usize;
    (
        (index % columns) as f64 * 10.0,
        (index / columns) as f64 * 10.0,
    )
}
