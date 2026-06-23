use std::num::NonZeroU64;

use wgpu::{Device, util::DeviceExt};

use crate::{
    shared::bd_record::BackdropRecord,
    wgpu::{buffer::WgpuBuffer, pipelines::utils::whole_buffer_binding},
};

const CUMSUM_SHADER: &str = include_str!("../../wgpu/shaders/backdrop_cumsum.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CumsumParams {
    pub path_count: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// wgpu compute backdrop_cumsum: one workgroup per path.
pub struct BackdropCumsumGpuPipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

pub struct BackdropCumsumPrepared {
    params_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    workgroups: u32,
}

impl BackdropCumsumPrepared {
    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, pipeline: &BackdropCumsumGpuPipeline) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("cumsum_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(self.workgroups, 1, 1);
    }
}

impl BackdropCumsumGpuPipeline {
    pub fn new(device: &Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cumsum_bind_group_layout"),
            entries: &[
                uniform_entry(0, std::mem::size_of::<CumsumParams>()),
                storage_entry(1, false),
                storage_entry(2, false),
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("backdrop_cumsum"),
            source: wgpu::ShaderSource::Wgsl(CUMSUM_SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cumsum_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("backdrop_cumsum"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
        }
    }

    pub fn prepare(
        &self,
        device: &Device,
        backdrops: &WgpuBuffer<BackdropRecord>,
        backdrop_pool: &WgpuBuffer<i32>,
        path_count: u32,
    ) -> BackdropCumsumPrepared {
        let params = CumsumParams {
            path_count,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cumsum_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cumsum_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: whole_buffer_binding(backdrops.buffer()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: whole_buffer_binding(backdrop_pool.buffer()),
                },
            ],
        });

        BackdropCumsumPrepared {
            params_buffer,
            bind_group,
            workgroups: path_count.max(1),
        }
    }
}

fn uniform_entry(binding: u32, size: usize) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: NonZeroU64::new(size as u64),
        },
        count: None,
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
