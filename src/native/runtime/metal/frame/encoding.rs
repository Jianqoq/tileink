//! End every encoder on both success and allocation/reflection error paths.
//! Releasing an encoder does not implicitly end it; leaving one open poisons
//! the command buffer and triggers Metal API validation during unwinding.
use objc2::runtime::ProtocolObject;
use objc2_metal::MTLCommandEncoder;

pub(super) struct Encoding<'a>(pub &'a ProtocolObject<dyn MTLCommandEncoder>);
impl Drop for Encoding<'_> {
    fn drop(&mut self) {
        self.0.endEncoding();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::runtime::Result;
    use objc2_metal::*;

    fn aborted_encoding(command: &ProtocolObject<dyn MTLCommandBuffer>) -> Result<()> {
        let encoder = command.blitCommandEncoder().ok_or("encoder allocation")?;
        let _encoding = Encoding(ProtocolObject::from_ref(&*encoder));
        Err("recording aborted before submission".into())
    }

    #[test]
    #[ignore = "requires physical Metal GPU and MTL_DEBUG_LAYER=1"]
    fn recording_error_closes_encoder_before_command_buffer_is_reused() -> Result<()> {
        let device = MTLCreateSystemDefaultDevice().ok_or("Metal device")?;
        let queue = device.newCommandQueue().ok_or("Metal queue")?;
        let command = queue.commandBuffer().ok_or("Metal command buffer")?;
        assert!(aborted_encoding(&command).is_err());
        // API validation rejects this call if the aborted encoder stayed active.
        let encoder = command.computeCommandEncoder().ok_or("second encoder")?;
        let encoding = Encoding(ProtocolObject::from_ref(&*encoder));
        drop(encoding);
        let completion = crate::native::runtime::metal::completion::Completion::new(&command);
        command.commit();
        completion.wait(&command, std::time::Duration::from_secs(30))
    }
}
