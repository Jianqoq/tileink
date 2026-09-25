use super::dispatch_2d;

#[test]
fn two_dimensional_dispatch_covers_each_linear_workgroup_in_order() {
    for limit in [1, 2, 7, 16, 31] {
        for count in 1..=limit * limit {
            let (width, height) = dispatch_2d(count, limit);
            assert!(width <= limit && height <= limit);
            let ids: Vec<_> = (0..height)
                .flat_map(|y| (0..width).map(move |x| y * width + x))
                .filter(|&id| id < count)
                .collect();
            assert_eq!(ids, (0..count).collect::<Vec<_>>());
            assert!(width * height - count < width);
        }
    }
}

#[test]
fn maximum_u32_workload_uses_two_dimensions_without_overflow() {
    assert_eq!(dispatch_2d(u32::MAX, 65_536), (65_536, 65_536));
    assert_eq!(dispatch_2d(65_535 * 65_535, 65_535), (65_535, 65_535));
}

#[test]
#[should_panic]
fn empty_dispatch_is_rejected() {
    dispatch_2d(0, 1);
}

#[test]
#[should_panic]
fn zero_device_dimension_is_rejected() {
    dispatch_2d(1, 0);
}

#[test]
#[should_panic(expected = "2D workgroup capacity")]
fn workload_beyond_two_device_dimensions_is_rejected() {
    dispatch_2d(10, 3);
}

#[test]
fn oversized_linear_dispatch_is_split_across_two_dimensions() {
    assert_eq!(dispatch_2d(96_000, 65_535), (65_535, 2));
    assert_eq!(dispatch_2d(65_535, 65_535), (65_535, 1));
}

#[test]
fn dispatch_tail_cannot_wrap_a_u32_linear_workgroup_index() {
    for limit in [
        65_537,
        65_538,
        70_000,
        100_000,
        2_000_000_000,
        u32::MAX - 1,
        u32::MAX,
    ] {
        let (width, height) = dispatch_2d(u32::MAX, limit);
        assert!(width <= limit && height <= limit);
        let dispatched = u64::from(width) * u64::from(height);
        assert!(dispatched >= u64::from(u32::MAX));
        assert!(
            dispatched <= u64::from(u32::MAX) + 1,
            "limit={limit}: the final workgroups would wrap to the start of the output"
        );
    }
}

#[test]
fn large_supported_single_dimension_dispatch_remains_single_dimension() {
    assert_eq!(dispatch_2d(70_000, 100_000), (70_000, 1));
}
