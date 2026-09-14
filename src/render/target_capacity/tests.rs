use super::{grown_dimension, resized_capacity};

#[test]
fn reuse_and_shrink_do_not_query_device_limits() {
    assert_eq!(
        resized_capacity((1600, 1000), (1500, 900), || panic!(
            "reuse must not query the device"
        )),
        None
    );
    assert_eq!(
        resized_capacity((4096, 2160), (1280, 720), || panic!(
            "shrink must not query the device"
        )),
        Some((1280, 720))
    );
}

#[test]
fn growth_queries_limits_once_for_both_dimensions() {
    let queries = std::cell::Cell::new(0);
    assert_eq!(
        resized_capacity((100, 80), (101, 81), || {
            queries.set(queries.get() + 1);
            4096
        }),
        Some((150, 120))
    );
    assert_eq!(queries.get(), 1);
}

#[test]
fn geometric_growth_saturates_before_applying_the_limit() {
    assert_eq!(
        grown_dimension(u32::MAX - 100, u32::MAX - 50, u32::MAX),
        u32::MAX
    );
}

#[test]
fn target_capacity_grows_geometrically() {
    assert_eq!(grown_dimension(100, 101, 4096), 150);
    assert_eq!(grown_dimension(100, 240, 4096), 240);
}

#[test]
fn target_capacity_does_not_grow_past_device_limit() {
    assert_eq!(grown_dimension(3000, 3500, 4096), 4096);
}

#[test]
fn target_capacity_reuses_interactive_shrink_range() {
    assert_eq!(resized_capacity((1600, 1000), (1472, 928), || 4096), None);
    assert_eq!(resized_capacity((256, 192), (128, 96), || 4096), None);
}

#[test]
fn target_capacity_releases_disproportionate_allocations() {
    assert_eq!(
        resized_capacity((4096, 2160), (1280, 720), || 8192),
        Some((1280, 720))
    );
}
