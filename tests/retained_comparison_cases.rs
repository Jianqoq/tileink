#[path = "../benches/support/retained_comparison_cases.rs"]
mod cases;

#[test]
fn inventory_includes_every_legacy_scale_ratio_and_stress_size() {
    let all = cases::Case::all();
    assert_eq!(all.len(), 25 * 5 + 26 + 30);
    let names: std::collections::HashSet<_> = all.iter().map(|case| case.name()).collect();
    assert_eq!(names.len(), all.len());
    assert_eq!(all.iter().filter(|case| case.rotating()).count(), 5);
}

#[test]
fn reused_workloads_accept_consecutive_alternating_mutations() {
    for case in cases::Case::all().into_iter().filter(|case| match case {
        cases::Case::Scale(_, count) => *count == 100,
        cases::Case::Dirty(_) | cases::Case::DirtyFull(_) => true,
        cases::Case::Stress(scenario, count) => *count == scenario.counts()[0],
    }) {
        let _mode = case.mode();
        let workload = case.workload();
        let mut scene = workload.scene();
        for frame in 0..6 {
            workload.mutate(&mut scene, frame);
        }
    }
}
