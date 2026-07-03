use crate::shared::image::Image;

pub(crate) struct WgpuTarget {
    texture: ::wgpu::Texture,
    view: ::wgpu::TextureView,
    width: u32,
    height: u32,
}

impl WgpuTarget {
    pub(crate) fn new(device: &::wgpu::Device, width: u32, height: u32) -> Self {
        let (texture, view) = create_target_texture(device, width, height);
        Self {
            texture,
            view,
            width,
            height,
        }
    }

    pub(crate) fn resize(&mut self, device: &::wgpu::Device, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        let (texture, view) = create_target_texture(device, width, height);
        self.texture = texture;
        self.view = view;
        self.width = width;
        self.height = height;
    }

    pub(crate) fn upload(&self, queue: &::wgpu::Queue, image: &Image) {
        if image.pixels.is_empty() {
            return;
        }
        queue.write_texture(
            self.texture.as_image_copy(),
            bytemuck::cast_slice(&image.pixels),
            ::wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * std::mem::size_of::<u32>() as u32),
                rows_per_image: Some(image.height),
            },
            ::wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(crate) fn texture(&self) -> &::wgpu::Texture {
        &self.texture
    }

    pub(crate) fn view(&self) -> &::wgpu::TextureView {
        &self.view
    }
}

fn create_target_texture(
    device: &::wgpu::Device,
    width: u32,
    height: u32,
) -> (::wgpu::Texture, ::wgpu::TextureView) {
    let texture = device.create_texture(&::wgpu::TextureDescriptor {
        label: Some("tileink wgpu render target"),
        size: ::wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: ::wgpu::TextureDimension::D2,
        format: ::wgpu::TextureFormat::Rgba8Unorm,
        usage: ::wgpu::TextureUsages::COPY_SRC
            | ::wgpu::TextureUsages::COPY_DST
            | ::wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&::wgpu::TextureViewDescriptor::default());
    (texture, view)
}
