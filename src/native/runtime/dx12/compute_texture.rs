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
    layout: Option<TextureLayout>,
}
#[derive(Clone)]
struct TextureLayout {
    footprints: Vec<D3D12_PLACED_SUBRESOURCE_FOOTPRINT>,
    size: usize,
    rows: usize,
    row_bytes: usize,
}
impl TextureLayout {
    fn new(device: &ID3D12Device, desc: &D3D12_RESOURCE_DESC) -> Result<Self> {
        let mut footprints =
            vec![D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); desc.DepthOrArraySize as usize];
        let mut total = 0;
        unsafe {
            device.GetCopyableFootprints(
                desc,
                0,
                footprints.len() as u32,
                0,
                Some(footprints.as_mut_ptr()),
                None,
                None,
                Some(&mut total),
            );
        }
        if total == u64::MAX {
            return Err("invalid native DX12 texture footprints".into());
        }
        Ok(Self {
            footprints,
            size: usize::try_from(total)?,
            rows: desc.Height as usize,
            row_bytes: desc.Width as usize * 4,
        })
    }
    fn rows(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        // Array subresources have independent device footprints, including layer gaps.
        self.footprints
            .iter()
            .enumerate()
            .flat_map(move |(layer, footprint)| {
                (0..self.rows).map(move |row| {
                    (
                        footprint.Offset as usize + row * footprint.Footprint.RowPitch as usize,
                        (layer * self.rows + row) * self.row_bytes,
                    )
                })
            })
    }
}
impl Readback {
    pub fn buffer(resource: ID3D12Resource, size: usize) -> Self {
        Self {
            resource,
            size,
            layout: None,
        }
    }
    pub fn read(&self) -> Result<Vec<u8>> {
        let bytes = buffer::read(&self.resource, self.size)?;
        let Some(layout) = &self.layout else {
            return Ok(bytes);
        };
        let mut packed = vec![0; layout.footprints.len() * layout.rows * layout.row_bytes];
        for (source, destination) in layout.rows() {
            packed[destination..destination + layout.row_bytes]
                .copy_from_slice(&bytes[source..source + layout.row_bytes]);
        }
        Ok(packed)
    }
}
unsafe fn copy(
    list: &ID3D12GraphicsCommandList,
    texture: &ID3D12Resource,
    transfer: &ID3D12Resource,
    layout: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    layer: u32,
    upload: bool,
) {
    unsafe {
        let mut image = D3D12_TEXTURE_COPY_LOCATION {
            pResource: ManuallyDrop::new(Some(texture.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                SubresourceIndex: layer,
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
        let texture = allocate(
            device,
            input.size,
            input.layers,
            D3D12_RESOURCE_STATE_COPY_DEST,
        )?;
        let desc = texture.GetDesc();
        let layout = TextureLayout::new(device, &desc)?;
        let size = layout.size;
        let mut bytes = vec![0u8; size];
        for (destination, source) in layout.rows() {
            bytes[destination..destination + layout.row_bytes]
                .copy_from_slice(&input.bytes[source..source + layout.row_bytes]);
        }
        let upload = buffer::create(
            device,
            size,
            D3D12_HEAP_TYPE_UPLOAD,
            D3D12_RESOURCE_STATE_GENERIC_READ,
            D3D12_RESOURCE_FLAG_NONE,
            Some(&bytes),
        )?;
        for (layer, footprint) in layout.footprints.iter().enumerate() {
            copy(list, &texture, &upload, *footprint, layer as u32, true);
        }
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
        let layout = TextureLayout::new(device, &desc)?;
        let size = layout.size;
        let resource = buffer::create(
            device,
            size,
            D3D12_HEAP_TYPE_READBACK,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_FLAG_NONE,
            None,
        )?;
        for (layer, footprint) in layout.footprints.iter().enumerate() {
            copy(list, texture, &resource, *footprint, layer as u32, false);
        }
        Ok(Readback {
            resource,
            size,
            layout: Some(layout),
        })
    }
}

pub(super) fn allocate(
    device: &ID3D12Device,
    size: [u32; 2],
    layers: u32,
    state: D3D12_RESOURCE_STATES,
) -> Result<ID3D12Resource> {
    unsafe {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: u64::from(size[0]),
            Height: size[1],
            DepthOrArraySize: u16::try_from(layers)?,
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
            state,
            None,
            &mut resource,
        )?;
        Ok(resource.unwrap())
    }
}
