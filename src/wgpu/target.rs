use crate::render::target_capacity::resized_capacity;

pub(crate) struct WgpuTarget {
    texture: ::wgpu::Texture,
    view: ::wgpu::TextureView,
    capacity_width: u32,
    capacity_height: u32,
}

impl WgpuTarget {
    pub(crate) fn new(device: &::wgpu::Device, width: u32, height: u32) -> Self {
        let (texture, view) = create_target_texture(device, width, height);
        Self {
            texture,
            view,
            capacity_width: width.max(1),
            capacity_height: height.max(1),
        }
    }

    pub(crate) fn resize(&mut self, device: &::wgpu::Device, width: u32, height: u32) {
        let requested = (width.max(1), height.max(1));
        let current = (self.capacity_width, self.capacity_height);
        let Some(capacity) = resized_capacity(current, requested, || {
            device.limits().max_texture_dimension_2d
        }) else {
            return;
        };
        // Render targets are transient capacity, not logical scene bounds. Keeping the largest
        // allocation avoids destroying and recreating several full-size GPU textures on every
        // tick of an interactive resize. A greater-than-2x shrink releases excess capacity so a
        // formerly large window does not retain disproportionate memory indefinitely. The
        // renderer still dispatches and copies only its current logical size.
        let (texture, view) = create_target_texture(device, capacity.0, capacity.1);
        self.texture = texture;
        self.view = view;
        self.capacity_width = capacity.0;
        self.capacity_height = capacity.1;
    }

    pub(crate) fn texture(&self) -> &::wgpu::Texture {
        &self.texture
    }

    pub(crate) fn view(&self) -> &::wgpu::TextureView {
        &self.view
    }

    pub(crate) fn fits(&self, size: (u32, u32)) -> bool {
        size.0.max(1) <= self.capacity_width && size.1.max(1) <= self.capacity_height
    }

    pub(crate) fn byte_len(&self) -> u64 {
        self.capacity_width as u64 * self.capacity_height as u64 * 4
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
            | ::wgpu::TextureUsages::STORAGE_BINDING
            | ::wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&::wgpu::TextureViewDescriptor::default());
    (texture, view)
}

impl crate::render::retained_surfaces::SurfaceAllocation for WgpuTarget {
    fn byte_len(&self) -> u64 {
        WgpuTarget::byte_len(self)
    }
}
