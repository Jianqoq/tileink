#[path = "../src/native/runtime/program/scatter.rs"]
mod scatter;
use scatter::Scatter;

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[test]
fn accepts_empty_adjacent_and_multi_workgroup_ranges() {
    for (upload, size, count) in [
        (vec![4, 0, 0, 0], 4, 0),
        (vec![8, 1, 0, 0, 0, 0, 0, 0], 4, 1),
        (vec![12, 2, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 7, 9], 8, 2),
    ] {
        let command = Scatter::new(bytes(&upload), vec![0xa5; size]).unwrap();
        assert_eq!(command.workgroups(), count);
        assert_eq!(command.destination(), vec![0xa5; size]);
        assert_eq!(command.source(), bytes(&upload));
    }
    let mut upload = vec![8, 1, 0, 0, 1, 0, 513, 0];
    upload.extend(0..513);
    assert!(Scatter::new(bytes(&upload), vec![0; 515 * 4]).is_ok());
}

#[test]
fn rejects_malformed_headers_bounds_and_overlapping_writes() {
    for upload in [
        vec![],
        vec![4],
        vec![3, 0, 0, 0],
        vec![8, 1, 0, 0],
        vec![4, 1, 0, 0, 0, 0, 1, 0, 7],
        vec![8, 1, 0, 0, 0, 0, 2, 0, 7],
        vec![8, 1, 0, 0, 4, 0, 1, 0, 7],
        vec![8, 1, 0, 0, u32::MAX, 0, 2, 0, 7, 9],
        vec![8, 1, 0, 0, 0, u32::MAX, 2, 0, 7, 9],
        vec![12, 2, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 7, 9],
        vec![12, 2, 0, 0, 2, 0, 1, 0, 1, 1, 1, 0, 7, 9],
        vec![4, 65536, 0, 0],
    ] {
        assert!(
            Scatter::new(bytes(&upload), vec![0; 16]).is_err(),
            "{upload:?}"
        );
    }
    assert!(Scatter::new(bytes(&[4, 0, 0, 0]), vec![]).is_err());
    assert!(Scatter::new(bytes(&[4, 0, 0, 0]), vec![0; 3]).is_err());
    assert!(Scatter::new(vec![0; 17], vec![0; 4]).is_err());
}
