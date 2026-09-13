use crate::render::{
    backend::{BatchAdapter, SubmitError},
    commands::{CommandBatch, CommandError},
    upload::uniforms::UniformWrites,
};
use std::convert::Infallible;

pub(crate) type WgpuCommandBatch = CommandBatch<WgpuCommands>;

pub(crate) struct WgpuCommands {
    device: ::wgpu::Device,
    queue: ::wgpu::Queue,
}

impl BatchAdapter for WgpuCommands {
    type Buffer = ::wgpu::Buffer;
    type Encoder = ::wgpu::CommandEncoder;
    type Submission = ::wgpu::SubmissionIndex;
    // WGPU's enqueue API is infallible; asynchronous device errors continue to
    // use its existing error reporting. Native adapters return their API errors.
    type Error = Infallible;

    fn create_encoder(&mut self, label: &'static str) -> Result<Self::Encoder, Self::Error> {
        Ok(self
            .device
            .create_command_encoder(&::wgpu::CommandEncoderDescriptor { label: Some(label) }))
    }

    fn submit(
        &mut self,
        encoder: Self::Encoder,
        uniforms: &UniformWrites<Self::Buffer>,
    ) -> Result<Self::Submission, SubmitError<Self::Error>> {
        for (buffer, bytes) in uniforms.iter() {
            self.queue.write_buffer(buffer, 0, bytes);
        }
        Ok(self.queue.submit([encoder.finish()]))
    }
}

// The WGPU-specific methods expose its infallible recording API. Scheduling,
// uploads, completion bookkeeping and abort semantics live in CommandBatch.
impl CommandBatch<WgpuCommands> {
    pub(crate) fn new(device: &::wgpu::Device, queue: &::wgpu::Queue, label: &'static str) -> Self {
        Self::from_adapter(
            WgpuCommands {
                device: device.clone(),
                queue: queue.clone(),
            },
            label,
        )
    }

    pub(crate) fn device(&self) -> &::wgpu::Device {
        &self.adapter().device
    }

    pub(crate) fn encoder(&mut self) -> &mut ::wgpu::CommandEncoder {
        infallible(self.try_encoder())
    }

    pub(crate) fn write_uniform_slot(
        &mut self,
        buffer: &::wgpu::Buffer,
        size: ::wgpu::BufferAddress,
        stride: ::wgpu::BufferAddress,
        slots: u64,
        bytes: &[u8],
    ) -> ::wgpu::BufferAddress {
        infallible(self.try_write_uniform_slot(buffer, size, stride, slots, bytes))
    }

    pub(crate) fn begin_root_batch(&mut self) {
        infallible(self.try_begin_root_batch());
    }

    #[cfg(test)]
    pub(crate) fn submit_current(&mut self) {
        infallible(self.try_submit());
    }

    #[cfg(test)]
    pub(crate) fn finish(self) -> u32 {
        self.finish_with_status(true)
    }

    pub(crate) fn finish_with_status(self, succeeded: bool) -> u32 {
        if succeeded {
            infallible(
                self.try_finish()
                    .map(|outcome| outcome.submissions)
                    .map_err(|failure| failure.error),
            )
        } else {
            self.abort().submissions
        }
    }
}

fn infallible<T>(result: Result<T, CommandError<Infallible>>) -> T {
    match result {
        Ok(value) => value,
        Err(CommandError::Recording(error))
        | Err(CommandError::Submission(
            SubmitError::Rejected(error) | SubmitError::Unconfirmed(error),
        )) => match error {},
        Err(CommandError::Aborted) => {
            unreachable!("infallible WGPU recording cannot poison a batch")
        }
    }
}

pub(crate) const WGPU_CONFIG_SLOTS: u64 = 4096;

pub(crate) fn aligned_uniform_stride(
    device: &::wgpu::Device,
    size: ::wgpu::BufferAddress,
) -> ::wgpu::BufferAddress {
    align_to(
        size,
        device.limits().min_uniform_buffer_offset_alignment as ::wgpu::BufferAddress,
    )
}

pub(crate) fn uniform_slots_buffer_size(
    device: &::wgpu::Device,
    size: ::wgpu::BufferAddress,
) -> ::wgpu::BufferAddress {
    aligned_uniform_stride(device, size) * WGPU_CONFIG_SLOTS
}

fn align_to(
    value: ::wgpu::BufferAddress,
    alignment: ::wgpu::BufferAddress,
) -> ::wgpu::BufferAddress {
    if alignment <= 1 {
        return value;
    }
    value.div_ceil(alignment) * alignment
}
