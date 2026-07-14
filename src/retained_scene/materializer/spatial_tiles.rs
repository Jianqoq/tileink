use std::iter::FusedIterator;

use crate::{Bounds, TILE_SIZE};

/// Allocation-free row-major traversal of the canvas tiles touched by pixel bounds.
///
/// Bounds are clipped to the tile grid exactly once at construction. Keeping traversal state in
/// coordinates lets spatial-index callers consume tiles directly instead of allocating and
/// immediately discarding a temporary `Vec` for every bounds query.
pub(crate) struct BoundsTileIter {
    width: u32,
    x0: u32,
    x1: u32,
    x: u32,
    y: u32,
    y1: u32,
}

impl BoundsTileIter {
    pub(super) fn new(bounds: Bounds, width: u32, height: u32) -> Self {
        let x0 = (bounds.x0.max(0) as u32 / TILE_SIZE).min(width);
        let y0 = (bounds.y0.max(0) as u32 / TILE_SIZE).min(height);
        let x1 = (bounds.x1.max(0) as u32).div_ceil(TILE_SIZE).min(width);
        let y1 = (bounds.y1.max(0) as u32).div_ceil(TILE_SIZE).min(height);
        Self {
            width,
            x0,
            x1,
            x: x0,
            y: if !bounds.is_empty() && x0 < x1 && y0 < y1 {
                y0
            } else {
                y1
            },
            y1,
        }
    }

    fn remaining(&self) -> usize {
        if self.y >= self.y1 {
            return 0;
        }
        let row_width = (self.x1 - self.x0) as usize;
        (self.y1 - self.y - 1) as usize * row_width + (self.x1 - self.x) as usize
    }
}

impl Iterator for BoundsTileIter {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.y >= self.y1 {
            return None;
        }
        let tile = (self.y * self.width + self.x) as usize;
        self.x += 1;
        if self.x == self.x1 {
            self.x = self.x0;
            self.y += 1;
        }
        Some(tile)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.remaining();
        (remaining, Some(remaining))
    }

    fn fold<B, F>(self, init: B, mut fold: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        let mut value = init;
        let mut x = self.x;
        for y in self.y..self.y1 {
            while x < self.x1 {
                value = fold(value, (y * self.width + x) as usize);
                x += 1;
            }
            x = self.x0;
        }
        value
    }
}

impl ExactSizeIterator for BoundsTileIter {}
impl FusedIterator for BoundsTileIter {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traverses_bounds_in_row_major_order() {
        let tiles = BoundsTileIter::new(Bounds::new(16, 16, 48, 48), 4, 4).collect::<Vec<_>>();
        assert_eq!(tiles, [5, 6, 9, 10]);
    }

    #[test]
    fn clips_negative_and_oversized_bounds_to_canvas() {
        let tiles = BoundsTileIter::new(Bounds::new(-20, -20, 80, 40), 3, 2).collect::<Vec<_>>();
        assert_eq!(tiles, [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn rejects_empty_and_fully_outside_bounds() {
        for bounds in [
            Bounds::new(8, 8, 8, 16),
            Bounds::new(16, 16, 8, 32),
            Bounds::new(-32, -32, -1, -1),
            Bounds::new(64, 64, 80, 80),
        ] {
            assert!(BoundsTileIter::new(bounds, 4, 4).next().is_none());
        }
    }

    #[test]
    fn exact_length_tracks_consumption() {
        let mut tiles = BoundsTileIter::new(Bounds::new(1, 1, 33, 33), 4, 4);
        assert_eq!(tiles.len(), 9);
        assert_eq!(tiles.next(), Some(0));
        assert_eq!(tiles.len(), 8);
        assert_eq!(tiles.count(), 8);
    }
}
