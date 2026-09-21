// The backend adapter consumes text state; these CPU fixture tests only exercise
// scene transactions and image data, so its private state is intentionally unused.
#[allow(dead_code)]
#[path = "../benches/support/backend_workload.rs"]
mod workload;

#[test]
fn image_workload_has_384_distinct_contents() {
    let mut images = std::collections::HashSet::new();
    for index in 0..384 {
        assert!(images.insert(workload::image(index).pixels));
    }
}

#[test]
fn every_workload_completes_the_same_phase_cycle() {
    for name in workload::CASES {
        let mut workload = workload::Workload::new(name);
        let initial = workload.size();
        for _ in 0..16 {
            workload.advance();
            let [width, height] = workload.size();
            assert!((1216..=1280).contains(&width));
            assert!((760..=800).contains(&height));
        }
        assert_eq!(workload.size(), initial);
    }
}

#[test]
fn replacement_images_are_unique_across_resources_and_revisions() {
    let mut images = std::collections::HashSet::new();
    for revision in [0, 1, 15, 16, 17, 255, 256, 1 << 24, 1 << 48, u64::MAX] {
        for index in 0..384 {
            let image = workload::replacement_image(index, revision);
            assert!(image.pixels.iter().all(|pixel| pixel >> 24 == 255));
            assert!(
                images.insert(image.pixels),
                "duplicate image {index}/{revision}"
            );
        }
    }
}
