#![cfg(feature = "bench-internals")]

// Exercise the public benchmark helpers in a dependency build: unit tests alone
// enable cfg(test) and hide missing native bench-internals feature gates.
#[test]
fn dirty_range_helpers_recycle_across_empty_and_nonempty_frames() {
    let mut benchmark = tileink::GpuDirtyRangesBenchmark::new();
    for count in [0, 1, 32, 0, 512, 1] {
        assert_eq!(benchmark.path_plan_cycle(count), count * 4);
        assert_eq!(benchmark.tile_bin_cycle(count), count * 2);
    }
}
