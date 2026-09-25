use super::*;

fn names(report: &CpuProfile) -> Vec<&'static str> {
    report.entries.iter().map(|entry| entry.name).collect()
}

#[test]
fn disabled_scopes_do_not_start_a_profile() {
    assert!(start_cpu_scope("disabled").is_none());
    assert_eq!(profile_cpu("value", || 42), 42);
    assert!(start_cpu_scope("still-disabled").is_none());
}

#[test]
fn nested_profiles_keep_their_own_entries_and_restore_the_parent() {
    let mut parent = CpuProfiler::default();
    parent.start();
    let outer = start_cpu_scope("outer").unwrap();
    let mut child = CpuProfiler::default();
    child.start();
    profile_cpu("child", || {});
    let child_report = child.finish();
    profile_cpu("parent-after-child", || {});
    drop(outer);
    let parent_report = parent.finish();
    assert_eq!(names(&child_report), ["child"]);
    assert_eq!(names(&parent_report), ["parent-after-child", "outer"]);
    assert!(
        parent_report
            .entries
            .iter()
            .all(|entry| entry.cpu_duration.unwrap() <= parent_report.total)
    );
    assert!(start_cpu_scope("finished").is_none());
}

#[test]
fn scopes_from_a_previous_generation_cannot_enter_a_restarted_profile() {
    let mut profiler = CpuProfiler::default();
    profiler.start();
    let old_scope = start_cpu_scope("old").unwrap();
    profiler.start();
    drop(old_scope);
    profile_cpu("current", || {});
    assert_eq!(names(&profiler.finish()), ["current"]);
}

#[test]
fn dropping_an_active_child_restores_the_parent_and_invalidates_child_scopes() {
    let mut parent = CpuProfiler::default();
    parent.start();
    let mut child = CpuProfiler::default();
    child.start();
    let unfinished = start_cpu_scope("discarded").unwrap();
    drop(child);
    drop(unfinished);
    profile_cpu("parent", || {});
    assert_eq!(names(&parent.finish()), ["parent"]);
    assert!(start_cpu_scope("finished").is_none());
}

#[test]
fn ending_a_parent_first_does_not_disable_an_active_child() {
    let mut parent = CpuProfiler::default();
    parent.start();
    let mut child = CpuProfiler::default();
    child.start();
    assert!(parent.finish().entries.is_empty());
    profile_cpu("child-still-active", || {});
    assert_eq!(names(&child.finish()), ["child-still-active"]);
    assert!(start_cpu_scope("finished").is_none());
}

#[test]
fn repeated_restarts_do_not_leave_an_abandoned_active_profile() {
    let mut parent = CpuProfiler::default();
    parent.start();
    let mut child = CpuProfiler::default();
    for _ in 0..100 {
        child.start();
        profile_cpu("child", || {});
    }
    assert_eq!(names(&child.finish()), ["child"]);
    profile_cpu("parent", || {});
    assert_eq!(names(&parent.finish()), ["parent"]);
    assert!(start_cpu_scope("finished").is_none());
}

#[test]
fn unwinding_a_scope_records_it_without_losing_the_session() {
    let mut profiler = CpuProfiler::default();
    profiler.start();
    let panic = std::panic::catch_unwind(|| profile_cpu("unwind", || panic!("scope test")));
    assert!(panic.is_err());
    profile_cpu("after-unwind", || {});
    assert_eq!(names(&profiler.finish()), ["unwind", "after-unwind"]);
}
#[test]
fn unavailable_gpu_events_preserve_their_position_among_cpu_scopes() {
    let mut profiler = CpuProfiler::default();
    profiler.start();
    profile_cpu("before", || {});
    record_unavailable_gpu_scope("unavailable-gpu");
    profile_cpu("after", || {});
    let report = profiler.finish();
    assert_eq!(names(&report), ["before", "unavailable-gpu", "after"]);
    assert_eq!(report.entries[1].cpu_duration, None);
    assert_eq!(report.entries[1].gpu_duration, None);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn tls_owner_can_drop_after_the_active_stack() {
    // The owner initializes first, so ACTIVE is destroyed before the owner at
    // thread exit. Both unfinished and finished sessions must remain safe to drop.
    for finish in [false, true] {
        std::thread::spawn(move || {
            thread_local! {
                static OWNER: std::cell::RefCell<CpuProfiler> = Default::default();
            }
            OWNER.with(|owner| {
                let mut owner = owner.borrow_mut();
                owner.start();
                profile_cpu("tls-owner", || {});
                if finish {
                    assert_eq!(names(&owner.finish()), ["tls-owner"]);
                }
            });
        })
        .join()
        .unwrap();
    }
}
