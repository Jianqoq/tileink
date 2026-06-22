use std::cell::RefCell;

use crate::memory::{Allocation, Memory};

pub struct CpuMemory {
    inner: RefCell<CpuMemoryInner>,
    bump: usize,
}

pub struct CpuMemoryInner {
    buffer: Vec<u8>,
    capacity: usize,
    generation: u64,
}

impl Memory for CpuMemory {
    type Buffer<'a> = &'a [u8];

    fn allocate(&mut self, size: usize, align: usize) -> Allocation {
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
        inner.buffer.resize(capacity, 0);
        inner.capacity = capacity;
        inner.generation = inner.generation.wrapping_add(1);
    }

    fn with_buffer<T>(&self, f: impl FnOnce(Self::Buffer<'_>) -> T) -> T {
        let inner = self.inner.borrow();
        f(&inner.buffer)
    }

    fn write_at(&self, offset: usize, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        let end = offset + data.len();
        debug_assert!(end <= inner.capacity);
        inner.buffer[offset..end].copy_from_slice(data);
    }

    fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.buffer.fill(0);
    }

    fn read_range(&self, offset: usize, size: usize) -> Vec<u8> {
        self.inner.borrow().buffer[offset..offset + size].to_vec()
    }
}
