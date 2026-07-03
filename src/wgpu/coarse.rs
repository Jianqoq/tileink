#![allow(clippy::too_many_arguments)]

use crate::shared::gpu_plan::{COARSE_CHUNK_SIZE, GpuBufferLengths};

use super::scene::{WgpuCoarseBindings, WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers};

const WORKGROUP_SIZE: u32 = 256;
const STORAGE_BINDING_COUNT: u32 = 45;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WgpuCoarseBatch {
    pub(crate) draw_start: u32,
    pub(crate) draw_end: u32,
    pub(crate) layer_stack_start: u32,
    pub(crate) layer_stack_end: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CoarseConfig {
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_start: u32,
    draw_end: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    ptcl_capacity: u32,
    glyph_capacity: u32,
    chunk_count: u32,
    _pad0: u32,
    _pad1: u32,
}

unsafe impl bytemuck::Zeroable for CoarseConfig {}
unsafe impl bytemuck::Pod for CoarseConfig {}

pub(crate) struct WgpuCoarsePipeline {
    count: ::wgpu::ComputePipeline,
    ptcl_prefix_chunks: ::wgpu::ComputePipeline,
    ptcl_chunk_offsets: ::wgpu::ComputePipeline,
    ptcl_apply_chunk_offsets: ::wgpu::ComputePipeline,
    glyph_prefix_chunks: ::wgpu::ComputePipeline,
    glyph_chunk_offsets: ::wgpu::ComputePipeline,
    glyph_apply_chunk_offsets: ::wgpu::ComputePipeline,
    emit: ::wgpu::ComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
}

impl WgpuCoarsePipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        if device.limits().max_storage_buffers_per_shader_stage < STORAGE_BINDING_COUNT {
            return None;
        }

        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu coarse bind group layout"),
                entries: &coarse_layout_entries(),
            });
        let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu coarse shader"),
            source: ::wgpu::ShaderSource::Wgsl(include_str!("coarse.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu coarse pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu coarse config"),
            size: std::mem::size_of::<CoarseConfig>() as ::wgpu::BufferAddress,
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Some(Self {
            count: create_pipeline(device, &pipeline_layout, &shader, "coarse_count"),
            ptcl_prefix_chunks: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "coarse_ptcl_prefix_chunks",
            ),
            ptcl_chunk_offsets: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "coarse_ptcl_chunk_offsets",
            ),
            ptcl_apply_chunk_offsets: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "coarse_ptcl_apply_chunk_offsets",
            ),
            glyph_prefix_chunks: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "coarse_glyph_prefix_chunks",
            ),
            glyph_chunk_offsets: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "coarse_glyph_chunk_offsets",
            ),
            glyph_apply_chunk_offsets: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "coarse_glyph_apply_chunk_offsets",
            ),
            emit: create_pipeline(device, &pipeline_layout, &shader, "coarse_emit"),
            bind_group_layout,
            config,
        })
    }

    pub(crate) fn run(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        scene: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &mut WgpuCoarseBuffers,
        lengths: GpuBufferLengths,
        batch: WgpuCoarseBatch,
    ) {
        let tile_count = lengths.tile_count as u32;
        let chunk_count = lengths.coarse_chunk_count as u32;
        if tile_count == 0 || chunk_count == 0 {
            return;
        }

        queue.write_buffer(
            &self.config,
            0,
            bytemuck::bytes_of(&CoarseConfig {
                tile_count,
                tiles_width: lengths.tiles_width as u32,
                tiles_height: lengths.tiles_height as u32,
                draw_start: batch.draw_start,
                draw_end: batch.draw_end,
                layer_stack_start: batch.layer_stack_start,
                layer_stack_end: batch.layer_stack_end,
                ptcl_capacity: lengths.coarse_ptcl_capacity as u32,
                glyph_capacity: lengths.coarse_glyph_capacity as u32,
                chunk_count,
                _pad0: 0,
                _pad1: 0,
            }),
        );
        let bindings = scene.coarse_bindings(scan, coarse);
        let bind_group = self.create_bind_group(device, &bindings);
        let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
            label: Some("tileink wgpu coarse encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu coarse pass"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_pipeline(&self.count);
            pass.dispatch_workgroups(tile_count, 1, 1);

            pass.set_pipeline(&self.ptcl_prefix_chunks);
            pass.dispatch_workgroups(chunk_count, 1, 1);
            pass.set_pipeline(&self.ptcl_chunk_offsets);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(&self.ptcl_apply_chunk_offsets);
            pass.dispatch_workgroups(chunk_count, 1, 1);

            pass.set_pipeline(&self.glyph_prefix_chunks);
            pass.dispatch_workgroups(chunk_count, 1, 1);
            pass.set_pipeline(&self.glyph_chunk_offsets);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(&self.glyph_apply_chunk_offsets);
            pass.dispatch_workgroups(chunk_count, 1, 1);

            if batch.draw_start < batch.draw_end && lengths.coarse_ptcl_capacity > 0 {
                pass.set_pipeline(&self.emit);
                pass.dispatch_workgroups(tile_count, 1, 1);
            }
        }
        queue.submit([encoder.finish()]);
    }

    fn create_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuCoarseBindings<'_>,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu coarse bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                bind_buffer(0, &self.config),
                bind_buffer(1, bindings.draw_path_ids),
                bind_buffer(2, bindings.draw_glyph_run_ids),
                bind_buffer(3, bindings.glyph_run_starts),
                bind_buffer(4, bindings.glyph_run_counts),
                bind_buffer(5, bindings.glyph_image_ids),
                bind_buffer(6, bindings.glyph_x),
                bind_buffer(7, bindings.glyph_y),
                bind_buffer(8, bindings.glyph_image_left),
                bind_buffer(9, bindings.glyph_image_top),
                bind_buffer(10, bindings.glyph_image_width),
                bind_buffer(11, bindings.glyph_image_height),
                bind_buffer(12, bindings.draw_flags),
                bind_buffer(13, bindings.draw_brush_colors),
                bind_buffer(14, bindings.draw_pixel_x0),
                bind_buffer(15, bindings.draw_pixel_y0),
                bind_buffer(16, bindings.draw_pixel_x1),
                bind_buffer(17, bindings.draw_pixel_y1),
                bind_buffer(18, bindings.backdrop_data_offsets),
                bind_buffer(19, bindings.backdrop_tile_x0),
                bind_buffer(20, bindings.backdrop_tile_y0),
                bind_buffer(21, bindings.backdrop_tile_x1),
                bind_buffer(22, bindings.backdrop_tile_y1),
                bind_buffer(23, bindings.backdrops),
                bind_buffer(24, bindings.segment_starts),
                bind_buffer(25, bindings.segment_ends),
                bind_buffer(26, bindings.layer_stack_tags),
                bind_buffer(27, bindings.layer_stack_draws),
                bind_buffer(28, bindings.layer_stack_payloads),
                bind_buffer(29, bindings.tile_ptcl_counts),
                bind_buffer(30, bindings.tile_ptcl_range_starts),
                bind_buffer(31, bindings.tile_ptcl_range_ends),
                bind_buffer(32, bindings.tile_glyph_counts),
                bind_buffer(33, bindings.tile_glyph_range_starts),
                bind_buffer(34, bindings.tile_glyph_range_ends),
                bind_buffer(35, bindings.chunk_totals),
                bind_buffer(36, bindings.chunk_offsets),
                bind_buffer(37, bindings.glyph_chunk_totals),
                bind_buffer(38, bindings.glyph_chunk_offsets),
                bind_buffer(39, bindings.ptcl_tags),
                bind_buffer(40, bindings.ptcl_backdrops),
                bind_buffer(41, bindings.ptcl_fill_rules),
                bind_buffer(42, bindings.ptcl_segment_starts),
                bind_buffer(43, bindings.ptcl_segment_ends),
                bind_buffer(44, bindings.ptcl_colors),
                bind_buffer(45, bindings.glyph_indices),
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

fn coarse_layout_entries() -> [::wgpu::BindGroupLayoutEntry; 46] {
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
        storage_entry(17, true),
        storage_entry(18, true),
        storage_entry(19, true),
        storage_entry(20, true),
        storage_entry(21, true),
        storage_entry(22, true),
        storage_entry(23, false),
        storage_entry(24, true),
        storage_entry(25, true),
        storage_entry(26, true),
        storage_entry(27, true),
        storage_entry(28, true),
        storage_entry(29, false),
        storage_entry(30, false),
        storage_entry(31, false),
        storage_entry(32, false),
        storage_entry(33, false),
        storage_entry(34, false),
        storage_entry(35, false),
        storage_entry(36, false),
        storage_entry(37, false),
        storage_entry(38, false),
        storage_entry(39, false),
        storage_entry(40, false),
        storage_entry(41, false),
        storage_entry(42, false),
        storage_entry(43, false),
        storage_entry(44, false),
        storage_entry(45, false),
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

const _: () = assert!(COARSE_CHUNK_SIZE == WORKGROUP_SIZE);
