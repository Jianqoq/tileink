//! Command neighbours flatten groups while retaining layer and mask boundaries.
use super::*;
use std::ops::Bound::{Excluded, Unbounded};

#[derive(Clone, Copy)]
pub(super) enum Direction {
    Previous,
    Next,
}

pub(super) fn command_container(scene: &RetainedScene, mut id: RetainedNodeId) -> RetainedParent {
    loop {
        let parent = scene.nodes[&id].parent.expect("non-root node has parent");
        if parent.node == scene.root || matches!(scene.nodes[&parent.node].kind, NodeKind::Layer(_))
        {
            return parent;
        }
        id = parent.node;
    }
}

fn edge_command_node(
    scene: &RetainedScene,
    id: RetainedNodeId,
    direction: Direction,
) -> Option<RetainedNodeId> {
    if !matches!(scene.nodes[&id].kind, NodeKind::Group) {
        return Some(id);
    }
    let children = &scene.nodes[&id].content.order;
    match direction {
        Direction::Next => children
            .values()
            .find_map(|&child| edge_command_node(scene, child, direction)),
        Direction::Previous => children
            .values()
            .rev()
            .find_map(|&child| edge_command_node(scene, child, direction)),
    }
}

pub(super) fn command_neighbor(
    scene: &RetainedScene,
    mut id: RetainedNodeId,
    container: RetainedParent,
    direction: Direction,
) -> Option<RetainedNodeId> {
    loop {
        let parent = scene.nodes[&id].parent.expect("non-root node has parent");
        let siblings = scene.nodes[&parent.node]
            .children(parent)
            .expect("validated retained branch");
        let key = siblings
            .key_of(id)
            .expect("retained child has an order key");
        let neighbor = match direction {
            Direction::Next => siblings
                .order
                .range((Excluded(key), Unbounded))
                .find_map(|(_, &child)| edge_command_node(scene, child, direction)),
            Direction::Previous => siblings
                .order
                .range((Unbounded, Excluded(key)))
                .rev()
                .find_map(|(_, &child)| edge_command_node(scene, child, direction)),
        };
        if neighbor.is_some() {
            return neighbor;
        }
        if parent == container {
            return None;
        }
        id = parent.node;
    }
}
