//! A submission owns its staging, indirect texture leases and readback buffers.
//! Encoder boundaries with tracked resources establish visibility between passes.
mod encoding;
use encoding::Encoding;
#[cfg(test)]
mod ordering_tests;
mod pass;
mod readback;
mod render;
mod resources;
use super::{Metal, Object, Result, memory, pipeline};
use crate::native::runtime::compute::{Command, ComputeBatch};
use crate::native::shaders::BindingKind;
use objc2_metal::*;
use resources::Resource;

pub(super) struct Frame {
    pub command: Object<dyn MTLCommandBuffer>,
    completion: super::completion::Completion,
    _resources: Vec<Resource>,
    _staging: Vec<Object<dyn MTLBuffer>>,
    outputs: Vec<readback::Output>,
}
impl Frame {
    pub fn record(device: &Metal, batch: &ComputeBatch) -> Result<Self> {
        let command = device
            .queue
            .commandBuffer()
            .ok_or("Metal command buffer allocation failed")?;
        let mut staging = Vec::new();
        if let Some((_, crate::native::interop::Synchronization::Metal(sync))) =
            &batch.synchronization
        {
            sync.validate(&device.device)?;
            for point in &sync.waits {
                command.encodeWaitForEvent_value(
                    objc2::runtime::ProtocolObject::from_ref(&*point.event),
                    point.value,
                );
            }
        }
        let resources = resources::allocate(&device.device, &command, batch, &mut staging)?;
        for command_kind in batch.commands() {
            match command_kind {
                Command::CopyTexture(copy) => {
                    let encoder = command
                        .blitCommandEncoder()
                        .ok_or("Metal blit encoder failed")?;
                    let encoding = Encoding(objc2::runtime::ProtocolObject::from_ref(&*encoder));
                    let source = resources[copy.source.index()].texture()?;
                    let destination = resources[copy.destination.index()].texture()?;
                    for layer in 0..copy.extent[2] {
                        // SAFETY: ComputeBatch validates both complete regions and
                        // prohibits aliased copies before adding this command.
                        unsafe {
                            encoder.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(source,(copy.source_origin[2]+layer) as usize,0,memory::origin([copy.source_origin[0],copy.source_origin[1]]),memory::size([copy.extent[0],copy.extent[1],1]),destination,(copy.destination_origin[2]+layer) as usize,0,memory::origin([copy.destination_origin[0],copy.destination_origin[1]]));
                        }
                    }
                    drop(encoding);
                }
                Command::Dispatch(index) => {
                    let pass = &batch.passes()[*index];
                    if batch.skip_initialization(pass) {
                        continue;
                    }
                    let pipeline = &device.pipelines[pass.shader.entry];
                    let encoder =
                        pass::PassEncoder::new(&command, &pipeline.state, batch, pass, &resources)?;
                    for binding in pass.shader.bindings.iter().filter(|b| b.internal) {
                        let words = match binding.slot {
                            31 => vec![pass.grid[0], pass.grid[1], pass.grid[2], 0],
                            29 => {
                                let mut lengths = vec![0; 32];
                                for (resource_binding, id) in &pass.bindings {
                                    if matches!(
                                        resource_binding.kind,
                                        BindingKind::Uniform
                                            | BindingKind::Read
                                            | BindingKind::Write
                                    ) {
                                        lengths[resource_binding.slot as usize] =
                                            u32::try_from(batch.size(*id)? / 4)?;
                                    }
                                }
                                lengths
                            }
                            _ => return Err("unknown Metal internal binding".into()),
                        };
                        // The 16-byte dispatch grid and 128-byte buffer-length
                        // table are consumed only by this pass. Inline binding
                        // avoids a separate tracked allocation per encoder.
                        // SAFETY: reflection fixes the slot and Metal copies
                        // these initialized words during this call.
                        unsafe {
                            encoder
                                .bytes(bytemuck::cast_slice(&words), pipeline::slot(binding.slot));
                        }
                    }
                    for (binding, id) in &pass.bindings {
                        let slot = pipeline::slot(binding.slot);
                        let resource = &resources[id.index()];
                        // SAFETY: ComputeBatch and pipeline reflection agree on
                        // every resource's kind, size, slot and write ownership.
                        unsafe {
                            match binding.kind {
                                BindingKind::Uniform | BindingKind::Read | BindingKind::Write => {
                                    encoder.buffer(resource.buffer()?, slot)
                                }
                                BindingKind::Texture
                                | BindingKind::TextureWrite
                                | BindingKind::TextureArray => {
                                    encoder.texture(resource.texture()?, slot)
                                }
                                BindingKind::Sampler => {
                                    let Resource::Sampler(sampler) = resource else {
                                        return Err("Metal sampler mismatch".into());
                                    };
                                    encoder.sampler(sampler, slot);
                                }
                                BindingKind::TextureTable => {
                                    let Resource::Table(images) = resource else {
                                        return Err("Metal texture table mismatch".into());
                                    };
                                    let arguments = &pipeline.arguments[&binding.slot];
                                    let buffer = memory::buffer(
                                        &device.device,
                                        arguments.encodedLength(),
                                        MTLResourceOptions::StorageModeShared,
                                    )?;
                                    arguments.setArgumentBuffer_offset(Some(&buffer), 0);
                                    for (index, image) in images.iter().enumerate() {
                                        arguments.setTexture_atIndex(Some(image), index);
                                        encoder.sampled_resource(
                                            objc2::runtime::ProtocolObject::from_ref(&**image),
                                        );
                                    }
                                    encoder.buffer(&buffer, slot);
                                    staging.push(buffer);
                                }
                            }
                        }
                    }
                    encoder.execute(pass);
                }
            }
        }
        let outputs = readback::record(&device.device, &command, batch, &resources)?;
        if let Some((_, crate::native::interop::Synchronization::Metal(sync))) =
            &batch.synchronization
        {
            for point in &sync.signals {
                command.encodeSignalEvent_value(
                    objc2::runtime::ProtocolObject::from_ref(&*point.event),
                    point.value,
                );
            }
        }
        let completion = super::completion::Completion::new(&command);
        Ok(Self {
            command,
            completion,
            _resources: resources,
            _staging: staging,
            outputs,
        })
    }
    pub fn wait(&self) -> Result<()> {
        self.completion
            .wait(&self.command, std::time::Duration::from_secs(30))?;
        Ok(())
    }
    pub fn readback(self) -> Result<Vec<Vec<u8>>> {
        self.outputs.iter().map(readback::Output::read).collect()
    }
}
