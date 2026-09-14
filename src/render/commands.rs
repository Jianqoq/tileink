//! Shared lazy command recording, uniform slots, and explicit batch completion.
//!
//! Dropping or aborting a batch never submits its uncommitted suffix. A failed
//! batch keeps resource versions pending even if an earlier prefix was accepted.

use super::{
    backend::{BatchAdapter, SubmitError},
    batches::BatchSchedule,
    resource_writes::ResourceWrites,
    upload::uniforms::UniformWrites,
};

#[derive(Debug)]
pub(crate) enum CommandError<E> {
    Recording(E),
    Submission(SubmitError<E>),
    /// Further operations after an adapter error are rejected without side effects.
    Aborted,
}

#[derive(Debug)]
pub(crate) struct BatchOutcome<S> {
    pub(crate) submissions: u32,
    #[cfg_attr(
        all(not(test), feature = "wgpu"),
        expect(
            dead_code,
            reason = "M1 completion contract; WGPU owns submitted resource retirement"
        )
    )]
    pub(crate) last_submission: Option<S>,
}

#[derive(Debug)]
pub(crate) struct BatchFailure<E, S> {
    pub(crate) error: CommandError<E>,
    /// The last confirmed prefix only. An Unconfirmed submission error does not
    /// turn this older receipt into a completion token for the failed attempt.
    #[cfg_attr(
        all(not(test), feature = "wgpu"),
        expect(
            dead_code,
            reason = "M1 completion contract; WGPU owns submitted resource retirement"
        )
    )]
    pub(crate) submitted: BatchOutcome<S>,
}

pub(crate) struct CommandBatch<A: BatchAdapter> {
    pub(crate) resource_writes: ResourceWrites,
    adapter: A,
    encoder: Option<A::Encoder>,
    uniform_writes: UniformWrites<A::Buffer>,
    schedule: BatchSchedule,
    last_submission: Option<A::Submission>,
    poisoned: bool,
    label: &'static str,
}

impl<A: BatchAdapter> CommandBatch<A> {
    pub(crate) fn from_adapter(adapter: A, label: &'static str) -> Self {
        Self {
            resource_writes: ResourceWrites::default(),
            adapter,
            encoder: None,
            uniform_writes: UniformWrites::default(),
            schedule: BatchSchedule::default(),
            last_submission: None,
            poisoned: false,
            label,
        }
    }

    pub(crate) fn adapter(&self) -> &A {
        &self.adapter
    }

    pub(crate) fn try_encoder(&mut self) -> Result<&mut A::Encoder, CommandError<A::Error>> {
        self.ensure_open()?;
        if self.encoder.is_none() {
            match self.adapter.create_encoder(self.label) {
                Ok(encoder) => self.encoder = Some(encoder),
                Err(error) => {
                    self.poisoned = true;
                    return Err(CommandError::Recording(error));
                }
            }
        }
        Ok(self
            .encoder
            .as_mut()
            .expect("encoder initialized before recording"))
    }

    pub(crate) fn try_write_uniform_slot(
        &mut self,
        buffer: &A::Buffer,
        size: u64,
        stride: u64,
        slots: u64,
        bytes: &[u8],
    ) -> Result<u64, CommandError<A::Error>> {
        self.ensure_open()?;
        if self.uniform_writes.is_full(buffer) {
            self.try_submit()?;
        }
        Ok(self
            .uniform_writes
            .write(buffer, size, stride, slots, bytes))
    }

    pub(crate) fn set_initial_root_batch_budget(&mut self, batches: usize) {
        self.schedule.set_initial_root_batch_budget(batches);
    }

    pub(crate) fn try_begin_root_batch(&mut self) -> Result<(), CommandError<A::Error>> {
        self.ensure_open()?;
        if self.schedule.begin_root_batch() {
            self.try_submit()?;
        }
        Ok(())
    }

    pub(crate) fn try_submit(&mut self) -> Result<(), CommandError<A::Error>> {
        self.ensure_open()?;
        let Some(encoder) = self.encoder.take() else {
            self.uniform_writes.clear();
            return Ok(());
        };
        let result = self.adapter.submit(encoder, &self.uniform_writes);
        self.uniform_writes.clear();
        match result {
            Ok(submission) => {
                self.last_submission = Some(submission);
                self.schedule.record_submission();
                Ok(())
            }
            Err(error) => {
                self.poisoned = true;
                Err(CommandError::Submission(error))
            }
        }
    }

    pub(crate) fn try_finish(mut self) -> BatchResult<A::Error, A::Submission> {
        if let Err(error) = self.try_submit() {
            return Err(self.into_failure(error));
        }
        self.resource_writes.commit();
        Ok(self.into_outcome())
    }

    /// Discards the unsubmitted suffix while retaining the confirmed-prefix receipt.
    pub(crate) fn abort(self) -> BatchOutcome<A::Submission> {
        self.into_outcome()
    }

    pub(crate) fn into_failure(
        self,
        error: CommandError<A::Error>,
    ) -> BatchFailure<A::Error, A::Submission> {
        BatchFailure {
            error,
            submitted: self.into_outcome(),
        }
    }

    fn into_outcome(self) -> BatchOutcome<A::Submission> {
        BatchOutcome {
            submissions: self.schedule.submissions(),
            last_submission: self.last_submission,
        }
    }

    fn ensure_open(&self) -> Result<(), CommandError<A::Error>> {
        if self.poisoned {
            Err(CommandError::Aborted)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;

pub(crate) type BatchResult<E, S> = Result<BatchOutcome<S>, BatchFailure<E, S>>;
