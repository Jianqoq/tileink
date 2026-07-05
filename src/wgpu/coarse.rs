#![allow(clippy::too_many_arguments)]

use crate::shared::gpu_plan::{COARSE_CHUNK_SIZE, GpuBufferLengths};

use super::canvas::{WgpuCoarseBindings, WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers};
use super::commands::{
    WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
};
use super::profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope};

const WORKGROUP_SIZE: u32 = 256;
const COUNT_STORAGE_BINDING_COUNT: u32 = 7;
const PREFIX_STORAGE_BINDING_COUNT: u32 = 8;
const EMIT_STORAGE_BINDING_COUNT: u32 = 8;

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
    text_run_count: u32,
    text_glyph_count: u32,
    tile_draw_index_count: u32,
    emit_chunk_capacity: u32,
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
    emit_chunk_counts: ::wgpu::ComputePipeline,
    emit_prefix_chunks: ::wgpu::ComputePipeline,
    emit_chunk_offsets: ::wgpu::ComputePipeline,
    emit_apply_chunk_offsets: ::wgpu::ComputePipeline,
    emit_fill_refs: ::wgpu::ComputePipeline,
    emit_chunk_particle_counts: ::wgpu::ComputePipeline,
    emit_chunk_particle_offsets: ::wgpu::ComputePipeline,
    emit: ::wgpu::ComputePipeline,
    emit_web: ::wgpu::ComputePipeline,
    count_bind_group_layout: ::wgpu::BindGroupLayout,
    prefix_bind_group_layout: ::wgpu::BindGroupLayout,
    emit_bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
    portable_emit: bool,
}

impl WgpuCoarsePipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        let portable_emit = !device
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        let max_storage = device.limits().max_storage_buffers_per_shader_stage;
        if max_storage
            < COUNT_STORAGE_BINDING_COUNT
                .max(PREFIX_STORAGE_BINDING_COUNT)
                .max(EMIT_STORAGE_BINDING_COUNT)
        {
            return None;
        }

        let count_layout_entries = count_layout_entries();
        let prefix_layout_entries = prefix_layout_entries();
        let emit_layout_entries = emit_layout_entries();
        let count_bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu coarse count bind group layout"),
                entries: &count_layout_entries,
            });
        let prefix_bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu coarse prefix bind group layout"),
                entries: &prefix_layout_entries,
            });
        let emit_bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu coarse emit bind group layout"),
                entries: &emit_layout_entries,
            });
        let count_shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu coarse count shader"),
            source: ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_count.wgsl")).into(),
            ),
        });
        let prefix_shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu coarse prefix shader"),
            source: ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_prefix.wgsl")).into(),
            ),
        });
        let emit_shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu coarse emit shader"),
            source: ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_emit.wgsl")).into(),
            ),
        });
        let emit_web_shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu coarse web emit shader"),
            source: ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(
                    env!("OUT_DIR"),
                    "/tileink_wgpu_coarse_emit_web.wgsl"
                ))
                .into(),
            ),
        });
        let count_pipeline_layout =
            device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
                label: Some("tileink wgpu coarse count pipeline layout"),
                bind_group_layouts: &[Some(&count_bind_group_layout)],
                immediate_size: 0,
            });
        let prefix_pipeline_layout =
            device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
                label: Some("tileink wgpu coarse prefix pipeline layout"),
                bind_group_layouts: &[Some(&prefix_bind_group_layout)],
                immediate_size: 0,
            });
        let emit_pipeline_layout =
            device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
                label: Some("tileink wgpu coarse emit pipeline layout"),
                bind_group_layouts: &[Some(&emit_bind_group_layout)],
                immediate_size: 0,
            });
        let config_size = std::mem::size_of::<CoarseConfig>() as ::wgpu::BufferAddress;
        let config_stride = aligned_uniform_stride(device, config_size);
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu coarse config"),
            size: uniform_slots_buffer_size(device, config_size),
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Some(Self {
            count: create_pipeline(
                device,
                &count_pipeline_layout,
                &count_shader,
                "coarse_count",
            ),
            ptcl_prefix_chunks: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_ptcl_prefix_chunks",
            ),
            ptcl_chunk_offsets: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_ptcl_chunk_offsets",
            ),
            ptcl_apply_chunk_offsets: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_ptcl_apply_chunk_offsets",
            ),
            glyph_prefix_chunks: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_glyph_prefix_chunks",
            ),
            glyph_chunk_offsets: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_glyph_chunk_offsets",
            ),
            glyph_apply_chunk_offsets: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_glyph_apply_chunk_offsets",
            ),
            emit_chunk_counts: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_emit_chunk_counts",
            ),
            emit_prefix_chunks: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_emit_prefix_chunks",
            ),
            emit_chunk_offsets: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_emit_chunk_offsets",
            ),
            emit_apply_chunk_offsets: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_emit_apply_chunk_offsets",
            ),
            emit_fill_refs: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_emit_fill_refs",
            ),
            emit_chunk_particle_counts: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_emit_chunk_particle_counts",
            ),
            emit_chunk_particle_offsets: create_pipeline(
                device,
                &prefix_pipeline_layout,
                &prefix_shader,
                "coarse_emit_chunk_particle_offsets",
            ),
            emit: create_pipeline(device, &emit_pipeline_layout, &emit_shader, "coarse_emit"),
            emit_web: create_pipeline(
                device,
                &emit_pipeline_layout,
                &emit_web_shader,
                "coarse_emit",
            ),
            count_bind_group_layout,
            prefix_bind_group_layout,
            emit_bind_group_layout,
            config,
            config_size,
            config_stride,
            portable_emit,
        })
    }

    #[cfg(test)]
    pub(crate) fn uses_portable_emit(&self) -> bool {
        self.portable_emit
    }

    pub(crate) fn run(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &mut WgpuCoarseBuffers,
        lengths: GpuBufferLengths,
        batch: WgpuCoarseBatch,
    ) {
        let mut commands = WgpuCommandBatch::new(device, queue, "tileink wgpu coarse encoder");
        self.encode_in(&mut commands, canvas, scan, coarse, lengths, batch);
        commands.finish();
    }

    pub(crate) fn encode_in(
        &self,
        commands: &mut WgpuCommandBatch,
        canvas: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &mut WgpuCoarseBuffers,
        lengths: GpuBufferLengths,
        batch: WgpuCoarseBatch,
    ) {
        let _profile_scope = start_cpu_scope("coarse");
        let tile_count = lengths.tile_count as u32;
        let chunk_count = lengths.coarse_chunk_count as u32;
        if tile_count == 0 || chunk_count == 0 {
            return;
        }

        let config_offset = commands.write_uniform_slot(
            "coarse.config",
            &self.config,
            self.config_size,
            self.config_stride,
            WGPU_CONFIG_SLOTS,
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
                text_run_count: lengths.text_run_count as u32,
                text_glyph_count: lengths.text_glyph_count as u32,
                tile_draw_index_count: lengths.tile_draw_index_count as u32,
                emit_chunk_capacity: lengths.tile_draw_chunk_count as u32,
            }),
        );
        let bindings = canvas.coarse_bindings(scan, coarse);
        let count_bind_group =
            self.create_count_bind_group(commands.device(), &bindings, config_offset);
        let prefix_bind_group =
            self.create_prefix_bind_group(commands.device(), &bindings, config_offset);
        let emit_bind_group =
            self.create_emit_bind_group(commands.device(), &bindings, config_offset);
        let gpu_scope = start_gpu_scope(commands.device(), "coarse");
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu coarse pass"),
                timestamp_writes,
            });
            pass.set_bind_group(0, &count_bind_group, &[]);
            pass.set_pipeline(&self.count);
            pass.dispatch_workgroups(tile_count, 1, 1);

            pass.set_bind_group(0, &prefix_bind_group, &[]);
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
                if self.portable_emit {
                    let emit_chunk_count = lengths.tile_draw_chunk_count as u32;
                    if emit_chunk_count > 0 {
                        pass.set_bind_group(0, &prefix_bind_group, &[]);
                        pass.set_pipeline(&self.emit_chunk_counts);
                        pass.dispatch_workgroups(chunk_count, 1, 1);
                        pass.set_pipeline(&self.emit_prefix_chunks);
                        pass.dispatch_workgroups(chunk_count, 1, 1);
                        pass.set_pipeline(&self.emit_chunk_offsets);
                        pass.dispatch_workgroups(1, 1, 1);
                        pass.set_pipeline(&self.emit_apply_chunk_offsets);
                        pass.dispatch_workgroups(chunk_count, 1, 1);
                        pass.set_pipeline(&self.emit_fill_refs);
                        pass.dispatch_workgroups(chunk_count, 1, 1);
                        pass.set_pipeline(&self.emit_chunk_particle_counts);
                        pass.dispatch_workgroups(emit_chunk_count, 1, 1);
                        pass.set_pipeline(&self.emit_chunk_particle_offsets);
                        pass.dispatch_workgroups(chunk_count, 1, 1);

                        pass.set_bind_group(0, &emit_bind_group, &[]);
                        pass.set_pipeline(&self.emit_web);
                        pass.dispatch_workgroups(emit_chunk_count, 1, 1);
                    }
                } else {
                    pass.set_bind_group(0, &emit_bind_group, &[]);
                    pass.set_pipeline(&self.emit);
                    pass.dispatch_workgroups(tile_count, 1, 1);
                }
            }
        }
        finish_gpu_scope(encoder, gpu_scope);
    }

    fn create_count_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuCoarseBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu coarse count bind group"),
            layout: &self.count_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.draw_records),
                bind_buffer(3, bindings.text_blob),
                bind_buffer(18, bindings.path_records),
                bind_buffer(19, bindings.backdrops),
                bind_buffer(20, bindings.segment_ranges),
                bind_buffer(22, bindings.layer_stack),
                bind_buffer(25, bindings.coarse_work),
            ],
        })
    }

    fn create_prefix_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuCoarseBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu coarse prefix bind group"),
            layout: &self.prefix_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.draw_records),
                bind_buffer(3, bindings.text_blob),
                bind_buffer(18, bindings.path_records),
                bind_buffer(19, bindings.backdrops),
                bind_buffer(20, bindings.segment_ranges),
                bind_buffer(22, bindings.layer_stack),
                bind_buffer(25, bindings.coarse_work),
                bind_buffer(31, bindings.chunk_records),
            ],
        })
    }

    fn create_emit_bind_group(
        &self,
        device: &::wgpu::Device,
        bindings: &WgpuCoarseBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu coarse emit bind group"),
            layout: &self.emit_bind_group_layout,
            entries: &[
                bind_config_buffer(0, &self.config, config_offset, self.config_size),
                bind_buffer(1, bindings.draw_records),
                bind_buffer(3, bindings.text_blob),
                bind_buffer(13, bindings.brush_blob),
                bind_buffer(18, bindings.path_records),
                bind_buffer(19, bindings.backdrops),
                bind_buffer(20, bindings.segment_ranges),
                bind_buffer(22, bindings.layer_stack),
                bind_buffer(25, bindings.coarse_work),
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

fn count_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(3, true),
        storage_entry(18, true),
        storage_entry(19, false),
        storage_entry(20, true),
        storage_entry(22, true),
        storage_entry(25, false),
    ]
}

fn prefix_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(3, true),
        storage_entry(18, true),
        storage_entry(19, false),
        storage_entry(20, true),
        storage_entry(22, true),
        storage_entry(25, false),
        storage_entry(31, false),
    ]
}

fn emit_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(3, true),
        storage_entry(13, true),
        storage_entry(18, true),
        storage_entry(19, false),
        storage_entry(20, true),
        storage_entry(22, true),
        storage_entry(25, false),
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

const _: () = assert!(COARSE_CHUNK_SIZE == WORKGROUP_SIZE);

#[cfg(test)]
mod tests {
    use super::{
        COUNT_STORAGE_BINDING_COUNT, EMIT_STORAGE_BINDING_COUNT, PREFIX_STORAGE_BINDING_COUNT,
        count_layout_entries, emit_layout_entries, prefix_layout_entries,
    };

    #[test]
    fn coarse_pipeline_storage_bindings_match_split_layouts() {
        assert_eq!(
            storage_count(&count_layout_entries()),
            COUNT_STORAGE_BINDING_COUNT
        );
        assert_eq!(
            storage_count(&prefix_layout_entries()),
            PREFIX_STORAGE_BINDING_COUNT
        );
        assert_eq!(
            storage_count(&emit_layout_entries()),
            EMIT_STORAGE_BINDING_COUNT
        );
        assert_eq!(COUNT_STORAGE_BINDING_COUNT, 7);
        assert_eq!(PREFIX_STORAGE_BINDING_COUNT, 8);
        assert_eq!(EMIT_STORAGE_BINDING_COUNT, 8);
    }

    fn storage_count(entries: &[::wgpu::BindGroupLayoutEntry]) -> u32 {
        entries.iter().filter(|entry| is_storage(entry)).count() as u32
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
