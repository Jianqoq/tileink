use super::{
    super::{Result, compute::Texture},
    buffer,
};
use std::mem::ManuallyDrop;
use windows::Win32::Graphics::{Direct3D12::*, Dxgi::Common::*};

#[derive(Clone)]
pub(super) struct Readback {
    resource: ID3D12Resource,
    size: usize,
    rows: usize,
    row_bytes: usize,
    pitch: usize,
}
impl Readback {
    pub fn buffer(resource: ID3D12Resource, size: usize) -> Self {
        Self {
            resource,
            size,
            rows: 1,
            row_bytes: size,
            pitch: size,
        }
    }
    pub fn read(&self) -> Result<Vec<u8>> {
        let bytes = buffer::read(&self.resource, self.size)?;
        // Buffer readback already owns tightly packed bytes; preserve that allocation.
        if self.pitch == self.row_bytes {
            return Ok(bytes);
        }
        let mut packed = Vec::with_capacity(self.rows * self.row_bytes);
        for row in 0..self.rows {
            packed.extend_from_slice(&bytes[row * self.pitch..row * self.pitch + self.row_bytes]);
        }
        Ok(packed)
    }
}

fn footprint(
    device: &ID3D12Device,
    desc: &D3D12_RESOURCE_DESC,
) -> Result<(D3D12_PLACED_SUBRESOURCE_FOOTPRINT, usize)> {
    let mut layout = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
    let mut total = 0;
    unsafe {
        device.GetCopyableFootprints(
            desc,
            0,
            1,
            0,
            Some(&mut layout),
            None,
            None,
            Some(&mut total),
        );
    }
    if total == u64::MAX {
        return Err("invalid native DX12 texture footprint".into());
    }
    Ok((layout, usize::try_from(total)?))
}

unsafe fn copy(
    list: &ID3D12GraphicsCommandList,
    texture: &ID3D12Resource,
    transfer: &ID3D12Resource,
    layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    upload: bool,
) {
    unsafe {
        let mut image = D3D12_TEXTURE_COPY_LOCATION {
            pResource: ManuallyDrop::new(Some(texture.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: 0,
            },
        };
        let mut buffer = D3D12_TEXTURE_COPY_LOCATION {
            pResource: ManuallyDrop::new(Some(transfer.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: layout,
            },
        };
        let (destination, source) = if upload {
            (&image, &buffer)
        } else {
            (&buffer, &image)
        };
        list.CopyTextureRegion(destination, 0, 0, 0, source, None);
        ManuallyDrop::drop(&mut image.pResource);
        ManuallyDrop::drop(&mut buffer.pResource);
    }
}

pub(super) fn upload(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    input: &Texture,
) -> Result<(ID3D12Resource, ID3D12Resource)> {
    unsafe {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: u64::from(input.size[0]),
            Height: input.size[1],
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            ..Default::default()
        };
        let mut resource = None;
        device.CreateCommittedResource(
            &D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                CreationNodeMask: 1,
                VisibleNodeMask: 1,
                ..Default::default()
            },
            D3D12_HEAP_FLAG_NONE,
            &desc,
            D3D12_RESOURCE_STATE_COPY_DEST,
            None,
            &mut resource,
        )?;
        let texture: ID3D12Resource = resource.unwrap();
        let (layout, size) = footprint(device, &desc)?;
        let row_bytes = input.size[0] as usize * 4;
        let pitch = layout.Footprint.RowPitch as usize;
        let mut bytes = vec![0u8; size];
        for row in 0..input.size[1] as usize {
            bytes[row * pitch..row * pitch + row_bytes]
                .copy_from_slice(&input.bytes[row * row_bytes..(row + 1) * row_bytes]);
        }
        let upload = buffer::create(
            device,
            size,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            D3D12_RESOURCE_FLAG_NONE,
            Some(&bytes),
        )?;
        copy(list, &texture, &upload, layout, true);
        Ok((texture, upload))
    }
}

pub(super) fn readback(
    device: &ID3D12Device,
    list: &ID3D12GraphicsCommandList,
    texture: &ID3D12Resource,
) -> Result<Readback> {
    unsafe {
        let desc = texture.GetDesc();
        let (layout, size) = footprint(device, &desc)?;
        let resource = buffer::create(
            device,
            size,
            D3D12_HEAP_TYPE_READBACK,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_FLAG_NONE,
            None,
        )?;
        copy(list, texture, &resource, layout, false);
        Ok(Readback {
            resource,
            size,
            rows: desc.Height as usize,
            row_bytes: desc.Width as usize * 4,
            pitch: layout.Footprint.RowPitch as usize,
        })
    }
}
