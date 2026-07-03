use crate::shared::gpu_plan::{GpuBufferLengths, SCAN_CHUNK_SIZE};

use super::commands::{
    WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
};
use super::profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope};
use super::canvas::{WgpuScanBindings, WgpuScanBuffers, WgpuSceneBuffers};

const WORKGROUP_SIZE: u32 = 256;
const STORAGE_BINDING_COUNT: u32 = 29;

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
    bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
}

impl WgpuScanPipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        if device.limits().max_storage_buffers_per_shader_stage < STORAGE_BINDING_COUNT {
            return None;
        }

        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu scan bind group layout"),
                entries: &scan_layout_entries(),
            });
        let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu scan shader"),
            source: ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_scan.wgsl")).into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu scan pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let config_size = std::mem::size_of::<ScanConfig>() as ::wgpu::BufferAddress;
        let config_stride = aligned_uniform_stride(device, config_size);
        Some(Self {
            clear: create_pipeline(device, &pipeline_layout, &shader, "scan_clear"),
            count: create_pipeline(device, &pipeline_layout, &shader, "scan_count"),
            prefix_chunks: create_pipeline(device, &pipeline_layout, &shader, "scan_prefix_chunks"),
            chunk_offsets: create_pipeline(device, &pipeline_layout, &shader, "scan_chunk_offsets"),
            apply_chunk_offsets: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "scan_apply_chunk_offsets",
            ),
            emit: create_pipeline(device, &pipeline_layout, &shader, "scan_emit"),
            bind_group_layout,
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
        let bind_group = self.create_bind_group(commands.device(), &bindings, config_offset);
        let gpu_scope = start_gpu_scope(commands.device(), "scan");
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu scan pass"),
                timestamp_writes,
            });
            pass.set_bind_group(0, &bind_group, &[]);
            if clear_len > 0 {
                pass.set_pipeline(&self.clear);
                pass.dispatch_workgroups(clear_len.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if line_count > 0 {
                pass.set_pipeline(&self.count);
                pass.dispatch_workgroups(line_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if scan_chunk_count > 0 {
                pass.set_pipeline(&self.prefix_chunks);
                pass.dispatch_workgroups(scan_chunk_count, 1, 1);
            }
            if path_count > 0 {
                pass.set_pipeline(&self.chunk_offsets);
                pass.dispatch_workgroups(path_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
            if scan_chunk_count > 0 {
                pass.set_pipeline(&self.apply_chunk_offsets);
                pass.dispatch_workgroups(scan_chunk_count, 1, 1);
            }
            if line_count > 0 && segment_capacity > 0 {
                pass.set_pipeline(&self.emit);
                pass.dispatch_workgroups(line_count.div_ceil(WORKGROUP_SIZE), 1, 1);
            }
        }
        finish_gpu_scope(encoder, gpu_scope);
    }

    fn create_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuScanBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu scan bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.line_path_ids),
                bind_buffer(2, bindings.line_p0x),
                bind_buffer(3, bindings.line_p0y),
                bind_buffer(4, bindings.line_p1x),
                bind_buffer(5, bindings.line_p1y),
                bind_buffer(6, bindings.path_flags),
                bind_buffer(7, bindings.backdrop_data_offsets),
                bind_buffer(8, bindings.backdrop_tile_x0),
                bind_buffer(9, bindings.backdrop_tile_y0),
                bind_buffer(10, bindings.backdrop_tile_x1),
                bind_buffer(11, bindings.backdrop_tile_y1),
                bind_buffer(12, bindings.scan_chunk_backdrop_offsets),
                bind_buffer(13, bindings.scan_chunk_lens),
                bind_buffer(14, bindings.scan_chunk_range_starts),
                bind_buffer(15, bindings.scan_chunk_range_ends),
                bind_buffer(16, bindings.backdrop_segment_starts),
                bind_buffer(17, bindings.backdrops),
                bind_buffer(18, bindings.tile_segment_range_starts),
                bind_buffer(19, bindings.tile_segment_range_ends),
                bind_buffer(20, bindings.segment_tile_counts),
                bind_buffer(21, bindings.segment_tile_cursors),
                bind_buffer(22, bindings.segment_bumps),
                bind_buffer(23, bindings.chunk_totals),
                bind_buffer(24, bindings.chunk_offsets),
                bind_buffer(25, bindings.segment_p0x),
                bind_buffer(26, bindings.segment_p0y),
                bind_buffer(27, bindings.segment_p1x),
                bind_buffer(28, bindings.segment_p1y),
                bind_buffer(29, bindings.segment_y_edge),
            ],
        })
    }
}

fn create_pipeline(
    device: &::wgpu::Device,
    layout: &::wgpu::PipelineLayout,
    shader: &::wgpu::ShaderModule,
    entry_point: &'static str,
) -> ::wgpu::ComputePipeline {
    device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
        label: Some(entry_point),
        layout: Some(layout),
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: ::wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

fn scan_layout_entries() -> [::wgpu::BindGroupLayoutEntry; 30] {
    [
        ::wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: ::wgpu::ShaderStages::COMPUTE,
            ty: ::wgpu::BindingType::Buffer {
                ty: ::wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, true),
        storage_entry(4, true),
        storage_entry(5, true),
        storage_entry(6, true),
        storage_entry(7, true),
        storage_entry(8, true),
        storage_entry(9, true),
        storage_entry(10, true),
        storage_entry(11, true),
        storage_entry(12, true),
        storage_entry(13, true),
        storage_entry(14, true),
        storage_entry(15, true),
        storage_entry(16, true),
        storage_entry(17, false),
        storage_entry(18, false),
        storage_entry(19, false),
        storage_entry(20, false),
        storage_entry(21, false),
        storage_entry(22, false),
        storage_entry(23, false),
        storage_entry(24, false),
        storage_entry(25, false),
        storage_entry(26, false),
        storage_entry(27, false),
        storage_entry(28, false),
        storage_entry(29, false),
    ]
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

const _: () = assert!(SCAN_CHUNK_SIZE == WORKGROUP_SIZE);
