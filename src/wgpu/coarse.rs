#![allow(clippy::too_many_arguments)]

use crate::shared::{
    gpu_coarse::coarse_work_active_tile_list_word_offset,
    gpu_plan::{COARSE_CHUNK_SIZE, GpuBufferLengths},
};

use super::canvas::{
    WgpuCoarseBindGroups, WgpuCoarseBindings, WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers,
};
use super::commands::{
    WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
};
use super::dispatch_2d;
use super::lazy::{LazyComputePipeline, LazyShaderModule, PipelineCompilationTracker};
use super::profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

const WORKGROUP_SIZE: u32 = 256;
const COUNT_STORAGE_BINDING_COUNT: u32 = 9;
const PREFIX_STORAGE_BINDING_COUNT: u32 = 10;
const EMIT_STORAGE_BINDING_COUNT: u32 = 9;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WgpuCoarseBatch {
    pub(crate) draw_start: u32,
    pub(crate) draw_end: u32,
    pub(crate) layer_stack_start: u32,
    pub(crate) layer_stack_end: u32,
    pub(crate) active_tile_count: Option<u32>,
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
    paint_brush_base: u32,
    text_enabled: u32,
    active_tile_count: u32,
    active_tile_list_base: u32,
    incremental: u32,
}

unsafe impl bytemuck::Zeroable for CoarseConfig {}
unsafe impl bytemuck::Pod for CoarseConfig {}

pub(crate) struct WgpuCoarsePipeline {
    count_shader: LazyShaderModule,
    prefix_shader: LazyShaderModule,
    emit_shader: LazyShaderModule,
    emit_chunk_shader: LazyShaderModule,
    count_bins: LazyCoarseKernel,
    count_tiles: LazyCoarseKernel,
    ptcl_prefix_chunks: LazyCoarseKernel,
    ptcl_chunk_offsets: LazyCoarseKernel,
    ptcl_apply_chunk_offsets: LazyCoarseKernel,
    glyph_prefix_chunks: LazyCoarseKernel,
    glyph_chunk_offsets: LazyCoarseKernel,
    glyph_apply_chunk_offsets: LazyCoarseKernel,
    emit_chunk_counts: LazyCoarseKernel,
    emit_prefix_chunks: LazyCoarseKernel,
    emit_chunk_offsets: LazyCoarseKernel,
    emit_apply_chunk_offsets: LazyCoarseKernel,
    emit_fill_refs: LazyCoarseKernel,
    emit_chunk_particle_counts: LazyCoarseKernel,
    emit_chunk_particle_offsets: LazyCoarseKernel,
    tile_counts_from_emit_chunks: LazyCoarseKernel,
    emit_bins: LazyCoarseKernel,
    emit_tiles: LazyCoarseKernel,
    emit_web: LazyCoarseKernel,
    emit_chunk_tile_kinds: LazyCoarseKernel,
    count_bind_group_layout: ::wgpu::BindGroupLayout,
    prefix_bind_group_layout: ::wgpu::BindGroupLayout,
    emit_bind_group_layout: ::wgpu::BindGroupLayout,
    count_pipeline_layout: ::wgpu::PipelineLayout,
    prefix_pipeline_layout: ::wgpu::PipelineLayout,
    emit_pipeline_layout: ::wgpu::PipelineLayout,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
}

#[derive(Clone, Copy)]
enum CoarseShaderKind {
    Count,
    Prefix,
    Emit,
    EmitChunk,
}

#[derive(Clone, Copy)]
enum CoarseLayoutKind {
    Count,
    Prefix,
    Emit,
}

struct LazyCoarseKernel {
    pipeline: LazyComputePipeline,
    shader: CoarseShaderKind,
    layout: CoarseLayoutKind,
}

impl LazyCoarseKernel {
    fn new(
        entry_point: &'static str,
        shader: CoarseShaderKind,
        layout: CoarseLayoutKind,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Self {
        Self {
            pipeline: LazyComputePipeline::new(
                entry_point,
                entry_point,
                pipeline_cache,
                compilation_tracker,
            ),
            shader,
            layout,
        }
    }
}

impl WgpuCoarsePipeline {
    pub(crate) fn new(
        device: &::wgpu::Device,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Option<Self> {
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

        let kernel = |entry_point, shader, layout| {
            LazyCoarseKernel::new(
                entry_point,
                shader,
                layout,
                pipeline_cache,
                compilation_tracker,
            )
        };
        Some(Self {
            count_shader: LazyShaderModule::new("tileink wgpu coarse count shader"),
            prefix_shader: LazyShaderModule::new("tileink wgpu coarse prefix shader"),
            emit_shader: LazyShaderModule::new("tileink wgpu coarse emit shader"),
            emit_chunk_shader: LazyShaderModule::new("tileink wgpu coarse chunk emit shader"),
            count_bins: kernel(
                "coarse_count_bins",
                CoarseShaderKind::Count,
                CoarseLayoutKind::Count,
            ),
            count_tiles: kernel(
                "coarse_count",
                CoarseShaderKind::Count,
                CoarseLayoutKind::Count,
            ),
            ptcl_prefix_chunks: kernel(
                "coarse_ptcl_prefix_chunks",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            ptcl_chunk_offsets: kernel(
                "coarse_ptcl_chunk_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            ptcl_apply_chunk_offsets: kernel(
                "coarse_ptcl_apply_chunk_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            glyph_prefix_chunks: kernel(
                "coarse_glyph_prefix_chunks",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            glyph_chunk_offsets: kernel(
                "coarse_glyph_chunk_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            glyph_apply_chunk_offsets: kernel(
                "coarse_glyph_apply_chunk_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_chunk_counts: kernel(
                "coarse_emit_chunk_counts",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_prefix_chunks: kernel(
                "coarse_emit_prefix_chunks",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_chunk_offsets: kernel(
                "coarse_emit_chunk_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_apply_chunk_offsets: kernel(
                "coarse_emit_apply_chunk_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_fill_refs: kernel(
                "coarse_emit_fill_refs",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_chunk_particle_counts: kernel(
                "coarse_emit_chunk_particle_counts",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_chunk_particle_offsets: kernel(
                "coarse_emit_chunk_particle_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            tile_counts_from_emit_chunks: kernel(
                "coarse_tile_counts_from_emit_chunks",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            emit_bins: kernel(
                "coarse_emit_bins",
                CoarseShaderKind::Emit,
                CoarseLayoutKind::Emit,
            ),
            emit_tiles: kernel(
                "coarse_emit",
                CoarseShaderKind::Emit,
                CoarseLayoutKind::Emit,
            ),
            emit_web: kernel(
                "coarse_emit",
                CoarseShaderKind::EmitChunk,
                CoarseLayoutKind::Emit,
            ),
            emit_chunk_tile_kinds: kernel(
                "coarse_emit_chunk_tile_kinds",
                CoarseShaderKind::EmitChunk,
                CoarseLayoutKind::Emit,
            ),
            count_bind_group_layout,
            prefix_bind_group_layout,
            emit_bind_group_layout,
            count_pipeline_layout,
            prefix_pipeline_layout,
            emit_pipeline_layout,
            config,
            config_size,
            config_stride,
        })
    }

    fn pipeline<'a>(
        &'a self,
        device: &::wgpu::Device,
        kernel: &'a LazyCoarseKernel,
    ) -> &'a ::wgpu::ComputePipeline {
        kernel.pipeline.get(
            device,
            self.layout_for(kernel.layout),
            self.shader_for(device, kernel.shader),
        )
    }

    fn shader_for(
        &self,
        device: &::wgpu::Device,
        shader: CoarseShaderKind,
    ) -> &::wgpu::ShaderModule {
        match shader {
            CoarseShaderKind::Count => self.count_shader.get(device, || {
                ::wgpu::ShaderSource::Wgsl(
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_count.wgsl"))
                        .into(),
                )
            }),
            CoarseShaderKind::Prefix => self.prefix_shader.get(device, || {
                ::wgpu::ShaderSource::Wgsl(
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_prefix.wgsl"))
                        .into(),
                )
            }),
            CoarseShaderKind::Emit => self.emit_shader.get(device, || {
                ::wgpu::ShaderSource::Wgsl(
                    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_coarse_emit.wgsl")).into(),
                )
            }),
            CoarseShaderKind::EmitChunk => self.emit_chunk_shader.get(device, || {
                ::wgpu::ShaderSource::Wgsl(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/tileink_wgpu_coarse_emit_web.wgsl"
                    ))
                    .into(),
                )
            }),
        }
    }

    fn layout_for(&self, layout: CoarseLayoutKind) -> &::wgpu::PipelineLayout {
        match layout {
            CoarseLayoutKind::Count => &self.count_pipeline_layout,
            CoarseLayoutKind::Prefix => &self.prefix_pipeline_layout,
            CoarseLayoutKind::Emit => &self.emit_pipeline_layout,
        }
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
        let bin_count = coarse_bin_count(lengths);
        let active_tile_count = batch.active_tile_count.unwrap_or(tile_count);
        let incremental = batch.active_tile_count.is_some();
        let prefix_chunk_count = if incremental {
            active_tile_count.div_ceil(WORKGROUP_SIZE)
        } else {
            chunk_count
        };
        let active_tile_list_base = coarse_work_active_tile_list_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
            lengths.tile_draw_index_count,
            lengths.tile_draw_chunk_count,
        ) as u32;

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
                chunk_count: prefix_chunk_count,
                text_run_count: lengths.text_run_count as u32,
                text_glyph_count: lengths.text_glyph_count as u32,
                tile_draw_index_count: lengths.tile_draw_index_count as u32,
                emit_chunk_capacity: lengths.tile_draw_chunk_count as u32,
                paint_brush_base: canvas.paint_brush_base(),
                text_enabled: u32::from(lengths.text_enabled),
                active_tile_count,
                active_tile_list_base,
                incremental: u32::from(incremental),
            }),
        );
        let bindings = canvas.coarse_bindings(scan, coarse);
        let bind_groups = {
            let _profile_scope = start_cpu_scope("coarse.bind_groups");
            let slot = (config_offset / self.config_stride) as usize;
            coarse.cached_bind_groups(bindings.key, slot, || WgpuCoarseBindGroups {
                count: self.create_count_bind_group(commands.device(), &bindings, config_offset),
                prefix: self.create_prefix_bind_group(commands.device(), &bindings, config_offset),
                emit: self.create_emit_bind_group(commands.device(), &bindings, config_offset),
            })
        };
        let count_bind_group = bind_groups.count;
        let prefix_bind_group = bind_groups.prefix;
        let emit_bind_group = bind_groups.emit;
        if profile_coarse_passes() && !incremental {
            self.encode_profiled_chunked(
                commands,
                &count_bind_group,
                &prefix_bind_group,
                &emit_bind_group,
                lengths,
                batch,
                chunk_count,
                bin_count,
            );
            return;
        }
        let emit_chunk_count = lengths.tile_draw_chunk_count as u32;
        let use_emit_chunks = !incremental
            && coarse_emit_chunks_enabled()
            && batch.draw_start < batch.draw_end
            && lengths.coarse_ptcl_capacity > 0
            && emit_chunk_count > 0;
        let device = commands.device();
        let max_workgroups = device.limits().max_compute_workgroups_per_dimension;
        let ptcl_prefix_chunks = self.pipeline(device, &self.ptcl_prefix_chunks);
        let ptcl_chunk_offsets = self.pipeline(device, &self.ptcl_chunk_offsets);
        let ptcl_apply_chunk_offsets = self.pipeline(device, &self.ptcl_apply_chunk_offsets);
        let glyph_prefix_chunks = self.pipeline(device, &self.glyph_prefix_chunks);
        let glyph_chunk_offsets = self.pipeline(device, &self.glyph_chunk_offsets);
        let glyph_apply_chunk_offsets = self.pipeline(device, &self.glyph_apply_chunk_offsets);
        let emit_chunk_counts =
            use_emit_chunks.then(|| self.pipeline(device, &self.emit_chunk_counts));
        let emit_prefix_chunks =
            use_emit_chunks.then(|| self.pipeline(device, &self.emit_prefix_chunks));
        let emit_chunk_offsets =
            use_emit_chunks.then(|| self.pipeline(device, &self.emit_chunk_offsets));
        let emit_apply_chunk_offsets =
            use_emit_chunks.then(|| self.pipeline(device, &self.emit_apply_chunk_offsets));
        let emit_fill_refs = use_emit_chunks.then(|| self.pipeline(device, &self.emit_fill_refs));
        let emit_chunk_particle_counts =
            use_emit_chunks.then(|| self.pipeline(device, &self.emit_chunk_particle_counts));
        let tile_counts_from_emit_chunks =
            use_emit_chunks.then(|| self.pipeline(device, &self.tile_counts_from_emit_chunks));
        let emit_chunk_particle_offsets =
            use_emit_chunks.then(|| self.pipeline(device, &self.emit_chunk_particle_offsets));
        let emit_web = use_emit_chunks.then(|| self.pipeline(device, &self.emit_web));
        let emit_chunk_tile_kinds =
            use_emit_chunks.then(|| self.pipeline(device, &self.emit_chunk_tile_kinds));
        let count = (!use_emit_chunks).then(|| {
            self.pipeline(
                device,
                if incremental {
                    &self.count_tiles
                } else {
                    &self.count_bins
                },
            )
        });
        let emit = (!use_emit_chunks
            && batch.draw_start < batch.draw_end
            && lengths.coarse_ptcl_capacity > 0)
            .then(|| {
                self.pipeline(
                    device,
                    if incremental {
                        &self.emit_tiles
                    } else {
                        &self.emit_bins
                    },
                )
            });
        let gpu_scope = start_gpu_scope(commands.device(), "coarse");
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu coarse pass"),
                timestamp_writes,
            });

            if use_emit_chunks {
                pass.set_bind_group(0, &prefix_bind_group, &[]);
                pass.set_pipeline(emit_chunk_counts.unwrap());
                pass.dispatch_workgroups(chunk_count, 1, 1);
                pass.set_pipeline(emit_prefix_chunks.unwrap());
                pass.dispatch_workgroups(chunk_count, 1, 1);
                pass.set_pipeline(emit_chunk_offsets.unwrap());
                pass.dispatch_workgroups(1, 1, 1);
                pass.set_pipeline(emit_apply_chunk_offsets.unwrap());
                pass.dispatch_workgroups(chunk_count, 1, 1);
                pass.set_pipeline(emit_fill_refs.unwrap());
                pass.dispatch_workgroups(chunk_count, 1, 1);
                pass.set_pipeline(emit_chunk_particle_counts.unwrap());
                let (x, y) = dispatch_2d(emit_chunk_count, max_workgroups);
                pass.dispatch_workgroups(x, y, 1);
                pass.set_pipeline(tile_counts_from_emit_chunks.unwrap());
                pass.dispatch_workgroups(chunk_count, 1, 1);
                pass.set_pipeline(ptcl_prefix_chunks);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);
                pass.set_pipeline(ptcl_chunk_offsets);
                pass.dispatch_workgroups(1, 1, 1);
                pass.set_pipeline(ptcl_apply_chunk_offsets);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);
                pass.set_pipeline(glyph_prefix_chunks);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);
                pass.set_pipeline(glyph_chunk_offsets);
                pass.dispatch_workgroups(1, 1, 1);
                pass.set_pipeline(glyph_apply_chunk_offsets);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);
                pass.set_pipeline(emit_chunk_particle_offsets.unwrap());
                pass.dispatch_workgroups(chunk_count, 1, 1);

                pass.set_bind_group(0, &emit_bind_group, &[]);
                pass.set_pipeline(emit_web.unwrap());
                let (x, y) = dispatch_2d(emit_chunk_count, max_workgroups);
                pass.dispatch_workgroups(x, y, 1);
                pass.set_pipeline(emit_chunk_tile_kinds.unwrap());
                pass.dispatch_workgroups(chunk_count, 1, 1);
            } else {
                pass.set_bind_group(0, &count_bind_group, &[]);
                pass.set_pipeline(count.unwrap());
                pass.dispatch_workgroups(
                    if incremental {
                        active_tile_count
                    } else {
                        bin_count
                    },
                    1,
                    1,
                );
                pass.set_bind_group(0, &prefix_bind_group, &[]);
                pass.set_pipeline(ptcl_prefix_chunks);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);
                pass.set_pipeline(ptcl_chunk_offsets);
                pass.dispatch_workgroups(1, 1, 1);
                pass.set_pipeline(ptcl_apply_chunk_offsets);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);
                pass.set_pipeline(glyph_prefix_chunks);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);
                pass.set_pipeline(glyph_chunk_offsets);
                pass.dispatch_workgroups(1, 1, 1);
                pass.set_pipeline(glyph_apply_chunk_offsets);
                pass.dispatch_workgroups(prefix_chunk_count, 1, 1);

                if let Some(emit) = emit {
                    pass.set_bind_group(0, &emit_bind_group, &[]);
                    pass.set_pipeline(emit);
                    pass.dispatch_workgroups(
                        if incremental {
                            active_tile_count
                        } else {
                            bin_count
                        },
                        1,
                        1,
                    );
                }
            }
        }
        finish_gpu_scope(encoder, gpu_scope);
    }

    fn encode_profiled_chunked(
        &self,
        commands: &mut WgpuCommandBatch,
        count_bind_group: &::wgpu::BindGroup,
        prefix_bind_group: &::wgpu::BindGroup,
        emit_bind_group: &::wgpu::BindGroup,
        lengths: GpuBufferLengths,
        batch: WgpuCoarseBatch,
        chunk_count: u32,
        bin_count: u32,
    ) {
        let emit_chunk_count = lengths.tile_draw_chunk_count as u32;
        if coarse_emit_chunks_enabled()
            && batch.draw_start < batch.draw_end
            && lengths.coarse_ptcl_capacity > 0
            && emit_chunk_count > 0
        {
            self.dispatch_profiled_kernel(
                commands,
                "coarse.emit_chunk_counts",
                prefix_bind_group,
                &self.emit_chunk_counts,
                chunk_count,
            );
            self.dispatch_profiled_kernel(
                commands,
                "coarse.emit_prefix_chunks",
                prefix_bind_group,
                &self.emit_prefix_chunks,
                chunk_count,
            );
            self.dispatch_profiled_kernel(
                commands,
                "coarse.emit_chunk_offsets",
                prefix_bind_group,
                &self.emit_chunk_offsets,
                1,
            );
            self.dispatch_profiled_kernel(
                commands,
                "coarse.emit_apply_chunk_offsets",
                prefix_bind_group,
                &self.emit_apply_chunk_offsets,
                chunk_count,
            );
            self.dispatch_profiled_kernel(
                commands,
                "coarse.emit_fill_refs",
                prefix_bind_group,
                &self.emit_fill_refs,
                chunk_count,
            );
            self.dispatch_profiled_large_kernel(
                commands,
                "coarse.emit_chunk_particle_counts",
                prefix_bind_group,
                &self.emit_chunk_particle_counts,
                emit_chunk_count,
            );
            self.dispatch_profiled_kernel(
                commands,
                "coarse.tile_counts_from_emit_chunks",
                prefix_bind_group,
                &self.tile_counts_from_emit_chunks,
                chunk_count,
            );
            self.encode_profiled_tile_offsets(commands, prefix_bind_group, chunk_count);
            self.dispatch_profiled_kernel(
                commands,
                "coarse.emit_chunk_particle_offsets",
                prefix_bind_group,
                &self.emit_chunk_particle_offsets,
                chunk_count,
            );
            self.dispatch_profiled_large_kernel(
                commands,
                "coarse.emit_web",
                emit_bind_group,
                &self.emit_web,
                emit_chunk_count,
            );
            self.dispatch_profiled_kernel(
                commands,
                "coarse.emit_chunk_tile_kinds",
                emit_bind_group,
                &self.emit_chunk_tile_kinds,
                chunk_count,
            );
        } else {
            self.dispatch_profiled_kernel(
                commands,
                "coarse.count_bins",
                count_bind_group,
                &self.count_bins,
                bin_count,
            );
            self.encode_profiled_tile_offsets(commands, prefix_bind_group, chunk_count);
            if batch.draw_start < batch.draw_end && lengths.coarse_ptcl_capacity > 0 {
                self.dispatch_profiled_kernel(
                    commands,
                    "coarse.emit_bins",
                    emit_bind_group,
                    &self.emit_bins,
                    bin_count,
                );
            }
        }
    }

    fn encode_profiled_tile_offsets(
        &self,
        commands: &mut WgpuCommandBatch,
        prefix_bind_group: &::wgpu::BindGroup,
        chunk_count: u32,
    ) {
        self.dispatch_profiled_kernel(
            commands,
            "coarse.ptcl_prefix_chunks",
            prefix_bind_group,
            &self.ptcl_prefix_chunks,
            chunk_count,
        );
        self.dispatch_profiled_kernel(
            commands,
            "coarse.ptcl_chunk_offsets",
            prefix_bind_group,
            &self.ptcl_chunk_offsets,
            1,
        );
        self.dispatch_profiled_kernel(
            commands,
            "coarse.ptcl_apply_chunk_offsets",
            prefix_bind_group,
            &self.ptcl_apply_chunk_offsets,
            chunk_count,
        );
        self.dispatch_profiled_kernel(
            commands,
            "coarse.glyph_prefix_chunks",
            prefix_bind_group,
            &self.glyph_prefix_chunks,
            chunk_count,
        );
        self.dispatch_profiled_kernel(
            commands,
            "coarse.glyph_chunk_offsets",
            prefix_bind_group,
            &self.glyph_chunk_offsets,
            1,
        );
        self.dispatch_profiled_kernel(
            commands,
            "coarse.glyph_apply_chunk_offsets",
            prefix_bind_group,
            &self.glyph_apply_chunk_offsets,
            chunk_count,
        );
    }

    fn dispatch_profiled_kernel(
        &self,
        commands: &mut WgpuCommandBatch,
        name: &'static str,
        bind_group: &::wgpu::BindGroup,
        kernel: &LazyCoarseKernel,
        workgroups: u32,
    ) {
        let pipeline = self.pipeline(commands.device(), kernel);
        dispatch_profiled(commands, name, bind_group, pipeline, workgroups);
    }

    fn dispatch_profiled_large_kernel(
        &self,
        commands: &mut WgpuCommandBatch,
        name: &'static str,
        bind_group: &::wgpu::BindGroup,
        kernel: &LazyCoarseKernel,
        workgroups: u32,
    ) {
        let pipeline = self.pipeline(commands.device(), kernel);
        dispatch_profiled_large(commands, name, bind_group, pipeline, workgroups);
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
                bind_buffer(2, bindings.text_blob),
                bind_buffer(3, bindings.path_records),
                bind_buffer(4, bindings.backdrops),
                bind_buffer(5, bindings.segment_ranges),
                bind_buffer(6, bindings.layer_stack),
                bind_buffer(7, bindings.coarse_work),
                bind_buffer(8, bindings.paint_blob),
                bind_buffer(9, bindings.draw_batch_ids),
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
                bind_buffer(2, bindings.text_blob),
                bind_buffer(3, bindings.path_records),
                bind_buffer(4, bindings.backdrops),
                bind_buffer(5, bindings.segment_ranges),
                bind_buffer(6, bindings.layer_stack),
                bind_buffer(7, bindings.coarse_work),
                bind_buffer(8, bindings.chunk_records),
                bind_buffer(9, bindings.paint_blob),
                bind_buffer(10, bindings.draw_batch_ids),
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
                bind_buffer(2, bindings.text_blob),
                bind_buffer(3, bindings.paint_blob),
                bind_buffer(4, bindings.path_records),
                bind_buffer(5, bindings.backdrops),
                bind_buffer(6, bindings.segment_ranges),
                bind_buffer(7, bindings.layer_stack),
                bind_buffer(8, bindings.coarse_work),
                bind_buffer(9, bindings.draw_batch_ids),
            ],
        })
    }

    #[cfg(test)]
    pub(crate) fn initialized_pipeline_count(&self) -> usize {
        [
            &self.count_bins,
            &self.count_tiles,
            &self.ptcl_prefix_chunks,
            &self.ptcl_chunk_offsets,
            &self.ptcl_apply_chunk_offsets,
            &self.glyph_prefix_chunks,
            &self.glyph_chunk_offsets,
            &self.glyph_apply_chunk_offsets,
            &self.emit_chunk_counts,
            &self.emit_prefix_chunks,
            &self.emit_chunk_offsets,
            &self.emit_apply_chunk_offsets,
            &self.emit_fill_refs,
            &self.emit_chunk_particle_counts,
            &self.emit_chunk_particle_offsets,
            &self.tile_counts_from_emit_chunks,
            &self.emit_bins,
            &self.emit_tiles,
            &self.emit_web,
            &self.emit_chunk_tile_kinds,
        ]
        .into_iter()
        .filter(|kernel| kernel.pipeline.is_initialized())
        .count()
    }
}

fn profile_coarse_passes() -> bool {
    profile_coarse_passes_value(
        std::env::var("TILEINK_PROFILE_COARSE_PASSES")
            .ok()
            .as_deref(),
    )
}

fn profile_coarse_passes_value(value: Option<&str>) -> bool {
    value == Some("1")
}

fn coarse_emit_chunks_enabled() -> bool {
    #[cfg(test)]
    if FORCE_COARSE_EMIT_CHUNKS.load(Ordering::Relaxed) {
        return true;
    }
    std::env::var("TILEINK_COARSE_CHUNKS").ok().as_deref() == Some("1")
}

#[cfg(test)]
static FORCE_COARSE_EMIT_CHUNKS: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn force_coarse_emit_chunks_for_test(enabled: bool) -> bool {
    FORCE_COARSE_EMIT_CHUNKS.swap(enabled, Ordering::Relaxed)
}

fn coarse_bin_count(lengths: GpuBufferLengths) -> u32 {
    let bins_x = (lengths.tiles_width as u32).div_ceil(16);
    let bins_y = (lengths.tiles_height as u32).div_ceil(16);
    bins_x * bins_y
}

fn dispatch_profiled(
    commands: &mut WgpuCommandBatch,
    name: &'static str,
    bind_group: &::wgpu::BindGroup,
    pipeline: &::wgpu::ComputePipeline,
    workgroups: u32,
) {
    let gpu_scope = start_gpu_scope(commands.device(), name);
    let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
    let encoder = commands.encoder();
    {
        let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
            label: Some(name),
            timestamp_writes,
        });
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_pipeline(pipeline);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    finish_gpu_scope(encoder, gpu_scope);
}

fn dispatch_profiled_large(
    commands: &mut WgpuCommandBatch,
    name: &'static str,
    bind_group: &::wgpu::BindGroup,
    pipeline: &::wgpu::ComputePipeline,
    workgroups: u32,
) {
    let (x, y) = dispatch_2d(
        workgroups,
        commands
            .device()
            .limits()
            .max_compute_workgroups_per_dimension,
    );
    let gpu_scope = start_gpu_scope(commands.device(), name);
    let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
    let encoder = commands.encoder();
    {
        let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
            label: Some(name),
            timestamp_writes,
        });
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_pipeline(pipeline);
        pass.dispatch_workgroups(x, y, 1);
    }
    finish_gpu_scope(encoder, gpu_scope);
}

fn count_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, true),
        storage_entry(4, false),
        storage_entry(5, true),
        storage_entry(6, true),
        storage_entry(7, false),
        storage_entry(8, true),
        storage_entry(9, true),
    ]
}

fn prefix_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, true),
        storage_entry(4, false),
        storage_entry(5, true),
        storage_entry(6, true),
        storage_entry(7, false),
        storage_entry(8, false),
        storage_entry(9, true),
        storage_entry(10, true),
    ]
}

fn emit_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, true),
        storage_entry(4, true),
        storage_entry(5, false),
        storage_entry(6, true),
        storage_entry(7, true),
        storage_entry(8, false),
        storage_entry(9, true),
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
        profile_coarse_passes_value,
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
        assert_eq!(COUNT_STORAGE_BINDING_COUNT, 9);
        assert_eq!(PREFIX_STORAGE_BINDING_COUNT, 10);
        assert_eq!(EMIT_STORAGE_BINDING_COUNT, 9);
    }

    #[test]
    fn coarse_layout_entries_are_contiguous() {
        for entries in [
            count_layout_entries(),
            prefix_layout_entries(),
            emit_layout_entries(),
        ] {
            assert_contiguous_bindings(&entries);
        }
    }

    #[test]
    fn profile_coarse_passes_only_accepts_one() {
        assert!(profile_coarse_passes_value(Some("1")));
        assert!(!profile_coarse_passes_value(None));
        assert!(!profile_coarse_passes_value(Some("true")));
        assert!(!profile_coarse_passes_value(Some("0")));
    }

    fn assert_contiguous_bindings(entries: &[::wgpu::BindGroupLayoutEntry]) {
        for (expected, entry) in entries.iter().enumerate() {
            assert_eq!(entry.binding, expected as u32);
        }
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
