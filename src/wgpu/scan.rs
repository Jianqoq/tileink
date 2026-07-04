use crate::shared::gpu_plan::{GpuBufferLengths, SCAN_CHUNK_SIZE};

use super::canvas::{WgpuScanBindings, WgpuScanBuffers, WgpuSceneBuffers};
use super::commands::{
    WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
};
use super::profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope};

const WORKGROUP_SIZE: u32 = 256;
const CLEAR_STORAGE_BINDING_COUNT: u32 = 7;
const COUNT_STORAGE_BINDING_COUNT: u32 = 4;
const PREFIX_STORAGE_BINDING_COUNT: u32 = 5;
const CHUNK_OFFSETS_STORAGE_BINDING_COUNT: u32 = 6;
const APPLY_CHUNK_OFFSETS_STORAGE_BINDING_COUNT: u32 = 5;
const EMIT_STORAGE_BINDING_COUNT: u32 = 4;
const STORAGE_BINDING_COUNTS: [u32; 6] = [
    CLEAR_STORAGE_BINDING_COUNT,
    COUNT_STORAGE_BINDING_COUNT,
    PREFIX_STORAGE_BINDING_COUNT,
    CHUNK_OFFSETS_STORAGE_BINDING_COUNT,
    APPLY_CHUNK_OFFSETS_STORAGE_BINDING_COUNT,
    EMIT_STORAGE_BINDING_COUNT,
];

#[repr(C)]
#[derive(Clone, Copy)]
struct ScanConfig {
    clear_len: u32,
    backdrop_len: u32,
    path_count: u32,
    scan_chunk_count: u32,
    line_count: u32,
    segment_capacity: u32,
    _pad0: u32,
    _pad1: u32,
}

unsafe impl bytemuck::Zeroable for ScanConfig {}
unsafe impl bytemuck::Pod for ScanConfig {}

pub(crate) struct WgpuScanPipeline {
    clear: ::wgpu::ComputePipeline,
    count: ::wgpu::ComputePipeline,
    prefix_chunks: ::wgpu::ComputePipeline,
    chunk_offsets: ::wgpu::ComputePipeline,
    apply_chunk_offsets: ::wgpu::ComputePipeline,
    emit: ::wgpu::ComputePipeline,
    clear_bind_group_layout: ::wgpu::BindGroupLayout,
    count_bind_group_layout: ::wgpu::BindGroupLayout,
    prefix_bind_group_layout: ::wgpu::BindGroupLayout,
    chunk_offsets_bind_group_layout: ::wgpu::BindGroupLayout,
    apply_chunk_offsets_bind_group_layout: ::wgpu::BindGroupLayout,
    emit_bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
}

impl WgpuScanPipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        if device.limits().max_storage_buffers_per_shader_stage < max_storage_binding_count() {
            return None;
        }

        let (clear, clear_bind_group_layout) = create_kernel(
            device,
            "tileink wgpu scan clear",
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_clear.wgsl")),
            "scan_clear",
            &clear_layout_entries(),
        );
        let (count, count_bind_group_layout) = create_kernel(
            device,
            "tileink wgpu scan count",
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_count.wgsl")),
            "scan_count",
            &count_layout_entries(),
        );
        let (prefix_chunks, prefix_bind_group_layout) = create_kernel(
            device,
            "tileink wgpu scan prefix chunks",
            include_str!(concat!(
                env!("OUT_DIR"),
                "/tileink_wgpu_scan_prefix_chunks.wgsl"
            )),
            "scan_prefix_chunks",
            &prefix_layout_entries(),
        );
        let (chunk_offsets, chunk_offsets_bind_group_layout) = create_kernel(
            device,
            "tileink wgpu scan chunk offsets",
            include_str!(concat!(
                env!("OUT_DIR"),
                "/tileink_wgpu_scan_chunk_offsets.wgsl"
            )),
            "scan_chunk_offsets",
            &chunk_offsets_layout_entries(),
        );
        let (apply_chunk_offsets, apply_chunk_offsets_bind_group_layout) = create_kernel(
            device,
            "tileink wgpu scan apply chunk offsets",
            include_str!(concat!(
                env!("OUT_DIR"),
                "/tileink_wgpu_scan_apply_chunk_offsets.wgsl"
            )),
            "scan_apply_chunk_offsets",
            &apply_chunk_offsets_layout_entries(),
        );
        let (emit, emit_bind_group_layout) = create_kernel(
            device,
            "tileink wgpu scan emit",
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_emit.wgsl")),
            "scan_emit",
            &emit_layout_entries(),
        );

        let config_size = std::mem::size_of::<ScanConfig>() as ::wgpu::BufferAddress;
        let config_stride = aligned_uniform_stride(device, config_size);
        Some(Self {
            clear,
            count,
            prefix_chunks,
            chunk_offsets,
            apply_chunk_offsets,
            emit,
            clear_bind_group_layout,
            count_bind_group_layout,
            prefix_bind_group_layout,
            chunk_offsets_bind_group_layout,
            apply_chunk_offsets_bind_group_layout,
            emit_bind_group_layout,
            config: device.create_buffer(&::wgpu::BufferDescriptor {
                label: Some("tileink wgpu scan config"),
                size: uniform_slots_buffer_size(device, config_size),
                usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            config_size,
            config_stride,
        })
    }

    pub(crate) fn run(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &WgpuSceneBuffers,
        scan: &mut WgpuScanBuffers,
        lengths: GpuBufferLengths,
    ) {
        let mut commands = WgpuCommandBatch::new(device, queue, "tileink wgpu scan encoder");
        self.run_in(&mut commands, canvas, scan, lengths);
        commands.finish();
    }

    pub(crate) fn run_in(
        &self,
        commands: &mut WgpuCommandBatch,
        canvas: &WgpuSceneBuffers,
        scan: &mut WgpuScanBuffers,
        lengths: GpuBufferLengths,
    ) {
        let _profile_scope = start_cpu_scope("scan");
        let backdrop_len = lengths.backdrop_len as u32;
        let path_count = lengths.path_count as u32;
        let line_count = lengths.line_count as u32;
        let scan_chunk_count = lengths.scan_chunk_count as u32;
        let segment_capacity = lengths.segment_capacity as u32;
        let clear_len = backdrop_len.max(path_count).max(scan_chunk_count);
        let config_offset = commands.write_uniform_slot(
            "scan.config",
            &self.config,
            self.config_size,
            self.config_stride,
            WGPU_CONFIG_SLOTS,
            bytemuck::bytes_of(&ScanConfig {
                clear_len,
                backdrop_len,
                path_count,
                scan_chunk_count,
                line_count,
                segment_capacity,
                _pad0: 0,
                _pad1: 0,
            }),
        );

        let bindings = canvas.scan_bindings(scan);
        let clear_bind_group =
            self.create_clear_bind_group(commands.device(), &bindings, config_offset);
        let count_bind_group =
            self.create_count_bind_group(commands.device(), &bindings, config_offset);
        let prefix_bind_group =
            self.create_prefix_bind_group(commands.device(), &bindings, config_offset);
        let chunk_offsets_bind_group =
            self.create_chunk_offsets_bind_group(commands.device(), &bindings, config_offset);
        let apply_chunk_offsets_bind_group =
            self.create_apply_chunk_offsets_bind_group(commands.device(), &bindings, config_offset);
        let emit_bind_group =
            self.create_emit_bind_group(commands.device(), &bindings, config_offset);
        let gpu_scope = start_gpu_scope(commands.device(), "scan");
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu scan pass"),
                timestamp_writes,
            });
            if clear_len > 0 {
                pass.set_bind_group(0, &clear_bind_group, &[]);
                pass.set_pipeline(&self.clear);
                pass.dispatch_workgroups(clear_len.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if line_count > 0 {
                pass.set_bind_group(0, &count_bind_group, &[]);
                pass.set_pipeline(&self.count);
                pass.dispatch_workgroups(line_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if scan_chunk_count > 0 {
                pass.set_bind_group(0, &prefix_bind_group, &[]);
                pass.set_pipeline(&self.prefix_chunks);
                pass.dispatch_workgroups(scan_chunk_count, 1, 1);
            }
            if path_count > 0 {
                pass.set_bind_group(0, &chunk_offsets_bind_group, &[]);
                pass.set_pipeline(&self.chunk_offsets);
                pass.dispatch_workgroups(path_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if scan_chunk_count > 0 {
                pass.set_bind_group(0, &apply_chunk_offsets_bind_group, &[]);
                pass.set_pipeline(&self.apply_chunk_offsets);
                pass.dispatch_workgroups(scan_chunk_count, 1, 1);
            }
            if line_count > 0 && segment_capacity > 0 {
                pass.set_bind_group(0, &emit_bind_group, &[]);
                pass.set_pipeline(&self.emit);
                pass.dispatch_workgroups(line_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
        }
        finish_gpu_scope(encoder, gpu_scope);
    }

    fn create_clear_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuScanBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu scan clear bind group"),
            layout: &self.clear_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(7, bindings.backdrops),
                bind_buffer(8, bindings.tile_segment_ranges),
                bind_buffer(10, bindings.segment_tile_counts),
                bind_buffer(11, bindings.segment_tile_cursors),
                bind_buffer(12, bindings.segment_bumps),
                bind_buffer(13, bindings.chunk_totals),
                bind_buffer(14, bindings.chunk_offsets),
            ],
        })
    }

    fn create_count_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuScanBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu scan count bind group"),
            layout: &self.count_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.lines),
                bind_buffer(2, bindings.path_records),
                bind_buffer(7, bindings.backdrops),
                bind_buffer(10, bindings.segment_tile_counts),
            ],
        })
    }

    fn create_prefix_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuScanBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu scan prefix bind group"),
            layout: &self.prefix_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(3, bindings.scan_chunk_backdrop_offsets),
                bind_buffer(4, bindings.scan_chunk_lens),
                bind_buffer(8, bindings.tile_segment_ranges),
                bind_buffer(10, bindings.segment_tile_counts),
                bind_buffer(13, bindings.chunk_totals),
            ],
        })
    }

    fn create_chunk_offsets_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuScanBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu scan chunk offsets bind group"),
            layout: &self.chunk_offsets_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(2, bindings.path_records),
                bind_buffer(5, bindings.scan_chunk_range_starts),
                bind_buffer(6, bindings.scan_chunk_range_ends),
                bind_buffer(12, bindings.segment_bumps),
                bind_buffer(13, bindings.chunk_totals),
                bind_buffer(14, bindings.chunk_offsets),
            ],
        })
    }

    fn create_apply_chunk_offsets_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuScanBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu scan apply chunk offsets bind group"),
            layout: &self.apply_chunk_offsets_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(3, bindings.scan_chunk_backdrop_offsets),
                bind_buffer(4, bindings.scan_chunk_lens),
                bind_buffer(8, bindings.tile_segment_ranges),
                bind_buffer(11, bindings.segment_tile_cursors),
                bind_buffer(14, bindings.chunk_offsets),
            ],
        })
    }

    fn create_emit_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuScanBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu scan emit bind group"),
            layout: &self.emit_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.lines),
                bind_buffer(2, bindings.path_records),
                bind_buffer(11, bindings.segment_tile_cursors),
                bind_buffer(15, bindings.segments),
            ],
        })
    }
}

fn create_kernel(
    device: &::wgpu::Device,
    label: &'static str,
    source: &'static str,
    entry_point: &'static str,
    entries: &[::wgpu::BindGroupLayoutEntry],
) -> (::wgpu::ComputePipeline, ::wgpu::BindGroupLayout) {
    let bind_group_layout = device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries,
    });
    let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: ::wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some(entry_point),
        compilation_options: ::wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    (pipeline, bind_group_layout)
}

fn clear_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(7, false),
        storage_entry(8, false),
        storage_entry(10, false),
        storage_entry(11, false),
        storage_entry(12, false),
        storage_entry(13, false),
        storage_entry(14, false),
    ]
}

fn count_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(7, false),
        storage_entry(10, false),
    ]
}

fn prefix_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(3, true),
        storage_entry(4, true),
        storage_entry(8, false),
        storage_entry(10, false),
        storage_entry(13, false),
    ]
}

fn chunk_offsets_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(2, true),
        storage_entry(5, true),
        storage_entry(6, true),
        storage_entry(12, false),
        storage_entry(13, false),
        storage_entry(14, false),
    ]
}

fn apply_chunk_offsets_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(3, true),
        storage_entry(4, true),
        storage_entry(8, false),
        storage_entry(11, false),
        storage_entry(14, false),
    ]
}

fn emit_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(11, false),
        storage_entry(15, false),
    ]
}

fn uniform_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Buffer {
            ty: ::wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Buffer {
            ty: ::wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bind_buffer(binding: u32, buffer: &::wgpu::Buffer) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn bind_config_buffer(
    binding: u32,
    buffer: &::wgpu::Buffer,
    offset: ::wgpu::BufferAddress,
    size: ::wgpu::BufferAddress,
) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::Buffer(::wgpu::BufferBinding {
            buffer,
            offset,
            size: ::wgpu::BufferSize::new(size),
        }),
    }
}

fn max_storage_binding_count() -> u32 {
    STORAGE_BINDING_COUNTS.into_iter().max().unwrap_or(0)
}

const _: () = assert!(SCAN_CHUNK_SIZE == WORKGROUP_SIZE);

#[cfg(test)]
mod tests {
    use super::{
        APPLY_CHUNK_OFFSETS_STORAGE_BINDING_COUNT, CHUNK_OFFSETS_STORAGE_BINDING_COUNT,
        CLEAR_STORAGE_BINDING_COUNT, COUNT_STORAGE_BINDING_COUNT, EMIT_STORAGE_BINDING_COUNT,
        PREFIX_STORAGE_BINDING_COUNT, apply_chunk_offsets_layout_entries,
        chunk_offsets_layout_entries, clear_layout_entries, count_layout_entries,
        emit_layout_entries, max_storage_binding_count, prefix_layout_entries,
    };

    #[test]
    fn scan_pipeline_storage_bindings_are_split_by_kernel() {
        let counts = [
            ("clear", clear_layout_entries(), CLEAR_STORAGE_BINDING_COUNT),
            ("count", count_layout_entries(), COUNT_STORAGE_BINDING_COUNT),
            (
                "prefix",
                prefix_layout_entries(),
                PREFIX_STORAGE_BINDING_COUNT,
            ),
            (
                "chunk_offsets",
                chunk_offsets_layout_entries(),
                CHUNK_OFFSETS_STORAGE_BINDING_COUNT,
            ),
            (
                "apply_chunk_offsets",
                apply_chunk_offsets_layout_entries(),
                APPLY_CHUNK_OFFSETS_STORAGE_BINDING_COUNT,
            ),
            ("emit", emit_layout_entries(), EMIT_STORAGE_BINDING_COUNT),
        ];

        for (name, entries, expected) in counts {
            let actual = entries.iter().filter(|entry| is_storage(entry)).count() as u32;
            assert_eq!(actual, expected, "{name}");
            assert!(actual <= max_storage_binding_count(), "{name}");
        }
        assert_eq!(max_storage_binding_count(), CLEAR_STORAGE_BINDING_COUNT);
    }

    fn is_storage(entry: &::wgpu::BindGroupLayoutEntry) -> bool {
        matches!(
            entry.ty,
            ::wgpu::BindingType::Buffer {
                ty: ::wgpu::BufferBindingType::Storage { .. },
                ..
            }
        )
    }
}
