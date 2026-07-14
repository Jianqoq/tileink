use std::collections::HashMap;

use crate::{Bounds, TILE_SIZE};

/// Exact dirty-tile membership plus the compact worklist consumed by incremental GPU dispatches.
#[derive(Clone, Debug)]
pub(crate) struct DamageTiles {
    tiles_width: u32,
    tiles_height: u32,
    bits: Vec<u64>,
    list: Vec<u32>,
}

impl DamageTiles {
    /// Word masks amortize their setup once a rectangle is both wide and large enough. Narrow or
    /// tiny damage remains on the direct tile path because it is the dominant retained-update case.
    const WORD_MASK_MIN_WIDTH: u32 = 12;
    const WORD_MASK_MIN_AREA: u32 = 64;

    pub(crate) fn new(size: (u32, u32)) -> Self {
        let tiles_width = size.0.div_ceil(TILE_SIZE);
        let tiles_height = size.1.div_ceil(TILE_SIZE);
        let tile_count = tiles_width.saturating_mul(tiles_height);
        Self {
            tiles_width,
            tiles_height,
            bits: vec![0; tile_count.div_ceil(64) as usize],
            list: Vec::new(),
        }
    }

    pub(crate) fn full(size: (u32, u32)) -> Self {
        let mut damage = Self::new(size);
        let count = damage.total_tiles();
        damage.list.extend(0..count);
        for tile in 0..count {
            damage.bits[tile as usize / 64] |= 1 << (tile % 64);
        }
        damage
    }

    pub(crate) fn add_bounds(&mut self, bounds: Bounds) {
        let canvas = Bounds::canvas(
            self.tiles_width.saturating_mul(TILE_SIZE),
            self.tiles_height.saturating_mul(TILE_SIZE),
        );
        let bounds = bounds.intersect(canvas);
        if bounds.is_empty() {
            return;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_height);
        let tile_width = x1 - x0;
        let tile_height = y1 - y0;
        if tile_width == 1 && tile_height == 1 {
            let tile = y0 * self.tiles_width + x0;
            self.add_tile(tile);
            return;
        }
        if tile_width >= Self::WORD_MASK_MIN_WIDTH
            && tile_width.saturating_mul(tile_height) >= Self::WORD_MASK_MIN_AREA
        {
            self.add_tile_rect_words(x0, y0, x1, y1);
            return;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                self.add_tile(y * self.tiles_width + x);
            }
        }
    }

    #[inline(always)]
    fn add_tile(&mut self, tile: u32) {
        // Every caller clips tile coordinates to the dimensions used to allocate `bits`.
        let word = unsafe { self.bits.get_unchecked_mut(tile as usize / 64) };
        let mask = 1 << (tile % 64);
        if *word & mask == 0 {
            *word |= mask;
            self.list.push(tile);
        }
    }

    /// Adds a large tile rectangle one machine word at a time while retaining the compact list.
    /// Enumerating only newly set bits avoids revisiting every covered tile for overlapping bounds
    /// and preserves the exact row-major insertion order used by GPU worklists.
    #[cold]
    #[inline(never)]
    fn add_tile_rect_words(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        for y in y0..y1 {
            self.add_tile_range_words(y * self.tiles_width + x0, y * self.tiles_width + x1);
        }
    }

    fn add_tile_range_words(&mut self, mut start: u32, end: u32) {
        while start < end {
            let word_index = start as usize / 64;
            let bit = start % 64;
            let width = (64 - bit).min(end - start);
            let mask = if width == 64 {
                u64::MAX
            } else {
                ((1u64 << width) - 1) << bit
            };
            let mut added = mask & !self.bits[word_index];
            self.bits[word_index] |= mask;
            while added != 0 {
                let bit = added.trailing_zeros();
                self.list.push(word_index as u32 * 64 + bit);
                added &= added - 1;
            }
            start += width;
        }
    }

    pub(crate) fn contains(&self, tile: u32) -> bool {
        self.bits
            .get(tile as usize / 64)
            .is_some_and(|word| *word & (1 << (tile % 64)) != 0)
    }

    pub(crate) fn intersects_bounds(&self, bounds: Bounds) -> bool {
        if bounds.is_empty() {
            return false;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_height);
        (y0..y1).any(|y| (x0..x1).any(|x| self.contains(y * self.tiles_width + x)))
    }

    pub(crate) fn intersects_tile_rect(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> bool {
        let x1 = x1.min(self.tiles_width);
        let y1 = y1.min(self.tiles_height);
        (y0.min(y1)..y1).any(|y| (x0.min(x1)..x1).any(|x| self.contains(y * self.tiles_width + x)))
    }

    pub(crate) fn count_in_bounds(&self, bounds: Bounds) -> u32 {
        if bounds.is_empty() {
            return 0;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_height);
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| y * self.tiles_width + x))
            .filter(|tile| self.contains(*tile))
            .count() as u32
    }

    pub(crate) fn list(&self) -> &[u32] {
        &self.list
    }

    pub(crate) fn len(&self) -> u32 {
        self.list.len() as u32
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub(crate) fn total_tiles(&self) -> u32 {
        self.tiles_width.saturating_mul(self.tiles_height)
    }

    pub(crate) fn dimensions(&self) -> (u32, u32) {
        (self.tiles_width, self.tiles_height)
    }

    /// Decomposes dirty tiles into non-overlapping pixel rectangles.
    ///
    /// Horizontal runs with the same extent on adjacent tile rows are merged
    /// vertically. GPU execution consumes `list` directly as one compact
    /// dispatch; rectangles remain useful for CPU damage propagation, bounds
    /// transforms, and diagnostics without expanding sparse damage to its
    /// bounding box.
    pub(crate) fn coalesced_rects(&self, physical_size: (u32, u32)) -> Vec<Bounds> {
        let mut rects = Vec::<Bounds>::new();
        let mut previous_row = HashMap::<(u32, u32), usize>::new();
        for y in 0..self.tiles_height {
            let mut current_row = HashMap::new();
            let mut x = 0;
            while x < self.tiles_width {
                while x < self.tiles_width && !self.contains(y * self.tiles_width + x) {
                    x += 1;
                }
                let start = x;
                while x < self.tiles_width && self.contains(y * self.tiles_width + x) {
                    x += 1;
                }
                if start < x {
                    let key = (start, x);
                    let y1 = ((y + 1) * TILE_SIZE).min(physical_size.1) as i32;
                    let index = if let Some(&index) = previous_row.get(&key) {
                        rects[index].y1 = y1;
                        index
                    } else {
                        let index = rects.len();
                        rects.push(Bounds::new(
                            (start * TILE_SIZE) as i32,
                            (y * TILE_SIZE) as i32,
                            (x * TILE_SIZE).min(physical_size.0) as i32,
                            y1,
                        ));
                        index
                    };
                    current_row.insert(key, index);
                }
            }
            previous_row = current_row;
        }
        rects
    }

    /// Expands pixel damage while preserving the tile-grid representation.
    ///
    /// Offscreen filters use this to redraw source pixels outside the visible
    /// output damage that can still be sampled into that output.
    pub(crate) fn outset(&self, physical_size: (u32, u32), pixels: i32) -> Self {
        if pixels <= 0 {
            return self.clone();
        }
        let canvas = Bounds::canvas(physical_size.0, physical_size.1);
        let mut expanded = Self::new(physical_size);
        for rect in self.coalesced_rects(physical_size) {
            expanded.add_bounds(rect.outset(pixels).intersect(canvas));
        }
        expanded
    }
}

/// Criterion-only access to the production damage-tile implementation.
///
/// Keeping the benchmark adapter here ensures performance experiments exercise the same code used
/// by the renderer without exposing damage bookkeeping as public API in normal builds.
#[cfg(feature = "bench-internals")]
#[doc(hidden)]
pub struct DamageTilesBenchmark(DamageTiles);

#[cfg(feature = "bench-internals")]
impl DamageTilesBenchmark {
    pub fn new(size: (u32, u32)) -> Self {
        Self(DamageTiles::new(size))
    }

    pub fn add_bounds(&mut self, bounds: Bounds) {
        self.0.add_bounds(bounds);
    }

    pub fn len(&self) -> u32 {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn list(&self) -> &[u32] {
        self.0.list()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn coalesced_rects_merge_adjacent_dirty_tile_rows() {
        let mut damage = DamageTiles::new((50, 20));
        damage.add_bounds(Bounds::new(15, 0, 34, 17));
        assert_eq!(
            damage.coalesced_rects((50, 20)),
            vec![Bounds::new(0, 0, 48, 20)]
        );
        assert_eq!(damage.count_in_bounds(Bounds::new(16, 0, 32, 16)), 1);
    }

    #[test]
    fn large_damage_crossing_words_keeps_exact_row_major_tiles() {
        let mut damage = DamageTiles::new((80 * TILE_SIZE, 2 * TILE_SIZE));
        damage.add_bounds(Bounds::new(
            4 * TILE_SIZE as i32,
            0,
            76 * TILE_SIZE as i32,
            2 * TILE_SIZE as i32,
        ));

        let expected = (0..2)
            .flat_map(|y| (4..76).map(move |x| y * 80 + x))
            .collect::<Vec<_>>();
        assert_eq!(damage.list(), expected);
        assert_eq!(damage.len(), 144);
    }

    #[test]
    fn overlapping_large_damage_appends_only_new_tiles() {
        let mut damage = DamageTiles::new((80 * TILE_SIZE, 4 * TILE_SIZE));
        damage.add_bounds(Bounds::new(0, 0, 64 * TILE_SIZE as i32, TILE_SIZE as i32));
        damage.add_bounds(Bounds::new(
            16 * TILE_SIZE as i32,
            0,
            80 * TILE_SIZE as i32,
            TILE_SIZE as i32,
        ));

        assert_eq!(damage.list(), (0..80).collect::<Vec<_>>());
        assert_eq!(damage.len(), 80);
    }

    #[test]
    fn narrow_and_large_damage_share_one_deduplicated_worklist() {
        let mut damage = DamageTiles::new((80 * TILE_SIZE, 2 * TILE_SIZE));
        damage.add_bounds(Bounds::new(
            70 * TILE_SIZE as i32,
            TILE_SIZE as i32,
            71 * TILE_SIZE as i32,
            2 * TILE_SIZE as i32,
        ));
        damage.add_bounds(Bounds::new(
            0,
            TILE_SIZE as i32,
            80 * TILE_SIZE as i32,
            2 * TILE_SIZE as i32,
        ));

        let expected = std::iter::once(150)
            .chain((80..160).filter(|tile| *tile != 150))
            .collect::<Vec<_>>();
        assert_eq!(damage.list(), expected);
        assert_eq!(damage.len(), 80);
    }

    #[test]
    fn optimized_damage_matches_per_tile_reference_for_clipped_and_unaligned_bounds() {
        let size = (83 * TILE_SIZE + 7, 70 * TILE_SIZE + 3);
        let tiles_width = size.0.div_ceil(TILE_SIZE);
        let tiles_height = size.1.div_ceil(TILE_SIZE);
        let canvas = Bounds::canvas(tiles_width * TILE_SIZE, tiles_height * TILE_SIZE);
        let bounds = [
            Bounds::new(10, 10, 10, 40),
            Bounds::new(
                -300,
                -80,
                20 * TILE_SIZE as i32 + 3,
                40 * TILE_SIZE as i32 + 5,
            ),
            Bounds::new(
                57 * TILE_SIZE as i32 + 9,
                11,
                82 * TILE_SIZE as i32 + 2,
                19 * TILE_SIZE as i32 + 7,
            ),
            Bounds::new(
                5 * TILE_SIZE as i32,
                21 * TILE_SIZE as i32,
                6 * TILE_SIZE as i32,
                22 * TILE_SIZE as i32,
            ),
            Bounds::new(-1000, -1000, -1, -1),
            Bounds::new(-100, -100, size.0 as i32 + 100, size.1 as i32 + 100),
        ];
        let mut damage = DamageTiles::new(size);
        let mut expected = Vec::new();
        let mut seen = HashSet::new();

        for bounds in bounds {
            damage.add_bounds(bounds);
            let bounds = bounds.intersect(canvas);
            if !bounds.is_empty() {
                let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
                let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
                let x1 = (bounds.x1.max(0) as u32)
                    .div_ceil(TILE_SIZE)
                    .min(tiles_width);
                let y1 = (bounds.y1.max(0) as u32)
                    .div_ceil(TILE_SIZE)
                    .min(tiles_height);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let tile = y * tiles_width + x;
                        if seen.insert(tile) {
                            expected.push(tile);
                        }
                    }
                }
            }
            assert_eq!(damage.list(), expected);
            assert_eq!(damage.len() as usize, seen.len());
        }
    }

    #[test]
    fn coalesced_rects_keep_different_row_runs_separate() {
        let mut damage = DamageTiles::new((64, 48));
        damage.add_bounds(Bounds::new(0, 0, 32, 32));
        damage.add_bounds(Bounds::new(32, 16, 48, 48));

        assert_eq!(
            damage.coalesced_rects((64, 48)),
            vec![
                Bounds::new(0, 0, 32, 16),
                Bounds::new(0, 16, 48, 32),
                Bounds::new(32, 32, 48, 48),
            ]
        );
    }

    #[test]
    fn outset_includes_filter_source_tiles_beyond_visible_output() {
        let mut output = DamageTiles::new((328, 200));
        output.add_bounds(Bounds::new(112, 112, 256, 176));

        let source = output.outset((328, 200), 24);

        assert!(source.intersects_bounds(Bounds::new(112, 176, 256, 200)));
        assert_eq!(source.coalesced_rects((328, 200)).last().unwrap().y1, 200);
    }
}
