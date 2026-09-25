use super::*;
use crate::native::runtime::compute::{Resource as Input, SamplerFilter};
pub(super) enum Resource {
    Buffer(Object<dyn MTLBuffer>),
    Texture(Object<dyn MTLTexture>),
    Sampler(Object<dyn MTLSamplerState>),
    Table(Vec<Object<dyn MTLTexture>>),
}
impl Resource {
    pub fn buffer(&self) -> Result<&objc2::runtime::ProtocolObject<dyn MTLBuffer>> {
        match self {
            Self::Buffer(buffer) => Ok(buffer),
            _ => Err("Metal resource is not a buffer".into()),
        }
    }
    pub fn texture(&self) -> Result<&objc2::runtime::ProtocolObject<dyn MTLTexture>> {
        match self {
            Self::Texture(texture) => Ok(texture),
            _ => Err("Metal resource is not a texture".into()),
        }
    }
}
pub(super) fn allocate(
    device: &objc2::runtime::ProtocolObject<dyn MTLDevice>,
    command: &objc2::runtime::ProtocolObject<dyn MTLCommandBuffer>,
    batch: &ComputeBatch,
    staging: &mut Vec<Object<dyn MTLBuffer>>,
) -> Result<Vec<Resource>> {
    let encoder = command
        .blitCommandEncoder()
        .ok_or("Metal upload encoder failed")?;
    let encoding = Encoding(objc2::runtime::ProtocolObject::from_ref(&*encoder));
    let mut resources = Vec::new();
    for input in batch.resources() {
        let resource = match input {
            Input::Buffer(bytes) => Resource::Buffer(memory::upload(device, bytes)?),
            Input::PersistentBuffer(upload) => {
                let crate::native::runtime::buffer::Allocation::Metal(destination) =
                    &upload.buffer.state.allocation;
                if !upload.bytes.is_empty() {
                    let source = memory::upload(device, &upload.bytes)?;
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
                if let Some(persistent) = &input.persistent {
                    let crate::native::runtime::texture::Allocation::Metal(texture) =
                        &persistent.state.allocation;
                    Resource::Texture(texture.clone())
                } else {
                    let texture = memory::texture(device, input.size, input.layers, input.array)?;
                    let row = input.size[0] as usize * 4;
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
                    for layer in 0..input.layers as usize {
                        // SAFETY: complete padded layer uploads into new storage.
                        unsafe {
                            encoder.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(&source,layer*image,pitch,image,memory::size([input.size[0],input.size[1],1]),&texture,layer,0,memory::origin([0,0]));
                        }
                    }
                    staging.push(source);
                    Resource::Texture(texture)
                }
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
                        Resource::Texture(texture) => Ok(texture.clone()),
                        _ => Err("Metal table requires 2D textures".into()),
                    })
                    .collect::<Result<Vec<_>>>()?;
                Resource::Table(images)
            }
        };
        resources.push(resource);
    }
    drop(encoding);
    Ok(resources)
}
