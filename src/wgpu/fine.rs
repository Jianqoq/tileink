#![allow(clippy::too_many_arguments)]

use crate::shared::{
    gpu_layout::fine as fine_layout, gpu_plan::GpuBufferLengths, image::premul_color_to_rgba8_pack,
};

use super::{
    buffer::WgpuBuffer,
    canvas::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers, WgpuTileFineBindings},
    commands::{
        WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
    },
    profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope},
    target::WgpuTarget,
};

const TILE_STORAGE_BINDING_COUNT: u32 = fine_layout::STORAGE_BUFFER_COUNT;

pub(crate) struct WgpuFinePipeline {
    pipeline: ::wgpu::ComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
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
        let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu fine shader"),
            source: ::wgpu::ShaderSource::Wgsl(fine_shader_source(portable_textures).into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu tile fine pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
            label: Some("tileink wgpu tile fine pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("fine_tile_main"),
            compilation_options: ::wgpu::PipelineCompilationOptions::default(),
            cache: None,
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
            pipeline,
            bind_group_layout,
            config,
            config_size,
            config_stride,
        })
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
                paint_sdf_shadow_base: scene_buffers.fine_paint_sdf_shadow_base(),
                paint_brush_base: scene_buffers.fine_paint_brush_base(),
                text_image_base: scene_buffers.fine_text_image_base(),
                text_image_data_base: scene_buffers.fine_text_image_data_base(),
                group_spill_base: (lengths.tile_count
                    * clip_spill_depth as usize
                    * crate::shared::gpu_plan::FINE_WORKGROUP_SIZE as usize)
                    as u32,
            }),
        );

        let bindings = scene_buffers.tile_fine_bindings(scan, coarse, fine_spills);
        let bind_group = self.create_tile_bind_group_for_view(
            commands.device(),
            target,
            &self.bind_group_layout,
            &bindings,
            config_offset,
        );
        let gpu_scope = start_gpu_scope(commands.device(), "fine");
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu tile fine pass"),
                timestamp_writes,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(lengths.tile_count as u32, 1, 1);
        }
        finish_gpu_scope(encoder, gpu_scope);
        true
    }

    fn create_tile_bind_group_for_view(
        &self,
        device: &::wgpu::Device,
        texture: &::wgpu::TextureView,
        layout: &::wgpu::BindGroupLayout,
        bindings: &WgpuTileFineBindings<'_>,
        config_offset: ::wgpu::BufferAddress,
    ) -> ::wgpu::BindGroup {
        let fine = &bindings.fine;
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu tile fine bind group"),
            layout,
            entries: &[
                config_buffer_binding(0, &self.config, config_offset, self.config_size),
                texture_binding(1, texture),
                buffer_binding(2, fine.draw_records),
                buffer_binding(8, fine.paint_blob),
                buffer_binding(29, bindings.coarse_work),
                buffer_binding(37, bindings.segments),
                buffer_binding(43, bindings.text_blob),
                buffer_binding(53, bindings.spills),
                buffer_binding(
                    fine_layout::IMAGE_RESOURCE_METADATA_BINDING,
                    fine.image_resource_metadata,
                ),
                buffer_binding(
                    fine_layout::IMAGE_RESOURCE_PIXELS_BINDING,
                    fine.image_resource_pixels,
                ),
            ],
        })
    }
}

fn tile_fine_layout_entries(portable_textures: bool) -> Vec<::wgpu::BindGroupLayoutEntry> {
    vec![
        uniform_layout_entry(0),
        storage_texture_layout_entry(1, portable_textures),
        storage_layout_entry(2, true),
        storage_layout_entry(8, true),
        storage_layout_entry(29, true),
        storage_layout_entry(37, true),
        storage_layout_entry(43, true),
        storage_layout_entry(53, false),
        storage_layout_entry(fine_layout::IMAGE_RESOURCE_METADATA_BINDING, true),
        storage_layout_entry(fine_layout::IMAGE_RESOURCE_PIXELS_BINDING, true),
    ]
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

fn storage_texture_layout_entry(
    binding: u32,
    portable_textures: bool,
) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::StorageTexture {
            access: if portable_textures {
                ::wgpu::StorageTextureAccess::WriteOnly
            } else {
                ::wgpu::StorageTextureAccess::ReadWrite
            },
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
        assert_eq!(TILE_STORAGE_BINDING_COUNT, 8);
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
