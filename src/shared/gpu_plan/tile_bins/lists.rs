//! Dense-keyed variable-length lists in one index arena. Exact full builds and size-class
//! reuse replace thousands of small allocations and padded 256-ID tile pages.
use std::ops::{Index, IndexMut, Range};

#[derive(Clone, Copy, Debug, Default)]
struct Span {
    start: u32,
    len: u32,
    capacity: u32,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PackedLists {
    pub(super) values: Vec<u32>,
    spans: Vec<Span>,
    free: Vec<Vec<u32>>,
    live_words: usize,
}

impl PackedLists {
    pub(super) fn clear(&mut self, lists: usize) {
        self.values.clear();
        self.spans.clear();
        self.spans.resize(lists, Span::default());
        for blocks in &mut self.free {
            blocks.clear();
        }
        self.live_words = 0;
    }

    pub(super) fn reset_counts(&mut self, counts: impl ExactSizeIterator<Item = usize>) {
        self.clear(counts.len());
        let mut words = 0;
        for (span, count) in self.spans.iter_mut().zip(counts) {
            *span = Span {
                start: words as u32,
                len: 0,
                capacity: count as u32,
            };
            words += count;
        }
        assert!(words < u32::MAX as usize);
        self.values.resize(words, 0);
    }

    pub(super) fn append_exact(&mut self, list: usize, values: &[u32]) {
        let start = self.values.len();
        assert!(start + values.len() < u32::MAX as usize);
        self.spans[list] = Span {
            start: start as u32,
            len: values.len() as u32,
            capacity: values.len() as u32,
        };
        self.values.extend_from_slice(values);
        self.live_words += values.len();
    }

    pub(super) const fn len(&self) -> usize {
        self.spans.len()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &[u32]> {
        self.spans
            .iter()
            .map(|span| &self.values[span.start as usize..(span.start + span.len) as usize])
    }

    pub(super) fn start(&self, list: usize) -> u32 {
        self.spans[list].start
    }

    pub(super) fn push(&mut self, list: usize, value: u32) {
        let len = self.spans[list].len;
        self.reserve_list(list, len as usize + 1);
        let span = &mut self.spans[list];
        self.values[(span.start + len) as usize] = value;
        span.len += 1;
        self.live_words += 1;
    }

    pub(super) fn retain(&mut self, list: usize, mut keep: impl FnMut(u32) -> bool) {
        let span = &mut self.spans[list];
        let mut len = 0;
        for i in 0..span.len {
            let value = self.values[(span.start + i) as usize];
            if keep(value) {
                self.values[(span.start + len) as usize] = value;
                len += 1;
            }
        }
        self.live_words -= (span.len - len) as usize;
        span.len = len;
    }

    /// Only initialized live IDs are uploaded. Capacity tails and freed holes are never read.
    pub(super) fn write(&mut self, list: usize, values: &[u32]) -> Range<usize> {
        self.reserve_list(list, values.len());
        let span = &mut self.spans[list];
        self.live_words = self.live_words - span.len as usize + values.len();
        span.len = values.len() as u32;
        let range = span.start as usize..span.start as usize + values.len();
        self.values[range.clone()].copy_from_slice(values);
        range
    }

    pub(super) fn release_empty(&mut self, list: usize) {
        let span = self.spans[list];
        if span.len == 0 && span.capacity != 0 {
            self.free_span(span);
            self.spans[list] = Span::default();
        }
    }

    fn reserve_list(&mut self, list: usize, needed: usize) {
        let old = self.spans[list];
        if needed <= old.capacity as usize {
            return;
        }
        assert!(needed <= 1usize << 31);
        let capacity = needed.next_power_of_two();
        let start = self.allocate(capacity);
        let len = old.len as usize;
        self.values
            .copy_within(old.start as usize..old.start as usize + len, start);
        self.free_span(old);
        self.spans[list] = Span {
            start: start as u32,
            len: old.len,
            capacity: capacity as u32,
        };
    }

    fn allocate(&mut self, capacity: usize) -> usize {
        let class = capacity.trailing_zeros() as usize;
        self.free
            .resize_with(self.free.len().max(class + 1), Vec::new);
        for larger in class..self.free.len() {
            if let Some(start) = self.free[larger].pop() {
                for split in (class..larger).rev() {
                    self.free[split].push(start + (1u32 << split));
                }
                return start as usize;
            }
        }
        let start = self.values.len();
        assert!(start + capacity < u32::MAX as usize);
        self.values.resize(start + capacity, 0);
        start
    }

    /// Full builds have exact, possibly non-power-of-two spans. Partition a released span
    /// into size-class blocks without imposing alignment or per-list allocation headers.
    fn free_span(&mut self, span: Span) {
        let (mut start, mut remaining) = (span.start, span.capacity);
        while remaining != 0 {
            let class = remaining.ilog2() as usize;
            self.free
                .resize_with(self.free.len().max(class + 1), Vec::new);
            self.free[class].push(start);
            let words = 1u32 << class;
            start += words;
            remaining -= words;
        }
    }

    pub(super) fn maybe_compact(&mut self) -> bool {
        if self.values.len() < 16
            || (self.values.len() - self.live_words) * 10 <= self.values.len() * 3
        {
            return false;
        }
        let mut values = Vec::with_capacity(self.live_words);
        for span in &mut self.spans {
            let start = values.len() as u32;
            values.extend_from_slice(
                &self.values[span.start as usize..(span.start + span.len) as usize],
            );
            *span = Span {
                start,
                capacity: span.len,
                ..*span
            };
        }
        self.values = values;
        for blocks in &mut self.free {
            blocks.clear();
        }
        true
    }
}

impl Index<usize> for PackedLists {
    type Output = [u32];
    fn index(&self, list: usize) -> &[u32] {
        let span = self.spans[list];
        &self.values[span.start as usize..(span.start + span.len) as usize]
    }
}

impl IndexMut<usize> for PackedLists {
    fn index_mut(&mut self, list: usize) -> &mut [u32] {
        let span = self.spans[list];
        &mut self.values[span.start as usize..(span.start + span.len) as usize]
    }
}

#[cfg(test)]
mod tests;
