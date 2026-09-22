use super::Result;

/// Pack complete pass tables into bounded heap pairs. This removes the artificial
/// per-frame limit without splitting a table or overwriting descriptors in flight.
pub(super) fn plan(
    counts: impl IntoIterator<Item = Result<[usize; 2]>>,
    limits: [usize; 2],
) -> Result<Vec<(usize, [usize; 2])>> {
    let mut pages = Vec::new();
    let mut used = [0; 2];
    let mut end = 0;
    for counts in counts {
        let counts = counts?;
        if (0..2).any(|index| counts[index] > limits[index]) {
            return Err("native DX12 pass descriptor table exceeds heap capacity".into());
        }
        if (0..2).any(|index| counts[index] > limits[index] - used[index]) {
            pages.push((end, used));
            used = [0; 2];
        }
        for index in 0..2 {
            used[index] += counts[index];
        }
        end += 1;
    }
    if end != 0 {
        pages.push((end, used));
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_exact_limits_and_splits_before_overflow_without_splitting_tables() {
        let pages = plan([[3, 1], [5, 1], [1, 0], [0, 2], [8, 0]].map(Ok), [8, 2]).unwrap();
        assert_eq!(pages, vec![(2, [8, 2]), (4, [1, 2]), (5, [8, 0])]);
    }

    #[test]
    fn empty_passes_preserve_command_indices_and_oversized_tables_are_rejected() {
        assert!(plan([], [8, 2]).unwrap().is_empty());
        assert_eq!(
            plan([[0, 0], [8, 2], [0, 0], [1, 1]].map(Ok), [8, 2]).unwrap(),
            vec![(3, [8, 2]), (4, [1, 1])]
        );
        assert!(plan([Ok([9, 0])], [8, 2]).is_err());
        assert!(plan([Ok([0, 3])], [8, 2]).is_err());
    }
}
