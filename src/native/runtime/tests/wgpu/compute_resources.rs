use crate::native::{runtime::compute::Resource, shaders::BindingKind};
use wgpu::util::DeviceExt;

pub(super) enum GpuResource {
    Buffer(wgpu::Buffer),
    Texture(wgpu::Texture, wgpu::TextureView),
}
impl GpuResource {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, input: &Resource) -> Self {
        match input {
            Resource::Buffer(bytes) => Self::Buffer(device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytes,
                    usage: wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_SRC,
                },
            )),
            Resource::Texture(input) => {
                let size = wgpu::Extent3d {
                    width: input.size[0],
                    height: input.size[1],
                    depth_or_array_layers: input.layers,
                };
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::STORAGE_BINDING
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &input.bytes,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(input.size[0] * 4),
                        rows_per_image: Some(input.size[1]),
                    },
                    size,
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(if input.array {
                        wgpu::TextureViewDimension::D2Array
                    } else {
                        wgpu::TextureViewDimension::D2
                    }),
                    ..Default::default()
                });
                Self::Texture(texture, view)
            }
        }
    }
    pub fn binding(&self) -> wgpu::BindingResource<'_> {
        match self {
            Self::Buffer(buffer) => buffer.as_entire_binding(),
            Self::Texture(_, view) => wgpu::BindingResource::TextureView(view),
        }
    }
    pub fn readback(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) -> Readback {
        let (size, rows, row_bytes, pitch) = match self {
            Self::Buffer(buffer) => (
                buffer.size(),
                1,
                buffer.size() as usize,
                buffer.size() as usize,
            ),
            Self::Texture(texture, _) => {
                let row_bytes = texture.width() * 4;
                let pitch = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                    * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                (
                    u64::from(pitch)
                        * u64::from(texture.height())
                        * u64::from(texture.depth_or_array_layers()),
                    texture.height() as usize * texture.depth_or_array_layers() as usize,
                    row_bytes as usize,
                    pitch as usize,
                )
            }
        };
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        match self {
            Self::Buffer(source) => encoder.copy_buffer_to_buffer(source, 0, &buffer, 0, size),
            Self::Texture(texture, _) => encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(pitch as u32),
                        rows_per_image: Some(texture.height()),
                    },
                },
                texture.size(),
            ),
        }
        Readback {
            buffer,
            rows,
            row_bytes,
            pitch,
        }
    }
}
pub(super) struct Readback {
    pub buffer: wgpu::Buffer,
    rows: usize,
    row_bytes: usize,
    pitch: usize,
}
impl Readback {
    pub fn unpack(&self, bytes: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(self.rows * self.row_bytes);
        for row in 0..self.rows {
            output.extend_from_slice(&bytes[row * self.pitch..row * self.pitch + self.row_bytes]);
        }
        output
    }
}
pub(super) fn layout(kind: BindingKind) -> wgpu::BindingType {
    match kind {
        BindingKind::Texture | BindingKind::TextureArray => wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: if kind == BindingKind::TextureArray {
                wgpu::TextureViewDimension::D2Array
            } else {
                wgpu::TextureViewDimension::D2
            },
            multisampled: false,
        },
        BindingKind::TextureWrite => wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: wgpu::TextureFormat::Rgba8Unorm,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
        BindingKind::Uniform | BindingKind::Read | BindingKind::Write => {
            wgpu::BindingType::Buffer {
                ty: match kind {
                    BindingKind::Uniform => wgpu::BufferBindingType::Uniform,
                    BindingKind::Read => wgpu::BufferBindingType::Storage { read_only: true },
                    _ => wgpu::BufferBindingType::Storage { read_only: false },
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            }
        }
    }
}
