use crate::{
    Bounds, TILE_SIZE,
    shared::bounds::{PixelBounds, TileBbox},
};

#[derive(Clone, Copy)]
struct RowRun {
    x0: u32,
    x1: u32,
    rect: usize,
}

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
        damage.bits.fill(u64::MAX);
        if let (Some(last), remainder) = (damage.bits.last_mut(), count % 64)
            && remainder != 0
        {
            *last = low_mask(remainder);
        }
        damage
    }

    pub(crate) fn add_bounds(&mut self, bounds: Bounds) {
        // Keep the scalar conversion in this hot path. Returning a TileBbox from the shared query
        // helper measurably regresses workloads that add thousands of tiny bounds.
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

    fn tile_rect(&self, bounds: Bounds) -> Option<TileBbox> {
        if bounds.is_empty() {
            return None;
        }
        let rect = PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
        .tile_bbox(self.tiles_width, self.tiles_height);
        (rect.x0 < rect.x1 && rect.y0 < rect.y1).then_some(rect)
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
            let mask = low_mask(width) << bit;
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

    #[inline(always)]
    fn contains(&self, tile: u32) -> bool {
        self.bits
            .get(tile as usize / 64)
            .is_some_and(|word| *word & (1 << (tile % 64)) != 0)
    }

    #[inline(always)]
    pub(crate) fn intersects_bounds(&self, bounds: Bounds) -> bool {
        if bounds.is_empty() {
            return false;
        }
        // Full-canvas and top-left queries are frequent during retained fallback. Avoid all tile
        // coordinate conversion when their first possible tile is already dirty.
        if bounds.x0 == 0 && bounds.y0 == 0 && self.contains(0) {
            return true;
        }
        let x0 = bounds.x0.max(0) as u32 / TILE_SIZE;
        let y0 = bounds.y0.max(0) as u32 / TILE_SIZE;
        let x1 = (bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_width);
        let y1 = (bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(self.tiles_height);
        if x0 >= x1 || y0 >= y1 {
            return false;
        }
        let first = y0 * self.tiles_width + x0;
        if self.contains(first) {
            return true;
        }
        (y0..y1).any(|y| {
            self.intersects_tile_range(y * self.tiles_width + x0, y * self.tiles_width + x1)
        })
    }

    pub(crate) fn intersects_tile_rect(&self, x0: u32, y0: u32, x1: u32, y1: u32) -> bool {
        let rect = TileBbox {
            x0: x0.min(self.tiles_width),
            y0: y0.min(self.tiles_height),
            x1: x1.min(self.tiles_width),
            y1: y1.min(self.tiles_height),
        };
        rect.x0 < rect.x1 && rect.y0 < rect.y1 && self.intersects_rect(rect)
    }

    pub(crate) fn count_in_bounds(&self, bounds: Bounds) -> u32 {
        let Some(rect) = self.tile_rect(bounds) else {
            return 0;
        };
        (rect.y0..rect.y1)
            .map(|y| {
                self.count_tile_range(
                    y * self.tiles_width + rect.x0,
                    y * self.tiles_width + rect.x1,
                )
            })
            .sum()
    }

    fn intersects_rect(&self, rect: TileBbox) -> bool {
        // Preserve the single-bit fast path for the common case where damage starts at the query
        // origin; the word scan below is valuable only after that immediate check misses.
        let first = rect.y0 * self.tiles_width + rect.x0;
        if self.contains(first) {
            return true;
        }
        (rect.y0..rect.y1).any(|y| {
            self.intersects_tile_range(
                y * self.tiles_width + rect.x0,
                y * self.tiles_width + rect.x1,
            )
        })
    }

    fn intersects_tile_range(&self, mut start: u32, end: u32) -> bool {
        while start < end {
            let word_index = start as usize / 64;
            let bit = start % 64;
            let width = (64 - bit).min(end - start);
            if self.bits[word_index] & (low_mask(width) << bit) != 0 {
                return true;
            }
            start += width;
        }
        false
    }

    fn count_tile_range(&self, mut start: u32, end: u32) -> u32 {
        let mut count = 0;
        while start < end {
            let word_index = start as usize / 64;
            let bit = start % 64;
            let width = (64 - bit).min(end - start);
            count += (self.bits[word_index] & (low_mask(width) << bit)).count_ones();
            start += width;
        }
        count
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

    /// Returns the tile-aligned union by scanning packed rows without materializing rectangles.
    /// Keeping this calculation off the insertion path is important because bounds are often
    /// added thousands of times but their union is consumed only once.
    pub(crate) fn bounds_union(&self, physical_size: (u32, u32)) -> Option<Bounds> {
        let mut x0 = self.tiles_width;
        let mut y0 = self.tiles_height;
        let mut x1 = 0;
        let mut y1 = 0;
        for y in 0..self.tiles_height {
            if let Some((row_x0, row_x1)) = self.row_extent(y) {
                x0 = x0.min(row_x0);
                y0 = y0.min(y);
                x1 = x1.max(row_x1);
                y1 = y + 1;
            }
        }
        (x0 < x1 && y0 < y1).then(|| {
            Bounds::new(
                (x0 * TILE_SIZE) as i32,
                (y0 * TILE_SIZE) as i32,
                (x1 * TILE_SIZE).min(physical_size.0) as i32,
                (y1 * TILE_SIZE).min(physical_size.1) as i32,
            )
        })
    }

    fn row_extent(&self, y: u32) -> Option<(u32, u32)> {
        let row_start = y * self.tiles_width;
        let row_end = row_start + self.tiles_width;
        let mut start = row_start;
        let mut x0 = self.tiles_width;
        let mut x1 = 0;
        while start < row_end {
            let word_index = start as usize / 64;
            let bit = start % 64;
            let width = (64 - bit).min(row_end - start);
            let bits = (self.bits[word_index] >> bit) & low_mask(width);
            if bits != 0 {
                let segment_x = start - row_start;
                x0 = x0.min(segment_x + bits.trailing_zeros());
                x1 = x1.max(segment_x + (u64::BITS - bits.leading_zeros()).min(width));
            }
            start += width;
        }
        (x0 < x1).then_some((x0, x1))
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
        let mut previous_row = Vec::<RowRun>::new();
        let mut current_row = Vec::<RowRun>::new();
        let mut tile_runs = Vec::<(u32, u32)>::new();
        for y in 0..self.tiles_height {
            tile_runs.clear();
            self.row_runs(y, &mut tile_runs);
            current_row.clear();
            let mut previous = 0;
            for &(x0, x1) in &tile_runs {
                while previous < previous_row.len() && previous_row[previous].x0 < x0 {
                    previous += 1;
                }
                let y1 = ((y + 1) * TILE_SIZE).min(physical_size.1) as i32;
                let rect = if previous_row
                    .get(previous)
                    .is_some_and(|run| run.x0 == x0 && run.x1 == x1)
                {
                    let rect = previous_row[previous].rect;
                    rects[rect].y1 = y1;
                    rect
                } else {
                    let rect = rects.len();
                    rects.push(Bounds::new(
                        (x0 * TILE_SIZE) as i32,
                        (y * TILE_SIZE) as i32,
                        (x1 * TILE_SIZE).min(physical_size.0) as i32,
                        y1,
                    ));
                    rect
                };
                current_row.push(RowRun { x0, x1, rect });
            }
            std::mem::swap(&mut previous_row, &mut current_row);
        }
        rects
    }

    /// Extracts contiguous dirty runs directly from packed words. Runs spanning a word boundary
    /// remain merged, which preserves the rectangle decomposition of the former per-tile scan.
    fn row_runs(&self, y: u32, runs: &mut Vec<(u32, u32)>) {
        let row_start = y * self.tiles_width;
        let row_end = row_start + self.tiles_width;
        let mut start = row_start;
        let mut open_run = None;
        while start < row_end {
            let word_index = start as usize / 64;
            let bit = start % 64;
            let width = (64 - bit).min(row_end - start);
            let mut bits = (self.bits[word_index] >> bit) & low_mask(width);
            let segment_x = start - row_start;
            let mut offset = 0;
            while offset < width {
                if bits & 1 == 0 {
                    if let Some(x0) = open_run.take() {
                        runs.push((x0, segment_x + offset));
                    }
                    let zeros = bits.trailing_zeros().min(width - offset);
                    offset += zeros;
                    bits = checked_shr(bits, zeros);
                } else {
                    open_run.get_or_insert(segment_x + offset);
                    let ones = bits.trailing_ones().min(width - offset);
                    offset += ones;
                    bits = checked_shr(bits, ones);
                }
            }
            start += width;
        }
        if let Some(x0) = open_run {
            runs.push((x0, self.tiles_width));
        }
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

    pub fn full(size: (u32, u32)) -> Self {
        Self(DamageTiles::full(size))
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

    pub fn intersects_bounds(&self, bounds: Bounds) -> bool {
        self.0.intersects_bounds(bounds)
    }

    pub fn count_in_bounds(&self, bounds: Bounds) -> u32 {
        self.0.count_in_bounds(bounds)
    }

    pub fn coalesced_rects(&self, physical_size: (u32, u32)) -> Vec<Bounds> {
        self.0.coalesced_rects(physical_size)
    }

    pub fn bounds_union(&self, physical_size: (u32, u32)) -> Option<Bounds> {
        self.0.bounds_union(physical_size)
    }
}

#[inline(always)]
fn low_mask(width: u32) -> u64 {
    if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

#[inline(always)]
fn checked_shr(value: u64, amount: u32) -> u64 {
    if amount == 64 { 0 } else { value >> amount }
}

#[cfg(test)]
mod tests;
