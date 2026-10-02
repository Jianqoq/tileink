use super::*;
use std::{cell::Cell, rc::Rc};

struct Allocation {
    bytes: u64,
    drops: Rc<Cell<u32>>,
}

impl SurfaceAllocation for Allocation {
    fn byte_len(&self) -> u64 {
        self.bytes
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

fn allocation(bytes: u64, drops: &Rc<Cell<u32>>) -> Allocation {
    Allocation {
        bytes,
        drops: drops.clone(),
    }
}

fn id(node: u64, slot: u32) -> RetainedSurfaceId {
    RetainedSurfaceId {
        node: RetainedNodeId::for_owner(node),
        slot,
    }
}

fn meta(kind: RetainedSurfaceKind) -> RetainedSurfaceMeta {
    RetainedSurfaceMeta {
        revision: NodeGeneration::new(1),
        kind,
        size: (16, 16),
        origin: (0, 0),
        bounds: Bounds::new(0, 0, 16, 16),
    }
}

#[test]
fn the_budget_counts_all_three_allocations_and_exact_fit() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(60);
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Backdrop),
        allocation(10, &drops),
        Some(allocation(20, &drops)),
        Some(allocation(30, &drops)),
    );
    assert!(!cache.take_backdrop_evicted());
    cache.set_budget(59);
    assert!(cache.take(id(1, 0)).is_none());
    assert_eq!(drops.get(), 3);
    assert!(cache.take_backdrop_evicted());
    assert!(!cache.take_backdrop_evicted());
}

#[test]
fn rejected_replacement_cannot_leave_the_previous_contents_cached() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(10);
    let same_meta = meta(RetainedSurfaceKind::Filter);
    cache.insert(id(1, 0), same_meta, allocation(10, &drops), None, None);
    // Raster invalidation can replace pixels without changing the node's metadata.
    // A rejected larger allocation must also remove the old, now-stale entry.
    cache.insert(id(1, 0), same_meta, allocation(11, &drops), None, None);
    assert!(cache.take(id(1, 0)).is_none());
    assert_eq!(drops.get(), 2);
    cache.insert(id(2, 0), same_meta, allocation(10, &drops), None, None);
    assert!(cache.take(id(2, 0)).is_some());
}

#[test]
fn a_taken_and_reinserted_surface_becomes_most_recently_used() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(20);
    for node in [1, 2] {
        cache.insert(
            id(node, 0),
            meta(RetainedSurfaceKind::Group),
            allocation(10, &drops),
            None,
            None,
        );
    }
    let first = cache.take(id(1, 0)).unwrap();
    cache.insert(
        id(1, 0),
        first.meta,
        first.primary,
        first.secondary,
        first.backdrop_source,
    );
    cache.insert(
        id(3, 0),
        meta(RetainedSurfaceKind::Group),
        allocation(10, &drops),
        None,
        None,
    );
    assert!(cache.take(id(2, 0)).is_none());
    assert_eq!(drops.get(), 1);
    assert!(cache.take(id(1, 0)).is_some());
    assert!(cache.take(id(3, 0)).is_some());
    assert!(!cache.take_backdrop_evicted());
}

#[test]
fn taking_and_replacing_updates_accounting_without_double_charging() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(20);
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Mask),
        allocation(20, &drops),
        None,
        None,
    );
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Mask),
        allocation(10, &drops),
        None,
        None,
    );
    cache.insert(
        id(2, 0),
        meta(RetainedSurfaceKind::Filter),
        allocation(10, &drops),
        None,
        None,
    );
    assert_eq!(drops.get(), 1);
    drop(cache.take(id(1, 0)).unwrap());
    cache.set_budget(10);
    assert!(cache.take(id(2, 0)).is_some());
}

#[test]
fn node_removal_covers_every_slot_and_preserves_other_nodes() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(40);
    for (node, slot) in [(1, 0), (1, 1), (2, 0), (3, 0)] {
        cache.insert(
            id(node, slot),
            meta(RetainedSurfaceKind::Backdrop),
            allocation(10, &drops),
            None,
            None,
        );
    }
    cache.remove_nodes(&HashSet::new());
    assert_eq!(drops.get(), 0);
    cache.remove_nodes(&HashSet::from([RetainedNodeId::for_owner(1)]));
    assert_eq!(drops.get(), 2);
    assert!(cache.take(id(1, 0)).is_none());
    assert!(cache.take(id(1, 1)).is_none());
    cache.retain_nodes(&HashSet::from([RetainedNodeId::for_owner(2)]));
    assert_eq!(drops.get(), 3);
    assert!(!cache.take_backdrop_evicted());
    assert!(cache.take(id(2, 0)).is_some());
}

#[test]
fn a_rejected_backdrop_requires_history_rebuild_even_without_a_previous_entry() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(0);
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Backdrop),
        allocation(1, &drops),
        None,
        None,
    );
    assert!(cache.take(id(1, 0)).is_none());
    assert_eq!(drops.get(), 1);
    assert!(cache.take_backdrop_evicted());
    assert!(!cache.take_backdrop_evicted());
}

#[test]
fn clock_wrap_does_not_turn_the_newest_cache_entry_into_the_oldest() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(32);
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Filter),
        allocation(16, &drops),
        None,
        None,
    );
    cache.clock = u64::MAX - 1;
    cache.insert(
        id(2, 0),
        meta(RetainedSurfaceKind::Filter),
        allocation(16, &drops),
        None,
        None,
    );
    cache.insert(
        id(3, 0),
        meta(RetainedSurfaceKind::Filter),
        allocation(16, &drops),
        None,
        None,
    );
    assert!(
        cache.take(id(3, 0)).is_some(),
        "a wrapped clock must keep the newest entry newest"
    );
    assert!(cache.take(id(1, 0)).is_none());
    assert!(cache.take(id(2, 0)).is_some());
}

#[test]
fn partial_frame_protects_unvisited_backdrop_but_keeps_ordinary_lru_eviction() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(48);
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Backdrop),
        allocation(24, &drops),
        None,
        None,
    );
    cache.insert(
        id(2, 0),
        meta(RetainedSurfaceKind::Group),
        allocation(16, &drops),
        None,
        None,
    );
    cache.begin_frame(true);
    cache.insert(
        id(3, 0),
        meta(RetainedSurfaceKind::Filter),
        allocation(24, &drops),
        None,
        None,
    );
    assert_eq!(cache.byte_len, 48);
    assert!(cache.take(id(1, 0)).is_some());
    assert!(cache.take(id(2, 0)).is_none());
    assert!(cache.take(id(3, 0)).is_some());
    assert!(!cache.take_backdrop_evicted());
}

#[test]
fn a_visited_backdrop_is_evictable_and_invalidates_the_next_frame() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(32);
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Backdrop),
        allocation(16, &drops),
        None,
        None,
    );
    cache.insert(
        id(2, 0),
        meta(RetainedSurfaceKind::Filter),
        allocation(16, &drops),
        None,
        None,
    );
    cache.begin_frame(true);
    let surface = cache.take(id(1, 0)).unwrap();
    cache.insert(
        id(1, 0),
        surface.meta,
        surface.primary,
        surface.secondary,
        surface.backdrop_source,
    );
    for node in [3, 4] {
        cache.insert(
            id(node, 0),
            meta(RetainedSurfaceKind::Filter),
            allocation(16, &drops),
            None,
            None,
        );
    }
    assert_eq!(cache.byte_len, 32);
    assert!(cache.take(id(1, 0)).is_none());
    assert!(cache.take_backdrop_evicted());
}

#[test]
fn budget_pressure_rejects_the_new_entry_when_only_protected_history_can_pay() {
    for kind in [RetainedSurfaceKind::Filter, RetainedSurfaceKind::Backdrop] {
        let drops = Rc::new(Cell::new(0));
        let mut cache = RetainedSurfaceCache::new(32);
        cache.insert(
            id(1, 0),
            meta(RetainedSurfaceKind::Backdrop),
            allocation(32, &drops),
            None,
            None,
        );
        cache.begin_frame(true);
        cache.insert(id(2, 0), meta(kind), allocation(16, &drops), None, None);
        assert_eq!(cache.byte_len, 32);
        assert!(cache.take(id(1, 0)).is_some());
        assert!(cache.take(id(2, 0)).is_none());
        assert_eq!(
            cache.take_backdrop_evicted(),
            kind == RetainedSurfaceKind::Backdrop
        );
    }
}

#[test]
fn full_redraw_and_frame_end_allow_normal_backdrop_eviction() {
    for full in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let mut cache = RetainedSurfaceCache::new(32);
        cache.insert(
            id(1, 0),
            meta(RetainedSurfaceKind::Backdrop),
            allocation(32, &drops),
            None,
            None,
        );
        if full {
            cache.begin_frame(false);
        } else {
            cache.begin_frame(true);
            cache.end_frame();
        }
        cache.insert(
            id(2, 0),
            meta(RetainedSurfaceKind::Filter),
            allocation(32, &drops),
            None,
            None,
        );
        assert_eq!(cache.byte_len, 32);
        assert!(cache.take(id(1, 0)).is_none());
        assert!(cache.take(id(2, 0)).is_some());
        assert!(cache.take_backdrop_evicted());
    }
}

#[test]
fn clock_rebase_preserves_both_sides_of_the_partial_frame_boundary() {
    let drops = Rc::new(Cell::new(0));
    let mut cache = RetainedSurfaceCache::new(48);
    cache.insert(
        id(1, 0),
        meta(RetainedSurfaceKind::Backdrop),
        allocation(16, &drops),
        None,
        None,
    );
    cache.clock = u64::MAX - 1;
    cache.begin_frame(true);
    cache.insert(
        id(2, 0),
        meta(RetainedSurfaceKind::Backdrop),
        allocation(16, &drops),
        None,
        None,
    );
    cache.insert(
        id(3, 0),
        meta(RetainedSurfaceKind::Filter),
        allocation(32, &drops),
        None,
        None,
    );
    assert_eq!(cache.byte_len, 48);
    assert!(cache.take(id(1, 0)).is_some());
    assert!(cache.take(id(2, 0)).is_none());
    assert!(cache.take(id(3, 0)).is_some());
    assert!(cache.take_backdrop_evicted());
}

#[test]
#[should_panic(expected = "change the surface budget between partial frames")]
fn budget_changes_cannot_invalidate_protected_history_during_a_partial_frame() {
    let mut cache = RetainedSurfaceCache::<Allocation>::new(32);
    cache.begin_frame(true);
    cache.set_budget(0);
}
