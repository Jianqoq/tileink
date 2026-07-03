#![allow(clippy::too_many_arguments)]

use crate::shared::{gpu_plan::GpuBufferLengths, image::premul_color_to_rgba8_pack};

use super::{
    buffer::WgpuBuffer,
    scene::{WgpuCoarseBuffers, WgpuScanBuffers, WgpuSceneBuffers, WgpuTileFineBindings},
    target::WgpuTarget,
};

const TILE_STORAGE_BINDING_COUNT: u32 = 53;

pub(crate) struct WgpuFinePipeline {
    pipeline: ::wgpu::ComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
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
}

unsafe impl bytemuck::Zeroable for FineConfig {}
unsafe impl bytemuck::Pod for FineConfig {}

impl WgpuFinePipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        if !device
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            return None;
        }
        if device.limits().max_storage_buffers_per_shader_stage < TILE_STORAGE_BINDING_COUNT {
            return None;
        }

        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu tile fine bind group layout"),
                entries: &tile_fine_layout_entries(),
            });
        let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu fine shader"),
            source: ::wgpu::ShaderSource::Wgsl(include_str!("fine.wgsl").into()),
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
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu fine config"),
            size: std::mem::size_of::<FineConfig>() as ::wgpu::BufferAddress,
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            pipeline,
            bind_group_layout,
            config,
        })
    }

    pub(crate) fn render_tiles(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        width: u32,
        height: u32,
        lengths: GpuBufferLengths,
        scene_buffers: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &WgpuCoarseBuffers,
        clip_spills: &WgpuBuffer,
        group_spills: &WgpuBuffer,
        target: &mut WgpuTarget,
        clear_color: u32,
        load_target: bool,
        clip_spill_depth: u32,
        group_spill_depth: u32,
    ) -> bool {
        target.resize(device, width, height);
        self.render_tiles_to_view(
            device,
            queue,
            width,
            height,
            lengths,
            scene_buffers,
            scan,
            coarse,
            clip_spills,
            group_spills,
            target.view(),
            clear_color,
            load_target,
            clip_spill_depth,
            group_spill_depth,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_tiles_to_view(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        width: u32,
        height: u32,
        lengths: GpuBufferLengths,
        scene_buffers: &WgpuSceneBuffers,
        scan: &WgpuScanBuffers,
        coarse: &WgpuCoarseBuffers,
        clip_spills: &WgpuBuffer,
        group_spills: &WgpuBuffer,
        target: &::wgpu::TextureView,
        clear_color: u32,
        load_target: bool,
        clip_spill_depth: u32,
        group_spill_depth: u32,
    ) -> bool {
        if lengths.tile_count == 0 {
            return true;
        }

        queue.write_buffer(
            &self.config,
            0,
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
            }),
        );

        let bindings = scene_buffers.tile_fine_bindings(scan, coarse, clip_spills, group_spills);
        let bind_group = self.create_tile_bind_group_for_view(
            device,
            target,
            &self.bind_group_layout,
            &bindings,
        );
        let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
            label: Some("tileink wgpu tile fine encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu tile fine pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(lengths.tile_count as u32, 1, 1);
        }
        queue.submit([encoder.finish()]);
        true
    }

    fn create_tile_bind_group_for_view(
        &self,
        device: &::wgpu::Device,
        texture: &::wgpu::TextureView,
        layout: &::wgpu::BindGroupLayout,
        bindings: &WgpuTileFineBindings<'_>,
    ) -> ::wgpu::BindGroup {
        let fine = &bindings.fine;
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu tile fine bind group"),
            layout,
            entries: &[
                buffer_binding(0, &self.config),
                texture_binding(1, texture),
                buffer_binding(2, fine.draw_flags),
                buffer_binding(3, fine.draw_brush_colors),
                buffer_binding(4, fine.draw_pixel_x0),
                buffer_binding(5, fine.draw_pixel_y0),
                buffer_binding(6, fine.draw_pixel_x1),
                buffer_binding(7, fine.draw_pixel_y1),
                buffer_binding(8, fine.sdf_refs),
                buffer_binding(9, fine.sdf_kinds),
                buffer_binding(10, fine.sdf_x0),
                buffer_binding(11, fine.sdf_y0),
                buffer_binding(12, fine.sdf_x1),
                buffer_binding(13, fine.sdf_y1),
                buffer_binding(14, fine.sdf_r0),
                buffer_binding(15, fine.sdf_r1),
                buffer_binding(16, fine.sdf_r2),
                buffer_binding(17, fine.sdf_r3),
                buffer_binding(18, fine.sdf_stroke_top),
                buffer_binding(19, fine.sdf_stroke_right),
                buffer_binding(20, fine.sdf_stroke_bottom),
                buffer_binding(21, fine.sdf_stroke_left),
                buffer_binding(22, fine.sdf_shadow_offset_x),
                buffer_binding(23, fine.sdf_shadow_offset_y),
                buffer_binding(24, fine.sdf_shadow_expand),
                buffer_binding(25, fine.sdf_shadow_intensity),
                buffer_binding(26, fine.brush_data),
                buffer_binding(27, fine.brush_params),
                buffer_binding(28, fine.brush_payloads),
                buffer_binding(29, bindings.tile_range_starts),
                buffer_binding(30, bindings.tile_range_ends),
                buffer_binding(31, bindings.ptcl_tags),
                buffer_binding(32, bindings.ptcl_backdrops),
                buffer_binding(33, bindings.ptcl_fill_rules),
                buffer_binding(34, bindings.ptcl_segment_starts),
                buffer_binding(35, bindings.ptcl_segment_ends),
                buffer_binding(36, bindings.ptcl_colors),
                buffer_binding(37, bindings.segment_p0x),
                buffer_binding(38, bindings.segment_p0y),
                buffer_binding(39, bindings.segment_p1x),
                buffer_binding(40, bindings.segment_p1y),
                buffer_binding(41, bindings.segment_y_edge),
                buffer_binding(42, bindings.glyph_indices),
                buffer_binding(43, bindings.glyph_image_ids),
                buffer_binding(44, bindings.glyph_x),
                buffer_binding(45, bindings.glyph_y),
                buffer_binding(46, bindings.glyph_image_left),
                buffer_binding(47, bindings.glyph_image_top),
                buffer_binding(48, bindings.glyph_image_width),
                buffer_binding(49, bindings.glyph_image_height),
                buffer_binding(50, bindings.glyph_image_content),
                buffer_binding(51, bindings.glyph_image_data_offsets),
                buffer_binding(52, bindings.glyph_image_data),
                buffer_binding(53, bindings.clip_spills),
                buffer_binding(54, bindings.group_spills),
            ],
        })
    }
}

fn tile_fine_layout_entries() -> [::wgpu::BindGroupLayoutEntry; 55] {
    [
        uniform_layout_entry(0),
        storage_texture_layout_entry(1),
        storage_layout_entry(2, true),
        storage_layout_entry(3, true),
        storage_layout_entry(4, true),
        storage_layout_entry(5, true),
        storage_layout_entry(6, true),
        storage_layout_entry(7, true),
        storage_layout_entry(8, true),
        storage_layout_entry(9, true),
        storage_layout_entry(10, true),
        storage_layout_entry(11, true),
        storage_layout_entry(12, true),
        storage_layout_entry(13, true),
        storage_layout_entry(14, true),
        storage_layout_entry(15, true),
        storage_layout_entry(16, true),
        storage_layout_entry(17, true),
        storage_layout_entry(18, true),
        storage_layout_entry(19, true),
        storage_layout_entry(20, true),
        storage_layout_entry(21, true),
        storage_layout_entry(22, true),
        storage_layout_entry(23, true),
        storage_layout_entry(24, true),
        storage_layout_entry(25, true),
        storage_layout_entry(26, true),
        storage_layout_entry(27, true),
        storage_layout_entry(28, true),
        storage_layout_entry(29, true),
        storage_layout_entry(30, true),
        storage_layout_entry(31, true),
        storage_layout_entry(32, true),
        storage_layout_entry(33, true),
        storage_layout_entry(34, true),
        storage_layout_entry(35, true),
        storage_layout_entry(36, true),
        storage_layout_entry(37, true),
        storage_layout_entry(38, true),
        storage_layout_entry(39, true),
        storage_layout_entry(40, true),
        storage_layout_entry(41, true),
        storage_layout_entry(42, true),
        storage_layout_entry(43, true),
        storage_layout_entry(44, true),
        storage_layout_entry(45, true),
        storage_layout_entry(46, true),
        storage_layout_entry(47, true),
        storage_layout_entry(48, true),
        storage_layout_entry(49, true),
        storage_layout_entry(50, true),
        storage_layout_entry(51, true),
        storage_layout_entry(52, true),
        storage_layout_entry(53, false),
        storage_layout_entry(54, false),
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

fn storage_texture_layout_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::StorageTexture {
            access: ::wgpu::StorageTextureAccess::ReadWrite,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            view_dimension: ::wgpu::TextureViewDimension::D2,
        },
        count: None,
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

fn texture_binding(binding: u32, view: &::wgpu::TextureView) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::TextureView(view),
    }
}

pub(crate) fn premul_clear_color(clear: peniko::Color) -> u32 {
    premul_color_to_rgba8_pack(clear)
}
