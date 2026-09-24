#[path = "../benches/support/retained_comparison_cases.rs"]
mod cases;

#[test]
fn inventory_uses_the_selected_scale_ratios_and_stress_sizes() {
    let all = cases::Case::all();
    assert_eq!(all.len(), 25 * cases::COUNTS.len() + 26 + 28);
    let names: std::collections::HashSet<_> = all.iter().map(|case| case.name()).collect();
    assert_eq!(names.len(), all.len());
    assert!(names.iter().all(|name| !name.contains("100000")));
    assert_eq!(
        all.iter().filter(|case| case.rotating()).count(),
        cases::COUNTS.len()
    );
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
