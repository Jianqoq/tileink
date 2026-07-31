//! Reusable dense-integer membership scratch without per-update hashing or allocation.

#[derive(Clone, Debug, Default)]
pub(crate) struct GenerationMarks {
    marks: Vec<u32>,
    generation: u32,
}

impl GenerationMarks {
    pub(crate) fn begin(&mut self, len: usize) {
        self.marks.resize(len, 0);
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.marks.fill(0);
            self.generation = 1;
        }
    }

    #[inline]
    pub(crate) fn insert(&mut self, index: usize) -> bool {
        let mark = &mut self.marks[index];
        if *mark == self.generation {
            return false;
        }
        *mark = self.generation;
        true
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DenseIndexSet {
    bits: Vec<u64>,
    indices: Vec<usize>,
}

impl DenseIndexSet {
    pub(crate) fn begin(&mut self, len: usize) {
        for &index in &self.indices {
            self.bits[index / 64] &= !(1 << (index % 64));
        }
        self.indices.clear();
        self.bits.resize(len.div_ceil(64), 0);
    }

    #[inline]
    pub(crate) fn insert(&mut self, index: usize) -> bool {
        let word = &mut self.bits[index / 64];
        let mask = 1 << (index % 64);
        if *word & mask != 0 {
            return false;
        }
        *word |= mask;
        self.indices.push(index);
        true
    }

    #[inline]
    pub(crate) fn contains(&self, index: usize) -> bool {
        self.bits[index / 64] & (1 << (index % 64)) != 0
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.indices.len()
    }

    #[inline]
    pub(crate) fn get(&self, index: usize) -> usize {
        self.indices[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_marks_clear_logically_and_survive_wraparound() {
        let mut marks = GenerationMarks::default();
        marks.begin(4);
        assert!(marks.insert(2));
        assert!(!marks.insert(2));

        marks.begin(4);
        assert!(marks.insert(2));
        marks.generation = u32::MAX;
        marks.begin(4);
        assert!(marks.insert(2));
        assert!(!marks.insert(2));
    }

    #[test]
    fn dense_index_set_retains_capacity_without_retaining_membership() {
        let mut set = DenseIndexSet::default();
        set.begin(130);
        assert!(set.insert(0));
        assert!(set.insert(65));
        assert!(!set.insert(65));
        assert!(set.contains(0));
        assert!(set.contains(65));
        assert_eq!([set.get(0), set.get(1)], [0, 65]);

        set.begin(66);
        assert!(set.is_empty());
        assert!(!set.contains(0));
        assert!(!set.contains(65));
    }
}
