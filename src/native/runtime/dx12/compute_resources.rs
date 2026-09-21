use super::{buffer, compute_texture};
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, Resource, uniforms::Uniforms},
};
use windows::Win32::Graphics::Direct3D12::*;

/// Owns uploads and their resource views until the enclosing submission retires.
pub(super) struct Resources {
    handles: Vec<Option<ID3D12Resource>>,
    staging: Vec<super::buffer_cache::Buffer>,
    storage: Vec<super::buffer_cache::Buffer>,
    uniform_offsets: Vec<Option<u64>>,
}

impl Resources {
    pub fn record(
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        batch: &ComputeBatch,
        staging: &mut super::buffer_cache::Pool,
        storage: &mut super::buffer_cache::Pool,
    ) -> Result<Self> {
        let uniforms = Uniforms::new(
            batch,
            D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize,
        )?;
        // Empty / persistent-texture-only batches must not evict the previous
        // frame's upload cache: their next real draw can reuse it unchanged.
        let has_uploads = batch
            .resources()
            .iter()
            .any(|input| !input.bytes().is_empty());
        let mut cached = if has_uploads {
            staging.acquire()
        } else {
            Vec::new().into()
        };
        let has_storage = batch.resources().iter().enumerate().any(|(index, input)| {
            matches!(input, Resource::Buffer(_)) && uniforms.offsets[index].is_none()
        });
        let mut cached_storage = if has_storage {
            storage.acquire()
        } else {
            Vec::new().into()
        };
        unsafe {
            let uniform = if uniforms.bytes.is_empty() {
                None
            } else {
                Some(super::staging::prepare(
                    device,
                    &uniforms.bytes,
                    &mut cached,
                )?)
            };
            let mut this = Self {
                handles: Vec::new(),
                staging: uniform.iter().cloned().collect(),
                storage: Vec::new(),
                uniform_offsets: uniforms.offsets,
            };
            for (index, input) in batch.resources().iter().enumerate() {
                if this.uniform_offset(index).is_some() {
                    this.handles
                        .push(uniform.as_ref().map(|buffer| buffer.resource.clone()));
                    continue;
                }
                if matches!(input, Resource::Sampler(_) | Resource::TextureTable(_)) {
                    this.handles.push(None);
                    continue;
                }
                if let Resource::PersistentBuffer(input) = input {
                    let crate::native::runtime::buffer::Allocation::Dx12(resource) =
                        &input.buffer.state.allocation;
                    if !input.bytes.is_empty() {
                        let upload = super::staging::prepare(device, &input.bytes, &mut cached)?;
                        buffer::transition(
                            list,
                            resource,
                            D3D12_RESOURCE_STATE_COMMON,
                            D3D12_RESOURCE_STATE_COPY_DEST,
                        );
                        for &[source, destination, size] in &input.copies {
                            list.CopyBufferRegion(
                                resource,
                                destination,
                                &upload.resource,
                                source,
                                size,
                            );
                        }
                        this.staging.push(upload);
                    }
                    this.handles.push(Some(resource.clone()));
                    continue;
                }
                if let Resource::Texture(input) = input {
                    if let Some(texture) = &input.persistent
                        && input.bytes.is_empty()
                    {
                        let crate::native::runtime::texture::Allocation::Dx12(texture) =
                            &texture.state.allocation;
                        this.handles.push(Some(texture.resource.clone()));
                        continue;
                    }
                    let (texture, upload) =
                        compute_texture::upload(device, list, input, &mut cached)?;
                    this.handles.push(Some(texture));
                    this.staging.push(upload);
                    continue;
                }
                let size = input
                    .bytes()
                    .len()
                    .div_ceil(D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize)
                    * D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize;
                let resource = super::storage::prepare(device, size, &mut cached_storage)?;
                let upload = super::staging::prepare(device, input.bytes(), &mut cached)?;
                buffer::transition(
                    list,
                    &resource.resource,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                );
                list.CopyBufferRegion(
                    &resource.resource,
                    0,
                    &upload.resource,
                    0,
                    input.bytes().len() as u64,
                );
                this.handles.push(Some(resource.resource.clone()));
                this.storage.push(resource);
                this.staging.push(upload);
            }
            Ok(this)
        }
    }
    pub fn get(&self, index: usize) -> &ID3D12Resource {
        self.handles[index]
            .as_ref()
            .expect("materialized compute resource")
    }
    pub fn uniform_offset(&self, index: usize) -> Option<u64> {
        self.uniform_offsets[index]
    }
    pub fn into_owners(
        self,
    ) -> (
        Vec<ID3D12Resource>,
        Vec<super::buffer_cache::Buffer>,
        Vec<super::buffer_cache::Buffer>,
    ) {
        (
            self.handles.into_iter().flatten().collect(),
            self.staging,
            self.storage,
        )
    }
}
