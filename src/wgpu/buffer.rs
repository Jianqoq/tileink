use bytemuck::Pod;

pub(crate) struct WgpuBuffer {
    buffer: ::wgpu::Buffer,
    capacity: ::wgpu::BufferAddress,
}

impl WgpuBuffer {
    pub(crate) fn new(device: &::wgpu::Device, label: &'static str) -> Self {
        Self {
            buffer: create_buffer(device, label, 4),
            capacity: 4,
        }
    }

    pub(crate) fn upload<T: Pod>(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        label: &'static str,
        data: &[T],
    ) {
        let bytes = bytemuck::cast_slice(data);
        let capacity = (bytes.len() as ::wgpu::BufferAddress).max(4);
        if capacity > self.capacity {
            self.buffer = create_buffer(device, label, capacity.next_power_of_two());
            self.capacity = capacity.next_power_of_two();
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
    }

    pub(crate) fn resize_uninit<T: Pod>(
        &mut self,
        device: &::wgpu::Device,
        label: &'static str,
        len: usize,
    ) {
        let capacity = ((len * std::mem::size_of::<T>()) as ::wgpu::BufferAddress).max(4);
        if capacity > self.capacity {
            self.buffer = create_buffer(device, label, capacity.next_power_of_two());
            self.capacity = capacity.next_power_of_two();
        }
    }

    pub(crate) fn buffer(&self) -> &::wgpu::Buffer {
        &self.buffer
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> ::wgpu::BufferAddress {
        self.capacity
    }

    pub(crate) fn read<T: Pod>(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        len: usize,
    ) -> Vec<T> {
        let byte_len = (len * std::mem::size_of::<T>()) as ::wgpu::BufferAddress;
        if byte_len == 0 {
            return Vec::new();
        }
        let readback = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu buffer test readback"),
            size: byte_len,
            usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
            label: Some("tileink wgpu buffer test readback copy"),
        });
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &readback, 0, byte_len);
        queue.submit([encoder.finish()]);

        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(::wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap()
            });
        device.poll(::wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();

        let mapped = readback.slice(..).get_mapped_range();
        let values = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        readback.unmap();
        values
    }
}

fn create_buffer(
    device: &::wgpu::Device,
    label: &'static str,
    size: ::wgpu::BufferAddress,
) -> ::wgpu::Buffer {
    device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: ::wgpu::BufferUsages::STORAGE
            | ::wgpu::BufferUsages::COPY_DST
            | ::wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}
