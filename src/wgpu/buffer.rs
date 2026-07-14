use bytemuck::Pod;
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use super::lazy::{LazyComputePipeline, LazyShaderModule, PipelineCompilationTracker};

static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);
const RANGE_SCATTER_THRESHOLD: usize = 128;

pub(crate) struct WgpuRangeScatterPipeline {
    bind_group_layout: ::wgpu::BindGroupLayout,
    pipeline_layout: ::wgpu::PipelineLayout,
    shader: LazyShaderModule,
    pipeline: LazyComputePipeline,
}

impl WgpuRangeScatterPipeline {
    pub(crate) fn new(
        device: &::wgpu::Device,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Self {
        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu range scatter bind group layout"),
                entries: &[
                    ::wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ::wgpu::ShaderStages::COMPUTE,
                        ty: ::wgpu::BindingType::Buffer {
                            ty: ::wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    ::wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ::wgpu::ShaderStages::COMPUTE,
                        ty: ::wgpu::BindingType::Buffer {
                            ty: ::wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu range scatter pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        Self {
            bind_group_layout,
            pipeline_layout,
            shader: LazyShaderModule::new("tileink wgpu range scatter shader"),
            pipeline: LazyComputePipeline::new(
                "tileink wgpu range scatter pipeline",
                "main",
                pipeline_cache,
                compilation_tracker,
            ),
        }
    }

    fn pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        let shader = self.shader.get(device, || {
            ::wgpu::ShaderSource::Wgsl(include_str!("shaders/range_scatter.wgsl").into())
        });
        self.pipeline.get(device, &self.pipeline_layout, shader)
    }
}

pub(crate) struct WgpuRangeScatter {
    pipeline: Rc<WgpuRangeScatterPipeline>,
    encoder: Option<::wgpu::CommandEncoder>,
}

impl WgpuRangeScatter {
    pub(crate) fn new(pipeline: Rc<WgpuRangeScatterPipeline>) -> Self {
        Self {
            pipeline,
            encoder: None,
        }
    }

    fn encode(
        &mut self,
        device: &::wgpu::Device,
        bind_group: &::wgpu::BindGroup,
        range_count: u32,
    ) {
        let encoder = self.encoder.get_or_insert_with(|| {
            device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
                label: Some("tileink wgpu range scatter encoder"),
            })
        });
        let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
            label: Some("tileink wgpu range scatter pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(self.pipeline.pipeline(device));
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(range_count, 1, 1);
    }

    pub(crate) fn submit(&mut self, queue: &::wgpu::Queue) {
        if let Some(encoder) = self.encoder.take() {
            queue.submit([encoder.finish()]);
        }
    }
}

struct RangeScatterBinding {
    target_generation: u64,
    staging_generation: u64,
    bind_group: ::wgpu::BindGroup,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct WgpuBufferBindingKey {
    id: u64,
    generation: u64,
}

pub(crate) struct WgpuBuffer {
    id: u64,
    buffer: ::wgpu::Buffer,
    capacity: ::wgpu::BufferAddress,
    cached_upload: Vec<u8>,
    generation: u64,
    scatter_staging: Option<::wgpu::Buffer>,
    scatter_staging_capacity: ::wgpu::BufferAddress,
    scatter_staging_generation: u64,
    scatter_binding: Option<RangeScatterBinding>,
    scatter_words: Vec<u32>,
}

impl WgpuBuffer {
    pub(crate) fn new(device: &::wgpu::Device, label: &'static str) -> Self {
        Self {
            id: NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed),
            buffer: create_buffer(device, label, 4),
            capacity: 4,
            cached_upload: Vec::new(),
            generation: 1,
            scatter_staging: None,
            scatter_staging_capacity: 0,
            scatter_staging_generation: 0,
            scatter_binding: None,
            scatter_words: Vec::new(),
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
        scatter: &mut WgpuRangeScatter,
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
        validate_upload_ranges(label, ranges, data.len());
        if should_scatter_range_upload::<T>(ranges.len()) {
            if ranges.iter().all(std::ops::Range::is_empty) {
                return 0;
            }
            if scatter_upload_size::<T>(ranges).is_some_and(|size| {
                ranges.len() <= device.limits().max_compute_workgroups_per_dimension as usize
                    && self.capacity <= device.limits().max_storage_buffer_binding_size
                    && size <= device.limits().max_storage_buffer_binding_size
            }) {
                return self
                    .upload_ranges_scattered(device, queue, scatter, bytes, item_size, ranges);
            }
        }
        let mut uploaded = 0;
        for range in ranges {
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

    fn upload_ranges_scattered(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        scatter: &mut WgpuRangeScatter,
        bytes: &[u8],
        item_size: usize,
        ranges: &[std::ops::Range<usize>],
    ) -> usize {
        let payload_base = 4 + ranges.len() * 4;
        self.scatter_words.clear();
        self.scatter_words.resize(payload_base, 0);
        self.scatter_words[0] = u32::try_from(payload_base).expect("scatter header exceeds u32");
        self.scatter_words[1] =
            u32::try_from(ranges.len()).expect("scatter range count exceeds u32");
        let mut payload_words = 0usize;
        for (index, range) in ranges.iter().enumerate() {
            let byte_range = range.start * item_size..range.end * item_size;
            let descriptor = 4 + index * 4;
            self.scatter_words[descriptor] =
                u32::try_from(byte_range.start / 4).expect("scatter destination exceeds u32");
            self.scatter_words[descriptor + 1] =
                u32::try_from(payload_words).expect("scatter payload exceeds u32");
            self.scatter_words[descriptor + 2] =
                u32::try_from(byte_range.len() / 4).expect("scatter range exceeds u32");
            self.scatter_words
                .extend_from_slice(bytemuck::cast_slice(&bytes[byte_range.clone()]));
            self.cached_upload[byte_range.clone()].copy_from_slice(&bytes[byte_range]);
            payload_words += self.scatter_words[descriptor + 2] as usize;
        }

        let upload_bytes = bytemuck::cast_slice(self.scatter_words.as_slice());
        let required = upload_bytes.len() as ::wgpu::BufferAddress;
        if required > self.scatter_staging_capacity {
            let capacity =
                scatter_staging_capacity(required, device.limits().max_storage_buffer_binding_size);
            self.scatter_staging = Some(device.create_buffer(&::wgpu::BufferDescriptor {
                label: Some("tileink wgpu range scatter staging"),
                size: capacity,
                usage: ::wgpu::BufferUsages::STORAGE | ::wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.scatter_staging_capacity = capacity;
            self.scatter_staging_generation = self.scatter_staging_generation.wrapping_add(1);
            self.scatter_binding = None;
        }
        let staging = self
            .scatter_staging
            .as_ref()
            .expect("non-empty scatter upload has staging buffer");
        queue.write_buffer(staging, 0, upload_bytes);

        let binding_stale = self.scatter_binding.as_ref().is_none_or(|binding| {
            binding.target_generation != self.generation
                || binding.staging_generation != self.scatter_staging_generation
        });
        if binding_stale {
            self.scatter_binding = Some(RangeScatterBinding {
                target_generation: self.generation,
                staging_generation: self.scatter_staging_generation,
                bind_group: device.create_bind_group(&::wgpu::BindGroupDescriptor {
                    label: Some("tileink wgpu range scatter bind group"),
                    layout: &scatter.pipeline.bind_group_layout,
                    entries: &[
                        ::wgpu::BindGroupEntry {
                            binding: 0,
                            resource: staging.as_entire_binding(),
                        },
                        ::wgpu::BindGroupEntry {
                            binding: 1,
                            resource: self.buffer.as_entire_binding(),
                        },
                    ],
                }),
            });
        }
        scatter.encode(
            device,
            &self.scatter_binding.as_ref().unwrap().bind_group,
            ranges.len() as u32,
        );
        upload_bytes.len()
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

    pub(crate) fn binding_key(&self) -> WgpuBufferBindingKey {
        // Bind groups may outlive an upload, but not a buffer reallocation. The stable owner ID
        // also distinguishes otherwise identical generations in swapped local render contexts.
        WgpuBufferBindingKey {
            id: self.id,
            generation: self.generation,
        }
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

fn validate_upload_ranges(label: &str, ranges: &[std::ops::Range<usize>], len: usize) {
    let mut previous_end = 0;
    for range in ranges {
        assert!(
            range.start <= range.end && range.end <= len,
            "{label}: upload range {range:?} exceeds data length {len}"
        );
        if range.is_empty() {
            continue;
        }
        assert!(
            range.start >= previous_end,
            "upload ranges must be sorted and non-overlapping"
        );
        previous_end = range.end;
    }
}

fn should_scatter_range_upload<T>(range_count: usize) -> bool {
    range_count >= RANGE_SCATTER_THRESHOLD
        && std::mem::size_of::<T>().is_multiple_of(4)
        && std::mem::align_of::<T>() >= std::mem::align_of::<u32>()
}

fn scatter_upload_size<T>(ranges: &[std::ops::Range<usize>]) -> Option<u64> {
    let item_words = std::mem::size_of::<T>() / 4;
    let descriptor_words = ranges.len().checked_mul(4)?.checked_add(4)?;
    let payload_words = ranges.iter().try_fold(0usize, |total, range| {
        total.checked_add(range.len().checked_mul(item_words)?)
    })?;
    descriptor_words
        .checked_add(payload_words)?
        .checked_mul(4)?
        .try_into()
        .ok()
}

fn scatter_staging_capacity(required: u64, max_binding_size: u64) -> u64 {
    required.next_power_of_two().min(max_binding_size).max(4)
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

    use super::{
        changed_upload_range, required_storage_capacity, scatter_staging_capacity,
        should_scatter_range_upload, validate_upload_ranges,
    };

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

    #[test]
    fn four_word_aligned_ranges_select_scatter() {
        assert!(!should_scatter_range_upload::<u32>(3));
        assert!(should_scatter_range_upload::<u32>(4));
        assert!(!should_scatter_range_upload::<u8>(4));
    }

    #[test]
    fn scatter_ranges_accept_sorted_non_overlapping_input() {
        validate_upload_ranges("test", &[0..4, 8..12, 20..24], 24);
    }

    #[test]
    fn scatter_staging_growth_respects_non_power_of_two_binding_limit() {
        assert_eq!(scatter_staging_capacity(700, 1_000), 1_000);
        assert_eq!(scatter_staging_capacity(500, 1_000), 512);
    }

    #[test]
    #[should_panic(expected = "upload ranges must be sorted and non-overlapping")]
    fn scatter_ranges_reject_overlap_without_hashing_or_sorting() {
        validate_upload_ranges("test", &[0..4, 3..12], 12);
    }
}
