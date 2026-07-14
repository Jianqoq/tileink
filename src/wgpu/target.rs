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

fn resized_capacity(
    current: (u32, u32),
    required: (u32, u32),
    limit: impl FnOnce() -> u32,
) -> Option<(u32, u32)> {
    let fits = required.0 <= current.0 && required.1 <= current.1;
    if fits {
        let excessively_wide = required.0.saturating_mul(2) < current.0;
        let excessively_tall = required.1.saturating_mul(2) < current.1;
        return (excessively_wide || excessively_tall).then_some(required);
    }
    let limit = limit();
    Some((
        if required.0 > current.0 {
            grown_dimension(current.0, required.0, limit)
        } else {
            current.0
        },
        if required.1 > current.1 {
            grown_dimension(current.1, required.1, limit)
        } else {
            current.1
        },
    ))
}

fn grown_dimension(current: u32, required: u32, limit: u32) -> u32 {
    debug_assert!(current > 0);
    debug_assert!(required > current);
    required.max(current.saturating_add(current / 2).min(limit))
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

#[cfg(test)]
mod tests {
    use super::{grown_dimension, resized_capacity};

    #[test]
    fn target_capacity_grows_geometrically() {
        assert_eq!(grown_dimension(100, 101, 4096), 150);
        assert_eq!(grown_dimension(100, 240, 4096), 240);
    }

    #[test]
    fn target_capacity_does_not_grow_past_device_limit() {
        assert_eq!(grown_dimension(3000, 3500, 4096), 4096);
    }

    #[test]
    fn target_capacity_reuses_interactive_shrink_range() {
        assert_eq!(resized_capacity((1600, 1000), (1472, 928), || 4096), None);
        assert_eq!(resized_capacity((256, 192), (128, 96), || 4096), None);
    }

    #[test]
    fn target_capacity_releases_disproportionate_allocations() {
        assert_eq!(
            resized_capacity((4096, 2160), (1280, 720), || 8192),
            Some((1280, 720))
        );
    }
}
