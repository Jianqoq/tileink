use super::*;
use crate::render::{backend::SubmitError, resource_writes::ReadyResource};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    Create,
    Reject,
    Unconfirmed,
}

#[derive(Debug)]
struct Submitted {
    commands: Vec<u32>,
    uniforms: Vec<(u64, Vec<u8>)>,
}

#[derive(Default)]
struct Probe {
    created: usize,
    next_failure: Option<Failure>,
    confirmed: u64,
    attempts: Vec<Submitted>,
}

struct Adapter(Rc<RefCell<Probe>>);

impl BatchAdapter for Adapter {
    type Buffer = u64;
    type Encoder = Vec<u32>;
    type Submission = u64;
    type Error = Failure;

    fn create_encoder(&mut self, _label: &'static str) -> Result<Self::Encoder, Self::Error> {
        let mut probe = self.0.borrow_mut();
        if probe.next_failure == Some(Failure::Create) {
            probe.next_failure = None;
            return Err(Failure::Create);
        }
        probe.created += 1;
        Ok(Vec::new())
    }

    fn submit(
        &mut self,
        encoder: Self::Encoder,
        uniforms: &UniformWrites<Self::Buffer>,
    ) -> Result<Self::Submission, SubmitError<Self::Error>> {
        let mut probe = self.0.borrow_mut();
        let mut writes: Vec<_> = uniforms
            .iter()
            .map(|(id, bytes)| (*id, bytes.to_vec()))
            .collect();
        writes.sort_by_key(|(id, _)| *id);
        probe.attempts.push(Submitted {
            commands: encoder,
            uniforms: writes,
        });
        match probe.next_failure.take() {
            Some(Failure::Reject) => Err(SubmitError::Rejected(Failure::Reject)),
            Some(Failure::Unconfirmed) => Err(SubmitError::Unconfirmed(Failure::Unconfirmed)),
            Some(Failure::Create) => panic!("creation failure must be consumed by create_encoder"),
            None => {
                probe.confirmed += 1;
                Ok(probe.confirmed)
            }
        }
    }
}

fn batch() -> (CommandBatch<Adapter>, Rc<RefCell<Probe>>) {
    let probe = Rc::new(RefCell::new(Probe::default()));
    (
        CommandBatch::from_adapter(Adapter(probe.clone()), "CPU batch contract"),
        probe,
    )
}

fn record(batch: &mut CommandBatch<Adapter>, buffer: u64, slots: u64, value: u8) -> u64 {
    let offset = batch
        .try_write_uniform_slot(&buffer, 4, 16, slots, &[value; 4])
        .unwrap();
    batch.try_encoder().unwrap().push(u32::from(value));
    offset
}

#[test]
fn empty_finish_does_not_allocate_an_encoder_or_submit() {
    let (batch, probe) = batch();
    let outcome = batch.try_finish().unwrap();
    assert_eq!(outcome.submissions, 0);
    assert_eq!(outcome.last_submission, None);
    assert_eq!(probe.borrow().created, 0);
    assert!(probe.borrow().attempts.is_empty());
}

#[test]
fn dropping_unsubmitted_work_discards_it_and_does_not_publish_resources() {
    let (mut batch, probe) = batch();
    record(&mut batch, 1, 2, 7);
    let ready = ReadyResource::pending();
    batch.resource_writes.record(&ready);
    drop(batch);
    assert_eq!(probe.borrow().created, 1);
    assert!(probe.borrow().attempts.is_empty());
    assert!(!ready.is_submitted());
}

#[test]
fn abort_preserves_only_the_last_confirmed_prefix() {
    let (mut batch, probe) = batch();
    record(&mut batch, 1, 2, 7);
    let ready = ReadyResource::pending();
    batch.resource_writes.record(&ready);
    batch.try_submit().unwrap();
    record(&mut batch, 1, 2, 9);
    let outcome = batch.abort();
    assert_eq!(outcome.submissions, 1);
    assert_eq!(outcome.last_submission, Some(1));
    assert_eq!(probe.borrow().attempts.len(), 1);
    assert_eq!(probe.borrow().attempts[0].commands, [7]);
    assert!(!ready.is_submitted());
}

#[test]
fn successful_finish_commits_resources_and_keeps_uniform_allocations_distinct() {
    let (mut batch, probe) = batch();
    assert_eq!(record(&mut batch, 1, 2, 7), 0);
    assert_eq!(record(&mut batch, 2, 2, 9), 0);
    let ready = ReadyResource::pending();
    batch.resource_writes.record(&ready);
    assert_eq!(batch.try_finish().unwrap().submissions, 1);
    let probe = probe.borrow();
    assert_eq!(probe.created, 1);
    assert_eq!(probe.attempts[0].commands, [7, 9]);
    assert_eq!(probe.attempts[0].uniforms.len(), 2);
    assert_eq!(&probe.attempts[0].uniforms[0].1[..4], &[7; 4]);
    assert_eq!(&probe.attempts[0].uniforms[1].1[..4], &[9; 4]);
    assert!(
        probe.attempts[0]
            .uniforms
            .iter()
            .all(|(_, bytes)| bytes[4..].iter().all(|byte| *byte == 0))
    );
    assert!(ready.is_submitted());
}

#[test]
fn uniform_rollover_submits_before_reusing_slot_zero() {
    let (mut batch, probe) = batch();
    assert_eq!(record(&mut batch, 1, 2, 7), 0);
    assert_eq!(record(&mut batch, 1, 2, 8), 16);
    assert_eq!(record(&mut batch, 1, 2, 9), 0);
    assert_eq!(probe.borrow().attempts.len(), 1);
    assert_eq!(batch.try_finish().unwrap().submissions, 2);
    let probe = probe.borrow();
    assert_eq!(probe.created, 2);
    assert_eq!(probe.attempts[0].commands, [7, 8]);
    assert_eq!(&probe.attempts[0].uniforms[0].1[16..20], &[8; 4]);
    assert_eq!(probe.attempts[1].commands, [9]);
}

#[test]
fn early_root_submission_starts_before_the_real_successor_batch() {
    let (mut batch, probe) = batch();
    batch.set_initial_root_batch_budget(2);
    for value in 1..=3 {
        batch.try_begin_root_batch().unwrap();
        record(&mut batch, 1, 8, value);
    }
    assert_eq!(probe.borrow().attempts.len(), 1);
    assert_eq!(probe.borrow().attempts[0].commands, [1, 2]);
    assert_eq!(batch.try_finish().unwrap().submissions, 2);
}

#[test]
fn uniform_rollover_cancels_the_additional_early_root_submission() {
    let (mut batch, probe) = batch();
    batch.set_initial_root_batch_budget(2);
    for value in 1..=3 {
        batch.try_begin_root_batch().unwrap();
        record(&mut batch, 1, 1, value);
    }
    assert_eq!(batch.try_finish().unwrap().submissions, 3);
    assert_eq!(probe.borrow().attempts.len(), 3);
    assert!(
        probe
            .borrow()
            .attempts
            .iter()
            .all(|attempt| attempt.commands.len() == 1)
    );
}

#[test]
fn creation_failure_aborts_without_submitting() {
    let (mut batch, probe) = batch();
    probe.borrow_mut().next_failure = Some(Failure::Create);
    let error = batch.try_encoder().unwrap_err();
    let failure = batch.into_failure(error);
    assert!(matches!(
        failure.error,
        CommandError::Recording(Failure::Create)
    ));
    assert_eq!(failure.submitted.last_submission, None);
    assert!(probe.borrow().attempts.is_empty());
}

#[test]
fn rejected_submission_keeps_the_prefix_and_prevents_further_recording() {
    let (mut batch, probe) = batch();
    record(&mut batch, 1, 2, 1);
    batch.try_submit().unwrap();
    record(&mut batch, 1, 2, 2);
    let ready = ReadyResource::pending();
    batch.resource_writes.record(&ready);
    probe.borrow_mut().next_failure = Some(Failure::Reject);
    let error = batch.try_submit().unwrap_err();
    assert!(matches!(batch.try_encoder(), Err(CommandError::Aborted)));
    let failure = batch.into_failure(error);
    assert!(matches!(
        failure.error,
        CommandError::Submission(SubmitError::Rejected(Failure::Reject))
    ));
    assert_eq!(failure.submitted.last_submission, Some(1));
    assert_eq!(failure.submitted.submissions, 1);
    assert!(!ready.is_submitted());
    assert_eq!(probe.borrow().attempts.len(), 2);
}

#[test]
fn unconfirmed_submission_is_distinct_from_the_last_waitable_prefix() {
    let (mut batch, probe) = batch();
    record(&mut batch, 1, 2, 1);
    batch.try_submit().unwrap();
    record(&mut batch, 1, 2, 2);
    probe.borrow_mut().next_failure = Some(Failure::Unconfirmed);
    let failure = batch.try_finish().unwrap_err();
    assert!(matches!(
        failure.error,
        CommandError::Submission(SubmitError::Unconfirmed(Failure::Unconfirmed))
    ));
    assert_eq!(failure.submitted.last_submission, Some(1));
    assert_eq!(failure.submitted.submissions, 1);
}

#[test]
fn failed_uniform_rollover_does_not_return_a_reused_slot() {
    let (mut batch, probe) = batch();
    record(&mut batch, 1, 1, 7);
    probe.borrow_mut().next_failure = Some(Failure::Reject);
    let error = batch
        .try_write_uniform_slot(&1, 4, 16, 1, &[8; 4])
        .unwrap_err();
    let failure = batch.into_failure(error);
    assert!(matches!(
        failure.error,
        CommandError::Submission(SubmitError::Rejected(Failure::Reject))
    ));
    assert_eq!(failure.submitted.submissions, 0);
    assert_eq!(probe.borrow().attempts[0].commands, [7]);
}
