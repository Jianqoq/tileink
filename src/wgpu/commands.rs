pub(crate) struct WgpuCommandBatch {
    device: ::wgpu::Device,
    queue: ::wgpu::Queue,
    encoder: Option<::wgpu::CommandEncoder>,
    uniform_writes: Vec<UniformWriteArena>,
    has_work: bool,
    submissions: u32,
    root_batches_before_submit: Option<usize>,
    label: &'static str,
}

pub(crate) const WGPU_CONFIG_SLOTS: u64 = 4096;

struct UniformWriteArena {
    key: &'static str,
    buffer: ::wgpu::Buffer,
    size: ::wgpu::BufferAddress,
    stride: ::wgpu::BufferAddress,
    slots: u64,
    used_slots: u64,
    bytes: Vec<u8>,
}

impl WgpuCommandBatch {
    pub(crate) fn new(device: &::wgpu::Device, queue: &::wgpu::Queue, label: &'static str) -> Self {
        Self {
            device: device.clone(),
            queue: queue.clone(),
            // Allocate on first encoded command. A submitted batch must not create an unused
            // successor encoder merely so `finish` or `Drop` can discard it.
            encoder: None,
            uniform_writes: Vec::new(),
            has_work: false,
            submissions: 0,
            root_batches_before_submit: None,
            label,
        }
    }

    pub(crate) fn device(&self) -> &::wgpu::Device {
        &self.device
    }

    pub(crate) fn encoder(&mut self) -> &mut ::wgpu::CommandEncoder {
        self.has_work = true;
        self.encoder
            .get_or_insert_with(|| create_encoder(&self.device, self.label))
    }

    pub(crate) fn write_uniform_slot(
        &mut self,
        key: &'static str,
        buffer: &::wgpu::Buffer,
        size: ::wgpu::BufferAddress,
        stride: ::wgpu::BufferAddress,
        slots: u64,
        bytes: &[u8],
    ) -> ::wgpu::BufferAddress {
        debug_assert!(bytes.len() as ::wgpu::BufferAddress <= size);
        if let Some(ix) = self
            .uniform_writes
            .iter()
            .position(|arena| arena.key == key)
            && self.uniform_writes[ix].used_slots >= self.uniform_writes[ix].slots
        {
            self.submit_current();
        }

        let arena = match self
            .uniform_writes
            .iter()
            .position(|arena| arena.key == key)
        {
            Some(ix) => &mut self.uniform_writes[ix],
            None => {
                self.uniform_writes.push(UniformWriteArena {
                    key,
                    buffer: buffer.clone(),
                    size,
                    stride,
                    slots,
                    used_slots: 0,
                    bytes: Vec::with_capacity((stride * 8) as usize),
                });
                self.uniform_writes.last_mut().unwrap()
            }
        };
        debug_assert_eq!(arena.size, size);
        debug_assert_eq!(arena.stride, stride);

        let offset = arena.used_slots * arena.stride;
        arena.used_slots += 1;
        let start = offset as usize;
        let end = start + arena.size as usize;
        if arena.bytes.len() < end {
            arena.bytes.resize(end, 0);
        }
        arena.bytes[start..start + bytes.len()].copy_from_slice(bytes);
        offset
    }

    pub(crate) fn set_initial_root_batch_budget(&mut self, batches: usize) {
        self.root_batches_before_submit = Some(batches);
    }

    pub(crate) fn begin_root_batch(&mut self) {
        let Some(remaining) = self.root_batches_before_submit.take() else {
            return;
        };
        // A uniform-arena rollover may already have started the GPU. Do not add another
        // latency submission in that case. Wait for a real successor batch so a short frame
        // never submits an otherwise-final encoder merely to create an empty successor.
        if self.submissions == 0 {
            if remaining == 0 {
                self.submit_current();
            } else {
                self.root_batches_before_submit = Some(remaining - 1);
            }
        }
    }

    pub(crate) fn submit_current(&mut self) {
        if !self.has_work {
            self.uniform_writes.clear();
            return;
        }
        for arena in &self.uniform_writes {
            self.queue.write_buffer(&arena.buffer, 0, &arena.bytes);
        }
        self.uniform_writes.clear();
        let encoder = self
            .encoder
            .take()
            .expect("wgpu command batch encoder exists while submitting");
        self.queue.submit([encoder.finish()]);
        self.submissions += 1;
        self.has_work = false;
    }

    pub(crate) fn finish(mut self) -> u32 {
        self.submit_current();
        self.submissions
    }
}

impl Drop for WgpuCommandBatch {
    fn drop(&mut self) {
        self.submit_current();
    }
}

fn create_encoder(device: &::wgpu::Device, label: &'static str) -> ::wgpu::CommandEncoder {
    device.create_command_encoder(&::wgpu::CommandEncoderDescriptor { label: Some(label) })
}

pub(crate) fn aligned_uniform_stride(
    device: &::wgpu::Device,
    size: ::wgpu::BufferAddress,
) -> ::wgpu::BufferAddress {
    align_to(
        size,
        device.limits().min_uniform_buffer_offset_alignment as ::wgpu::BufferAddress,
    )
}

pub(crate) fn uniform_slots_buffer_size(
    device: &::wgpu::Device,
    size: ::wgpu::BufferAddress,
) -> ::wgpu::BufferAddress {
    aligned_uniform_stride(device, size) * WGPU_CONFIG_SLOTS
}

fn align_to(
    value: ::wgpu::BufferAddress,
    alignment: ::wgpu::BufferAddress,
) -> ::wgpu::BufferAddress {
    if alignment <= 1 {
        return value;
    }
    value.div_ceil(alignment) * alignment
}
