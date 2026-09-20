//! Static GPU command seam used by the shared batch scheduler.
//!
//! This is the first implemented adapter boundary. Pipeline/resource operations
//! remain in their current modules until they move through the same seam.

use super::upload::uniforms::UniformWrites;
use std::hash::Hash;

#[derive(Debug)]
#[cfg_attr(
    all(not(test), feature = "wgpu", not(tileink_native_runtime)),
    expect(
        dead_code,
        reason = "M1 shared submission contract; WGPU enqueue is infallible"
    )
)]
pub(crate) enum SubmitError<E> {
    /// The attempted submission was not accepted; an older prefix may still run.
    Rejected(E),
    /// Work may have been accepted without a usable completion receipt. The
    /// adapter keeps its leases and reports recovery/device loss to the caller.
    Unconfirmed(E),
}

pub(crate) trait BatchAdapter {
    type Buffer: Clone + Eq + Hash;
    type Encoder;
    type Submission;
    type Error;

    fn create_encoder(&mut self, label: &'static str) -> Result<Self::Encoder, Self::Error>;

    /// Makes uniform bytes visible before their encoded consumers, then submits
    /// the ordered batch. Each success receipt covers this submission and every
    /// earlier confirmed submission from the same batch; only the last is retained.
    ///
    /// GPU-use leases have a retirement owner that outlives the batch, its adapter
    /// value and returned receipts, including an Unconfirmed submission attempt.
    /// Native mapped ranges, descriptors and command allocators cannot be reused
    /// merely because submit, finish or abort returns, or a receipt is dropped.
    /// Retirement requires observed completion or safe device-loss teardown.
    fn submit(
        &mut self,
        encoder: Self::Encoder,
        uniforms: &UniformWrites<Self::Buffer>,
    ) -> Result<Self::Submission, SubmitError<Self::Error>>;
}
