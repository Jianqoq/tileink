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
        let capacity = required_storage_capacity::<T>(data.len());
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
        let capacity = required_storage_capacity::<T>(len);
        if capacity > self.capacity {
            self.buffer = create_buffer(device, label, capacity.next_power_of_two());
            self.capacity = capacity.next_power_of_two();
        }
    }

    pub(crate) fn write_at<T: Pod>(
        &self,
        queue: &::wgpu::Queue,
        offset: ::wgpu::BufferAddress,
        data: &[T],
    ) {
        let bytes = bytemuck::cast_slice(data);
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, offset, bytes);
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

        let mapped = readback
            .slice(..)
            .get_mapped_range()
            .expect("read mapped wgpu readback buffer");
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
            | ::wgpu::BufferUsages::INDIRECT
            | ::wgpu::BufferUsages::COPY_DST
            | ::wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn required_storage_capacity<T: Pod>(len: usize) -> ::wgpu::BufferAddress {
    ((len * std::mem::size_of::<T>()) as ::wgpu::BufferAddress)
        .max(std::mem::size_of::<T>() as ::wgpu::BufferAddress)
        .max(4)
}

#[cfg(test)]
mod tests {
    use crate::shared::path::PathRecord;

    use super::required_storage_capacity;

    #[test]
    fn empty_typed_storage_buffer_keeps_one_element_stride() {
        assert_eq!(
            required_storage_capacity::<PathRecord>(0),
            std::mem::size_of::<PathRecord>() as u64
        );
        assert_eq!(required_storage_capacity::<u32>(0), 4);
    }
}
