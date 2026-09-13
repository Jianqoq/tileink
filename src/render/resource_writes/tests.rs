use super::*;

#[test]
fn encoded_writes_are_available_only_inside_their_batch_until_submission() {
    let resource = ReadyResource::pending();
    let mut batch = ResourceWrites::default();
    assert!(!batch.available(&resource));
    batch.record(&resource);
    assert!(batch.available(&resource));
    assert!(!resource.is_submitted());
    assert!(!ResourceWrites::default().available(&resource));
    batch.commit();
    assert!(resource.is_submitted());
    assert!(ResourceWrites::default().available(&resource));
}

#[test]
fn dropping_a_batch_does_not_publish_its_partial_results() {
    let first = ReadyResource::pending();
    let second = ReadyResource::pending();
    {
        let mut failed = ResourceWrites::default();
        failed.record(&first);
        failed.record(&second);
        // The backend may already have submitted an early batch when a later
        // operation fails. Neither resource is committed by dropping this ledger.
    }
    let retry = ResourceWrites::default();
    assert!(!retry.available(&first));
    assert!(!retry.available(&second));
}

#[test]
fn duplicate_recording_has_one_commit_entry() {
    let resource = ReadyResource::pending();
    let mut batch = ResourceWrites::default();
    batch.record(&resource);
    batch.record(&resource);
    assert_eq!(batch.pending.len(), 1);
    batch.commit();
    batch.record(&resource);
    assert!(batch.pending.is_empty());
}

#[test]
fn failed_new_work_preserves_previously_submitted_resources() {
    let old = ReadyResource::pending();
    let new = ReadyResource::pending();
    let mut committed = ResourceWrites::default();
    committed.record(&old);
    committed.commit();
    let mut failed = ResourceWrites::default();
    failed.record(&old);
    failed.record(&new);
    drop(failed);
    assert!(old.is_submitted());
    assert!(!new.is_submitted());
}

#[test]
fn committing_an_old_destination_does_not_commit_its_replacement() {
    let old = ReadyResource::pending();
    let replacement = ReadyResource::pending();
    let mut old_batch = ResourceWrites::default();
    old_batch.record(&old);
    old_batch.commit();
    assert!(old.is_submitted());
    assert!(!replacement.is_submitted());
}

#[test]
fn empty_batch_has_no_allocated_state() {
    let mut batch = ResourceWrites::default();
    assert_eq!(batch.id, 0);
    assert_eq!(batch.pending.capacity(), 0);
    batch.commit();
    assert_eq!(batch.id, 0);
}
