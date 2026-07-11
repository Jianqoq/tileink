use crate::shared::gpu_plan::{GpuBufferLengths, SCAN_CHUNK_SIZE};

use super::canvas::{WgpuScanBindings, WgpuScanBuffers, WgpuSceneBuffers};
use super::commands::{
    WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
};
use super::dispatch_2d;
use super::incremental::ActiveScanPlan;
use super::lazy::{LazyComputePipeline, LazyShaderModule, PipelineCompilationTracker};
use super::profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope};

const WORKGROUP_SIZE: u32 = 256;
const CLEAR_STORAGE_BINDING_COUNT: u32 = 8;
const COUNT_STORAGE_BINDING_COUNT: u32 = 5;
const PREFIX_STORAGE_BINDING_COUNT: u32 = 5;
const CHUNK_OFFSETS_STORAGE_BINDING_COUNT: u32 = 6;
const APPLY_CHUNK_OFFSETS_STORAGE_BINDING_COUNT: u32 = 5;
const EMIT_STORAGE_BINDING_COUNT: u32 = 5;
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
    incremental: u32,
    line_base: u32,
    path_base: u32,
    chunk_base: u32,
    backdrop_base: u32,
}

unsafe impl bytemuck::Zeroable for ScanConfig {}
unsafe impl bytemuck::Pod for ScanConfig {}

pub(crate) struct WgpuScanPipeline {
    clear: LazyScanKernel,
    count: LazyScanKernel,
    prefix_chunks: LazyScanKernel,
    chunk_offsets: LazyScanKernel,
    apply_chunk_offsets: LazyScanKernel,
    emit: LazyScanKernel,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
}

struct LazyScanKernel {
    shader: LazyShaderModule,
    pipeline: LazyComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    pipeline_layout: ::wgpu::PipelineLayout,
    source: &'static str,
}

impl LazyScanKernel {
    fn new(
        device: &::wgpu::Device,
        label: &'static str,
        source: &'static str,
        entry_point: &'static str,
        entries: &[::wgpu::BindGroupLayoutEntry],
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Self {
        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        Self {
            shader: LazyShaderModule::new(label),
            pipeline: LazyComputePipeline::new(
                label,
                entry_point,
                pipeline_cache,
                compilation_tracker,
            ),
            bind_group_layout,
            pipeline_layout,
            source,
        }
    }

    fn pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        let shader = self
            .shader
            .get(device, || ::wgpu::ShaderSource::Wgsl(self.source.into()));
        self.pipeline.get(device, &self.pipeline_layout, shader)
    }
}

impl WgpuScanPipeline {
    pub(crate) fn new(
        device: &::wgpu::Device,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Option<Self> {
        if device.limits().max_storage_buffers_per_shader_stage < max_storage_binding_count() {
            return None;
        }

        let clear = LazyScanKernel::new(
            device,
            "tileink wgpu scan clear",
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_clear.wgsl")),
            "scan_clear",
            &clear_layout_entries(),
            pipeline_cache,
            compilation_tracker,
        );
        let count = LazyScanKernel::new(
            device,
            "tileink wgpu scan count",
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_count.wgsl")),
            "scan_count",
            &count_layout_entries(),
            pipeline_cache,
            compilation_tracker,
        );
        let prefix_chunks = LazyScanKernel::new(
            device,
            "tileink wgpu scan prefix chunks",
            include_str!(concat!(
                env!("OUT_DIR"),
                "/tileink_wgpu_scan_prefix_chunks.wgsl"
            )),
            "scan_prefix_chunks",
            &prefix_layout_entries(),
            pipeline_cache,
            compilation_tracker,
        );
        let chunk_offsets = LazyScanKernel::new(
            device,
            "tileink wgpu scan chunk offsets",
            include_str!(concat!(
                env!("OUT_DIR"),
                "/tileink_wgpu_scan_chunk_offsets.wgsl"
            )),
            "scan_chunk_offsets",
            &chunk_offsets_layout_entries(),
            pipeline_cache,
            compilation_tracker,
        );
        let apply_chunk_offsets = LazyScanKernel::new(
            device,
            "tileink wgpu scan apply chunk offsets",
            include_str!(concat!(
                env!("OUT_DIR"),
                "/tileink_wgpu_scan_apply_chunk_offsets.wgsl"
            )),
            "scan_apply_chunk_offsets",
            &apply_chunk_offsets_layout_entries(),
            pipeline_cache,
            compilation_tracker,
        );
        let emit = LazyScanKernel::new(
            device,
            "tileink wgpu scan emit",
            include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan_emit.wgsl")),
            "scan_emit",
            &emit_layout_entries(),
            pipeline_cache,
            compilation_tracker,
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
        self.run_in(&mut commands, canvas, scan, lengths, None);
        commands.finish();
    }

    pub(crate) fn run_in(
        &self,
        commands: &mut WgpuCommandBatch,
        canvas: &WgpuSceneBuffers,
        scan: &mut WgpuScanBuffers,
        lengths: GpuBufferLengths,
        active: Option<&ActiveScanPlan>,
    ) {
        let _profile_scope = start_cpu_scope("scan");
        let backdrop_len = active.map_or(lengths.backdrop_len as u32, |plan| plan.backdrop_count);
        let path_count = active.map_or(lengths.path_count as u32, |plan| plan.path_count);
        let line_count = active.map_or(lengths.line_count as u32, |plan| plan.line_count);
        let scan_chunk_count =
            active.map_or(lengths.scan_chunk_count as u32, |plan| plan.chunk_count);
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
                incremental: u32::from(active.is_some()),
                line_base: active.map_or(0, |plan| plan.line_base),
                path_base: active.map_or(0, |plan| plan.path_base),
                chunk_base: active.map_or(0, |plan| plan.chunk_base),
                backdrop_base: active.map_or(0, |plan| plan.backdrop_base),
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
        let clear_pipeline = (clear_len > 0).then(|| self.clear.pipeline(commands.device()));
        let count_pipeline = (line_count > 0).then(|| self.count.pipeline(commands.device()));
        let prefix_chunks_pipeline =
            (scan_chunk_count > 0).then(|| self.prefix_chunks.pipeline(commands.device()));
        let chunk_offsets_pipeline =
            (path_count > 0).then(|| self.chunk_offsets.pipeline(commands.device()));
        let apply_chunk_offsets_pipeline =
            (scan_chunk_count > 0).then(|| self.apply_chunk_offsets.pipeline(commands.device()));
        let emit_pipeline =
            (line_count > 0 && segment_capacity > 0).then(|| self.emit.pipeline(commands.device()));
        let gpu_scope = start_gpu_scope(commands.device(), "scan");
        let max_workgroups = commands
            .device()
            .limits()
            .max_compute_workgroups_per_dimension;
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu scan pass"),
                timestamp_writes,
            });
            if let Some(clear_pipeline) = clear_pipeline {
                pass.set_bind_group(0, &clear_bind_group, &[]);
                pass.set_pipeline(clear_pipeline);
                pass.dispatch_workgroups(clear_len.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if let Some(count_pipeline) = count_pipeline {
                pass.set_bind_group(0, &count_bind_group, &[]);
                pass.set_pipeline(count_pipeline);
                pass.dispatch_workgroups(line_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if let Some(prefix_chunks_pipeline) = prefix_chunks_pipeline {
                pass.set_bind_group(0, &prefix_bind_group, &[]);
                pass.set_pipeline(prefix_chunks_pipeline);
                let (x, y) = dispatch_2d(scan_chunk_count, max_workgroups);
                pass.dispatch_workgroups(x, y, 1);
            }
            if let Some(chunk_offsets_pipeline) = chunk_offsets_pipeline {
                pass.set_bind_group(0, &chunk_offsets_bind_group, &[]);
                pass.set_pipeline(chunk_offsets_pipeline);
                pass.dispatch_workgroups(path_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if let Some(apply_chunk_offsets_pipeline) = apply_chunk_offsets_pipeline {
                pass.set_bind_group(0, &apply_chunk_offsets_bind_group, &[]);
                pass.set_pipeline(apply_chunk_offsets_pipeline);
                let (x, y) = dispatch_2d(scan_chunk_count, max_workgroups);
                pass.dispatch_workgroups(x, y, 1);
            }
            if let Some(emit_pipeline) = emit_pipeline {
                pass.set_bind_group(0, &emit_bind_group, &[]);
                pass.set_pipeline(emit_pipeline);
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
            layout: &self.clear.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.backdrops),
                bind_buffer(2, bindings.tile_segment_ranges),
                bind_buffer(3, bindings.segment_tile_counts),
                bind_buffer(4, bindings.segment_tile_cursors),
                bind_buffer(5, bindings.segment_bumps),
                bind_buffer(6, bindings.chunk_totals),
                bind_buffer(7, bindings.chunk_offsets),
                bind_buffer(8, bindings.active_indices),
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
            layout: &self.count.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.lines),
                bind_buffer(2, bindings.path_records),
                bind_buffer(3, bindings.backdrops),
                bind_buffer(4, bindings.segment_tile_counts),
                bind_buffer(5, bindings.active_indices),
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
            layout: &self.prefix_chunks.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.scan_chunks),
                bind_buffer(2, bindings.tile_segment_ranges),
                bind_buffer(3, bindings.segment_tile_counts),
                bind_buffer(4, bindings.chunk_totals),
                bind_buffer(5, bindings.active_indices),
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
            layout: &self.chunk_offsets.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.path_records),
                bind_buffer(2, bindings.scan_chunk_ranges),
                bind_buffer(3, bindings.segment_bumps),
                bind_buffer(4, bindings.chunk_totals),
                bind_buffer(5, bindings.chunk_offsets),
                bind_buffer(6, bindings.active_indices),
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
            layout: &self.apply_chunk_offsets.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.scan_chunks),
                bind_buffer(2, bindings.tile_segment_ranges),
                bind_buffer(3, bindings.segment_tile_cursors),
                bind_buffer(4, bindings.chunk_offsets),
                bind_buffer(5, bindings.active_indices),
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
            layout: &self.emit.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.lines),
                bind_buffer(2, bindings.path_records),
                bind_buffer(3, bindings.segment_tile_cursors),
                bind_buffer(4, bindings.segments),
                bind_buffer(5, bindings.active_indices),
            ],
        })
    }

    #[cfg(test)]
    pub(crate) fn initialized_pipeline_count(&self) -> usize {
        [
            &self.clear,
            &self.count,
            &self.prefix_chunks,
            &self.chunk_offsets,
            &self.apply_chunk_offsets,
            &self.emit,
        ]
        .into_iter()
        .filter(|kernel| kernel.pipeline.is_initialized())
        .count()
    }
}

fn clear_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, false),
        storage_entry(2, false),
        storage_entry(3, false),
        storage_entry(4, false),
        storage_entry(5, false),
        storage_entry(6, false),
        storage_entry(7, false),
        storage_entry(8, true),
    ]
}

fn count_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, false),
        storage_entry(4, false),
        storage_entry(5, true),
    ]
}

fn prefix_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, false),
        storage_entry(3, false),
        storage_entry(4, false),
        storage_entry(5, true),
    ]
}

fn chunk_offsets_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, false),
        storage_entry(4, false),
        storage_entry(5, false),
        storage_entry(6, true),
    ]
}

fn apply_chunk_offsets_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, false),
        storage_entry(3, false),
        storage_entry(4, false),
        storage_entry(5, true),
    ]
}

fn emit_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, false),
        storage_entry(4, false),
        storage_entry(5, true),
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

    #[test]
    fn scan_layout_entries_are_contiguous() {
        for entries in [
            clear_layout_entries(),
            count_layout_entries(),
            prefix_layout_entries(),
            chunk_offsets_layout_entries(),
            apply_chunk_offsets_layout_entries(),
            emit_layout_entries(),
        ] {
            assert_contiguous_bindings(&entries);
        }
    }

    fn assert_contiguous_bindings(entries: &[::wgpu::BindGroupLayoutEntry]) {
        for (expected, entry) in entries.iter().enumerate() {
            assert_eq!(entry.binding, expected as u32);
        }
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
