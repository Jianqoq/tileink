#![allow(clippy::too_many_arguments)]

use peniko::{
    BlendMode, Compose, Mix,
    kurbo::{Rect, Shape},
};

use crate::shared::{
    bounds::Bounds,
    gpu_plan::GpuBufferLengths,
    layer::{
        filter::{
            ColorChannel, CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting,
            DisplacementMap, Filter, LightSource, RectLiquidGlass, RectLiquidGlassRegion,
            SpecularLighting, Turbulence, TurbulenceKind,
        },
        mask::MaskKind,
        region::Region,
    },
};

use super::scene::WgpuFilterBindings;

pub(crate) const FILTER_BRIGHTNESS: u32 = 1;
pub(crate) const FILTER_CONTRAST: u32 = 2;
pub(crate) const FILTER_GRAYSCALE: u32 = 3;
pub(crate) const FILTER_HUE_ROTATE: u32 = 4;
pub(crate) const FILTER_INVERT: u32 = 5;
pub(crate) const FILTER_OPACITY: u32 = 6;
pub(crate) const FILTER_SATURATE: u32 = 7;
pub(crate) const FILTER_SEPIA: u32 = 8;
pub(crate) const SVG_MASK_ALPHA: u32 = 0;
pub(crate) const SVG_MASK_LUMINANCE: u32 = 1;

const WORKGROUP_SIZE: u32 = 256;
const STORAGE_BINDING_COUNT: u32 = 53;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FilterConfig {
    width: u32,
    height: u32,
    tiles_width: u32,
    tiles_height: u32,
    region_x0: u32,
    region_y0: u32,
    region_width: u32,
    region_height: u32,
    pixel_count: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    draw_ix: u32,
    mask_enabled: u32,
    blend_mode: u32,
    mask_kind: u32,
    clear_color: u32,
    filter_kind: u32,
    table_index: u32,
    brush_index: u32,
    offset_x: i32,
    offset_y: i32,
    morphology_radius: u32,
    morphology_operator: u32,
    morphology_axis: u32,
    blur_axis: u32,
    kernel_offset: u32,
    kernel_columns: u32,
    kernel_rows: u32,
    kernel_target_x: u32,
    kernel_target_y: u32,
    kernel_edge_mode: u32,
    kernel_preserve_alpha: u32,
    lighting_output_kind: u32,
    surface_origin_x: i32,
    surface_origin_y: i32,
    light_kind: u32,
    amount: f32,
    rect_x0: f32,
    rect_y0: f32,
    rect_x1: f32,
    rect_y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
    surface_scale: f32,
    light_constant: f32,
    specular_exponent: f32,
    light_r: f32,
    light_g: f32,
    light_b: f32,
    light_p0: f32,
    light_p1: f32,
    light_p2: f32,
    light_p3: f32,
    light_p4: f32,
    light_p5: f32,
    light_p6: f32,
    light_p7: f32,
    light_p8: f32,
    turbulence_base_frequency_x: f32,
    turbulence_base_frequency_y: f32,
    turbulence_num_octaves: u32,
    turbulence_stitch_tiles: u32,
    turbulence_kind: u32,
    turbulence_linear_rgb: u32,
    turbulence_pad0: u32,
    turbulence_pad1: u32,
    turbulence_transform_x: f32,
    turbulence_transform_y: f32,
    turbulence_scale_x: f32,
    turbulence_scale_y: f32,
    turbulence_tile_x: f32,
    turbulence_tile_y: f32,
    turbulence_tile_width: f32,
    turbulence_tile_height: f32,
    liquid_tint_r: f32,
    liquid_tint_g: f32,
    liquid_tint_b: f32,
    liquid_tint_a: f32,
    liquid_refraction_thickness: f32,
    liquid_refraction_factor: f32,
    liquid_refraction_dispersion: f32,
    liquid_fresnel_range: f32,
    liquid_fresnel_hardness: f32,
    liquid_fresnel_factor: f32,
    liquid_glare_range: f32,
    liquid_glare_hardness: f32,
    liquid_glare_convergence: f32,
    liquid_glare_opposite_factor: f32,
    liquid_glare_factor: f32,
    liquid_glare_angle: f32,
    matrix_r: [f32; 4],
    matrix_g: [f32; 4],
    matrix_b: [f32; 4],
    matrix_a: [f32; 4],
    matrix_bias: [f32; 4],
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            tiles_width: 0,
            tiles_height: 0,
            region_x0: 0,
            region_y0: 0,
            region_width: 0,
            region_height: 0,
            pixel_count: 0,
            layer_stack_start: 0,
            layer_stack_end: 0,
            draw_ix: 0,
            mask_enabled: 0,
            blend_mode: 0,
            mask_kind: 0,
            clear_color: 0,
            filter_kind: 0,
            table_index: 0,
            brush_index: 0,
            offset_x: 0,
            offset_y: 0,
            morphology_radius: 0,
            morphology_operator: 0,
            morphology_axis: 0,
            blur_axis: 0,
            kernel_offset: 0,
            kernel_columns: 0,
            kernel_rows: 0,
            kernel_target_x: 0,
            kernel_target_y: 0,
            kernel_edge_mode: 0,
            kernel_preserve_alpha: 0,
            lighting_output_kind: 0,
            surface_origin_x: 0,
            surface_origin_y: 0,
            light_kind: 0,
            amount: 0.0,
            rect_x0: 0.0,
            rect_y0: 0.0,
            rect_x1: 0.0,
            rect_y1: 0.0,
            radius_top_left: 0.0,
            radius_top_right: 0.0,
            radius_bottom_left: 0.0,
            radius_bottom_right: 0.0,
            surface_scale: 0.0,
            light_constant: 0.0,
            specular_exponent: 0.0,
            light_r: 0.0,
            light_g: 0.0,
            light_b: 0.0,
            light_p0: 0.0,
            light_p1: 0.0,
            light_p2: 0.0,
            light_p3: 0.0,
            light_p4: 0.0,
            light_p5: 0.0,
            light_p6: 0.0,
            light_p7: 0.0,
            light_p8: 0.0,
            turbulence_base_frequency_x: 0.0,
            turbulence_base_frequency_y: 0.0,
            turbulence_num_octaves: 0,
            turbulence_stitch_tiles: 0,
            turbulence_kind: 0,
            turbulence_linear_rgb: 0,
            turbulence_pad0: 0,
            turbulence_pad1: 0,
            turbulence_transform_x: 0.0,
            turbulence_transform_y: 0.0,
            turbulence_scale_x: 0.0,
            turbulence_scale_y: 0.0,
            turbulence_tile_x: 0.0,
            turbulence_tile_y: 0.0,
            turbulence_tile_width: 0.0,
            turbulence_tile_height: 0.0,
            liquid_tint_r: 0.0,
            liquid_tint_g: 0.0,
            liquid_tint_b: 0.0,
            liquid_tint_a: 0.0,
            liquid_refraction_thickness: 0.0,
            liquid_refraction_factor: 0.0,
            liquid_refraction_dispersion: 0.0,
            liquid_fresnel_range: 0.0,
            liquid_fresnel_hardness: 0.0,
            liquid_fresnel_factor: 0.0,
            liquid_glare_range: 0.0,
            liquid_glare_hardness: 0.0,
            liquid_glare_convergence: 0.0,
            liquid_glare_opposite_factor: 0.0,
            liquid_glare_factor: 0.0,
            liquid_glare_angle: 0.0,
            matrix_r: [0.0; 4],
            matrix_g: [0.0; 4],
            matrix_b: [0.0; 4],
            matrix_a: [0.0; 4],
            matrix_bias: [0.0; 4],
        }
    }
}

pub(crate) struct WgpuFilterPipeline {
    clear_region: ::wgpu::ComputePipeline,
    copy_region: ::wgpu::ComputePipeline,
    source_alpha_region: ::wgpu::ComputePipeline,
    source_over_region: ::wgpu::ComputePipeline,
    tile_region: ::wgpu::ComputePipeline,
    offset_region: ::wgpu::ComputePipeline,
    flood_region: ::wgpu::ComputePipeline,
    drop_shadow_mask_region: ::wgpu::ComputePipeline,
    morphology_axis_region: ::wgpu::ComputePipeline,
    blur_region: ::wgpu::ComputePipeline,
    svg_mask_coverage_region: ::wgpu::ComputePipeline,
    apply_region_mask: ::wgpu::ComputePipeline,
    color_filter_region: ::wgpu::ComputePipeline,
    color_matrix_region: ::wgpu::ComputePipeline,
    component_transfer_region: ::wgpu::ComputePipeline,
    convolve_matrix_region: ::wgpu::ComputePipeline,
    lighting_region: ::wgpu::ComputePipeline,
    liquid_glass_region: ::wgpu::ComputePipeline,
    blend_region: ::wgpu::ComputePipeline,
    composite_inputs_region: ::wgpu::ComputePipeline,
    displacement_map_region: ::wgpu::ComputePipeline,
    turbulence_region: ::wgpu::ComputePipeline,
    composite_drop_shadow_region: ::wgpu::ComputePipeline,
    layer_mask_region: ::wgpu::ComputePipeline,
    rect_mask_region: ::wgpu::ComputePipeline,
    path_mask_region: ::wgpu::ComputePipeline,
    composite_stack_region: ::wgpu::ComputePipeline,
    composite_blend_stack_region: ::wgpu::ComputePipeline,
    composite_surface_stack_region: ::wgpu::ComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    config: ::wgpu::Buffer,
    _dummy_texture: ::wgpu::Texture,
    dummy_texture_view: ::wgpu::TextureView,
    dummy_read: ::wgpu::Buffer,
    dummy_read_write: ::wgpu::Buffer,
}

pub(crate) struct WgpuFilterBrushBindings<'a> {
    pub(crate) data: &'a ::wgpu::Buffer,
    pub(crate) params: &'a ::wgpu::Buffer,
    pub(crate) payloads: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuFilterTurbulenceBindings<'a> {
    pub(crate) selectors: &'a ::wgpu::Buffer,
    pub(crate) gradients: &'a ::wgpu::Buffer,
}

pub(crate) struct WgpuFilterPathBindings<'a> {
    pub(crate) range_starts: &'a ::wgpu::Buffer,
    pub(crate) range_ends: &'a ::wgpu::Buffer,
    pub(crate) p0x: &'a ::wgpu::Buffer,
    pub(crate) p0y: &'a ::wgpu::Buffer,
    pub(crate) p1x: &'a ::wgpu::Buffer,
    pub(crate) p1y: &'a ::wgpu::Buffer,
}

impl WgpuFilterPipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        if !device
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        {
            return None;
        }
        if device.limits().max_storage_buffers_per_shader_stage < STORAGE_BINDING_COUNT {
            return None;
        }

        let bind_group_layout =
            device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
                label: Some("tileink wgpu filter bind group layout"),
                entries: &filter_layout_entries(),
            });
        let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
            label: Some("tileink wgpu filter shader"),
            source: ::wgpu::ShaderSource::Wgsl(
                include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter.wgsl")).into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
            label: Some("tileink wgpu filter pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu filter config"),
            size: std::mem::size_of::<FilterConfig>() as ::wgpu::BufferAddress,
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_read = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu filter read dummy buffer"),
            size: 4,
            usage: ::wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let dummy_read_write = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu filter read-write dummy buffer"),
            size: 4,
            usage: ::wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let dummy_texture = device.create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu filter dummy texture"),
            size: ::wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let dummy_texture_view =
            dummy_texture.create_view(&::wgpu::TextureViewDescriptor::default());
        Some(Self {
            clear_region: create_pipeline(device, &pipeline_layout, &shader, "filter_clear_region"),
            copy_region: create_pipeline(device, &pipeline_layout, &shader, "filter_copy_region"),
            source_alpha_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_source_alpha_region",
            ),
            source_over_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_source_over_region",
            ),
            tile_region: create_pipeline(device, &pipeline_layout, &shader, "filter_tile_region"),
            offset_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_offset_region",
            ),
            flood_region: create_pipeline(device, &pipeline_layout, &shader, "filter_flood_region"),
            drop_shadow_mask_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_drop_shadow_mask_region",
            ),
            morphology_axis_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_morphology_axis_region",
            ),
            blur_region: create_pipeline(device, &pipeline_layout, &shader, "filter_blur_region"),
            svg_mask_coverage_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_svg_mask_coverage_region",
            ),
            apply_region_mask: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_apply_region_mask",
            ),
            color_filter_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_color_region",
            ),
            color_matrix_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_color_matrix_region",
            ),
            component_transfer_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_component_transfer_region",
            ),
            convolve_matrix_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_convolve_matrix_region",
            ),
            lighting_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_lighting_region",
            ),
            liquid_glass_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_liquid_glass_region",
            ),
            blend_region: create_pipeline(device, &pipeline_layout, &shader, "filter_blend_region"),
            composite_inputs_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_composite_inputs_region",
            ),
            displacement_map_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_displacement_map_region",
            ),
            turbulence_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_turbulence_region",
            ),
            composite_drop_shadow_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_composite_drop_shadow_region",
            ),
            layer_mask_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_layer_mask_region",
            ),
            rect_mask_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_rect_mask_region",
            ),
            path_mask_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_path_mask_region",
            ),
            composite_stack_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_composite_stack_region",
            ),
            composite_blend_stack_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_composite_blend_stack_region",
            ),
            composite_surface_stack_region: create_pipeline(
                device,
                &pipeline_layout,
                &shader,
                "filter_composite_surface_stack_region",
            ),
            bind_group_layout,
            config,
            _dummy_texture: dummy_texture,
            dummy_texture_view,
            dummy_read,
            dummy_read_write,
        })
    }

    pub(crate) fn clear_buffer(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        color: u32,
    ) {
        self.clear_region(
            device,
            queue,
            target,
            size,
            lengths,
            Bounds::canvas(size.0, size.1),
            color,
        );
    }

    pub(crate) fn clear_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        color: u32,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.clear_color = color;
        self.dispatch(
            device,
            queue,
            &self.clear_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn copy_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
    ) {
        let Some(config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        self.dispatch(
            device,
            queue,
            &self.copy_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn source_alpha_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
    ) {
        let Some(config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        self.dispatch(
            device,
            queue,
            &self.source_alpha_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn source_over_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
    ) {
        let Some(config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        self.dispatch(
            device,
            queue,
            &self.source_over_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn tile_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        source_region: Bounds,
    ) {
        if source_region.is_empty() {
            return;
        }
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.rect_x0 = source_region.x0 as f32;
        config.rect_y0 = source_region.y0 as f32;
        config.rect_x1 = source_region.x1 as f32;
        config.rect_y1 = source_region.y1 as f32;
        self.dispatch(
            device,
            queue,
            &self.tile_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn offset_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.offset_x = dx;
        config.offset_y = dy;
        self.dispatch(
            device,
            queue,
            &self.offset_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn blend_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        input1: &::wgpu::TextureView,
        input2: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        mode: Mix,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.blend_mode = encode_blend_mode(BlendMode::new(mode, Compose::SrcOver));
        self.dispatch(
            device,
            queue,
            &self.blend_region,
            &config,
            input1,
            input2,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_inputs_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        input1: &::wgpu::TextureView,
        input2: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        operator: CompositeOperator,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.filter_kind = encode_composite_operator(operator);
        config.matrix_bias = composite_arithmetic(operator);
        self.dispatch(
            device,
            queue,
            &self.composite_inputs_region,
            &config,
            input1,
            input2,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn displacement_map_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        input1: &::wgpu::TextureView,
        input2: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        displacement: &DisplacementMap,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.amount = displacement.scale_x;
        config.rect_x0 = displacement.scale_y;
        config.kernel_edge_mode = encode_color_channel(displacement.x_channel);
        config.kernel_preserve_alpha = encode_color_channel(displacement.y_channel);
        config.lighting_output_kind = u32::from(displacement.linear_rgb);
        self.dispatch(
            device,
            queue,
            &self.displacement_map_region,
            &config,
            input1,
            input2,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn turbulence_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        turbulence: &Turbulence,
        table_index: u32,
        tables: &WgpuFilterTurbulenceBindings<'_>,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.table_index = table_index;
        config.turbulence_base_frequency_x = turbulence.base_frequency_x;
        config.turbulence_base_frequency_y = turbulence.base_frequency_y;
        config.turbulence_num_octaves = turbulence.num_octaves;
        config.turbulence_stitch_tiles = u32::from(turbulence.stitch_tiles);
        config.turbulence_kind = encode_turbulence_kind(turbulence.kind);
        config.turbulence_linear_rgb = u32::from(turbulence.linear_rgb);
        config.turbulence_transform_x = turbulence.transform_x;
        config.turbulence_transform_y = turbulence.transform_y;
        config.turbulence_scale_x = turbulence.scale_x;
        config.turbulence_scale_y = turbulence.scale_y;
        config.turbulence_tile_x = turbulence.tile_x;
        config.turbulence_tile_y = turbulence.tile_y;
        config.turbulence_tile_width = turbulence.tile_width;
        config.turbulence_tile_height = turbulence.tile_height;
        self.dispatch_with_extra(
            device,
            queue,
            &self.turbulence_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
            None,
            None,
            None,
            Some(tables),
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn flood_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        brush_index: u32,
        brushes: &WgpuFilterBrushBindings<'_>,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.brush_index = brush_index;
        self.dispatch_with_extra(
            device,
            queue,
            &self.flood_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
            None,
            Some(brushes),
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_drop_shadow_mask(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.offset_x = dx;
        config.offset_y = dy;
        self.dispatch(
            device,
            queue,
            &self.drop_shadow_mask_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn morphology_axis_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        radius: u32,
        operator: u32,
        axis: u32,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.morphology_radius = radius;
        config.morphology_operator = operator;
        config.morphology_axis = axis;
        self.dispatch(
            device,
            queue,
            &self.morphology_axis_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn blur_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        std_dev: f32,
        axis: u32,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.amount = std_dev;
        config.blur_axis = axis;
        self.dispatch(
            device,
            queue,
            &self.blur_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn svg_mask_coverage(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        kind: MaskKind,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.mask_kind = encode_mask_kind(kind);
        self.dispatch(
            device,
            queue,
            &self.svg_mask_coverage_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn apply_region_mask(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        mask: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
    ) {
        let Some(config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        self.dispatch(
            device,
            queue,
            &self.apply_region_mask,
            &config,
            &self.dummy_texture_view,
            mask,
            target,
            None,
        );
    }

    pub(crate) fn apply_color_filter(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        filter_kind: u32,
        amount: f32,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.filter_kind = filter_kind;
        config.amount = amount;
        self.dispatch(
            device,
            queue,
            &self.color_filter_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_color_matrix(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        matrix: [f32; 20],
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.matrix_r = [matrix[0], matrix[1], matrix[2], matrix[3]];
        config.matrix_g = [matrix[5], matrix[6], matrix[7], matrix[8]];
        config.matrix_b = [matrix[10], matrix[11], matrix[12], matrix[13]];
        config.matrix_a = [matrix[15], matrix[16], matrix[17], matrix[18]];
        config.matrix_bias = [matrix[4], matrix[9], matrix[14], matrix[19]];
        self.dispatch(
            device,
            queue,
            &self.color_matrix_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_component_transfer(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        table_index: u32,
        transfer_tables: &::wgpu::Buffer,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.table_index = table_index;
        self.dispatch_with_transfer(
            device,
            queue,
            &self.component_transfer_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
            Some(transfer_tables),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convolve_matrix_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        matrix: &ConvolveMatrix,
        kernel_offset: u32,
        kernels: &::wgpu::Buffer,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.kernel_offset = kernel_offset;
        config.kernel_columns = matrix.columns;
        config.kernel_rows = matrix.rows;
        config.kernel_target_x = matrix.target_x;
        config.kernel_target_y = matrix.target_y;
        config.kernel_edge_mode = encode_convolve_edge_mode(matrix.edge_mode);
        config.kernel_preserve_alpha = u32::from(matrix.preserve_alpha);
        config.amount = matrix.divisor;
        config.rect_x0 = matrix.bias;
        self.dispatch_with_extra(
            device,
            queue,
            &self.convolve_matrix_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
            None,
            None,
            Some(kernels),
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn diffuse_lighting_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        lighting: &DiffuseLighting,
        surface_origin: (i32, i32),
    ) {
        self.lighting_region(
            device,
            queue,
            source,
            target,
            size,
            lengths,
            bounds,
            0,
            lighting.surface_scale,
            lighting.diffuse_constant,
            1.0,
            lighting.lighting_color,
            lighting.light_source,
            surface_origin,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn specular_lighting_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        lighting: &SpecularLighting,
        surface_origin: (i32, i32),
    ) {
        self.lighting_region(
            device,
            queue,
            source,
            target,
            size,
            lengths,
            bounds,
            1,
            lighting.surface_scale,
            lighting.specular_constant,
            lighting.specular_exponent,
            lighting.lighting_color,
            lighting.light_source,
            surface_origin,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rect_liquid_glass_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        blurred: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        glass: RectLiquidGlass,
        region: RectLiquidGlassRegion,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        let [tint_r, tint_g, tint_b, tint_a] = glass.tint.components;
        config.rect_x0 = region.x0;
        config.rect_y0 = region.y0;
        config.rect_x1 = region.x1;
        config.rect_y1 = region.y1;
        config.radius_top_left = region.radius_top_left;
        config.radius_top_right = region.radius_top_right;
        config.radius_bottom_left = region.radius_bottom_left;
        config.radius_bottom_right = region.radius_bottom_right;
        config.mask_enabled = u32::from(glass.blur_edge);
        config.liquid_tint_r = tint_r;
        config.liquid_tint_g = tint_g;
        config.liquid_tint_b = tint_b;
        config.liquid_tint_a = tint_a;
        config.liquid_refraction_thickness = glass.refraction_thickness;
        config.liquid_refraction_factor = glass.refraction_factor;
        config.liquid_refraction_dispersion = glass.refraction_dispersion;
        config.liquid_fresnel_range = glass.fresnel_range;
        config.liquid_fresnel_hardness = glass.fresnel_hardness * 0.01;
        config.liquid_fresnel_factor = glass.fresnel_factor * 0.01;
        config.liquid_glare_range = glass.glare_range;
        config.liquid_glare_hardness = glass.glare_hardness * 0.01;
        config.liquid_glare_convergence = glass.glare_convergence * 0.01;
        config.liquid_glare_opposite_factor = glass.glare_opposite_factor * 0.01;
        config.liquid_glare_factor = glass.glare_factor * 0.01;
        config.liquid_glare_angle = glass.glare_angle;
        self.dispatch(
            device,
            queue,
            &self.liquid_glass_region,
            &config,
            source,
            blurred,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn lighting_region(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        output_kind: u32,
        surface_scale: f32,
        light_constant: f32,
        specular_exponent: f32,
        lighting_color: [f32; 3],
        light_source: LightSource,
        surface_origin: (i32, i32),
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        let light_params = light_source_params(light_source);
        config.lighting_output_kind = output_kind;
        config.surface_scale = surface_scale;
        config.light_constant = light_constant;
        config.specular_exponent = specular_exponent;
        config.light_r = lighting_color[0];
        config.light_g = lighting_color[1];
        config.light_b = lighting_color[2];
        config.light_kind = encode_light_source_kind(light_source);
        config.light_p0 = light_params[0];
        config.light_p1 = light_params[1];
        config.light_p2 = light_params[2];
        config.light_p3 = light_params[3];
        config.light_p4 = light_params[4];
        config.light_p5 = light_params[5];
        config.light_p6 = light_params[6];
        config.light_p7 = light_params[7];
        config.light_p8 = light_params[8];
        config.surface_origin_x = surface_origin.0;
        config.surface_origin_y = surface_origin.1;
        self.dispatch(
            device,
            queue,
            &self.lighting_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_drop_shadow(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        shadow_mask: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        brush_index: u32,
        brushes: &WgpuFilterBrushBindings<'_>,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.brush_index = brush_index;
        self.dispatch_with_extra(
            device,
            queue,
            &self.composite_drop_shadow_region,
            &config,
            &self.dummy_texture_view,
            shadow_mask,
            target,
            None,
            None,
            Some(brushes),
            None,
            None,
            None,
        );
    }

    pub(crate) fn build_layer_mask(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bindings: &WgpuFilterBindings<'_>,
        draw_ix: u32,
        bounds: Bounds,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.draw_ix = draw_ix;
        self.dispatch(
            device,
            queue,
            &self.layer_mask_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            Some(bindings),
        );
    }

    pub(crate) fn build_region_mask(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        region: &Region,
        path_index: Option<u32>,
        paths: &WgpuFilterPathBindings<'_>,
        bounds: Bounds,
    ) -> bool {
        match region {
            Region::Rect { rect, radius } => {
                let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
                    return true;
                };
                config.rect_x0 = rect.x0 as f32;
                config.rect_y0 = rect.y0 as f32;
                config.rect_x1 = rect.x1 as f32;
                config.rect_y1 = rect.y1 as f32;
                config.radius_top_left = radius.top_left;
                config.radius_top_right = radius.top_right;
                config.radius_bottom_left = radius.bottom_left;
                config.radius_bottom_right = radius.bottom_right;
                self.dispatch(
                    device,
                    queue,
                    &self.rect_mask_region,
                    &config,
                    &self.dummy_texture_view,
                    &self.dummy_texture_view,
                    target,
                    None,
                );
                true
            }
            Region::Path { .. } => {
                let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
                    return true;
                };
                let Some(path_index) = path_index else {
                    return false;
                };
                config.table_index = path_index;
                self.dispatch_with_extra(
                    device,
                    queue,
                    &self.path_mask_region,
                    &config,
                    &self.dummy_texture_view,
                    &self.dummy_texture_view,
                    target,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(paths),
                );
                true
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_src_over_with_stack(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        source: &::wgpu::TextureView,
        mask: Option<&::wgpu::TextureView>,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bindings: &WgpuFilterBindings<'_>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.layer_stack_start = layer_stack.start as u32;
        config.layer_stack_end = layer_stack.end as u32;
        config.mask_enabled = u32::from(mask.is_some());
        self.dispatch(
            device,
            queue,
            &self.composite_stack_region,
            &config,
            source,
            mask.unwrap_or(&self.dummy_texture_view),
            target,
            Some(bindings),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_blend_with_stack(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        source: &::wgpu::TextureView,
        mask: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bindings: &WgpuFilterBindings<'_>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        mode: BlendMode,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.layer_stack_start = layer_stack.start as u32;
        config.layer_stack_end = layer_stack.end as u32;
        config.blend_mode = encode_blend_mode(mode);
        self.dispatch(
            device,
            queue,
            &self.composite_blend_stack_region,
            &config,
            source,
            mask,
            target,
            Some(bindings),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_src_over_surface_with_stack(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        target: &::wgpu::TextureView,
        source: &::wgpu::TextureView,
        target_size: (u32, u32),
        source_size: (u32, u32),
        source_origin: (i32, i32),
        lengths: GpuBufferLengths,
        bindings: &WgpuFilterBindings<'_>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) {
        let Some(mut config) = config_for_bounds(target_size, lengths, bounds) else {
            return;
        };
        config.layer_stack_start = layer_stack.start as u32;
        config.layer_stack_end = layer_stack.end as u32;
        config.offset_x = source_origin.0;
        config.offset_y = source_origin.1;
        config.kernel_columns = source_size.0;
        config.kernel_rows = source_size.1;
        self.dispatch(
            device,
            queue,
            &self.composite_surface_stack_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            Some(bindings),
        );
    }

    fn dispatch(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        pipeline: &::wgpu::ComputePipeline,
        config: &FilterConfig,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        bindings: Option<&WgpuFilterBindings<'_>>,
    ) {
        self.dispatch_with_extra(
            device, queue, pipeline, config, source, aux, target, bindings, None, None, None, None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_with_transfer(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        pipeline: &::wgpu::ComputePipeline,
        config: &FilterConfig,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        bindings: Option<&WgpuFilterBindings<'_>>,
        transfer_tables: Option<&::wgpu::Buffer>,
    ) {
        self.dispatch_with_extra(
            device,
            queue,
            pipeline,
            config,
            source,
            aux,
            target,
            bindings,
            transfer_tables,
            None,
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_with_extra(
        &self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        pipeline: &::wgpu::ComputePipeline,
        config: &FilterConfig,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        bindings: Option<&WgpuFilterBindings<'_>>,
        transfer_tables: Option<&::wgpu::Buffer>,
        brushes: Option<&WgpuFilterBrushBindings<'_>>,
        convolve_kernels: Option<&::wgpu::Buffer>,
        turbulence_tables: Option<&WgpuFilterTurbulenceBindings<'_>>,
        path_bindings: Option<&WgpuFilterPathBindings<'_>>,
    ) {
        if config.pixel_count == 0 {
            return;
        }
        queue.write_buffer(&self.config, 0, bytemuck::bytes_of(config));
        let bind_group = self.create_bind_group(
            device,
            source,
            aux,
            target,
            bindings,
            transfer_tables,
            brushes,
            convolve_kernels,
            turbulence_tables,
            path_bindings,
        );
        let mut encoder = device.create_command_encoder(&::wgpu::CommandEncoderDescriptor {
            label: Some("tileink wgpu filter encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some("tileink wgpu filter pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(config.pixel_count.div_ceil(WORKGROUP_SIZE), 1, 1);
        }
        queue.submit([encoder.finish()]);
    }

    fn create_bind_group(
        &self,
        device: &::wgpu::Device,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        bindings: Option<&WgpuFilterBindings<'_>>,
        transfer_tables: Option<&::wgpu::Buffer>,
        brushes: Option<&WgpuFilterBrushBindings<'_>>,
        convolve_kernels: Option<&::wgpu::Buffer>,
        turbulence_tables: Option<&WgpuFilterTurbulenceBindings<'_>>,
        path_bindings: Option<&WgpuFilterPathBindings<'_>>,
    ) -> ::wgpu::BindGroup {
        let fallback = WgpuFilterBindings {
            draw_path_ids: &self.dummy_read,
            draw_flags: &self.dummy_read,
            draw_pixel_x0: &self.dummy_read,
            draw_pixel_y0: &self.dummy_read,
            draw_pixel_x1: &self.dummy_read,
            draw_pixel_y1: &self.dummy_read,
            draw_sdf_refs: &self.dummy_read,
            sdf_kinds: &self.dummy_read,
            sdf_x0: &self.dummy_read,
            sdf_y0: &self.dummy_read,
            sdf_x1: &self.dummy_read,
            sdf_y1: &self.dummy_read,
            sdf_r0: &self.dummy_read,
            sdf_r1: &self.dummy_read,
            sdf_r2: &self.dummy_read,
            sdf_r3: &self.dummy_read,
            sdf_stroke_top: &self.dummy_read,
            sdf_stroke_right: &self.dummy_read,
            sdf_stroke_bottom: &self.dummy_read,
            sdf_stroke_left: &self.dummy_read,
            sdf_shadow_offset_x: &self.dummy_read,
            sdf_shadow_offset_y: &self.dummy_read,
            sdf_shadow_expand: &self.dummy_read,
            sdf_shadow_intensity: &self.dummy_read,
            backdrop_data_offsets: &self.dummy_read,
            backdrop_tile_x0: &self.dummy_read,
            backdrop_tile_y0: &self.dummy_read,
            backdrop_tile_x1: &self.dummy_read,
            backdrop_tile_y1: &self.dummy_read,
            backdrops: &self.dummy_read_write,
            segment_starts: &self.dummy_read,
            segment_ends: &self.dummy_read,
            segment_p0x: &self.dummy_read,
            segment_p0y: &self.dummy_read,
            segment_p1x: &self.dummy_read,
            segment_p1y: &self.dummy_read,
            segment_y_edge: &self.dummy_read,
            layer_stack_tags: &self.dummy_read,
            layer_stack_draws: &self.dummy_read,
            layer_stack_payloads: &self.dummy_read,
        };
        let bindings = bindings.unwrap_or(&fallback);
        let brush_data = brushes.map_or(&self.dummy_read, |brushes| brushes.data);
        let brush_params = brushes.map_or(&self.dummy_read, |brushes| brushes.params);
        let brush_payloads = brushes.map_or(&self.dummy_read, |brushes| brushes.payloads);
        let convolve_kernels = convolve_kernels.unwrap_or(&self.dummy_read);
        let turbulence_selectors =
            turbulence_tables.map_or(&self.dummy_read, |tables| tables.selectors);
        let turbulence_gradients =
            turbulence_tables.map_or(&self.dummy_read, |tables| tables.gradients);
        let path_range_starts =
            path_bindings.map_or(&self.dummy_read, |bindings| bindings.range_starts);
        let path_range_ends =
            path_bindings.map_or(&self.dummy_read, |bindings| bindings.range_ends);
        let path_p0x = path_bindings.map_or(&self.dummy_read, |bindings| bindings.p0x);
        let path_p0y = path_bindings.map_or(&self.dummy_read, |bindings| bindings.p0y);
        let path_p1x = path_bindings.map_or(&self.dummy_read, |bindings| bindings.p1x);
        let path_p1y = path_bindings.map_or(&self.dummy_read, |bindings| bindings.p1y);
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu filter bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                bind_buffer(0, &self.config),
                bind_texture(1, source),
                bind_texture(2, aux),
                bind_texture(3, target),
                bind_buffer(4, bindings.draw_path_ids),
                bind_buffer(5, bindings.draw_flags),
                bind_buffer(6, bindings.draw_pixel_x0),
                bind_buffer(7, bindings.draw_pixel_y0),
                bind_buffer(8, bindings.draw_pixel_x1),
                bind_buffer(9, bindings.draw_pixel_y1),
                bind_buffer(10, bindings.draw_sdf_refs),
                bind_buffer(11, bindings.sdf_kinds),
                bind_buffer(12, bindings.sdf_x0),
                bind_buffer(13, bindings.sdf_y0),
                bind_buffer(14, bindings.sdf_x1),
                bind_buffer(15, bindings.sdf_y1),
                bind_buffer(16, bindings.sdf_r0),
                bind_buffer(17, bindings.sdf_r1),
                bind_buffer(18, bindings.sdf_r2),
                bind_buffer(19, bindings.sdf_r3),
                bind_buffer(20, bindings.sdf_stroke_top),
                bind_buffer(21, bindings.sdf_stroke_right),
                bind_buffer(22, bindings.sdf_stroke_bottom),
                bind_buffer(23, bindings.sdf_stroke_left),
                bind_buffer(24, bindings.sdf_shadow_offset_x),
                bind_buffer(25, bindings.sdf_shadow_offset_y),
                bind_buffer(26, bindings.sdf_shadow_expand),
                bind_buffer(27, bindings.sdf_shadow_intensity),
                bind_buffer(28, bindings.backdrop_data_offsets),
                bind_buffer(29, bindings.backdrop_tile_x0),
                bind_buffer(30, bindings.backdrop_tile_y0),
                bind_buffer(31, bindings.backdrop_tile_x1),
                bind_buffer(32, bindings.backdrop_tile_y1),
                bind_buffer(33, bindings.backdrops),
                bind_buffer(34, bindings.segment_starts),
                bind_buffer(35, bindings.segment_ends),
                bind_buffer(36, bindings.segment_p0x),
                bind_buffer(37, bindings.segment_p0y),
                bind_buffer(38, bindings.segment_p1x),
                bind_buffer(39, bindings.segment_p1y),
                bind_buffer(40, bindings.segment_y_edge),
                bind_buffer(41, bindings.layer_stack_tags),
                bind_buffer(42, bindings.layer_stack_draws),
                bind_buffer(43, bindings.layer_stack_payloads),
                bind_buffer(44, transfer_tables.unwrap_or(&self.dummy_read)),
                bind_buffer(45, brush_data),
                bind_buffer(46, brush_params),
                bind_buffer(47, brush_payloads),
                bind_buffer(48, convolve_kernels),
                bind_buffer(49, turbulence_selectors),
                bind_buffer(50, turbulence_gradients),
                bind_buffer(51, path_range_starts),
                bind_buffer(52, path_range_ends),
                bind_buffer(53, path_p0x),
                bind_buffer(54, path_p0y),
                bind_buffer(55, path_p1x),
                bind_buffer(56, path_p1y),
            ],
        })
    }
}

pub(crate) fn encode_color_filter(filter: &Filter) -> Option<(u32, f32)> {
    match filter {
        Filter::Brightness(amount) => Some((FILTER_BRIGHTNESS, *amount)),
        Filter::Contrast(amount) => Some((FILTER_CONTRAST, *amount)),
        Filter::Grayscale(amount) => Some((FILTER_GRAYSCALE, *amount)),
        Filter::HueRotate(amount) => Some((FILTER_HUE_ROTATE, *amount)),
        Filter::Invert(amount) => Some((FILTER_INVERT, *amount)),
        Filter::Opacity(amount) => Some((FILTER_OPACITY, *amount)),
        Filter::Saturate(amount) => Some((FILTER_SATURATE, *amount)),
        Filter::Sepia(amount) => Some((FILTER_SEPIA, *amount)),
        _ => None,
    }
}

fn encode_convolve_edge_mode(edge_mode: ConvolveEdgeMode) -> u32 {
    match edge_mode {
        ConvolveEdgeMode::None => 0,
        ConvolveEdgeMode::Duplicate => 1,
        ConvolveEdgeMode::Wrap => 2,
    }
}

fn encode_color_channel(channel: ColorChannel) -> u32 {
    match channel {
        ColorChannel::R => 0,
        ColorChannel::G => 1,
        ColorChannel::B => 2,
        ColorChannel::A => 3,
    }
}

fn encode_light_source_kind(light_source: LightSource) -> u32 {
    match light_source {
        LightSource::Distant { .. } => 0,
        LightSource::Point { .. } => 1,
        LightSource::Spot { .. } => 2,
    }
}

fn light_source_params(light_source: LightSource) -> [f32; 9] {
    match light_source {
        LightSource::Distant { azimuth, elevation } => {
            [azimuth, elevation, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0]
        }
        LightSource::Point { x, y, z } => [x, y, z, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0],
        LightSource::Spot {
            x,
            y,
            z,
            points_at_x,
            points_at_y,
            points_at_z,
            specular_exponent,
            limiting_cone_angle,
        } => [
            x,
            y,
            z,
            points_at_x,
            points_at_y,
            points_at_z,
            specular_exponent,
            limiting_cone_angle.unwrap_or(-1.0),
            0.0,
        ],
    }
}

fn encode_composite_operator(operator: CompositeOperator) -> u32 {
    match operator {
        CompositeOperator::Over => 0,
        CompositeOperator::In => 1,
        CompositeOperator::Out => 2,
        CompositeOperator::Atop => 3,
        CompositeOperator::Xor => 4,
        CompositeOperator::Arithmetic { .. } => 5,
    }
}

fn composite_arithmetic(operator: CompositeOperator) -> [f32; 4] {
    match operator {
        CompositeOperator::Arithmetic { k1, k2, k3, k4 } => [k1, k2, k3, k4],
        _ => [0.0; 4],
    }
}

fn encode_turbulence_kind(kind: TurbulenceKind) -> u32 {
    match kind {
        TurbulenceKind::Turbulence => 0,
        TurbulenceKind::FractalNoise => 1,
    }
}

fn config_for_bounds(
    size: (u32, u32),
    lengths: GpuBufferLengths,
    bounds: Bounds,
) -> Option<FilterConfig> {
    let canvas = Bounds::canvas(size.0, size.1);
    let bounds = bounds.intersect(canvas);
    if bounds.is_empty() {
        return None;
    }
    let width = bounds.width();
    let height = bounds.height();
    Some(FilterConfig {
        width: size.0,
        height: size.1,
        tiles_width: lengths.tiles_width as u32,
        tiles_height: lengths.tiles_height as u32,
        region_x0: bounds.x0 as u32,
        region_y0: bounds.y0 as u32,
        region_width: width,
        region_height: height,
        pixel_count: width * height,
        ..FilterConfig::default()
    })
}

fn encode_mask_kind(kind: MaskKind) -> u32 {
    match kind {
        MaskKind::Alpha => SVG_MASK_ALPHA,
        MaskKind::Luminance => SVG_MASK_LUMINANCE,
    }
}

fn encode_blend_mode(mode: BlendMode) -> u32 {
    mode.mix as u32 | ((mode.compose as u32) << 8)
}

pub(crate) fn region_bounds(region: &Region) -> Bounds {
    match region {
        Region::Rect { rect, .. } => rect_bounds(*rect),
        Region::Path {
            path,
            transform,
            tolerance: _,
        } => rect_bounds(transform.transform_rect_bbox(path.bounding_box())),
    }
}

fn rect_bounds(rect: Rect) -> Bounds {
    Bounds::new(
        rect.x0.floor() as i32,
        rect.y0.floor() as i32,
        rect.x1.ceil() as i32,
        rect.y1.ceil() as i32,
    )
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

fn filter_layout_entries() -> [::wgpu::BindGroupLayoutEntry; 57] {
    [
        uniform_entry(0),
        storage_texture_entry(1, ::wgpu::StorageTextureAccess::ReadOnly),
        storage_texture_entry(2, ::wgpu::StorageTextureAccess::ReadOnly),
        storage_texture_entry(3, ::wgpu::StorageTextureAccess::ReadWrite),
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
        storage_entry(23, true),
        storage_entry(24, true),
        storage_entry(25, true),
        storage_entry(26, true),
        storage_entry(27, true),
        storage_entry(28, true),
        storage_entry(29, true),
        storage_entry(30, true),
        storage_entry(31, true),
        storage_entry(32, true),
        storage_entry(33, false),
        storage_entry(34, true),
        storage_entry(35, true),
        storage_entry(36, true),
        storage_entry(37, true),
        storage_entry(38, true),
        storage_entry(39, true),
        storage_entry(40, true),
        storage_entry(41, true),
        storage_entry(42, true),
        storage_entry(43, true),
        storage_entry(44, true),
        storage_entry(45, true),
        storage_entry(46, true),
        storage_entry(47, true),
        storage_entry(48, true),
        storage_entry(49, true),
        storage_entry(50, true),
        storage_entry(51, true),
        storage_entry(52, true),
        storage_entry(53, true),
        storage_entry(54, true),
        storage_entry(55, true),
        storage_entry(56, true),
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

fn storage_texture_entry(
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

fn bind_buffer(binding: u32, buffer: &::wgpu::Buffer) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn bind_texture(binding: u32, view: &::wgpu::TextureView) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::TextureView(view),
    }
}
