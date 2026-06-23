use std::num::NonZeroU64;

use wgpu::Device;
use wgpu::util::DeviceExt;

use crate::shared::{bd_record::BackdropRecord, line::Line, path::PathRecord};
use crate::wgpu::pipelines::utils::whole_buffer_binding;
use crate::wgpu::{buffer::WgpuBuffer, types::tile_seg::TileSegment};

const SCAN_SHADER: &str = include_str!("../../wgpu/shaders/scan_assign.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ScanParams {
    pub path_count: u32,
    pub width_in_tiles: u32,
    pub workgroup_count_x: u32,
    pub _pad: u32,
}

/// wgpu compute scan_assign: one workgroup per path.
pub struct ScanGpuPipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

#[allow(dead_code)]
pub struct ScanGpuPrepared {
    params_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    workgroups: u32,
}

impl ScanGpuPrepared {
    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, pipeline: &ScanGpuPipeline) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("scan_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.dispatch_workgroups(self.workgroups, 1, 1);
    }
}

impl ScanGpuPipeline {
    pub fn new(device: &Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scan_bind_group_layout"),
            entries: &[
                uniform_entry(0, std::mem::size_of::<ScanParams>()),
                storage_entry(1, false),
                storage_entry(2, false),
                storage_entry(3, false),
                storage_entry(4, false),
                storage_entry(5, false),
                storage_entry(6, false),
                storage_entry(7, false),
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scan_assign"),
            source: wgpu::ShaderSource::Wgsl(SCAN_SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scan_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("scan_assign"),
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
        paths: &WgpuBuffer<PathRecord>,
        backdrops: &WgpuBuffer<BackdropRecord>,
        lines: &WgpuBuffer<Line>,
        segments: &WgpuBuffer<TileSegment>,
        backdrop_pool: &WgpuBuffer<i32>,
        width_in_tiles: u32,
    ) -> ScanGpuPrepared {
        let path_count = paths.len() as u32;
        let starts_capacity = backdrop_pool
            .len()
            .saturating_add(path_count.max(1) as usize);
        let params = ScanParams {
            path_count,
            width_in_tiles,
            workgroup_count_x: path_count.max(1),
            _pad: 0,
        };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("scan_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let starts = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scan_starts"),
            size: (starts_capacity * std::mem::size_of::<i32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let cursors = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scan_cursors"),
            size: (backdrop_pool.len() * std::mem::size_of::<i32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scan_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: whole_buffer_binding(paths.buffer()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: whole_buffer_binding(lines.buffer()),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: whole_buffer_binding(backdrops.buffer()),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: whole_buffer_binding(backdrop_pool.buffer()),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: whole_buffer_binding(segments.buffer()),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: starts.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: cursors.as_entire_binding(),
                },
            ],
        });

        let workgroups = path_count.max(1);

        ScanGpuPrepared {
            params_buffer,
            bind_group,
            workgroups,
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
