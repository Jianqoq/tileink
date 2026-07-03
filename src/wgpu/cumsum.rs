use crate::shared::gpu_plan::{CUMSUM_CHUNK_SIZE, GpuBufferLengths};

use super::scene::{WgpuCumsumBindings, WgpuScanBuffers, WgpuSceneBuffers};

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
    prefix_chunks: ::wgpu::ComputePipeline,
    chunk_offsets: ::wgpu::ComputePipeline,
    apply_chunk_offsets: ::wgpu::ComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
}

impl WgpuCumsumPipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        if device.limits().max_storage_buffers_per_shader_stage < STORAGE_BINDING_COUNT {
            return None;
        }

        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu cumsum bind group layout"),
                entries: &cumsum_layout_entries(),
            });
        let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu cumsum shader"),
            source: ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_cumsum.wgsl")).into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu cumsum pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let prefix_chunks =
            create_pipeline(device, &pipeline_layout, &shader, "cumsum_prefix_chunks");
        let chunk_offsets =
            create_pipeline(device, &pipeline_layout, &shader, "cumsum_chunk_offsets");
        let apply_chunk_offsets = create_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "cumsum_apply_chunk_offsets",
        );
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu cumsum config"),
            size: std::mem::size_of::<CumsumConfig>() as ::wgpu::BufferAddress,
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Some(Self {
            prefix_chunks,
            chunk_offsets,
            apply_chunk_offsets,
            bind_group_layout,
            config,
        })
    }

    pub(crate) fn run(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        scene: &WgpuSceneBuffers,
        scan: &mut WgpuScanBuffers,
        lengths: GpuBufferLengths,
    ) {
        let chunk_count = lengths.cumsum_chunk_count as u32;
        if chunk_count == 0 {
            return;
        }

        queue.write_buffer(
            &self.config,
            0,
            bytemuck::bytes_of(&CumsumConfig {
                row_count: lengths.cumsum_row_count as u32,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            }),
        );
        let bindings = scene.cumsum_bindings(scan);
        let bind_group = self.create_bind_group(device, &bindings);
        let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
            label: Some("tileink wgpu cumsum encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu cumsum pass"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_pipeline(&self.prefix_chunks);
            pass.dispatch_workgroups(chunk_count, 1, 1);

            let row_count = lengths.cumsum_row_count as u32;
            if row_count != chunk_count {
                pass.set_pipeline(&self.chunk_offsets);
                pass.dispatch_workgroups(row_count.div_ceil(WORKGROUP_SIZE), 1, 1);
                pass.set_pipeline(&self.apply_chunk_offsets);
                pass.dispatch_workgroups(chunk_count, 1, 1);
            }
        }
        queue.submit([encoder.finish()]);
    }

    fn create_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuCumsumBindings<'_>,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu cumsum bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                bind_buffer(0, &self.config),
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

const _: () = assert!(CUMSUM_CHUNK_SIZE == WORKGROUP_SIZE);
