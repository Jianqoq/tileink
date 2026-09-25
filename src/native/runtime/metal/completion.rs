//! Event-driven completion keeps the bounded wait without a 1 ms polling floor.
//! Register before commit; the handler owns only a semaphore, never the frame or
//! command buffer. Timed-out work remains owned by Pending until safely retired.
use super::{Object, Result};
use block2::RcBlock;
use dispatch2::{DispatchRetained, DispatchSemaphore, DispatchTime};
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCommandBuffer, MTLCommandBufferStatus};
use std::{ptr::NonNull, time::Duration};

pub(super) struct Completion(DispatchRetained<DispatchSemaphore>);
impl Completion {
    pub fn new(command: &Object<dyn MTLCommandBuffer>) -> Self {
        let semaphore = DispatchSemaphore::new(0);
        let signal = semaphore.clone();
        let handler = RcBlock::new(move |_: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
            signal.signal();
        });
        // SAFETY: Metal copies the correctly typed handler before returning.
        // Its sole capture is a thread-safe retained dispatch semaphore.
        unsafe {
            command.addCompletedHandler(RcBlock::as_ptr(&handler));
        }
        Self(semaphore)
    }

    pub fn wait(
        &self,
        command: &ProtocolObject<dyn MTLCommandBuffer>,
        timeout: Duration,
    ) -> Result<()> {
        if let Some(result) = terminal(command) {
            return result;
        }
        let deadline =
            DispatchTime::try_from(timeout).map_err(|_| "Metal wait duration overflow")?;
        let timed_out = self.0.wait(deadline) != 0;
        // Check again even at the timeout boundary: completed work is safe to
        // retire whether the status transition or semaphore wake won the race.
        if let Some(result) = terminal(command) {
            return result;
        }
        if timed_out {
            Err(format!("Metal completion timed out after {timeout:?}").into())
        } else {
            Err("Metal completion handler signaled a nonterminal command".into())
        }
    }
}

fn terminal(command: &ProtocolObject<dyn MTLCommandBuffer>) -> Option<Result<()>> {
    match command.status() {
        MTLCommandBufferStatus::Completed => Some(Ok(())),
        MTLCommandBufferStatus::Error => Some(Err(format!(
            "Metal execution failed: {:?}",
            command.error()
        )
        .into())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_metal::*;

    #[test]
    #[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
    fn bounded_wait_preserves_pending_work_and_allows_repeat_completion_checks() -> Result<()> {
        let device = MTLCreateSystemDefaultDevice().ok_or("Metal device")?;
        let queue = device.newCommandQueue().ok_or("Metal queue")?;
        let command = queue.commandBuffer().ok_or("Metal command buffer")?;
        let event = device.newSharedEvent().ok_or("Metal event")?;
        command.encodeWaitForEvent_value(ProtocolObject::from_ref(&*event), 1);
        let completion = Completion::new(&command);
        command.commit();
        let result = completion.wait(&command, Duration::from_millis(2));
        // Release the GPU even if an assertion fails; timeout does not cancel it.
        event.setSignaledValue(1);
        assert!(result.unwrap_err().to_string().contains("timed out"));
        completion.wait(&command, Duration::from_secs(30))?;
        completion.wait(&command, Duration::ZERO)?;
        Ok(())
    }
}
