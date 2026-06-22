use std::cell::RefCell;

use wgpu::{Buffer, BufferUsages, Device, Queue};

use crate::memory::{Allocation, Memory as MemoryTrait};

pub struct Memory {
    inner: RefCell<WgpuMemoryInner>,
    device: Device,
    queue: Queue,
    bump: usize,
}

impl Memory {
    pub fn device(&self) -> &Device {
        &self.device
    }
    pub fn queue(&self) -> &Queue {
        &self.queue
    }
    pub fn bump(&self) -> usize {
        self.bump
    }
    pub fn allocate_image(&mut self, width: u32, height: u32, clear: peniko::Color) -> Allocation {
        let allocation = self.allocate((width * height) as usize * 4, 4);
        let px = u32::from_le_bytes(clear.to_rgba8().to_u8_array());
        self.write_at(
            allocation.offset,
            bytemuck::cast_slice(&vec![px; (width * height) as usize]),
        );
        allocation
    }
}

struct WgpuMemoryInner {
    device_buffer: Buffer,
    capacity: usize,
    generation: u64,
    mirror: Vec<u8>,
}

impl MemoryTrait for Memory {
    type Buffer<'a> = &'a Buffer;

    fn allocate(&mut self, size: usize, align: usize) -> crate::memory::Allocation {
        let align = align.max(256);
        let old_bump = self.bump;
        let bump = Self::align_up(old_bump, align);
        let end = bump.saturating_add(size);
        self.ensure_capacity(end);
        // Reused renderers keep the arena alive across frames, so both the alignment padding and
        // the newly allocated range must be initialized before any shader can legally observe them.
        if bump > old_bump {
            self.write_at(old_bump, &vec![0; bump - old_bump]);
        }
        if size != 0 {
            self.write_at(bump, &vec![0; size]);
        }
        let alloc = Allocation { offset: bump, size };
        self.bump = end;
        alloc
    }

    fn capacity(&self) -> usize {
        self.inner.borrow().capacity
    }

    fn generation(&self) -> u64 {
        self.inner.borrow().generation
    }

    fn ensure_capacity(&self, min_len: usize) {
        let mut inner = self.inner.borrow_mut();
        if min_len <= inner.capacity {
            return;
        }

        let mut capacity = inner.capacity;
        while capacity < min_len {
            capacity = capacity.saturating_mul(2).max(min_len);
        }

        inner.mirror.resize(capacity, 0);
        inner.device_buffer = create_buffer(&self.device, capacity);
        inner.capacity = capacity;
        inner.generation = inner.generation.wrapping_add(1);
        self.queue
            .write_buffer(&inner.device_buffer, 0, &inner.mirror);
    }

    fn with_buffer<T>(&self, f: impl FnOnce(Self::Buffer<'_>) -> T) -> T {
        let inner = self.inner.borrow();
        f(&inner.device_buffer)
    }

    fn write_at(&self, offset: usize, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let end = offset + data.len();
        debug_assert!(end <= inner.capacity);
        inner.mirror[offset..end].copy_from_slice(data);
        self.queue
            .write_buffer(&inner.device_buffer, offset as u64, data);
    }

    fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.mirror.fill(0);
        self.queue
            .write_buffer(&inner.device_buffer, 0, &inner.mirror);
    }

    fn read_range(&self, offset: usize, size: usize) -> Vec<u8> {
        if size == 0 {
            return Vec::new();
        }
        let inner = self.inner.borrow();
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("wgpu_memory_readback"),
            size: size as u64,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wgpu_memory_readback_encoder"),
            });
        encoder.copy_buffer_to_buffer(
            &inner.device_buffer,
            offset as u64,
            &staging,
            0,
            size as u64,
        );
        drop(inner);
        self.queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            if let Err(e) = sender.send(result) {
                panic!("Failed to send mapping result: {e}");
            }
        });
        let pool_res = self.device.poll(wgpu::PollType::wait_indefinitely());
        if let Err(e) = pool_res {
            panic!("Failed to poll device: {e}");
        }
        let res = receiver.recv().unwrap_or_else(|e| {
            panic!(
                "Failed to receive mapping result: {e}, {}",
                std::panic::Location::caller()
            )
        });
        if let Err(e) = res {
            panic!("wgpu memory readback failed: {e}\n",);
        }
        let mapped = slice.get_mapped_range();
        let out = mapped.to_vec();
        drop(mapped);
        staging.unmap();
        out
    }
}

fn create_buffer(device: &Device, capacity: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wgpu_memory_arena"),
        size: capacity as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}
