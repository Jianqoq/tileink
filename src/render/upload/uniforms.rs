//! Aggregate aligned uniform slots by resource identity, independently of the GPU API.
//!
//! A stage name is not an identity: deferred vector scenes have their own stage buffers
//! in the parent's command batch. `B` must compare the underlying allocation, including
//! through cloned handles. The adapter owns submission and in-flight resource lifetime.

use rustc_hash::FxHashMap;
use std::hash::Hash;

// Bound the linear scan independently of workload size. Tiny batches avoid the
// hash index's fixed cost; larger batches keep expected O(1) resource lookup.
const LINEAR_RESOURCE_LIMIT: usize = 16;

pub(crate) struct UniformWrites<B> {
    arenas: Vec<(B, UniformWriteArena)>,
    index: FxHashMap<B, usize>,
}

struct UniformWriteArena {
    size: u64,
    stride: u64,
    slots: u64,
    used_slots: u64,
    bytes: Vec<u8>,
}

impl<B> Default for UniformWrites<B> {
    fn default() -> Self {
        Self {
            arenas: Vec::new(),
            index: FxHashMap::default(),
        }
    }
}

impl<B: Clone + Eq + Hash> UniformWrites<B> {
    fn arena_index(&self, buffer: &B) -> Option<usize> {
        if self.arenas.len() <= LINEAR_RESOURCE_LIMIT {
            self.arenas.iter().position(|(key, _)| key == buffer)
        } else {
            self.index.get(buffer).copied()
        }
    }

    pub(crate) fn is_full(&self, buffer: &B) -> bool {
        self.arena_index(buffer).is_some_and(|index| {
            let arena = &self.arenas[index].1;
            arena.used_slots == arena.slots
        })
    }

    pub(crate) fn write(
        &mut self,
        buffer: &B,
        size: u64,
        stride: u64,
        slots: u64,
        bytes: &[u8],
    ) -> u64 {
        if let Some(index) = self.arena_index(buffer) {
            return self.arenas[index].1.write(size, stride, slots, bytes);
        }
        assert!(
            size > 0 && stride >= size && slots > 0,
            "invalid uniform layout"
        );
        let buffer_size = stride
            .checked_mul(slots)
            .expect("uniform buffer size overflow");
        usize::try_from(buffer_size).expect("uniform buffer size exceeds address space");
        let initial_capacity = usize::try_from(stride * slots.min(8)).unwrap();
        let mut arena = UniformWriteArena {
            size,
            stride,
            slots,
            used_slots: 0,
            bytes: Vec::with_capacity(initial_capacity),
        };
        let offset = arena.write(size, stride, slots, bytes);
        let index = self.arenas.len();
        self.arenas.push((buffer.clone(), arena));
        if index == LINEAR_RESOURCE_LIMIT {
            // Existing slot contents stay in place when lookup becomes indexed.
            self.index.extend(
                self.arenas
                    .iter()
                    .enumerate()
                    .map(|(index, (key, _))| (key.clone(), index)),
            );
        } else if index > LINEAR_RESOURCE_LIMIT {
            self.index.insert(buffer.clone(), index);
        }
        offset
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&B, &[u8])> {
        self.arenas
            .iter()
            .map(|(buffer, arena)| (buffer, arena.bytes.as_slice()))
    }

    /// The adapter must copy these bytes to its upload storage before clearing them.
    pub(crate) fn clear(&mut self) {
        self.arenas.clear();
        self.index.clear();
    }
}

impl UniformWriteArena {
    fn write(&mut self, size: u64, stride: u64, slots: u64, bytes: &[u8]) -> u64 {
        assert_eq!(
            (self.size, self.stride, self.slots),
            (size, stride, slots),
            "uniform layout changed inside a batch"
        );
        assert!(bytes.len() as u64 <= size, "uniform data exceeds its slot");
        assert!(
            self.used_slots < self.slots,
            "submit before reusing a full uniform buffer"
        );
        // The validated allocation bounds cover every slot and its padding.
        let offset = self.used_slots * self.stride;
        let start = offset as usize;
        let end = start + self.size as usize;
        self.bytes.resize(end, 0);
        self.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        self.used_slots += 1;
        offset
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "bench-internals")]
pub(crate) mod benchmark;
