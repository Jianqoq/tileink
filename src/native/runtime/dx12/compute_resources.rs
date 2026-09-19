use super::{buffer, compute_texture};
use crate::native::runtime::{
    Result,
    compute::{ComputeBatch, Resource, uniforms::Uniforms},
};
use windows::Win32::Graphics::Direct3D12::*;

/// Owns uploads and their resource views until the enclosing submission retires.
pub(super) struct Resources {
    handles: Vec<Option<ID3D12Resource>>,
    uploads: Vec<ID3D12Resource>,
    uniform_offsets: Vec<Option<u64>>,
}

impl Resources {
    pub fn record(
        device: &ID3D12Device,
        list: &ID3D12GraphicsCommandList,
        batch: &ComputeBatch,
    ) -> Result<Self> {
        let uniforms = Uniforms::new(
            batch,
            D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize,
        )?;
        unsafe {
            let uniform = if uniforms.bytes.is_empty() {
                None
            } else {
                Some(buffer::create(
                    device,
                    uniforms.bytes.len(),
                    D3D12_HEAP_TYPE_UPLOAD,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                    D3D12_RESOURCE_FLAG_NONE,
                    Some(&uniforms.bytes),
                )?)
            };
            let mut this = Self {
                handles: Vec::new(),
                uploads: Vec::new(),
                uniform_offsets: uniforms.offsets,
            };
            for (index, input) in batch.resources().iter().enumerate() {
                if this.uniform_offset(index).is_some() {
                    this.handles.push(uniform.clone());
                    continue;
                }
                if matches!(input, Resource::Sampler(_) | Resource::TextureTable(_)) {
                    this.handles.push(None);
                    continue;
                }
                if let Resource::Texture(input) = input {
                    if let Some(texture) = &input.persistent {
                        let texture = match &texture.state.allocation {
                            crate::native::runtime::texture::Allocation::Dx12(texture) => texture,
                            #[cfg(feature = "native-vulkan")]
                            crate::native::runtime::texture::Allocation::Vulkan(_) => {
                                return Err("non-DX12 persistent texture".into());
                            }
                        };
                        this.handles.push(Some(texture.clone()));
                        continue;
                    }
                    let (texture, upload) = compute_texture::upload(device, list, input)?;
                    this.handles.push(Some(texture));
                    this.uploads.push(upload);
                    continue;
                }
                let size = input
                    .bytes()
                    .len()
                    .div_ceil(D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize)
                    * D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as usize;
                let resource = buffer::create(
                    device,
                    size,
                    D3D12_HEAP_TYPE_DEFAULT,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
                    None,
                )?;
                let upload = buffer::create(
                    device,
                    input.bytes().len(),
                    D3D12_HEAP_TYPE_UPLOAD,
                    D3D12_RESOURCE_STATE_GENERIC_READ,
                    D3D12_RESOURCE_FLAG_NONE,
                    Some(input.bytes()),
                )?;
                buffer::transition(
                    list,
                    &resource,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                );
                list.CopyBufferRegion(&resource, 0, &upload, 0, input.bytes().len() as u64);
                this.handles.push(Some(resource));
                this.uploads.push(upload);
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
    pub fn into_owners(self) -> Vec<ID3D12Resource> {
        self.handles
            .into_iter()
            .flatten()
            .chain(self.uploads)
            .collect()
    }
}
