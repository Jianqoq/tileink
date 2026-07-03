use crate::shared::image::Image;

pub(crate) struct WgpuTarget {
    buffer: ::wgpu::Buffer,
    capacity: ::wgpu::BufferAddress,
}

impl WgpuTarget {
    pub(crate) fn new(device: &::wgpu::Device, byte_len: ::wgpu::BufferAddress) -> Self {
        let capacity = buffer_capacity(byte_len);
        Self {
            buffer: create_target_buffer(device, capacity),
            capacity,
        }
    }

    pub(crate) fn resize(&mut self, device: &::wgpu::Device, byte_len: ::wgpu::BufferAddress) {
        let capacity = buffer_capacity(byte_len);
        if capacity != self.capacity {
            self.buffer = create_target_buffer(device, capacity);
            self.capacity = capacity;
        }
    }

    pub(crate) fn upload(&self, queue: &::wgpu::Queue, image: &Image) {
        if image.pixels.is_empty() {
            return;
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&image.pixels));
    }

    pub(crate) fn buffer(&self) -> &::wgpu::Buffer {
        &self.buffer
    }
}

fn create_target_buffer(device: &::wgpu::Device, size: ::wgpu::BufferAddress) -> ::wgpu::Buffer {
    device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some("tileink wgpu render target"),
        size,
        usage: ::wgpu::BufferUsages::COPY_SRC
            | ::wgpu::BufferUsages::COPY_DST
            | ::wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    })
}

fn buffer_capacity(byte_len: ::wgpu::BufferAddress) -> ::wgpu::BufferAddress {
    byte_len.max(4)
}
