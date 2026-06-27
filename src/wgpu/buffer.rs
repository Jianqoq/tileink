use ::wgpu::{Buffer, BufferAddress, BufferUsages, Device, Queue};
use bytemuck::Pod;

const COPY_CHUNK_SIZE: usize = 64 * 1024;

pub(crate) struct WgpuBuffer<T> {
    buffer: Buffer,
    device: Device,
    queue: Queue,
    usage: BufferUsages,
    label: &'static str,
    len: BufferAddress,
    capacity: BufferAddress,
    marker: std::marker::PhantomData<T>,
}

impl<T: Pod> WgpuBuffer<T> {
    pub(crate) fn new(
        device: Device,
        queue: Queue,
        usage: BufferUsages,
        capacity: usize,
        label: &'static str,
    ) -> Self {
        let capacity = bytes_for::<T>(capacity).max(1);
        let buffer = create_buffer(&device, usage, capacity, label);
        Self {
            buffer,
            device,
            queue,
            usage,
            label,
            len: 0,
            capacity,
            marker: std::marker::PhantomData,
        }
    }

    pub(crate) fn with_data(
        device: Device,
        queue: Queue,
        usage: BufferUsages,
        data: &[T],
        label: &'static str,
    ) -> Self {
        let mut buffer = Self::new(device, queue, usage, data.len(), label);
        buffer.extend_from_slice(data);
        buffer
    }

    pub(crate) fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub(crate) fn len(&self) -> usize {
        elements_for::<T>(self.len)
    }

    pub(crate) fn capacity(&self) -> usize {
        elements_for::<T>(self.capacity)
    }

    pub(crate) fn bytes_len(&self) -> usize {
        self.len as usize
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }

    pub(crate) fn replace(&mut self, data: &[T]) {
        self.clear();
        self.extend_from_slice(data);
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        self.ensure_capacity_bytes(self.len.saturating_add(bytes_for::<T>(additional)));
    }

    pub(crate) fn ensure_capacity(&mut self, min_capacity: usize) {
        self.ensure_capacity_bytes(bytes_for::<T>(min_capacity));
    }

    fn ensure_capacity_bytes(&mut self, min_capacity: BufferAddress) {
        if min_capacity <= self.capacity {
            return;
        }

        let mut next_capacity = self.capacity.max(1);
        while next_capacity < min_capacity {
            next_capacity = next_capacity.saturating_mul(2).max(min_capacity);
        }

        let new_buffer = create_buffer(&self.device, self.usage, next_capacity, self.label);
        if self.len != 0 {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("growable_buffer_resize"),
                });
            encoder.copy_buffer_to_buffer(&self.buffer, 0, &new_buffer, 0, self.len);
            self.queue.submit(Some(encoder.finish()));
        }
        self.buffer = new_buffer;
        self.capacity = next_capacity;
    }

    pub(crate) fn resize_zeroed(&mut self, new_len: usize) {
        let new_len = bytes_for::<T>(new_len);
        if new_len <= self.len {
            self.len = new_len;
            return;
        }

        self.ensure_capacity_bytes(new_len);
        self.zero_range(self.len, new_len - self.len);
        self.len = new_len;
    }

    pub(crate) fn resize_zeroed_all(&mut self, new_len: usize) {
        let new_len = bytes_for::<T>(new_len);
        self.ensure_capacity_bytes(new_len);
        self.zero_range(0, new_len);
        self.len = new_len;
    }

    pub(crate) fn fill(&mut self, len: usize, value: T) {
        let byte_len = bytes_for::<T>(len);
        self.ensure_capacity_bytes(byte_len);
        self.len = byte_len;
        if len == 0 {
            return;
        }

        let chunk_len = (COPY_CHUNK_SIZE / std::mem::size_of::<T>()).max(1);
        let chunk = vec![value; chunk_len.min(len)];
        let mut written = 0usize;
        while written < len {
            let count = (len - written).min(chunk.len());
            self.queue.write_buffer(
                &self.buffer,
                bytes_for::<T>(written),
                bytemuck::cast_slice(&chunk[..count]),
            );
            written += count;
        }
    }

    pub(crate) fn write(&mut self, index: usize, data: &[T]) {
        let offset = bytes_for::<T>(index);
        let raw = bytemuck::cast_slice(data);
        let end = offset.saturating_add(raw.len() as BufferAddress);
        self.ensure_capacity_bytes(end);
        if offset > self.len {
            self.zero_range(self.len, offset - self.len);
        }
        if !raw.is_empty() {
            self.queue.write_buffer(&self.buffer, offset, raw);
        }
        self.len = self.len.max(end);
    }

    pub(crate) fn push(&mut self, value: T) {
        self.extend_from_slice(std::slice::from_ref(&value));
    }

    pub(crate) fn extend_from_slice(&mut self, data: &[T]) {
        self.write(elements_for::<T>(self.len), data);
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.len = self.len.min(bytes_for::<T>(len));
    }

    pub(crate) fn as_entire_binding(&self) -> ::wgpu::BindingResource<'_> {
        ::wgpu::BindingResource::Buffer(::wgpu::BufferBinding {
            buffer: &self.buffer,
            offset: 0,
            size: None,
        })
    }

    fn zero_range(&self, offset: BufferAddress, size: BufferAddress) {
        if size == 0 {
            return;
        }

        let zeroes = [0u8; COPY_CHUNK_SIZE];
        let mut written = 0u64;
        while written < size {
            let remaining = (size - written) as usize;
            let chunk = remaining.min(zeroes.len());
            self.queue
                .write_buffer(&self.buffer, offset + written, &zeroes[..chunk]);
            written += chunk as u64;
        }
    }
}

pub(crate) struct GpuImageBuffer {
    pixels: WgpuBuffer<u32>,
    width: u32,
    height: u32,
}

impl GpuImageBuffer {
    pub(crate) fn new(
        device: Device,
        queue: Queue,
        usage: BufferUsages,
        width: u32,
        height: u32,
        label: &'static str,
    ) -> Self {
        let mut image = Self {
            pixels: WgpuBuffer::new(device, queue, usage, 0, label),
            width: 0,
            height: 0,
        };
        image.resize_zeroed(width, height);
        image
    }

    pub(crate) fn resize_zeroed(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels.resize_zeroed(width as usize * height as usize);
    }

    pub(crate) fn clear(&mut self, color: u32) {
        let len = self.width as usize * self.height as usize;
        self.pixels.fill(len, color);
    }

    pub(crate) fn buffer(&self) -> &Buffer {
        self.pixels.buffer()
    }

    pub(crate) fn as_entire_binding(&self) -> ::wgpu::BindingResource<'_> {
        self.pixels.as_entire_binding()
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }
}

fn create_buffer(
    device: &Device,
    usage: BufferUsages,
    capacity: BufferAddress,
    label: &'static str,
) -> Buffer {
    device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some(label),
        size: capacity.max(1),
        usage: usage | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn bytes_for<T>(count: usize) -> BufferAddress {
    (count * std::mem::size_of::<T>()) as BufferAddress
}

fn elements_for<T>(bytes: BufferAddress) -> usize {
    let elem_size = std::mem::size_of::<T>() as BufferAddress;
    debug_assert!(elem_size != 0);
    debug_assert!(bytes.is_multiple_of(elem_size));
    (bytes / elem_size) as usize
}
