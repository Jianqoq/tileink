#![allow(clippy::too_many_arguments)]

use crate::shared::{
    gpu_coarse::{FINE_TILE_DISPATCH_WORDS, coarse_work_fine_tile_kind_word_offset},
    gpu_layout::fine as fine_layout,
    gpu_plan::GpuBufferLengths,
    image::premul_color_to_rgba8_pack,
};

use super::{
    buffer::WgpuBuffer,
    canvas::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers, WgpuTileFineBindings},
    commands::{
        WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
    },
    image_resources::{
        create_image_resource_bind_group, create_image_resource_bind_group_layout,
        large_texture_table_len, patch_image_resource_shader_source,
    },
    lazy::{LazyComputePipeline, LazyShaderModule},
    profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope},
    target::WgpuTarget,
};

const TILE_STORAGE_BINDING_COUNT: u32 = fine_layout::STORAGE_BUFFER_COUNT;

pub(crate) struct WgpuFinePipeline {
    fine_shader: LazyShaderModule,
    compact_shader: LazyShaderModule,
    pipeline: LazyComputePipeline,
    clear_pipeline: LazyComputePipeline,
    compact_pipeline: LazyComputePipeline,
    sdf_pipeline: LazyComputePipeline,
    mixed_pipeline: LazyComputePipeline,
    full_pipeline: LazyComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    image_bind_group_layout: ::wgpu::BindGroupLayout,
    compact_bind_group_layout: ::wgpu::BindGroupLayout,
    pipeline_layout: ::wgpu::PipelineLayout,
    compact_pipeline_layout: ::wgpu::PipelineLayout,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
    portable_textures: bool,
    large_texture_table_len: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FineConfig {
    width: u32,
    height: u32,
    clear_color: u32,
    tile_count: u32,
    tiles_width: u32,
    tiles_height: u32,
    load_target: u32,
    clip_spill_depth: u32,
    group_spill_depth: u32,
    ptcl_capacity: u32,
    paint_sdf_shadow_base: u32,
    paint_brush_base: u32,
    text_image_base: u32,
    text_image_data_base: u32,
    group_spill_base: u32,
    fine_tile_kind_base: u32,
}

unsafe impl bytemuck::Zeroable for FineConfig {}
unsafe impl bytemuck::Pod for FineConfig {}

impl WgpuFinePipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        let portable_textures = !device
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        if device.limits().max_storage_buffers_per_shader_stage < TILE_STORAGE_BINDING_COUNT {
            return None;
        }

        let layout_entries = tile_fine_layout_entries(portable_textures);
        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu tile fine bind group layout"),
                entries: &layout_entries,
            });
        let large_texture_table_len = large_texture_table_len(device);
        let image_bind_group_layout =
            create_image_resource_bind_group_layout(device, large_texture_table_len);
        let compact_bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu tile fine compact bind group layout"),
                entries: &[
                    uniform_layout_entry(0),
                    storage_layout_entry(1, false),
                    storage_layout_entry(2, false),
                ],
            });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu tile fine pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout), Some(&image_bind_group_layout)],
            immediate_size: 0,
        });
        let compact_pipeline_layout =
            device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
                label: Some("tileink wgpu tile fine compact pipeline layout"),
                bind_group_layouts: &[Some(&compact_bind_group_layout)],
                immediate_size: 0,
            });
        let config_size = std::mem::size_of::<FineConfig>() as ::wgpu::BufferAddress;
        let config_stride = aligned_uniform_stride(device, config_size);
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu fine config"),
            size: uniform_slots_buffer_size(device, config_size),
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            fine_shader: LazyShaderModule::new("tileink wgpu fine shader"),
            compact_shader: LazyShaderModule::new("tileink wgpu fine compact shader"),
            pipeline: LazyComputePipeline::new("tileink wgpu tile fine pipeline", "fine_tile_main"),
            clear_pipeline: LazyComputePipeline::new(
                "tileink wgpu tile fine indirect clear pipeline",
                "fine_clear_indirect_main",
            ),
            compact_pipeline: LazyComputePipeline::new(
                "tileink wgpu tile fine compact pipeline",
                "fine_compact_tiles_main",
            ),
            sdf_pipeline: LazyComputePipeline::new(
                "tileink wgpu tile fine sdf pipeline",
                "fine_tile_sdf_list_main",
            ),
            mixed_pipeline: LazyComputePipeline::new(
                "tileink wgpu tile fine mixed pipeline",
                "fine_tile_mixed_list_main",
            ),
            full_pipeline: LazyComputePipeline::new(
                "tileink wgpu tile fine full pipeline",
                "fine_tile_full_list_main",
            ),
            bind_group_layout,
            image_bind_group_layout,
            compact_bind_group_layout,
            pipeline_layout,
            compact_pipeline_layout,
            config,
            config_size,
            config_stride,
            portable_textures,
            large_texture_table_len,
        })
    }

    pub(crate) fn uses_portable_textures(&self) -> bool {
        self.portable_textures
    }

    pub(crate) fn render_tiles_in(
        &self,
        commands: &mut WgpuCommandBatch,
        width: u32,
        height: u32,
        lengths: GpuBufferLengths,
        scene_buffers: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &WgpuCoarseBuffers,
        fine_spills: &WgpuBuffer,
        fine_indirect_args: &WgpuBuffer,
        target: &mut WgpuTarget,
        clear_color: u32,
        load_target: bool,
        clip_spill_depth: u32,
        group_spill_depth: u32,
    ) -> bool {
        target.resize(commands.device(), width, height);
        self.render_tiles_to_view_in(
            commands,
            width,
            height,
            lengths,
            scene_buffers,
            scan,
            coarse,
            fine_spills,
            fine_indirect_args,
            target.view(),
            clear_color,
            load_target,
            clip_spill_depth,
            group_spill_depth,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_tiles_to_view_in(
        &self,
        commands: &mut WgpuCommandBatch,
        width: u32,
        height: u32,
        lengths: GpuBufferLengths,
        scene_buffers: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &WgpuCoarseBuffers,
        fine_spills: &WgpuBuffer,
        fine_indirect_args: &WgpuBuffer,
        target: &::wgpu::TextureView,
        clear_color: u32,
        load_target: bool,
        clip_spill_depth: u32,
        group_spill_depth: u32,
    ) -> bool {
        self.render_tiles_to_views_in(
            commands,
            width,
            height,
            lengths,
            scene_buffers,
            scan,
            coarse,
            fine_spills,
            fine_indirect_args,
            target,
            target,
            clear_color,
            load_target,
            clip_spill_depth,
            group_spill_depth,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_tiles_to_views_in(
        &self,
        commands: &mut WgpuCommandBatch,
        width: u32,
        height: u32,
        lengths: GpuBufferLengths,
        scene_buffers: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &WgpuCoarseBuffers,
        fine_spills: &WgpuBuffer,
        fine_indirect_args: &WgpuBuffer,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        clear_color: u32,
        load_target: bool,
        clip_spill_depth: u32,
        group_spill_depth: u32,
    ) -> bool {
        let _profile_scope = start_cpu_scope("fine");
        if lengths.tile_count == 0 {
            return true;
        }

        let config_offset = commands.write_uniform_slot(
            "fine.config",
            &self.config,
            self.config_size,
            self.config_stride,
            WGPU_CONFIG_SLOTS,
            bytemuck::bytes_of(&FineConfig {
                width,
                height,
                clear_color,
                tile_count: lengths.tile_count as u32,
                tiles_width: lengths.tiles_width as u32,
                tiles_height: lengths.tiles_height as u32,
                load_target: u32::from(load_target),
                clip_spill_depth,
                group_spill_depth,
                ptcl_capacity: lengths.coarse_ptcl_capacity as u32,
                paint_sdf_shadow_base: scene_buffers.paint_sdf_shadow_base(),
                paint_brush_base: scene_buffers.paint_brush_base(),
                text_image_base: scene_buffers.fine_text_image_base(),
                text_image_data_base: scene_buffers.fine_text_image_data_base(),
                group_spill_base: (lengths.tile_count
                    * clip_spill_depth as usize
                    * crate::shared::gpu_plan::FINE_WORKGROUP_SIZE as usize)
                    as u32,
                fine_tile_kind_base: coarse_work_fine_tile_kind_word_offset(
                    lengths.tile_count,
                    lengths.coarse_ptcl_capacity,
                    lengths.coarse_glyph_capacity,
                    lengths.tile_draw_index_count,
                    lengths.tile_draw_chunk_count,
                ) as u32,
            }),
        );

        let bindings = scene_buffers.tile_fine_bindings(scan, coarse, fine_spills);
        let bind_group = self.create_tile_bind_group_for_view(
            commands.device(),
            source,
            target,
            &self.bind_group_layout,
            &bindings,
            config_offset,
        );
        let image_bind_group = create_image_resource_bind_group(
            commands.device(),
            &self.image_bind_group_layout,
            &scene_buffers.image_resource_bindings(),
            self.large_texture_table_len,
        );
        let compact_bind_group = self.create_compact_bind_group(
            commands.device(),
            coarse,
            fine_indirect_args,
            config_offset,
        );
        let device = commands.device().clone();
        let use_indirect = fine_indirect_enabled();
        if use_indirect {
            let clear_pipeline = self.clear_pipeline(&device);
            let compact_pipeline = self.compact_pipeline(&device);
            let encoder = commands.encoder();
            let gpu_scope = start_gpu_scope(&device, "fine.compact");
            let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu tile fine compact pass"),
                timestamp_writes,
            });
            pass.set_bind_group(0, &compact_bind_group, &[]);
            pass.set_pipeline(clear_pipeline);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(compact_pipeline);
            pass.dispatch_workgroups(lengths.tile_count.div_ceil(256) as u32, 1, 1);
            drop(pass);
            finish_gpu_scope(encoder, gpu_scope);
        }
        {
            let indirect_pipelines = use_indirect.then(|| {
                (
                    self.sdf_pipeline(&device),
                    self.mixed_pipeline(&device),
                    self.full_pipeline(&device),
                )
            });
            let direct_pipeline = (!use_indirect).then(|| self.pipeline(&device));
            let encoder = commands.encoder();
            let gpu_scope = start_gpu_scope(&device, "fine");
            let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu tile fine pass"),
                timestamp_writes,
            });
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(1, &image_bind_group, &[]);
            if let Some((sdf_pipeline, mixed_pipeline, full_pipeline)) = indirect_pipelines {
                pass.set_pipeline(sdf_pipeline);
                pass.dispatch_workgroups_indirect(fine_indirect_args.buffer(), 0);
                pass.set_pipeline(mixed_pipeline);
                pass.dispatch_workgroups_indirect(
                    fine_indirect_args.buffer(),
                    fine_dispatch_args_stride(),
                );
                pass.set_pipeline(full_pipeline);
                pass.dispatch_workgroups_indirect(
                    fine_indirect_args.buffer(),
                    fine_dispatch_args_stride() * 2,
                );
            } else {
                pass.set_pipeline(direct_pipeline.expect("direct fine pipeline"));
                pass.dispatch_workgroups(lengths.tile_count as u32, 1, 1);
            }
            drop(pass);
            finish_gpu_scope(encoder, gpu_scope);
        }
        true
    }

    fn fine_shader(&self, device: &::wgpu::Device) -> &::wgpu::ShaderModule {
        self.fine_shader.get(device, || {
            let shader_source = patch_image_resource_shader_source(
                fine_shader_source(self.portable_textures),
                self.large_texture_table_len > 0,
            );
            ::wgpu::ShaderSource::Wgsl(shader_source.into())
        })
    }

    fn compact_shader(&self, device: &::wgpu::Device) -> &::wgpu::ShaderModule {
        self.compact_shader.get(device, || {
            ::wgpu::ShaderSource::Wgsl(fine_compact_shader_source().into())
        })
    }

    fn pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.pipeline
            .get(device, &self.pipeline_layout, self.fine_shader(device))
    }

    fn clear_pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.clear_pipeline.get(
            device,
            &self.compact_pipeline_layout,
            self.compact_shader(device),
        )
    }

    fn compact_pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.compact_pipeline.get(
            device,
            &self.compact_pipeline_layout,
            self.compact_shader(device),
        )
    }

    fn sdf_pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.sdf_pipeline
            .get(device, &self.pipeline_layout, self.fine_shader(device))
    }

    fn mixed_pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.mixed_pipeline
            .get(device, &self.pipeline_layout, self.fine_shader(device))
    }

    fn full_pipeline(&self, device: &::wgpu::Device) -> &::wgpu::ComputePipeline {
        self.full_pipeline
            .get(device, &self.pipeline_layout, self.fine_shader(device))
    }

    #[cfg(test)]
    pub(crate) fn initialized_pipeline_count(&self) -> usize {
        [
            &self.pipeline,
            &self.clear_pipeline,
            &self.compact_pipeline,
            &self.sdf_pipeline,
            &self.mixed_pipeline,
            &self.full_pipeline,
        ]
        .into_iter()
        .filter(|pipeline| pipeline.is_initialized())
        .count()
    }

    fn create_tile_bind_group_for_view(
        &self,
        device: &::wgpu::Device,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        layout: &::wgpu::BindGroupLayout,
        bindings: &WgpuTileFineBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        let fine = &bindings.fine;
        let mut entries = vec![
            config_buffer_binding(0, &self.config, config_offset, self.config_size),
            texture_binding(
                1,
                if self.portable_textures {
                    source
                } else {
                    target
                },
            ),
            buffer_binding(2, fine.draw_records),
            buffer_binding(3, fine.paint_blob),
            buffer_binding(4, bindings.coarse_work),
            buffer_binding(5, bindings.segments),
            buffer_binding(6, bindings.text_blob),
            buffer_binding(7, bindings.spills),
        ];
        if self.portable_textures {
            entries.push(texture_binding(8, target));
        }
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu tile fine bind group"),
            layout,
            entries: &entries,
        })
    }

    fn create_compact_bind_group(
        &self,
        device: &::wgpu::Device,
        coarse: &WgpuCoarseBuffers,
        fine_indirect_args: &WgpuBuffer,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu tile fine compact bind group"),
            layout: &self.compact_bind_group_layout,
            entries: &[
                config_buffer_binding(0, &self.config, config_offset, self.config_size),
                buffer_binding(1, coarse.work.buffer()),
                buffer_binding(2, fine_indirect_args.buffer()),
            ],
        })
    }
}

fn tile_fine_layout_entries(portable_textures: bool) -> Vec<::wgpu::BindGroupLayoutEntry> {
    let mut entries = vec![
        uniform_layout_entry(0),
        if portable_textures {
            sampled_texture_layout_entry(1)
        } else {
            storage_texture_layout_entry(1, ::wgpu::StorageTextureAccess::ReadWrite)
        },
        storage_layout_entry(2, true),
        storage_layout_entry(3, true),
        storage_layout_entry(4, false),
        storage_layout_entry(5, true),
        storage_layout_entry(6, true),
        storage_layout_entry(7, false),
    ];
    if portable_textures {
        entries.push(storage_texture_layout_entry(
            8,
            ::wgpu::StorageTextureAccess::WriteOnly,
        ));
    }
    entries
}

fn uniform_layout_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
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

fn sampled_texture_layout_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Texture {
            sample_type: ::wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: ::wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn storage_texture_layout_entry(
    binding: u32,
    access: ::wgpu::StorageTextureAccess,
) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::StorageTexture {
            access,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            view_dimension: ::wgpu::TextureViewDimension::D2,
        },
        count: None,
    }
}

fn fine_shader_source(portable_textures: bool) -> &'static str {
    if portable_textures {
        include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_web.wgsl"))
    } else {
        include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine.wgsl"))
    }
}

fn fine_compact_shader_source() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_fine_compact.wgsl"))
}

fn fine_indirect_enabled() -> bool {
    std::env::var("TILEINK_FINE_DIRECT").ok().as_deref() != Some("1")
}

fn fine_dispatch_args_stride() -> ::wgpu::BufferAddress {
    (FINE_TILE_DISPATCH_WORDS * std::mem::size_of::<u32>()) as ::wgpu::BufferAddress
}

fn storage_layout_entry(binding: u32, read_only: bool) -> ::wgpu::BindGroupLayoutEntry {
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

fn buffer_binding(binding: u32, buffer: &::wgpu::Buffer) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn config_buffer_binding(
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

fn texture_binding(binding: u32, view: &::wgpu::TextureView) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::TextureView(view),
    }
}

pub(crate) fn premul_clear_color(clear: peniko::Color) -> u32 {
    premul_color_to_rgba8_pack(clear)
}

#[cfg(test)]
mod tests {
    use super::{TILE_STORAGE_BINDING_COUNT, tile_fine_layout_entries};

    #[test]
    fn fine_pipeline_storage_bindings_match_layout_constant() {
        let entries = tile_fine_layout_entries(false);
        let storage_count = entries.iter().filter(|entry| is_storage(entry)).count() as u32;
        assert_eq!(storage_count, TILE_STORAGE_BINDING_COUNT);
        assert_eq!(TILE_STORAGE_BINDING_COUNT, 6);
    }

    #[test]
    fn fine_layout_entries_are_sorted_by_binding() {
        for portable_textures in [false, true] {
            let entries = tile_fine_layout_entries(portable_textures);
            assert_sorted_by_binding(&entries);
        }
    }

    #[test]
    fn fine_layout_entries_are_contiguous() {
        for portable_textures in [false, true] {
            let entries = tile_fine_layout_entries(portable_textures);
            assert_contiguous_bindings(&entries);
        }
    }

    fn assert_sorted_by_binding(entries: &[::wgpu::BindGroupLayoutEntry]) {
        for pair in entries.windows(2) {
            assert!(
                pair[0].binding < pair[1].binding,
                "bindings must be strictly sorted, got {} before {}",
                pair[0].binding,
                pair[1].binding
            );
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
