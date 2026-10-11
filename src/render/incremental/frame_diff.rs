//! Snapshot comparison resolves overlays before matching stable node identities.
use super::{Bounds, DamageTiles, RetainedDamage, RetainedFrame, TILE_SIZE};

#[derive(Clone, Copy)]
struct CommonNode {
    new_index: usize,
    influence: Bounds,
}

/// Reusable CPU storage for retained-frame comparison.
///
/// Frame indexes already live in `RetainedFrame`; this scratch only stores the common-node order
/// and a flat tile adjacency while checking painter-order inversions. Keeping it on the renderer
/// avoids rebuilding hash collections and thousands of per-tile vectors every frame.
#[derive(Default)]
pub(super) struct FrameDiffScratch {
    common: Vec<CommonNode>,
    tile_offsets: Vec<usize>,
    tile_cursors: Vec<usize>,
    tile_nodes: Vec<usize>,
    previous_overrides:
        rustc_hash::FxHashMap<crate::RetainedNodeId, Option<crate::canvas::RetainedNodeState>>,
    current_overrides:
        rustc_hash::FxHashMap<crate::RetainedNodeId, Option<crate::canvas::RetainedNodeState>>,
    previous_affected: Vec<u8>,
    current_affected: Vec<u8>,
}

pub(super) fn diff_frames_reusing(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &mut FrameDiffScratch,
) {
    scratch.common.clear();
    if previous.delta.is_none()
        && current.delta.is_none()
        && previous.state_pages.is_empty()
        && current.state_pages.is_empty()
    {
        diff_base_nodes(previous, current, damage, retained, &mut scratch.common);
    } else {
        collect_logical_overrides(previous, &mut scratch.previous_overrides);
        collect_logical_overrides(current, &mut scratch.current_overrides);
        if scratch.previous_overrides.is_empty() && scratch.current_overrides.is_empty() {
            diff_base_nodes(previous, current, damage, retained, &mut scratch.common);
        } else {
            mark_affected_base_nodes(previous, current, scratch);
            diff_unaffected_base_nodes(previous, current, damage, retained, scratch);
            damage_overridden_nodes(previous, current, damage, retained, scratch);
        }
    }

    let mut delta = current.delta.as_deref();
    while let Some(current) = delta {
        for &(id, bounds) in current.damage.iter() {
            damage.add_bounds(bounds);
            retained.add_node(id, bounds);
        }
        delta = current.previous.as_deref();
    }

    if !scratch
        .common
        .windows(2)
        .all(|pair| pair[0].new_index < pair[1].new_index)
    {
        damage_reordered_nodes(damage, retained, scratch);
    }
}

fn diff_base_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    common: &mut Vec<CommonNode>,
) {
    for node in current.nodes.iter() {
        let Some(&previous_index) = previous.node_index.get(&node.id) else {
            add_state_change_damage(damage, retained, None, Some(*node));
            continue;
        };
        let old = previous.nodes[previous_index];
        if node_output_changed(old, *node) {
            add_state_change_damage(damage, retained, Some(old), Some(*node));
        }
    }
    for node in previous.nodes.iter() {
        if let Some(&new_index) = current.node_index.get(&node.id) {
            common.push(CommonNode {
                new_index,
                influence: node.bounds.union(current.nodes[new_index].bounds),
            });
        } else {
            add_state_change_damage(damage, retained, Some(*node), None);
        }
    }
}

fn mark_affected_base_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    scratch: &mut FrameDiffScratch,
) {
    scratch.previous_affected.resize(previous.nodes.len(), 0);
    scratch.previous_affected.fill(0);
    scratch.current_affected.resize(current.nodes.len(), 0);
    scratch.current_affected.fill(0);
    for &id in scratch
        .previous_overrides
        .keys()
        .chain(scratch.current_overrides.keys())
    {
        if let Some(&index) = previous.node_index.get(&id) {
            scratch.previous_affected[index] = 1;
        }
        if let Some(&index) = current.node_index.get(&id) {
            scratch.current_affected[index] = 1;
        }
    }
}

fn diff_unaffected_base_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &mut FrameDiffScratch,
) {
    for (index, node) in current.nodes.iter().enumerate() {
        if scratch.current_affected[index] != 0 {
            continue;
        }
        let Some(&previous_index) = previous.node_index.get(&node.id) else {
            add_state_change_damage(damage, retained, None, Some(*node));
            continue;
        };
        let old = previous.nodes[previous_index];
        if node_output_changed(old, *node) {
            add_state_change_damage(damage, retained, Some(old), Some(*node));
        }
    }
    for (index, node) in previous.nodes.iter().enumerate() {
        if scratch.previous_affected[index] != 0 {
            let Some(old) = resolved_node_state(previous, &scratch.previous_overrides, node.id)
            else {
                continue;
            };
            if let Some(&new_index) = current.node_index.get(&node.id)
                && let Some(new) = resolved_node_state(current, &scratch.current_overrides, node.id)
            {
                scratch.common.push(CommonNode {
                    new_index,
                    influence: old.bounds.union(new.bounds),
                });
            }
            continue;
        }
        if let Some(&new_index) = current.node_index.get(&node.id) {
            scratch.common.push(CommonNode {
                new_index,
                influence: node.bounds.union(current.nodes[new_index].bounds),
            });
        } else {
            add_state_change_damage(damage, retained, Some(*node), None);
        }
    }
}

fn node_output_changed(
    old: crate::canvas::RetainedNodeState,
    new: crate::canvas::RetainedNodeState,
) -> bool {
    old.revision != new.revision
        || old.kind != new.kind
        || old.placement_bits != new.placement_bits
        || old.bounds != new.bounds
}

fn collect_logical_overrides(
    frame: &RetainedFrame,
    overrides: &mut rustc_hash::FxHashMap<
        crate::RetainedNodeId,
        Option<crate::canvas::RetainedNodeState>,
    >,
) {
    overrides.clear();
    let mut delta = frame.delta.as_deref();
    while let Some(current) = delta {
        for patch in current.patches.iter() {
            let id = patch.new.or(patch.old).unwrap().id;
            overrides.entry(id).or_insert(patch.new);
        }
        delta = current.previous.as_deref();
    }
    const PAGE_SIZE: usize = 256;
    for (&page_index, page) in frame.state_pages.iter() {
        let start = page_index * PAGE_SIZE;
        for (offset, &state) in page.iter().enumerate() {
            let base = frame.nodes[start + offset];
            if state != base {
                overrides.entry(state.id).or_insert(Some(state));
            }
        }
    }
}

fn resolved_node_state(
    frame: &RetainedFrame,
    overrides: &rustc_hash::FxHashMap<
        crate::RetainedNodeId,
        Option<crate::canvas::RetainedNodeState>,
    >,
    id: crate::RetainedNodeId,
) -> Option<crate::canvas::RetainedNodeState> {
    overrides
        .get(&id)
        .copied()
        .unwrap_or_else(|| frame.node_index.get(&id).map(|&index| frame.nodes[index]))
}

fn damage_overridden_nodes(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &FrameDiffScratch,
) {
    for &id in scratch.current_overrides.keys().chain(
        scratch
            .previous_overrides
            .keys()
            .filter(|id| !scratch.current_overrides.contains_key(id)),
    ) {
        let old = resolved_node_state(previous, &scratch.previous_overrides, id);
        let new = resolved_node_state(current, &scratch.current_overrides, id);
        if old.is_none_or(|old| new.is_none_or(|new| node_output_changed(old, new))) {
            add_state_change_damage(damage, retained, old, new);
        } else if let Some(state) = new
            && (!previous.node_index.contains_key(&id) || !current.node_index.contains_key(&id))
        {
            // Overlay-only nodes have no stable base painter index. Redrawing their local
            // influence is the conservative equivalent of including them in the order pass.
            damage.add_bounds(state.bounds);
            retained.add_node(id, state.bounds);
        }
    }
}

fn add_state_change_damage(
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    old: Option<crate::canvas::RetainedNodeState>,
    new: Option<crate::canvas::RetainedNodeState>,
) {
    let bounds = match (old, new) {
        (Some(old), Some(new)) => old.bounds.union(new.bounds),
        (Some(old), None) => {
            damage.add_bounds(old.bounds);
            // Removed commands have no insertion point in the current painter order, so expose
            // their old pixels to every backdrop dependency.
            retained.add_unattributed(old.bounds);
            return;
        }
        (None, Some(new)) => new.bounds,
        (None, None) => return,
    };
    damage.add_bounds(bounds);
    retained.add_node(new.or(old).unwrap().id, bounds);
}

#[cfg(test)]
pub(super) fn diff_frames(
    previous: &RetainedFrame,
    current: &RetainedFrame,
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
) {
    diff_frames_reusing(
        previous,
        current,
        damage,
        retained,
        &mut FrameDiffScratch::default(),
    );
}

/// Damages overlapping nodes whose painter order was inverted.
///
/// Reordering previously compared every common node pair, making even a reorder of disjoint
/// components quadratic. Tile buckets restrict comparisons to spatial neighbors. The remaining
/// worst case is intentionally output-sensitive: mutually overlapping reordered nodes can have
/// O(n²) real inversions, but processing stops as soon as every output tile is already dirty.
fn damage_reordered_nodes(
    damage: &mut DamageTiles,
    retained: &mut RetainedDamage,
    scratch: &mut FrameDiffScratch,
) {
    let (tiles_width, tiles_height) = damage.dimensions();
    let tile_count = damage.total_tiles() as usize;
    scratch.tile_offsets.clear();
    scratch.tile_offsets.resize(tile_count + 1, 0);

    for node in &scratch.common {
        if let Some(rect) = tile_rect(node.influence, tiles_width, tiles_height) {
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    scratch.tile_offsets[(y * tiles_width + x) as usize + 1] += 1;
                }
            }
        }
    }
    for tile in 0..tile_count {
        scratch.tile_offsets[tile + 1] += scratch.tile_offsets[tile];
    }

    scratch.tile_cursors.clear();
    scratch
        .tile_cursors
        .extend_from_slice(&scratch.tile_offsets[..tile_count]);
    scratch.tile_nodes.clear();
    scratch
        .tile_nodes
        .resize(scratch.tile_offsets[tile_count], 0);
    for (node_index, node) in scratch.common.iter().enumerate() {
        if let Some(rect) = tile_rect(node.influence, tiles_width, tiles_height) {
            for y in rect.y0..rect.y1 {
                for x in rect.x0..rect.x1 {
                    let tile = (y * tiles_width + x) as usize;
                    let cursor = &mut scratch.tile_cursors[tile];
                    scratch.tile_nodes[*cursor] = node_index;
                    *cursor += 1;
                }
            }
        }
    }

    for tile in 0..tile_count {
        let bucket =
            &scratch.tile_nodes[scratch.tile_offsets[tile]..scratch.tile_offsets[tile + 1]];
        if bucket
            .windows(2)
            .all(|pair| scratch.common[pair[0]].new_index < scratch.common[pair[1]].new_index)
        {
            continue;
        }
        for (offset, &a) in bucket.iter().enumerate() {
            for &b in &bucket[offset + 1..] {
                if scratch.common[a].new_index < scratch.common[b].new_index {
                    continue;
                }
                let bounds = scratch.common[a]
                    .influence
                    .intersect(scratch.common[b].influence);
                let Some(rect) = tile_rect(bounds, tiles_width, tiles_height) else {
                    continue;
                };
                // A pair can share many tile buckets. Its intersection's top-left tile is shared
                // by both nodes and gives it one allocation-free canonical comparison site.
                if tile != (rect.y0 * tiles_width + rect.x0) as usize {
                    continue;
                }
                damage.add_bounds(bounds);
                retained.add_unattributed(bounds);
                if damage.len() == damage.total_tiles() {
                    return;
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct TileRect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

#[inline]
fn tile_rect(bounds: Bounds, tiles_width: u32, tiles_height: u32) -> Option<TileRect> {
    let bounds = bounds.intersect(Bounds::canvas(
        tiles_width.saturating_mul(TILE_SIZE),
        tiles_height.saturating_mul(TILE_SIZE),
    ));
    if bounds.is_empty() {
        return None;
    }
    Some(TileRect {
        x0: bounds.x0.max(0) as u32 / TILE_SIZE,
        y0: bounds.y0.max(0) as u32 / TILE_SIZE,
        x1: (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(tiles_width),
        y1: (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(tiles_height),
    })
}
