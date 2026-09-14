#![allow(clippy::too_many_arguments)]
use crate::render::coarse::{CoarseBatch, CoarseConfig, CoarsePlan, CoarseProgram};

use crate::shared::gpu_constants::COARSE_WORKGROUP_SIZE;

use crate::shared::gpu_plan::{COARSE_CHUNK_SIZE, GpuBufferLengths};

use super::canvas::{
    WgpuCoarseBindGroups, WgpuCoarseBindings, WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers,
};
use super::commands::{
    WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
};
use super::lazy::{LazyComputePipeline, LazyShaderModule, PipelineCompilationTracker};
use super::profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

const COUNT_STORAGE_BINDING_COUNT: u32 = 9;
const PREFIX_STORAGE_BINDING_COUNT: u32 = 10;
const EMIT_STORAGE_BINDING_COUNT: u32 = 9;

pub(crate) struct WgpuCoarsePipeline {
    count_shader: LazyShaderModule,
    prefix_shader: LazyShaderModule,
    emit_shader: LazyShaderModule,
    emit_chunk_shader: LazyShaderModule,
    count_bins: LazyCoarseKernel,
    count_tiles: LazyCoarseKernel,
    prefix_chunks: LazyCoarseKernel,
    chunk_offsets: LazyCoarseKernel,
    apply_chunk_offsets: LazyCoarseKernel,
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
            prefix_chunks: kernel(
                "coarse_prefix_chunks",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            chunk_offsets: kernel(
                "coarse_chunk_offsets",
                CoarseShaderKind::Prefix,
                CoarseLayoutKind::Prefix,
            ),
            apply_chunk_offsets: kernel(
                "coarse_apply_chunk_offsets",
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

    #[cfg(test)]
    pub(crate) fn run(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        canvas: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &mut WgpuCoarseBuffers,
        lengths: GpuBufferLengths,
        batch: CoarseBatch,
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
        batch: CoarseBatch,
    ) {
        let _profile_scope = start_cpu_scope("coarse");
        let plan = CoarsePlan::new(
            lengths,
            batch,
            canvas.paint_brush_base(),
            coarse_emit_chunks_enabled(),
            commands
                .device()
                .limits()
                .max_compute_workgroups_per_dimension,
        )
        .expect("valid prepared coarse scene");
        if plan.passes().is_empty() {
            return;
        }
        let config_offset = commands.write_uniform_slot(
            &self.config,
            self.config_size,
            self.config_stride,
            WGPU_CONFIG_SLOTS,
            bytemuck::bytes_of(&plan.config),
        );
        let bindings = canvas.coarse_bindings(scan, coarse);
        let groups = {
            let _profile_scope = start_cpu_scope("coarse.bind_groups");
            let slot = (config_offset / self.config_stride) as usize;
            coarse.cached_bind_groups(bindings.key, slot, || WgpuCoarseBindGroups {
                count: self.create_count_bind_group(commands.device(), &bindings, config_offset),
                prefix: self.create_prefix_bind_group(commands.device(), &bindings, config_offset),
                emit: self.create_emit_bind_group(commands.device(), &bindings, config_offset),
            })
        };
        // Profiling changes pass boundaries only; both modes consume identical ordering.
        if profile_coarse_passes() && batch.active_tile_count.is_none() {
            for dispatch in plan.passes() {
                let (kernel, group) = self.scheduled_kernel(dispatch.program, &groups);
                let pipeline = self.pipeline(commands.device(), kernel);
                let scope = start_gpu_scope(commands.device(), dispatch.program.entry());
                let writes = scope.as_ref().map(|s| s.timestamp_writes());
                let encoder = commands.encoder();
                {
                    let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                        label: Some(dispatch.program.entry()),
                        timestamp_writes: writes,
                    });
                    pass.set_bind_group(0, group, &[]);
                    pass.set_pipeline(pipeline);
                    let [x, y, z] = dispatch.grid;
                    pass.dispatch_workgroups(x, y, z);
                }
                finish_gpu_scope(encoder, scope);
            }
            return;
        }
        // Resolve lazy pipelines before borrowing the command encoder.
        let pipelines = plan.resolve(|dispatch| {
            let (kernel, group) = self.scheduled_kernel(dispatch.program, &groups);
            (self.pipeline(commands.device(), kernel), group)
        });
        let scope = start_gpu_scope(commands.device(), "coarse");
        let writes = scope.as_ref().map(|s| s.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu coarse pass"),
                timestamp_writes: writes,
            });
            for (dispatch, pipeline) in plan.passes().iter().zip(pipelines.into_iter().flatten()) {
                let (pipeline, group) = pipeline;
                pass.set_bind_group(0, group, &[]);
                pass.set_pipeline(pipeline);
                let [x, y, z] = dispatch.grid;
                pass.dispatch_workgroups(x, y, z);
            }
        }
        finish_gpu_scope(encoder, scope);
    }
    fn scheduled_kernel<'a>(
        &'a self,
        program: CoarseProgram,
        groups: &'a WgpuCoarseBindGroups,
    ) -> (&'a LazyCoarseKernel, &'a ::wgpu::BindGroup) {
        match program {
            CoarseProgram::CountTiles => (&self.count_tiles, &groups.count),
            CoarseProgram::CountBins => (&self.count_bins, &groups.count),
            CoarseProgram::PrefixChunks => (&self.prefix_chunks, &groups.prefix),
            CoarseProgram::ChunkOffsets => (&self.chunk_offsets, &groups.prefix),
            CoarseProgram::ApplyChunkOffsets => (&self.apply_chunk_offsets, &groups.prefix),
            CoarseProgram::EmitChunkCounts => (&self.emit_chunk_counts, &groups.prefix),
            CoarseProgram::EmitPrefixChunks => (&self.emit_prefix_chunks, &groups.prefix),
            CoarseProgram::EmitChunkOffsets => (&self.emit_chunk_offsets, &groups.prefix),
            CoarseProgram::EmitApplyChunkOffsets => {
                (&self.emit_apply_chunk_offsets, &groups.prefix)
            }
            CoarseProgram::EmitFillRefs => (&self.emit_fill_refs, &groups.prefix),
            CoarseProgram::EmitChunkParticleCounts => {
                (&self.emit_chunk_particle_counts, &groups.prefix)
            }
            CoarseProgram::TileCountsFromEmitChunks => {
                (&self.tile_counts_from_emit_chunks, &groups.prefix)
            }
            CoarseProgram::EmitChunkParticleOffsets => {
                (&self.emit_chunk_particle_offsets, &groups.prefix)
            }
            CoarseProgram::EmitTiles => (&self.emit_tiles, &groups.emit),
            CoarseProgram::EmitBins => (&self.emit_bins, &groups.emit),
            CoarseProgram::EmitChunks => (&self.emit_web, &groups.emit),
            CoarseProgram::EmitChunkTileKinds => (&self.emit_chunk_tile_kinds, &groups.emit),
        }
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
            &self.prefix_chunks,
            &self.chunk_offsets,
            &self.apply_chunk_offsets,
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

fn count_layout_entries() -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_entry(0),
        storage_entry(1, true),
        storage_entry(2, true),
        storage_entry(3, true),
        storage_entry(4, true),
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
        storage_entry(4, true),
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
        storage_entry(5, true),
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

const _: () = assert!(COARSE_CHUNK_SIZE == COARSE_WORKGROUP_SIZE);

#[cfg(test)]
mod tests;
