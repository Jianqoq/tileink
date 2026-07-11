use bytemuck::Pod;

pub(crate) struct WgpuBuffer {
    buffer: ::wgpu::Buffer,
    capacity: ::wgpu::BufferAddress,
    cached_upload: Vec<u8>,
    generation: u64,
}

impl WgpuBuffer {
    pub(crate) fn new(device: &::wgpu::Device, label: &'static str) -> Self {
        Self {
            buffer: create_buffer(device, label, 4),
            capacity: 4,
            cached_upload: Vec::new(),
            generation: 1,
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
            self.generation += 1;
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
        self.cached_upload.clear();
    }

    /// Uploads only the changed contiguous range of an immutable input buffer.
    ///
    /// Scene buffers preserve absolute indices while retained nodes keep the
    /// same shape. Comparing at element boundaries lets a small component
    /// update avoid retransmitting unchanged prefix and suffix data without
    /// changing the shader-visible layout. Buffers written by the GPU or via
    /// [`Self::write_at`] deliberately invalidate this cache.
    pub(crate) fn upload_cached<T: Pod>(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        label: &'static str,
        data: &[T],
    ) -> usize {
        let bytes = bytemuck::cast_slice(data);
        let capacity = required_storage_capacity::<T>(data.len());
        let recreated = capacity > self.capacity;
        if recreated {
            self.buffer = create_buffer(device, label, capacity.next_power_of_two());
            self.capacity = capacity.next_power_of_two();
            self.generation += 1;
            self.cached_upload.clear();
        }
        if bytes == self.cached_upload {
            return 0;
        }

        let range = changed_upload_range(bytes, &self.cached_upload, std::mem::size_of::<T>());
        if !range.is_empty() {
            queue.write_buffer(&self.buffer, range.start as u64, &bytes[range.clone()]);
        }
        self.cached_upload.clear();
        self.cached_upload.extend_from_slice(bytes);
        range.len()
    }

    /// Uploads caller-provided changed element ranges without scanning the full retained arena.
    ///
    /// The retained scene allocator is the source of truth for dirtiness. A missing byte cache or
    /// buffer growth performs one full upload; ordinary node updates write only their allocations.
    pub(crate) fn upload_ranges<T: Pod>(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        label: &'static str,
        data: &[T],
        ranges: &[std::ops::Range<usize>],
    ) -> usize {
        let bytes = bytemuck::cast_slice(data);
        let item_size = std::mem::size_of::<T>();
        let capacity = required_storage_capacity::<T>(data.len());
        let full_upload = capacity > self.capacity || self.cached_upload.is_empty();
        if capacity > self.capacity {
            self.buffer = create_buffer(device, label, capacity.next_power_of_two());
            self.capacity = capacity.next_power_of_two();
            self.generation += 1;
        }
        if full_upload {
            if !bytes.is_empty() {
                queue.write_buffer(&self.buffer, 0, bytes);
            }
            self.cached_upload.clear();
            self.cached_upload.extend_from_slice(bytes);
            return bytes.len();
        }

        self.cached_upload.resize(bytes.len(), 0);
        let mut uploaded = 0;
        for range in ranges {
            assert!(range.start <= range.end && range.end <= data.len());
            let byte_range = range.start * item_size..range.end * item_size;
            if byte_range.is_empty() {
                continue;
            }
            queue.write_buffer(
                &self.buffer,
                byte_range.start as ::wgpu::BufferAddress,
                &bytes[byte_range.clone()],
            );
            self.cached_upload[byte_range.clone()].copy_from_slice(&bytes[byte_range.clone()]);
            uploaded += byte_range.len();
        }
        uploaded
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
            self.generation += 1;
            self.cached_upload.clear();
        }
    }

    pub(crate) fn write_at<T: Pod>(
        &mut self,
        queue: &::wgpu::Queue,
        offset: ::wgpu::BufferAddress,
        data: &[T],
    ) {
        let bytes = bytemuck::cast_slice(data);
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, offset, bytes);
        }
        self.cached_upload.clear();
    }

    pub(crate) fn buffer(&self) -> &::wgpu::Buffer {
        &self.buffer
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
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

fn changed_upload_range(
    current: &[u8],
    previous: &[u8],
    item_size: usize,
) -> std::ops::Range<usize> {
    assert!(item_size > 0);
    let current_len = current.len() / item_size;
    let previous_len = previous.len() / item_size;
    let mut start = 0;
    while start < current_len.min(previous_len)
        && current[start * item_size..(start + 1) * item_size]
            == previous[start * item_size..(start + 1) * item_size]
    {
        start += 1;
    }
    let mut end = current_len;
    if current_len == previous_len {
        while end > start
            && current[(end - 1) * item_size..end * item_size]
                == previous[(end - 1) * item_size..end * item_size]
        {
            end -= 1;
        }
    }
    start * item_size..end * item_size
}

#[cfg(test)]
mod tests {
    use crate::shared::path::PathRecord;

    use super::{changed_upload_range, required_storage_capacity};

    #[test]
    fn empty_typed_storage_buffer_keeps_one_element_stride() {
        assert_eq!(
            required_storage_capacity::<PathRecord>(0),
            std::mem::size_of::<PathRecord>() as u64
        );
        assert_eq!(required_storage_capacity::<u32>(0), 4);
    }

    #[test]
    fn retained_upload_range_keeps_equal_prefix_and_suffix() {
        let previous = [1u32, 2, 3, 4];
        let current = [1u32, 9, 3, 4];
        assert_eq!(
            changed_upload_range(
                bytemuck::cast_slice(&current),
                bytemuck::cast_slice(&previous),
                std::mem::size_of::<u32>(),
            ),
            4..8
        );
    }

    #[test]
    fn retained_upload_range_rewrites_shifted_suffix_after_resize() {
        let previous = [1u32, 2, 3];
        let current = [1u32, 9, 2, 3];
        assert_eq!(
            changed_upload_range(
                bytemuck::cast_slice(&current),
                bytemuck::cast_slice(&previous),
                std::mem::size_of::<u32>(),
            ),
            4..16
        );
    }
}
