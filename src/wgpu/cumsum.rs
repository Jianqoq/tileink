use crate::shared::gpu_plan::{CUMSUM_CHUNK_SIZE, GpuBufferLengths};

use super::canvas::{WgpuCumsumBindings, WgpuScanBuffers, WgpuSceneBuffers};
use super::commands::{
    WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
};
use super::incremental::ActiveScanPlan;
use super::lazy::{LazyComputePipeline, LazyShaderModule, PipelineCompilationTracker};
use super::profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope};

const WORKGROUP_SIZE: u32 = 256;
const STORAGE_BINDING_COUNT: u32 = 7;

#[repr(C)]
#[derive(Clone, Copy)]
struct CumsumConfig {
    row_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

unsafe impl bytemuck::Zeroable for CumsumConfig {}
unsafe impl bytemuck::Pod for CumsumConfig {}

pub(crate) struct WgpuCumsumPipeline {
    shader: LazyShaderModule,
    prefix_chunks: LazyComputePipeline,
    chunk_offsets: LazyComputePipeline,
    apply_chunk_offsets: LazyComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    pipeline_layout: ::wgpu::PipelineLayout,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
}

impl WgpuCumsumPipeline {
    pub(crate) fn new(
        device: &::wgpu::Device,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Option<Self> {
        if device.limits().max_storage_buffers_per_shader_stage < STORAGE_BINDING_COUNT {
            return None;
        }

        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu cumsum bind group layout"),
                entries: &cumsum_layout_entries(),
            });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu cumsum pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let config_size = std::mem::size_of::<CumsumConfig>() as ::wgpu::BufferAddress;
        let config_stride = aligned_uniform_stride(device, config_size);
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu cumsum config"),
            size: uniform_slots_buffer_size(device, config_size),
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Some(Self {
            shader: LazyShaderModule::new("tileink wgpu cumsum shader"),
            prefix_chunks: LazyComputePipeline::new(
                "cumsum_prefix_chunks",
                "cumsum_prefix_chunks",
                pipeline_cache,
                compilation_tracker,
            ),
            chunk_offsets: LazyComputePipeline::new(
                "cumsum_chunk_offsets",
                "cumsum_chunk_offsets",
                pipeline_cache,
                compilation_tracker,
            ),
            apply_chunk_offsets: LazyComputePipeline::new(
                "cumsum_apply_chunk_offsets",
                "cumsum_apply_chunk_offsets",
                pipeline_cache,
                compilation_tracker,
            ),
            bind_group_layout,
            pipeline_layout,
            config,
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
        let mut commands = WgpuCommandBatch::new(device, queue, "tileink wgpu cumsum encoder");
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
        let _profile_scope = start_cpu_scope("cumsum");
        let chunk_count = active.map_or(lengths.cumsum_chunk_count as u32, |plan| {
            plan.cumsum.chunk_lens.len() as u32
        });
        if chunk_count == 0 {
            return;
        }

        let config_offset = commands.write_uniform_slot(
            "cumsum.config",
            &self.config,
            self.config_size,
            self.config_stride,
            WGPU_CONFIG_SLOTS,
            bytemuck::bytes_of(&CumsumConfig {
                row_count: active.map_or(lengths.cumsum_row_count as u32, |plan| {
                    plan.cumsum.row_chunk_starts.len() as u32
                }),
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            }),
        );
        let bindings = canvas.cumsum_bindings(scan, active.is_some());
        let bind_group = self.create_bind_group(commands.device(), &bindings, config_offset);
        let row_count = active.map_or(lengths.cumsum_row_count as u32, |plan| {
            plan.cumsum.row_chunk_starts.len() as u32
        });
        let prefix_chunks = self.prefix_chunks(commands.device());
        let chunk_offsets =
            (row_count != chunk_count).then(|| self.chunk_offsets(commands.device()));
        let apply_chunk_offsets =
            (row_count != chunk_count).then(|| self.apply_chunk_offsets(commands.device()));
        let gpu_scope = start_gpu_scope(commands.device(), "cumsum");
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu cumsum pass"),
                timestamp_writes,
            });
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_pipeline(prefix_chunks);
            pass.dispatch_workgroups(chunk_count, 1, 1);

            if let (Some(chunk_offsets), Some(apply_chunk_offsets)) =
                (chunk_offsets, apply_chunk_offsets)
            {
                pass.set_pipeline(chunk_offsets);
                pass.dispatch_workgroups(row_count.div_ceil(WORKGROUP_SIZE), 1, 1);
                pass.set_pipeline(apply_chunk_offsets);
                pass.dispatch_workgroups(chunk_count, 1, 1);
            }
        }
        finish_gpu_scope(encoder, gpu_scope);
    }

    fn shader(&self, device: &::wgpu::Device) -> &::wgpu::ShaderModule {
        self.shader.get(device, || {
            ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_cumsum.wgsl")).into(),
            )
        })
    }

    fn prefix_chunks(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.prefix_chunks
            .get(device, &self.pipeline_layout, self.shader(device))
    }

    fn chunk_offsets(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.chunk_offsets
            .get(device, &self.pipeline_layout, self.shader(device))
    }

    fn apply_chunk_offsets(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.apply_chunk_offsets
            .get(device, &self.pipeline_layout, self.shader(device))
    }

    #[cfg(test)]
    pub(crate) fn initialized_pipeline_count(&self) -> usize {
        [
            &self.prefix_chunks,
            &self.chunk_offsets,
            &self.apply_chunk_offsets,
        ]
        .into_iter()
        .filter(|pipeline| pipeline.is_initialized())
        .count()
    }

    fn create_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuCumsumBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu cumsum bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.chunk_backdrop_offsets),
                bind_buffer(2, bindings.chunk_lens),
                bind_buffer(3, bindings.row_chunk_starts),
                bind_buffer(4, bindings.row_chunk_ends),
                bind_buffer(5, bindings.backdrops),
                bind_buffer(6, bindings.chunk_totals),
                bind_buffer(7, bindings.chunk_offsets),
            ],
        })
    }
}

fn cumsum_layout_entries() -> [::wgpu::BindGroupLayoutEntry; 8] {
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
        storage_entry(5, false),
        storage_entry(6, false),
        storage_entry(7, false),
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

const _: () = assert!(CUMSUM_CHUNK_SIZE == WORKGROUP_SIZE);
