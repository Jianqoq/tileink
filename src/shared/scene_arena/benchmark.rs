//! Criterion adapter for zero-initialized scene-arena allocations.

use super::{ArenaAllocation, SceneArena};

/// Exercises the production arena paths without exposing the arena itself as public API.
#[doc(hidden)]
pub struct SceneArenaFillBenchmark {
    arena: SceneArena<u8>,
    allocation: Option<ArenaAllocation>,
}

impl SceneArenaFillBenchmark {
    pub fn empty() -> Self {
        Self {
            arena: SceneArena::new(0),
            allocation: None,
        }
    }

    pub fn with_allocation(len: usize) -> Self {
        let mut benchmark = Self::empty();
        benchmark.allocation = Some(benchmark.arena.insert_filled(len, 0));
        benchmark.arena.take_dirty_ranges_into(&mut Vec::new());
        benchmark
    }

    pub fn insert_zeroed(&mut self, len: usize) -> usize {
        let allocation = self.arena.insert_filled(len, 0);
        self.arena.range(allocation).start
    }

    pub fn replace_zeroed(&mut self, len: usize) -> usize {
        let allocation = self
            .allocation
            .expect("replacement benchmark requires an allocation");
        let _ = self.arena.replace_filled(allocation, len, 0);
        self.arena.range(allocation).start
    }
}

/// Exercises repeated dirty-range collection while the arena remains alive across frames.
#[doc(hidden)]
pub struct SceneArenaDirtyBenchmark {
    arena: SceneArena<u8>,
    ranges: Vec<std::ops::Range<usize>>,
}

impl Default for SceneArenaDirtyBenchmark {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneArenaDirtyBenchmark {
    pub fn new() -> Self {
        Self {
            arena: SceneArena::new(0),
            ranges: Vec::new(),
        }
    }

    pub fn collect(&mut self, count: usize, overlapping: bool) -> (usize, usize) {
        self.arena.values.resize(count * 3 + 2, 0);
        for index in 0..count {
            let start = if overlapping { index } else { index * 3 };
            self.arena.mark_dirty(start..start + 2);
        }
        self.arena.take_dirty_ranges_into(&mut self.ranges);
        let checksum = self
            .ranges
            .iter()
            .fold(0usize, |sum, range| sum.wrapping_add(range.start));
        (self.ranges.len(), checksum)
    }
}
