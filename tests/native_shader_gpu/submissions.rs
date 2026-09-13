//! Ownership ledger for queued native verification work. Tickets do not own GPU leases.
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct Ticket {
    owner: Arc<()>,
    serial: u64,
}
impl Ticket {
    pub fn serial(&self) -> u64 {
        self.serial
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionError {
    WrongDevice,
    UnknownTicket,
    NotCompleted,
    InvalidCompletion,
    Unconfirmed,
    Exhausted,
}
impl std::fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native submission: {self:?}")
    }
}
impl std::error::Error for SubmissionError {}

#[derive(Clone)]
pub struct Pending<T> {
    owner: Arc<()>,
    last: u64,
    confirmed: u64,
    frames: BTreeMap<u64, T>,
}
impl<T> Pending<T> {
    pub fn new() -> Self {
        Self {
            owner: Arc::new(()),
            last: 0,
            confirmed: 0,
            frames: BTreeMap::new(),
        }
    }
    pub fn track(&mut self, frame: T) -> Result<Ticket, SubmissionError> {
        if self.last != self.confirmed {
            return Err(SubmissionError::Unconfirmed);
        }
        // UINT64_MAX is D3D12's removed-device sentinel, never a valid fence value.
        let serial = self
            .last
            .checked_add(1)
            .filter(|v| *v != u64::MAX)
            .ok_or(SubmissionError::Exhausted)?;
        self.last = serial;
        self.frames.insert(serial, frame);
        Ok(Ticket {
            owner: self.owner.clone(),
            serial,
        })
    }
    pub fn confirm(&mut self, ticket: &Ticket) -> Result<(), SubmissionError> {
        self.get(ticket)?;
        if ticket.serial != self.last || self.last != self.confirmed + 1 {
            return Err(SubmissionError::Unconfirmed);
        }
        self.confirmed = ticket.serial;
        Ok(())
    }
    pub fn get(&self, ticket: &Ticket) -> Result<&T, SubmissionError> {
        if !Arc::ptr_eq(&self.owner, &ticket.owner) {
            return Err(SubmissionError::WrongDevice);
        }
        self.frames
            .get(&ticket.serial)
            .ok_or(SubmissionError::UnknownTicket)
    }
    pub fn take_completed(&mut self, ticket: &Ticket, observed: u64) -> Result<T, SubmissionError> {
        self.get(ticket)?;
        if observed > self.confirmed {
            return Err(SubmissionError::InvalidCompletion);
        }
        if ticket.serial > observed {
            return Err(SubmissionError::NotCompleted);
        }
        Ok(self.frames.remove(&ticket.serial).unwrap())
    }
    pub fn len(&self) -> usize {
        self.frames.len()
    }
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.frames.values()
    }
    pub fn clear_after_completion(&mut self) {
        self.frames.clear();
    }
    pub fn quarantine(&mut self) {
        for (_, frame) in std::mem::take(&mut self.frames) {
            std::mem::forget(frame);
        }
    }
}

pub fn can_release_after_wait<E>(failed: bool, wait: impl FnOnce() -> Result<(), E>) -> bool {
    !failed && wait().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tickets_never_release_gpu_owners_and_only_observed_work_can_retire() {
        let resource = Arc::new(());
        let weak = Arc::downgrade(&resource);
        let mut queue = Pending::new();
        let ticket = queue.track(resource).unwrap();
        queue.confirm(&ticket).unwrap();
        let observer = ticket.clone();
        drop(ticket);
        assert!(weak.upgrade().is_some());
        assert_eq!(
            queue.take_completed(&observer, 0).unwrap_err(),
            SubmissionError::NotCompleted
        );
        assert_eq!(
            queue.take_completed(&observer, 2).unwrap_err(),
            SubmissionError::InvalidCompletion
        );
        drop(queue.take_completed(&observer, 1).unwrap());
        assert!(weak.upgrade().is_none());
        assert_eq!(
            queue.take_completed(&observer, 1).unwrap_err(),
            SubmissionError::UnknownTicket
        );
    }
    #[test]
    fn queues_reject_foreign_tickets_even_on_the_same_physical_device() {
        let mut first = Pending::new();
        let ticket = first.track(7).unwrap();
        first.confirm(&ticket).unwrap();
        let second = Pending::<u32>::new();
        assert_eq!(second.get(&ticket), Err(SubmissionError::WrongDevice));
    }
    #[test]
    fn unconfirmed_attempt_keeps_lease_and_blocks_new_work() {
        let mut queue = Pending::new();
        let ticket = queue.track(7).unwrap();
        assert_eq!(queue.track(8).unwrap_err(), SubmissionError::Unconfirmed);
        assert_eq!(
            queue.take_completed(&ticket, 1),
            Err(SubmissionError::InvalidCompletion)
        );
        assert_eq!(queue.len(), 1);
    }
    #[test]
    fn readback_order_is_independent_and_fence_sentinel_is_never_issued() {
        let mut queue = Pending::new();
        let a = queue.track(7).unwrap();
        queue.confirm(&a).unwrap();
        let b = queue.track(8).unwrap();
        queue.confirm(&b).unwrap();
        assert_eq!(queue.take_completed(&b, 2), Ok(8));
        assert_eq!(queue.take_completed(&a, 2), Ok(7));
        queue.last = u64::MAX - 1;
        queue.confirmed = u64::MAX - 1;
        assert_eq!(queue.track(9).unwrap_err(), SubmissionError::Exhausted);
    }
    #[test]
    fn failed_cleanup_never_waits_again_and_unknown_completion_never_releases() {
        assert!(!can_release_after_wait::<()>(true, || panic!(
            "failed cleanup must not wait again"
        )));
        assert!(!can_release_after_wait(false, || Err(())));
        assert!(can_release_after_wait(false, || Ok::<(), ()>(())))
    }
}
