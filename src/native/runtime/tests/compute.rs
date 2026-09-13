use super::*;
#[test]
fn compute_buffers_are_owned_and_readbacks_are_deduplicated() {
    let mut a = ComputeBatch::new();
    let mut b = ComputeBatch::new();
    let id = a.buffer(vec![7; 4]).unwrap();
    let foreign = b.buffer(vec![7; 4]).unwrap();
    assert!(a.size(foreign).is_err());
    assert!(a.readback(foreign).is_err());
    assert_eq!(a.readback(id).unwrap(), 0);
    assert_eq!(a.readback(id).unwrap(), 0);
    assert_eq!(a.outputs().len(), 1);
    assert!(a.buffer(vec![]).is_err());
    assert!(a.buffer(vec![0; 3]).is_err());
}
#[test]
fn compute_launch_rejects_layout_aliases_and_oversized_grids() {
    let mut batch = ComputeBatch::new();
    let mut other = ComputeBatch::new();
    let config = batch.buffer(vec![0; 16]).unwrap();
    let data = batch.buffer(vec![0; 4]).unwrap();
    let write = batch.buffer(vec![0; 4]).unwrap();
    let total = batch.buffer(vec![0; 4]).unwrap();
    let foreign = other.buffer(vec![0; 4]).unwrap();
    let valid = [(0, config), (1, data), (2, data), (5, write), (6, total)];
    unsafe {
        assert!(batch.dispatch("missing", &valid, [1, 1, 1]).is_err());
        assert!(
            batch
                .dispatch("cumsum_prefix_chunks", &valid, [65536, 1, 1])
                .is_err()
        );
        assert!(
            batch
                .dispatch("cumsum_prefix_chunks", &valid[..4], [1, 1, 1])
                .is_err()
        );
        for (slot, id) in [(6, write), (6, foreign), (0, data)] {
            let mut bad = valid;
            let index = bad
                .iter()
                .position(|(binding, _)| *binding == slot)
                .unwrap();
            bad[index].1 = id;
            assert!(
                batch
                    .dispatch("cumsum_prefix_chunks", &bad, [1, 1, 1])
                    .is_err()
            );
        }
        assert!(
            batch
                .dispatch("cumsum_prefix_chunks", &valid, [0, 1, 1])
                .is_ok()
        );
    }
    assert!(batch.passes().is_empty());
}
