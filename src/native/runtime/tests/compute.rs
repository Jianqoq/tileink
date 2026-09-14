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

#[test]
fn texture_resources_reject_invalid_extent_kind_owner_and_writable_alias() {
    let mut batch = ComputeBatch::new();
    for (size, bytes) in [
        ([0, 1], vec![]),
        ([1, 0], vec![]),
        ([2, 2], vec![0; 4]),
        ([u32::MAX, 2], vec![0; 4]),
    ] {
        assert!(batch.texture_rgba8(size, bytes).is_err());
    }
    let config = batch.buffer(vec![0; 16]).unwrap();
    let source = batch.texture_rgba8([2, 2], vec![0; 16]).unwrap();
    let target = batch.texture_rgba8([2, 2], vec![0; 16]).unwrap();
    let wrong = batch.buffer(vec![0; 16]).unwrap();
    // SAFETY: each call is rejected by resource validation before being recorded.
    unsafe {
        for bindings in [
            [(0, config), (1, wrong), (2, target)],
            [(0, source), (1, source), (2, target)],
            [(0, config), (1, source), (2, wrong)],
            [(0, config), (1, source), (2, source)],
        ] {
            assert!(
                batch
                    .dispatch("texture_flip", &bindings, [1, 1, 1])
                    .is_err()
            );
        }
    }
    let mut other = ComputeBatch::new();
    assert!(other.readback(source).is_err());
    assert!(batch.passes().is_empty());
}
#[test]
fn texture_array_dimensions_and_binding_views_are_explicit() {
    let mut batch = ComputeBatch::new();
    for size in [
        [1, 1, 0],
        [1, 1, 65536],
        [0, 1, 1],
        [u32::MAX, u32::MAX, u32::MAX],
    ] {
        assert!(batch.texture_array_rgba8(size, vec![]).is_err());
    }
    assert!(batch.texture_array_rgba8([1, 1, 2], vec![0; 4]).is_err());
    let array = batch.texture_array_rgba8([1, 1, 1], vec![0; 4]).unwrap();
    let plain = batch.texture_rgba8([1, 1], vec![0; 4]).unwrap();
    let target = batch.texture_rgba8([1, 1], vec![0; 4]).unwrap();
    let config = batch.buffer(vec![0; 16]).unwrap();
    // SAFETY: wrong view dimensions are rejected before any GPU work is recorded.
    unsafe {
        assert!(
            batch
                .dispatch(
                    "texture_flip",
                    &[(0, config), (1, array), (2, target)],
                    [1, 1, 1]
                )
                .is_err()
        );
        assert!(
            batch
                .dispatch(
                    "texture_layer",
                    &[(0, config), (1, plain), (2, target)],
                    [1, 1, 1]
                )
                .is_err()
        );
        assert!(
            batch
                .dispatch(
                    "texture_layer",
                    &[(0, config), (1, array), (2, array)],
                    [1, 1, 1]
                )
                .is_err()
        );
    }
    assert!(batch.passes().is_empty());
}
