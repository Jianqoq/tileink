#[path = "../benches/support/immediate_workloads.rs"]
mod cases;
#[allow(dead_code)]
#[path = "../benches/support/benchmark_evidence.rs"]
mod evidence;

#[test]
fn immediate_inventory_preserves_legacy_sizes_and_complete_resize_cycles() {
    let cases = cases::cases();
    assert_eq!(cases.len(), 62);
    let mut names = std::collections::HashSet::new();
    for case in cases {
        assert!(names.insert(case.name.clone()));
        let expected = if case.name == "filter-resize" {
            4
        } else if case.name.starts_with("dirty-immediate-") {
            2
        } else {
            1
        };
        assert_eq!(case.frames.len(), expected, "{}", case.name);
        for canvas in case.frames {
            assert!(canvas.physical_width() > 0 && canvas.physical_height() > 0);
        }
    }
}

#[test]
fn family_filters_do_not_silently_include_unrequested_modes() {
    for name in ["scale-static-100", "dirty-0.5pct"] {
        assert!(evidence::matches_filter(name, Some("scale-,dirty-")));
    }
    for name in ["forcefull-dirty-0.5pct", "stress-deep-hierarchy-revision-8"] {
        assert!(!evidence::matches_filter(name, Some("scale-,dirty-")));
        assert!(evidence::matches_filter(name, None));
        assert!(!evidence::matches_filter(name, Some("")));
    }
}
