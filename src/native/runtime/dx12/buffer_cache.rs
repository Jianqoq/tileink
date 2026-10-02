//! Available buffers from a fence-confirmed frame, kept separate by heap type.
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;

#[derive(Clone)]
pub(super) struct Buffer {
    pub resource: ID3D12Resource,
    capacity: u64,
}

impl From<ID3D12Resource> for Buffer {
    fn from(resource: ID3D12Resource) -> Self {
        Self {
            capacity: unsafe { resource.GetDesc().Width },
            resource,
        }
    }
}

/// A free list per completed submission. Keeping only the last retired frame
/// loses the other frame slots whenever resize drains the whole pipeline.
/// A list is created only when none is free, bounding the count by peak overlap.
#[derive(Clone, Default)]
pub(super) struct Pool {
    pub(super) frames: Vec<Vec<Buffer>>,
    idle: Vec<Buffer>,
    peak_frame_capacity: u64,
}

impl Pool {
    pub fn acquire(&mut self) -> Available {
        let mut resources = self.frames.pop().unwrap_or_default();
        resources.append(&mut self.idle);
        resources.into()
    }

    pub fn release_unused(&mut self, available: Available) {
        // Completed buffers skipped by a smaller frame remain available for a
        // later size instead of being destroyed and reallocated on every resize.
        self.idle.extend(available.0);
        let budget = self.peak_frame_capacity.saturating_mul(3);
        let mut total: u64 = self.idle.iter().map(|buffer| buffer.capacity).sum();
        while self.idle.len() > 256 || total > budget {
            let (index, capacity) = self
                .idle
                .iter()
                .enumerate()
                .min_by_key(|(_, buffer)| buffer.capacity)
                .map(|(index, buffer)| (index, buffer.capacity))
                .expect("idle pool exceeds budget");
            total -= capacity;
            self.idle.swap_remove(index);
        }
    }

    pub fn retire(&mut self, resources: Vec<Buffer>) {
        if !resources.is_empty() {
            self.peak_frame_capacity = self
                .peak_frame_capacity
                .max(resources.iter().map(|buffer| buffer.capacity).sum());
            self.frames.push(resources);
        }
    }
}

pub(super) struct Available(Vec<Buffer>);

impl From<Vec<Buffer>> for Available {
    fn from(mut resources: Vec<Buffer>) -> Self {
        // Capacity is immutable and travels with the COM owner. Reindex in place
        // instead of allocating and querying resource descriptions every frame.
        resources.sort_unstable_by_key(|buffer| buffer.capacity);
        Self(resources)
    }
}

impl Available {
    pub fn take(&mut self, size: usize) -> Option<Buffer> {
        // Recording order changes with scene contents. Best fit preserves larger
        // buffers for later requests instead of reallocating them every frame.
        let index = self
            .0
            .partition_point(|buffer| buffer.capacity < size as u64);
        (index < self.0.len()).then(|| self.0.remove(index))
    }
}
