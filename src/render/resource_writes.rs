//! Submission readiness for immutable versions of cached GPU content.
//!
//! Encoding a write makes it usable by later commands in that same ordered batch.
//! Other batches may reuse it only after successful submission. GPU completion and
//! resource leases remain the adapter's responsibility; this ledger never waits.

use std::{
    cell::Cell,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Default)]
struct ReadyState {
    submitted: bool,
    encoded_batch: u64,
}

/// A fresh marker is required when either allocation or intended content changes.
#[derive(Clone)]
pub(crate) struct ReadyResource(Rc<Cell<ReadyState>>);

impl ReadyResource {
    pub(crate) fn pending() -> Self {
        Self(Rc::new(Cell::new(ReadyState::default())))
    }

    pub(crate) fn is_submitted(&self) -> bool {
        self.0.get().submitted
    }
}

#[derive(Default)]
pub(crate) struct ResourceWrites {
    // Assign an identity lazily: ordinary frames without cached resource writes
    // retain their allocation-free, atomic-free command-batch construction.
    id: u64,
    pending: Vec<ReadyResource>,
}

impl ResourceWrites {
    pub(crate) fn available(&self, resource: &ReadyResource) -> bool {
        let state = resource.0.get();
        state.submitted || (self.id != 0 && state.encoded_batch == self.id)
    }

    /// Call after every command needed to produce this content has been encoded.
    pub(crate) fn record(&mut self, resource: &ReadyResource) {
        if self.available(resource) {
            return;
        }
        if self.id == 0 {
            static NEXT_BATCH: AtomicU64 = AtomicU64::new(1);
            self.id = NEXT_BATCH
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("resource write batch identity exhausted");
        }
        resource.0.set(ReadyState {
            submitted: false,
            encoded_batch: self.id,
        });
        self.pending.push(resource.clone());
    }

    /// Publish only after the adapter has successfully submitted the complete work.
    /// A failed or dropped batch deliberately leaves its versions uncommitted, even
    /// if a prefix was submitted, so a retry reconstructs the necessary content.
    pub(crate) fn commit(&mut self) {
        for resource in self.pending.drain(..) {
            resource.0.set(ReadyState {
                submitted: true,
                encoded_batch: 0,
            });
        }
    }
}

#[cfg(test)]
mod tests;
