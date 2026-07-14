use std::{iter::FusedIterator, rc::Rc, slice};

/// Chunk-local painter order without per-draw storage for the common sequential case.
pub(super) enum LocalDrawOrder {
    Sequential(u32),
    Indexed(Rc<Vec<u32>>),
}

impl LocalDrawOrder {
    pub(super) fn from_compiled(draws: Rc<Vec<u32>>) -> Self {
        if draws
            .iter()
            .enumerate()
            .all(|(index, &draw)| index == draw as usize)
        {
            Self::Sequential(draws.len().try_into().expect("draw order fits u32"))
        } else {
            Self::Indexed(draws)
        }
    }

    pub(super) fn physical(&self, base: usize) -> PhysicalDraws<'_> {
        match self {
            Self::Sequential(end) => PhysicalDraws::Sequential {
                draws: 0..*end,
                base,
            },
            Self::Indexed(draws) => PhysicalDraws::Indexed {
                draws: draws.iter(),
                base,
            },
        }
    }
}

pub(super) enum PhysicalDraws<'a> {
    Sequential {
        draws: std::ops::Range<u32>,
        base: usize,
    },
    Indexed {
        draws: slice::Iter<'a, u32>,
        base: usize,
    },
}

impl Iterator for PhysicalDraws<'_> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Sequential { draws, base } => draws.next().map(|draw| *base + draw as usize),
            Self::Indexed { draws, base } => draws.next().map(|&draw| *base + draw as usize),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Sequential { draws, .. } => draws.size_hint(),
            Self::Indexed { draws, .. } => draws.size_hint(),
        }
    }

    fn fold<B, F>(self, init: B, mut fold: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        match self {
            Self::Sequential { draws, base } => {
                draws.fold(init, |value, draw| fold(value, base + draw as usize))
            }
            Self::Indexed { draws, base } => {
                draws.fold(init, |value, &draw| fold(value, base + draw as usize))
            }
        }
    }
}

impl ExactSizeIterator for PhysicalDraws<'_> {}
impl FusedIterator for PhysicalDraws<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_order_needs_only_its_length() {
        let order = LocalDrawOrder::from_compiled(Rc::new(vec![0, 1, 2, 3]));
        assert!(matches!(order, LocalDrawOrder::Sequential(4)));
        assert_eq!(order.physical(7).collect::<Vec<_>>(), [7, 8, 9, 10]);
    }

    #[test]
    fn indexed_order_preserves_compiled_painter_order() {
        let order = LocalDrawOrder::from_compiled(Rc::new(vec![2, 0, 3, 1]));
        assert_eq!(order.physical(10).collect::<Vec<_>>(), [12, 10, 13, 11]);
    }
}
