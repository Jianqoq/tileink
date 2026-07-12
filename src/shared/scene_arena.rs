use rustc_hash::FxHashMap as HashMap;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

const COMPACTION_THRESHOLD: f32 = 0.30;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ArenaAllocation(u64);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Allocation {
    start: usize,
    len: usize,
    capacity: usize,
}

/// Stable variable-sized storage for retained scene records.
///
/// Allocations keep their offsets while unrelated nodes change. Replacements reuse existing
/// capacity, removals become mergeable holes, and compaction only happens when an append would
/// grow storage while more than 30% of the current high-water range is free.
pub(crate) struct SceneArena<T: Copy> {
    values: Vec<T>,
    vacant: T,
    allocations: HashMap<ArenaAllocation, Allocation>,
    free_by_start: BTreeMap<usize, usize>,
    free_by_size: BTreeSet<(usize, usize)>,
    free_len: usize,
    next_id: u64,
    live_len: usize,
    dirty: Vec<Range<usize>>,
    compactions: u64,
}

impl<T: Copy> SceneArena<T> {
    pub(crate) fn new(vacant: T) -> Self {
        Self {
            values: Vec::new(),
            vacant,
            allocations: HashMap::default(),
            free_by_start: BTreeMap::new(),
            free_by_size: BTreeSet::new(),
            free_len: 0,
            next_id: 1,
            live_len: 0,
            dirty: Vec::new(),
            compactions: 0,
        }
    }

    pub(crate) fn insert(&mut self, data: &[T]) -> ArenaAllocation {
        let id = ArenaAllocation(self.next_id);
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let allocation = self.allocate(data.len());
        self.allocations.insert(id, allocation);
        self.write_allocation(allocation, data);
        self.live_len += data.len();
        id
    }

    pub(crate) fn replace(&mut self, id: ArenaAllocation, data: &[T]) -> bool {
        let moved = self.resize(id, data.len());
        self.write(id, data);
        moved
    }

    /// Changes an allocation's logical length without copying temporary local-index records.
    /// Callers that remap cross-arena offsets can then write the final physical records once.
    pub(crate) fn resize(&mut self, id: ArenaAllocation, len: usize) -> bool {
        let old = self.allocations[&id];
        if old.len == len {
            return false;
        }
        self.live_len = self.live_len - old.len + len;
        if len <= old.capacity {
            self.allocations
                .get_mut(&id)
                .expect("allocation was read above")
                .len = len;
            if len < old.capacity {
                self.values[old.start + len..old.start + old.capacity].fill(self.vacant);
                self.mark_dirty(old.start + len..old.start + old.capacity);
            }
            return false;
        }

        // Remove the allocation before allocate() may compact. Otherwise compaction would treat
        // the just-freed range as live and can copy it after trailing-hole truncation.
        self.allocations.remove(&id);
        self.values[old.start..old.start + old.capacity].fill(self.vacant);
        self.mark_dirty(old.start..old.start + old.capacity);
        self.release_range(old.start..old.start + old.capacity);
        let allocation = self.allocate(len);
        self.allocations.insert(id, allocation);
        true
    }

    pub(crate) fn write(&mut self, id: ArenaAllocation, data: &[T]) {
        let allocation = self.allocations[&id];
        assert_eq!(allocation.len, data.len());
        self.write_allocation(allocation, data);
    }

    /// Writes transformed records directly into stable storage.
    ///
    /// Retained chunks store local indices, while GPU-facing arenas store physical indices. Doing
    /// that remap in-place avoids a temporary allocation for every record kind and changed node.
    pub(crate) fn write_mapped<U: Copy>(
        &mut self,
        id: ArenaAllocation,
        source: &[U],
        mut map: impl FnMut(U) -> T,
    ) {
        let allocation = self.allocations[&id];
        assert_eq!(allocation.len, source.len());
        for (target, source) in self.values[allocation.start..allocation.start + allocation.len]
            .iter_mut()
            .zip(source.iter().copied())
        {
            *target = map(source);
        }
        self.mark_dirty(allocation.start..allocation.start + allocation.len);
    }

    pub(crate) fn remove(&mut self, id: ArenaAllocation) -> bool {
        let Some(allocation) = self.allocations.remove(&id) else {
            return false;
        };
        self.live_len -= allocation.len;
        self.values[allocation.start..allocation.start + allocation.capacity].fill(self.vacant);
        // GPU consumers address the arena's physical high-water range. Clearing only the logical
        // length leaves stale records in the released padding until that capacity is reused.
        self.mark_dirty(allocation.start..allocation.start + allocation.capacity);
        self.release_range(allocation.start..allocation.start + allocation.capacity);
        true
    }

    pub(crate) fn range(&self, id: ArenaAllocation) -> Range<usize> {
        let allocation = self.allocations[&id];
        allocation.start..allocation.start + allocation.len
    }

    pub(crate) fn values(&self) -> &[T] {
        &self.values
    }

    pub(crate) fn live_len(&self) -> usize {
        self.live_len
    }

    pub(crate) fn fragmentation(&self) -> f32 {
        if self.values.is_empty() {
            0.0
        } else {
            self.free_len as f32 / self.values.len() as f32
        }
    }

    pub(crate) fn compactions(&self) -> u64 {
        self.compactions
    }

    pub(crate) fn take_dirty_ranges(&mut self) -> Vec<Range<usize>> {
        let len = self.values.len();
        let mut dirty = std::mem::take(&mut self.dirty)
            .into_iter()
            .filter_map(|range| {
                let range = range.start.min(len)..range.end.min(len);
                (!range.is_empty()).then_some(range)
            })
            .collect::<Vec<_>>();
        dirty.sort_unstable_by_key(|range| range.start);
        let mut merged = Vec::<Range<usize>>::with_capacity(dirty.len());
        for range in dirty {
            if let Some(previous) = merged.last_mut()
                && range.start <= previous.end
            {
                previous.end = previous.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        merged
    }

    fn allocate(&mut self, len: usize) -> Allocation {
        if len == 0 {
            return Allocation::default();
        }
        if let Some(&(capacity, start)) = self.free_by_size.range((len, 0)..).next() {
            let end = self.remove_free(start);
            if capacity > len {
                self.insert_free(start + len, end);
            }
            return Allocation {
                start,
                len,
                capacity: len,
            };
        }

        if self.fragmentation() > COMPACTION_THRESHOLD {
            self.compact();
        }
        let start = self.values.len();
        let capacity = len.saturating_add(len / 2).max(len);
        self.values.resize(start + capacity, self.vacant);
        // New GPU buffer bytes are not guaranteed to contain the arena's vacant sentinel. Upload
        // the padding once; later same-size rewrites can continue dirtying only the logical range.
        self.mark_dirty(start..start + capacity);
        Allocation {
            start,
            len,
            capacity,
        }
    }

    fn write_allocation(&mut self, allocation: Allocation, data: &[T]) {
        if data.is_empty() {
            return;
        }
        self.values[allocation.start..allocation.start + data.len()].copy_from_slice(data);
        self.mark_dirty(allocation.start..allocation.start + allocation.len);
    }

    fn release_range(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let mut start = range.start;
        let mut end = range.end;
        if let Some((&previous_start, &previous_end)) =
            self.free_by_start.range(..=start).next_back()
            && previous_end == start
        {
            self.remove_free(previous_start);
            start = previous_start;
        }
        if let Some((&next_start, &next_end)) = self.free_by_start.range(start..).next()
            && end == next_start
        {
            self.remove_free(next_start);
            end = next_end;
        }
        self.insert_free(start, end);
        // Keep trailing holes addressable until compaction. GPU buffers and tile pages may still
        // reference those physical slots for the current frame, so the inactive records written
        // by remove() must be uploaded instead of disappearing with a high-water truncation.
    }

    fn insert_free(&mut self, start: usize, end: usize) {
        debug_assert!(start < end);
        debug_assert!(!self.free_by_start.contains_key(&start));
        let len = end - start;
        self.free_by_start.insert(start, end);
        self.free_by_size.insert((len, start));
        self.free_len += len;
    }

    fn remove_free(&mut self, start: usize) -> usize {
        let end = self
            .free_by_start
            .remove(&start)
            .expect("free-range indices must stay synchronized");
        let len = end - start;
        assert!(self.free_by_size.remove(&(len, start)));
        self.free_len -= len;
        end
    }

    fn compact(&mut self) {
        let mut ids = self.allocations.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| self.allocations[id].start);
        let old = std::mem::take(&mut self.values);
        let mut values = Vec::with_capacity(self.live_len.next_power_of_two());
        for id in ids {
            let allocation = self.allocations[&id];
            let start = values.len();
            values.extend_from_slice(&old[allocation.start..allocation.start + allocation.len]);
            self.allocations.insert(
                id,
                Allocation {
                    start,
                    len: allocation.len,
                    capacity: allocation.len,
                },
            );
        }
        self.values = values;
        self.free_by_start.clear();
        self.free_by_size.clear();
        self.free_len = 0;
        self.dirty.clear();
        self.dirty.push(0..self.values.len());
        self.compactions += 1;
    }

    fn mark_dirty(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.dirty.push(range);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_allocations_keep_offsets_across_variable_length_updates() {
        let mut arena = SceneArena::new(0u32);
        let first = arena.insert(&[1, 2]);
        let second = arena.insert(&[3; 10]);
        let second_range = arena.range(second);

        assert!(arena.replace(first, &[5, 6, 7, 8, 9]));
        assert_eq!(arena.range(second), second_range);
        assert_eq!(&arena.values()[arena.range(first)], &[5, 6, 7, 8, 9]);
    }

    #[test]
    fn removed_ranges_are_reused_and_neutralized() {
        let mut arena = SceneArena::new(0u32);
        let removed = arena.insert(&[1, 2, 3, 4]);
        let survivor = arena.insert(&[5]);
        let range = arena.range(removed);
        assert!(arena.remove(removed));
        assert_eq!(&arena.values()[range.clone()], &[0, 0, 0, 0]);
        let reused = arena.insert(&[7, 8, 9]);
        assert_eq!(arena.range(reused).start, range.start);
        assert_eq!(&arena.values()[arena.range(survivor)], &[5]);
    }

    #[test]
    fn fresh_allocation_marks_vacant_gpu_padding_dirty() {
        let mut arena = SceneArena::new(0u32);
        let allocation = arena.insert(&[1, 2]);

        assert_eq!(arena.range(allocation), 0..2);
        assert_eq!(arena.values(), &[1, 2, 0]);
        assert_eq!(arena.take_dirty_ranges(), vec![0..3]);
    }

    #[test]
    fn allocation_reports_only_coalesced_logical_dirty_ranges() {
        let mut arena = SceneArena::new(0u32);
        let first = arena.insert(&[1, 2]);
        let _ = arena.take_dirty_ranges();
        arena.replace(first, &[3, 4]);
        arena.replace(first, &[5, 6]);
        assert_eq!(arena.take_dirty_ranges(), vec![0..2]);
    }

    #[test]
    fn mapped_write_transforms_directly_into_the_allocation() {
        let mut arena = SceneArena::new(0u32);
        let allocation = arena.insert(&[0; 3]);
        let _ = arena.take_dirty_ranges();

        arena.write_mapped(allocation, &[1u16, 2, 3], |value| u32::from(value) * 4);

        assert_eq!(&arena.values()[arena.range(allocation)], &[4, 8, 12]);
        assert_eq!(arena.take_dirty_ranges(), vec![0..3]);
    }

    #[test]
    fn same_length_resize_preserves_allocation_and_live_length() {
        let mut arena = SceneArena::new(0u32);
        let allocation = arena.insert(&[1, 2, 3]);
        let range = arena.range(allocation);
        let live_len = arena.live_len();

        assert!(!arena.resize(allocation, 3));
        assert_eq!(arena.range(allocation), range);
        assert_eq!(arena.live_len(), live_len);
        assert_eq!(&arena.values()[range], &[1, 2, 3]);
    }

    #[test]
    fn replacement_compaction_remaps_survivors_without_copying_freed_allocation() {
        let mut arena = SceneArena::new(0u32);
        let first = arena.insert(&[1; 10]);
        let middle = arena.insert(&[2; 10]);
        let replaced = arena.insert(&[3; 10]);
        let last = arena.insert(&[5; 10]);
        arena.remove(first);
        let old_last = arena.range(last);

        assert!(arena.replace(replaced, &[4; 20]));
        assert_eq!(&arena.values()[arena.range(replaced)], &[4; 20]);
        assert_eq!(&arena.values()[arena.range(middle)], &[2; 10]);
        assert_eq!(&arena.values()[arena.range(last)], &[5; 10]);
        assert_ne!(arena.range(last), old_last);
        assert_eq!(arena.compactions(), 1);
    }

    #[test]
    fn removing_trailing_allocation_keeps_an_inactive_dirty_physical_slot() {
        let mut arena = SceneArena::new(0u32);
        let first = arena.insert(&[1, 2]);
        let last = arena.insert(&[3, 4]);
        let _ = arena.take_dirty_ranges();
        arena.remove(last);

        assert_eq!(arena.take_dirty_ranges(), vec![3..6]);
        assert_eq!(&arena.values()[arena.range(first)], &[1, 2]);
    }
}
