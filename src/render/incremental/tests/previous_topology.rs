//! A topology overlay records output changes that are absent from its shared base order.
use super::*;

fn ordered_overlay(base: &RetainedFrame, version: u64, bounds: Bounds) -> RetainedFrame {
    let mut previous = base.clone();
    previous.version = version;
    previous.delta = Some(Rc::new(RetainedFrameDelta {
        from_version: version - 1,
        to_version: version,
        patches: Rc::new([]),
        previous: None,
        depth: 1,
        damage: vec![(RetainedNodeId::for_owner(2), bounds)].into(),
        dirty_backdrops: Rc::new([]),
        backdrop_damage_complete: false,
        index: Rc::new(HashMap::default()),
    }));
    previous
}
#[test]
fn previous_topology_damage_survives_rebuilt_or_shared_base_order() {
    let bounds = Bounds::new(16, 0, 32, 16);
    for shared in [false, true] {
        let base = frame(&[(2, 0, bounds), (3, 0, bounds)]);
        // The installed plan reordered the two overlapping peers; the immutable base was
        // retained, with the changed output represented by the topology delta's damage.
        let previous = ordered_overlay(&base, 1, bounds);
        let mut current = if shared {
            base.clone()
        } else {
            frame(&[(2, 0, bounds), (3, 0, bounds)])
        };
        current.version = 2;
        let mut damage = DamageTiles::new(current.physical_size);
        let mut retained = RetainedDamage::default();
        diff_frames(&previous, &current, &mut damage, &mut retained);
        assert_eq!(
            damage.list(),
            &[1],
            "shared={shared}: reverting to the base order must repaint the previous overlay's output"
        );
        assert_eq!(
            retained.node_bounds.get(&RetainedNodeId::for_owner(2)),
            Some(&bounds)
        );
    }
}
#[test]
fn fallback_comparison_includes_topology_output_from_both_sides() {
    let first = Bounds::new(16, 0, 32, 16);
    let second = Bounds::new(48, 0, 64, 16);
    let base = frame(&[(2, 0, first), (3, 0, second)]);
    let previous = ordered_overlay(&base, 1, first);
    let current = ordered_overlay(&base, 3, second);
    let mut damage = DamageTiles::new(current.physical_size);
    diff_frames(
        &previous,
        &current,
        &mut damage,
        &mut RetainedDamage::default(),
    );
    assert_eq!(
        damage.list(),
        &[1, 3],
        "each overlay differs from its base, even when base node states match"
    );
}
#[test]
fn unchanged_topology_overlay_keeps_the_zero_damage_fast_path() {
    let base = frame(&[(2, 0, Bounds::new(16, 0, 32, 16))]);
    let previous = ordered_overlay(&base, 1, Bounds::new(16, 0, 32, 16));
    let mut state = IncrementalState::default();
    state.commit(Some(previous.clone()));
    let plan = state.plan(
        Some(previous),
        base.physical_size,
        IncrementalRenderConfig::default(),
        true,
    );
    assert_eq!(plan.stats.dirty_tiles, 0);
    assert!(!plan.stats.full_redraw);
}
