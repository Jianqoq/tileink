use super::*;
use crate::native::runtime::compute::{Resource as Input, SamplerFilter};
pub(super) enum Resource {
    InlineUniform,
    Buffer(Object<dyn MTLBuffer>),
    Texture(Object<dyn MTLTexture>),
    Sampler(Object<dyn MTLSamplerState>),
    Table(Vec<Object<dyn MTLTexture>>),
}
impl Resource {
    // ComputeBatch validates binding/copy kinds before recording; allocation
    // preserves them. Inline constants never alias storage or readback outputs.
    pub fn buffer(&self) -> &objc2::runtime::ProtocolObject<dyn MTLBuffer> {
        let Self::Buffer(buffer) = self else {
            unreachable!()
        };
        buffer
    }
    pub fn texture(&self) -> &objc2::runtime::ProtocolObject<dyn MTLTexture> {
        let Self::Texture(texture) = self else {
            unreachable!()
        };
        texture
    }
}
pub(super) fn allocate(
    device: &objc2::runtime::ProtocolObject<dyn MTLDevice>,
    command: &objc2::runtime::ProtocolObject<dyn MTLCommandBuffer>,
    batch: &ComputeBatch,
    staging: &mut Vec<Object<dyn MTLBuffer>>,
) -> Result<Vec<Resource>> {
    let mut upload_encoder = super::encoding::Upload::new(command);
    let inline = super::inline::uniforms(batch);
    let mut resources = Vec::with_capacity(batch.resources().len());
    for (index, input) in batch.resources().iter().enumerate() {
        let resource = match input {
            Input::Buffer(_) if inline[index] => Resource::InlineUniform,
            Input::Buffer(bytes) => Resource::Buffer(memory::upload(device, bytes)?),
            Input::PersistentBuffer(upload) => {
                let crate::native::runtime::buffer::Allocation::Metal(destination) =
                    &upload.buffer.state.allocation;
                if !upload.bytes.is_empty() {
                    let source = memory::upload(device, &upload.bytes)?;
                    let encoder = upload_encoder.encoder()?;
                    for &[start, offset, count] in &upload.copies {
                        // SAFETY: shared BufferUpload validates all copy ranges.
                        unsafe {
                            encoder.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                                &source,
                                start as usize,
                                destination,
                                offset as usize,
                                count as usize,
                            );
                        }
                    }
                    staging.push(source);
                }
                Resource::Buffer(destination.clone())
            }
            Input::Texture(input) => {
                let texture = if let Some(persistent) = &input.persistent {
                    let crate::native::runtime::texture::Allocation::Metal(texture) =
                        &persistent.state.allocation;
                    texture.clone()
                } else {
                    memory::texture_format(
                        device,
                        input.size,
                        input.layers,
                        input.array,
                        input.format,
                    )?
                };
                if !input.bytes.is_empty() {
                    let row = input.size[0] as usize * input.format.bytes_per_pixel();
                    let pitch = row.next_multiple_of(256);
                    let image = pitch * input.size[1] as usize;
                    let mut bytes = vec![0; image * input.layers as usize];
                    for (source, destination) in input
                        .bytes
                        .chunks_exact(row)
                        .zip(bytes.chunks_exact_mut(pitch))
                    {
                        destination[..row].copy_from_slice(source);
                    }
                    let source = memory::upload(device, &bytes)?;
                    let encoder = upload_encoder.encoder()?;
                    for layer in 0..input.layers as usize {
                        // SAFETY: complete padded layer uploads into owned storage.
                        unsafe {
                            encoder.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(&source,layer*image,pitch,image,memory::size([input.size[0],input.size[1],1]),&texture,layer,0,memory::origin([0,0]));
                        }
                    }
                    staging.push(source);
                }
                Resource::Texture(texture)
            }
            Input::Sampler(filter) => {
                let descriptor = MTLSamplerDescriptor::new();
                let filter = match filter {
                    SamplerFilter::Nearest => MTLSamplerMinMagFilter::Nearest,
                    SamplerFilter::Linear => MTLSamplerMinMagFilter::Linear,
                };
                descriptor.setMinFilter(filter);
                descriptor.setMagFilter(filter);
                descriptor.setSAddressMode(MTLSamplerAddressMode::ClampToEdge);
                descriptor.setTAddressMode(MTLSamplerAddressMode::ClampToEdge);
                Resource::Sampler(
                    device
                        .newSamplerStateWithDescriptor(&descriptor)
                        .ok_or("Metal sampler allocation failed")?,
                )
            }
            Input::TextureTable(ids) => {
                let images = ids
                    .iter()
                    .map(|id| match &resources[id.index()] {
                        Resource::Texture(texture) => texture.clone(),
                        _ => unreachable!(), // Validated by ComputeBatch::texture_table.
                    })
                    .collect();
                Resource::Table(images)
            }
        };
        resources.push(resource);
    }
    drop(upload_encoder);
    Ok(resources)
}

#[cfg(test)]
mod tests;
