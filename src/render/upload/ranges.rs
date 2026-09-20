pub(crate) fn replace_or_push<T>(values: &mut Vec<T>, index: usize, value: T) {
    if index < values.len() {
        values[index] = value;
    } else {
        debug_assert_eq!(index, values.len());
        values.push(value);
    }
}

pub(crate) fn push_dirty_index(ranges: &mut Vec<std::ops::Range<usize>>, index: usize) {
    if let Some(last) = ranges.last_mut()
        && last.end == index
    {
        last.end += 1;
    } else {
        ranges.push(index..index + 1);
    }
}

pub(crate) fn indices_from_ranges(
    ranges: &[std::ops::Range<usize>],
    len: usize,
) -> impl Iterator<Item = usize> + '_ {
    debug_assert!(ranges.windows(2).all(|pair| pair[0].end < pair[1].start));
    ranges
        .iter()
        .flat_map(move |range| range.start.min(len)..range.end.min(len))
}

pub(crate) fn changed_ranges(
    ranges: Option<&[std::ops::Range<usize>]>,
    old_len: usize,
    new_len: usize,
) -> Vec<std::ops::Range<usize>> {
    let Some(ranges) = ranges else {
        return (!new_len.eq(&0))
            .then_some(0..new_len)
            .into_iter()
            .collect();
    };
    let result = ranges
        .iter()
        .map(|range| range.start.min(new_len)..range.end.min(new_len))
        .filter(|range| !range.is_empty())
        .collect::<Vec<_>>();
    if new_len > old_len {
        merge_sorted_dirty_ranges(&result, std::slice::from_ref(&(old_len..new_len)))
    } else {
        result
    }
}

pub(crate) fn patch_pod_ranges<T: bytemuck::Pod>(
    blob: &mut [u32],
    word_base: usize,
    values: &[T],
    ranges: &[std::ops::Range<usize>],
    dirty: &mut Vec<std::ops::Range<usize>>,
) {
    let words_per_item = std::mem::size_of::<T>() / 4;
    let words: &[u32] = bytemuck::cast_slice(values);
    for range in ranges {
        let source = range.start * words_per_item..range.end * words_per_item;
        let target = source.start + word_base..source.end + word_base;
        blob[target.clone()].copy_from_slice(&words[source]);
        dirty.push(target);
    }
}

pub(crate) fn patch_u32_ranges(
    target: &mut [u32],
    base: usize,
    source: &[u32],
    ranges: &[std::ops::Range<usize>],
) {
    for range in ranges {
        target[range.start + base..range.end + base].copy_from_slice(&source[range.clone()]);
    }
}

#[cfg(any(feature = "wgpu", test))]
pub(crate) fn contiguous_index_runs(
    indices: impl IntoIterator<Item = usize>,
) -> Vec<std::ops::Range<usize>> {
    let mut indices = indices.into_iter();
    let Some(first) = indices.next() else {
        return Vec::new();
    };
    let mut runs = Vec::new();
    let (mut start, mut end) = (first, first + 1);
    for index in indices {
        if index == end {
            end += 1;
        } else {
            runs.push(start..end);
            start = index;
            end = index + 1;
        }
    }
    runs.push(start..end);
    runs
}

pub(crate) fn merge_sorted_dirty_ranges(
    left: &[std::ops::Range<usize>],
    right: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    let mut merged = Vec::<std::ops::Range<usize>>::with_capacity(left.len() + right.len());
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() || right_index < right.len() {
        let range = if right_index == right.len()
            || (left_index < left.len() && left[left_index].start <= right[right_index].start)
        {
            let range = left[left_index].clone();
            left_index += 1;
            range
        } else {
            let range = right[right_index].clone();
            right_index += 1;
            range
        };
        if range.is_empty() {
            continue;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_change_ranges_are_traversed_without_materializing_indices() {
        assert_eq!(
            indices_from_ranges(&[1..3, 5..10], 7).collect::<Vec<_>>(),
            [1, 2, 5, 6]
        );
    }

    #[test]
    fn contiguous_dirty_indices_are_coalesced_without_bridging_gaps() {
        assert_eq!(
            contiguous_index_runs([2, 3, 4, 8, 10, 11]),
            [2..5, 8..9, 10..12]
        );
        assert!(contiguous_index_runs([]).is_empty());
    }

    #[test]
    fn sorted_draw_and_painter_ranges_merge_linearly() {
        assert_eq!(
            merge_sorted_dirty_ranges(&[0..4, 12..16], &[3..8, 20..24]),
            [0..8, 12..16, 20..24]
        );
    }

    #[test]
    fn growing_text_ranges_merge_the_dirty_tail_upstream() {
        let dirty = std::iter::once(2..6).collect::<Vec<_>>();
        let expected = std::iter::once(2..8).collect::<Vec<_>>();
        assert_eq!(changed_ranges(Some(&dirty), 4, 8), expected);
    }
}
