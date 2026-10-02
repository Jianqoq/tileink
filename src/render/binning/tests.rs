use super::*;
use crate::Canvas;

#[test]
fn coarse_bins_cover_partial_edge_tiles() {
    for (width, height, expected) in [(1, 1, 1), (256, 256, 1), (257, 256, 2), (257, 257, 4)] {
        let lengths = GpuBufferLengths::from_scene(&Canvas::new(width, height, 1.0));
        assert_eq!(coarse_bin_count(lengths), expected);
    }
}

#[test]
fn dense_binning_replaces_many_mostly_idle_incremental_workgroups() {
    let canvas = Canvas::new(3200, 2000, 1.0);
    let lengths = GpuBufferLengths::from_scene(&canvas);
    let stats =
        |active_tiles, compact_candidate_rounds, dense_candidate_rounds| CoarseBinningStats {
            active_tiles,
            compact_candidate_rounds,
            dense_candidate_rounds,
        };

    assert!(!prefer_dense_binning(lengths, stats(128, 128, 104)));
    assert!(prefer_dense_binning(lengths, stats(4096, 4096, 104)));
    assert!(!prefer_dense_binning(
        lengths,
        stats(4096, 4096 * 2, 104 * 512)
    ));
    assert!(!prefer_dense_binning(lengths, stats(0, 0, 104)));

    let (compact, dense) = coarse_binning_costs(lengths, stats(4096, 4096, 104));
    assert!(dense < compact);
}

#[test]
fn dense_binning_accounts_for_the_shared_prefix_chain() {
    // Sharing offsets makes dense binning cheaper at this boundary; the former duplicated
    // prefix cost incorrectly keeps these moderately sparse tiles on the compact path.
    let lengths = GpuBufferLengths::from_scene(&Canvas::new(3200, 2000, 1.0));
    assert!(prefer_dense_binning(
        lengths,
        CoarseBinningStats {
            active_tiles: 160,
            compact_candidate_rounds: 160,
            dense_candidate_rounds: 104,
        }
    ));
}
