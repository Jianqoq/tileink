#![allow(clippy::too_many_arguments)]

use peniko::{
    BlendMode, Compose, Mix,
    kurbo::{Rect, Shape},
};

use crate::shared::{
    bounds::Bounds,
    gpu_layout::filter as filter_layout,
    gpu_plan::GpuBufferLengths,
    layer::{
        filter::{
            BlurDownsampleFilter, BlurSampling, BlurUpsampleFilter, ColorChannel,
            CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting, DisplacementMap,
            Filter, LightSource, RectLiquidGlass, RectLiquidGlassRegion, SpecularLighting,
            Turbulence, TurbulenceKind,
        },
        mask::MaskKind,
        region::Region,
    },
};

use super::{
    canvas::WgpuFilterBindings,
    commands::{
        WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
    },
    profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope},
};

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
const SHARED_BLUR_TILE_WIDTH: u32 = 16;
const SHARED_BLUR_TILE_HEIGHT: u32 = 16;
const SHARED_BLUR_MAX_RADIUS: u32 = 16;
const STORAGE_BINDING_COUNT: u32 = filter_layout::MAX_STORAGE_BUFFER_COUNT;

const FILTER_RES_DRAW_RECORDS: u32 = 1 << 0;
const FILTER_RES_SDF_BLOB: u32 = 1 << 1;
const FILTER_RES_SDF_SHADOW_BLOB: u32 = 1 << 2;
const FILTER_RES_PATH_RECORDS: u32 = 1 << 3;
const FILTER_RES_BACKDROPS: u32 = 1 << 4;
const FILTER_RES_SEGMENT_RANGES: u32 = 1 << 5;
const FILTER_RES_SEGMENTS: u32 = 1 << 6;
const FILTER_RES_LAYER_STACK: u32 = 1 << 7;
const FILTER_RES_TRANSFER_TABLES: u32 = 1 << 8;
const FILTER_RES_BRUSH_BLOB: u32 = 1 << 9;
const FILTER_RES_CONVOLVE_KERNELS: u32 = 1 << 10;
const FILTER_RES_TURBULENCE_SELECTORS: u32 = 1 << 11;
const FILTER_RES_TURBULENCE_GRADIENTS: u32 = 1 << 12;
const FILTER_RES_PATH_RANGE_STARTS: u32 = 1 << 13;
const FILTER_RES_PATH_RANGE_ENDS: u32 = 1 << 14;
const FILTER_RES_PATH_P0X: u32 = 1 << 15;
const FILTER_RES_PATH_P0Y: u32 = 1 << 16;
const FILTER_RES_PATH_P1X: u32 = 1 << 17;
const FILTER_RES_PATH_P1Y: u32 = 1 << 18;

const FILTER_RES_SCENE_ALPHA: u32 = FILTER_RES_DRAW_RECORDS
    | FILTER_RES_SDF_BLOB
    | FILTER_RES_SDF_SHADOW_BLOB
    | FILTER_RES_PATH_RECORDS
    | FILTER_RES_BACKDROPS
    | FILTER_RES_SEGMENT_RANGES
    | FILTER_RES_SEGMENTS;
const FILTER_RES_SCENE_STACK: u32 = FILTER_RES_SCENE_ALPHA | FILTER_RES_LAYER_STACK;
const FILTER_RES_TRANSFER: u32 = FILTER_RES_TRANSFER_TABLES;
const FILTER_RES_BRUSH: u32 = FILTER_RES_BRUSH_BLOB;
const FILTER_RES_CONVOLVE: u32 = FILTER_RES_CONVOLVE_KERNELS;
const FILTER_RES_TURBULENCE: u32 =
    FILTER_RES_TURBULENCE_SELECTORS | FILTER_RES_TURBULENCE_GRADIENTS;
const FILTER_RES_PATH_MASK: u32 = FILTER_RES_PATH_RANGE_STARTS
    | FILTER_RES_PATH_RANGE_ENDS
    | FILTER_RES_PATH_P0X
    | FILTER_RES_PATH_P0Y
    | FILTER_RES_PATH_P1X
    | FILTER_RES_PATH_P1Y;

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
    downsample: u32,
    downsample_filter: u32,
    upsample_filter: u32,
    downsample_pad: u32,
    source_x0: u32,
    source_y0: u32,
    source_x1: u32,
    source_y1: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    draw_ix: u32,
    mask_enabled: u32,
    blend_mode: u32,
    mask_kind: u32,
    clear_color: u32,
    filter_kind: u32,
    table_index: u32,
    brush_offset: u32,
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
            downsample: 1,
            downsample_filter: 0,
            upsample_filter: 0,
            downsample_pad: 0,
            source_x0: 0,
            source_y0: 0,
            source_x1: 0,
            source_y1: 0,
            layer_stack_start: 0,
            layer_stack_end: 0,
            draw_ix: 0,
            mask_enabled: 0,
            blend_mode: 0,
            mask_kind: 0,
            clear_color: 0,
            filter_kind: 0,
            table_index: 0,
            brush_offset: 0,
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
    clear_region: FilterKernel,
    copy_region: FilterKernel,
    source_alpha_region: FilterKernel,
    source_over_region: FilterKernel,
    tile_region: FilterKernel,
    offset_region: FilterKernel,
    flood_region: FilterKernel,
    drop_shadow_mask_region: FilterKernel,
    morphology_axis_region: FilterKernel,
    downsample_region: FilterKernel,
    upsample_region: FilterKernel,
    upsample_rect_composite_region: FilterKernel,
    blur_region: FilterKernel,
    blur_shared_region: FilterKernel,
    svg_mask_coverage_region: FilterKernel,
    apply_region_mask: FilterKernel,
    color_filter_region: FilterKernel,
    color_matrix_region: FilterKernel,
    component_transfer_region: FilterKernel,
    convolve_matrix_region: FilterKernel,
    lighting_region: FilterKernel,
    liquid_glass_region: FilterKernel,
    liquid_glass_rect_composite_region: FilterKernel,
    blend_region: FilterKernel,
    composite_inputs_region: FilterKernel,
    displacement_map_region: FilterKernel,
    turbulence_region: FilterKernel,
    composite_drop_shadow_region: FilterKernel,
    layer_mask_region: FilterKernel,
    rect_mask_region: FilterKernel,
    path_mask_region: FilterKernel,
    composite_direct_region: FilterKernel,
    composite_rect_direct_region: FilterKernel,
    composite_stack_region: FilterKernel,
    composite_blend_stack_region: FilterKernel,
    composite_surface_direct_region: FilterKernel,
    composite_surface_stack_region: FilterKernel,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
    config_slots: u64,
    _dummy_texture: ::wgpu::Texture,
    dummy_texture_view: ::wgpu::TextureView,
    dummy_sampler: ::wgpu::Sampler,
    dummy_read: ::wgpu::Buffer,
    dummy_read_write: ::wgpu::Buffer,
}

struct FilterKernel {
    pipeline: ::wgpu::ComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    resources: u32,
    profile: FilterProfile,
    shared_workgroups: bool,
    portable_textures: bool,
}

#[derive(Clone, Copy)]
enum FilterProfile {
    Clear,
    Copy,
    SourceAlpha,
    SourceOver,
    Tile,
    Offset,
    Flood,
    DropShadowMask,
    MorphologyAxis,
    Downsample,
    Upsample,
    UpsampleRectComposite,
    Blur,
    SvgMaskCoverage,
    ApplyRegionMask,
    ColorFilter,
    ColorMatrix,
    ComponentTransfer,
    ConvolveMatrix,
    Lighting,
    LiquidGlass,
    LiquidGlassRectComposite,
    Blend,
    CompositeInputs,
    DisplacementMap,
    Turbulence,
    CompositeDropShadow,
    LayerMask,
    RectMask,
    PathMask,
    CompositeDirect,
    CompositeRectDirect,
    CompositeStack,
    CompositeBlendStack,
    CompositeSurfaceDirect,
    CompositeSurfaceStack,
}

pub(crate) struct WgpuFilterBrushBindings<'a> {
    pub(crate) blob: &'a ::wgpu::Buffer,
    pub(crate) image_resource_atlas: &'a ::wgpu::TextureView,
    pub(crate) image_resource_sampler: &'a ::wgpu::Sampler,
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

const DUMMY_STORAGE_BUFFER_SIZE: ::wgpu::BufferAddress = 256;

impl WgpuFilterPipeline {
    pub(crate) fn new(device: &::wgpu::Device) -> Option<Self> {
        let portable_textures = !device
            .features()
            .contains(::wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        if device.limits().max_storage_buffers_per_shader_stage < STORAGE_BINDING_COUNT {
            return None;
        }

        let shader_source = filter_shader_source(portable_textures);
        let config_size = std::mem::size_of::<FilterConfig>() as ::wgpu::BufferAddress;
        let config_stride = aligned_uniform_stride(device, config_size);
        let config_slots = WGPU_CONFIG_SLOTS;
        let config = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu filter config"),
            size: uniform_slots_buffer_size(device, config_size),
            usage: ::wgpu::BufferUsages::UNIFORM | ::wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_read = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu filter read dummy buffer"),
            size: DUMMY_STORAGE_BUFFER_SIZE,
            usage: ::wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let dummy_read_write = device.create_buffer(&::wgpu::BufferDescriptor {
            label: Some("tileink wgpu filter read-write dummy buffer"),
            size: DUMMY_STORAGE_BUFFER_SIZE,
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
            usage: ::wgpu::TextureUsages::STORAGE_BINDING | ::wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let dummy_texture_view =
            dummy_texture.create_view(&::wgpu::TextureViewDescriptor::default());
        let dummy_sampler = device.create_sampler(&::wgpu::SamplerDescriptor {
            label: Some("tileink wgpu filter dummy sampler"),
            mag_filter: ::wgpu::FilterMode::Linear,
            min_filter: ::wgpu::FilterMode::Linear,
            mipmap_filter: ::wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Some(Self {
            clear_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_clear_region",
                0,
                FilterProfile::Clear,
                false,
            ),
            copy_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_copy_region",
                0,
                FilterProfile::Copy,
                false,
            ),
            source_alpha_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_source_alpha_region",
                0,
                FilterProfile::SourceAlpha,
                false,
            ),
            source_over_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_source_over_region",
                0,
                FilterProfile::SourceOver,
                false,
            ),
            tile_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_tile_region",
                0,
                FilterProfile::Tile,
                false,
            ),
            offset_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_offset_region",
                0,
                FilterProfile::Offset,
                false,
            ),
            flood_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_flood_region",
                FILTER_RES_BRUSH,
                FilterProfile::Flood,
                false,
            ),
            drop_shadow_mask_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_drop_shadow_mask_region",
                0,
                FilterProfile::DropShadowMask,
                false,
            ),
            morphology_axis_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_morphology_axis_region",
                0,
                FilterProfile::MorphologyAxis,
                false,
            ),
            downsample_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_downsample_region",
                0,
                FilterProfile::Downsample,
                false,
            ),
            upsample_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_upsample_region",
                0,
                FilterProfile::Upsample,
                false,
            ),
            upsample_rect_composite_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_upsample_rect_composite_region",
                0,
                FilterProfile::UpsampleRectComposite,
                false,
            ),
            blur_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_blur_region",
                0,
                FilterProfile::Blur,
                false,
            ),
            blur_shared_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_blur_shared_region",
                0,
                FilterProfile::Blur,
                true,
            ),
            svg_mask_coverage_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_svg_mask_coverage_region",
                0,
                FilterProfile::SvgMaskCoverage,
                false,
            ),
            apply_region_mask: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_apply_region_mask",
                0,
                FilterProfile::ApplyRegionMask,
                false,
            ),
            color_filter_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_color_region",
                0,
                FilterProfile::ColorFilter,
                false,
            ),
            color_matrix_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_color_matrix_region",
                0,
                FilterProfile::ColorMatrix,
                false,
            ),
            component_transfer_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_component_transfer_region",
                FILTER_RES_TRANSFER,
                FilterProfile::ComponentTransfer,
                false,
            ),
            convolve_matrix_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_convolve_matrix_region",
                FILTER_RES_CONVOLVE,
                FilterProfile::ConvolveMatrix,
                false,
            ),
            lighting_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_lighting_region",
                0,
                FilterProfile::Lighting,
                false,
            ),
            liquid_glass_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_liquid_glass_region",
                0,
                FilterProfile::LiquidGlass,
                false,
            ),
            liquid_glass_rect_composite_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_liquid_glass_rect_composite_region",
                0,
                FilterProfile::LiquidGlassRectComposite,
                false,
            ),
            blend_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_blend_region",
                0,
                FilterProfile::Blend,
                false,
            ),
            composite_inputs_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_inputs_region",
                0,
                FilterProfile::CompositeInputs,
                false,
            ),
            displacement_map_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_displacement_map_region",
                0,
                FilterProfile::DisplacementMap,
                false,
            ),
            turbulence_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_turbulence_region",
                FILTER_RES_TURBULENCE,
                FilterProfile::Turbulence,
                false,
            ),
            composite_drop_shadow_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_drop_shadow_region",
                FILTER_RES_BRUSH,
                FilterProfile::CompositeDropShadow,
                false,
            ),
            layer_mask_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_layer_mask_region",
                FILTER_RES_SCENE_ALPHA,
                FilterProfile::LayerMask,
                false,
            ),
            rect_mask_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_rect_mask_region",
                0,
                FilterProfile::RectMask,
                false,
            ),
            path_mask_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_path_mask_region",
                FILTER_RES_PATH_MASK,
                FilterProfile::PathMask,
                false,
            ),
            composite_direct_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_direct_region",
                0,
                FilterProfile::CompositeDirect,
                false,
            ),
            composite_rect_direct_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_rect_direct_region",
                0,
                FilterProfile::CompositeRectDirect,
                false,
            ),
            composite_stack_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_stack_region",
                FILTER_RES_SCENE_STACK,
                FilterProfile::CompositeStack,
                false,
            ),
            composite_blend_stack_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_blend_stack_region",
                FILTER_RES_SCENE_STACK,
                FilterProfile::CompositeBlendStack,
                false,
            ),
            composite_surface_direct_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_surface_direct_region",
                0,
                FilterProfile::CompositeSurfaceDirect,
                false,
            ),
            composite_surface_stack_region: create_kernel(
                device,
                shader_source,
                portable_textures,
                "filter_composite_surface_stack_region",
                FILTER_RES_SCENE_STACK,
                FilterProfile::CompositeSurfaceStack,
                false,
            ),
            config,
            config_size,
            config_stride,
            config_slots,
            _dummy_texture: dummy_texture,
            dummy_texture_view,
            dummy_sampler,
            dummy_read,
            dummy_read_write,
        })
    }

    pub(crate) fn clear_buffer(
        &self,
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        color: u32,
    ) {
        self.clear_region(
            commands,
            target,
            size,
            lengths,
            Bounds::canvas(size.0, size.1),
            color,
        );
    }

    pub(crate) fn clear_region(
        &self,
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
    ) {
        let Some(config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        self.dispatch_with_target_read(
            commands,
            &self.source_over_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
            Some(target_read),
        );
    }

    pub(crate) fn tile_region(
        &self,
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
        input1: &::wgpu::TextureView,
        input2: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        mode: Mix,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.blend_mode = encode_blend_mode(BlendMode::new(mode, Compose::SrcOver));
        self.dispatch_with_target_read(
            commands,
            &self.blend_region,
            &config,
            input1,
            input2,
            target,
            None,
            Some(target_read),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_inputs_region(
        &self,
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        brush_offset: u32,
        brushes: &WgpuFilterBrushBindings<'_>,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.brush_offset = brush_offset;
        self.dispatch_with_extra(
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
        let pipeline = if shared_blur_radius(std_dev).is_some() {
            &self.blur_shared_region
        } else {
            &self.blur_region
        };
        self.dispatch(
            commands,
            pipeline,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    pub(crate) fn svg_mask_coverage(
        &self,
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
        mask: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
    ) {
        let Some(config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        self.dispatch_with_target_read(
            commands,
            &self.apply_region_mask,
            &config,
            &self.dummy_texture_view,
            mask,
            target,
            None,
            Some(target_read),
        );
    }

    pub(crate) fn apply_color_filter(
        &self,
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
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
        self.dispatch_with_target_read(
            commands,
            &self.color_filter_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
            Some(target_read),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_color_matrix(
        &self,
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
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
        self.dispatch_with_target_read(
            commands,
            &self.color_matrix_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            None,
            Some(target_read),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_component_transfer(
        &self,
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
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
        self.dispatch_with_transfer_target_read(
            commands,
            &self.component_transfer_region,
            &config,
            &self.dummy_texture_view,
            &self.dummy_texture_view,
            target,
            Some(target_read),
            None,
            Some(transfer_tables),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convolve_matrix_region(
        &self,
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        lighting: &DiffuseLighting,
        surface_origin: (i32, i32),
    ) {
        self.lighting_region(
            commands,
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
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        lighting: &SpecularLighting,
        surface_origin: (i32, i32),
    ) {
        self.lighting_region(
            commands,
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
        commands: &mut WgpuCommandBatch,
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
        configure_rect_liquid_glass(&mut config, glass, region);
        self.dispatch(
            commands,
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
        shadow_mask: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        brush_offset: u32,
        brushes: &WgpuFilterBrushBindings<'_>,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, bounds) else {
            return;
        };
        config.brush_offset = brush_offset;
        self.dispatch_with_extra_target_read(
            commands,
            &self.composite_drop_shadow_region,
            &config,
            &self.dummy_texture_view,
            shadow_mask,
            target,
            Some(target_read),
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
        commands: &mut WgpuCommandBatch,
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
            commands,
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
        commands: &mut WgpuCommandBatch,
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
                    commands,
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
                    commands,
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
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
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
        let needs_stack = !layer_stack.is_empty();
        let pipeline = if needs_stack {
            &self.composite_stack_region
        } else {
            &self.composite_direct_region
        };
        let bindings = if needs_stack { Some(bindings) } else { None };
        self.dispatch_with_target_read(
            commands,
            pipeline,
            &config,
            source,
            mask.unwrap_or(&self.dummy_texture_view),
            target,
            bindings,
            Some(target_read),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_src_over_rect_mask_direct(
        &self,
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
        source: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        bounds: Bounds,
        region: &Region,
    ) -> bool {
        let Region::Rect { rect, radius } = region else {
            return false;
        };
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
        self.dispatch_with_target_read(
            commands,
            &self.composite_rect_direct_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
            Some(target_read),
        );
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_blend_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
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
        self.dispatch_with_target_read(
            commands,
            &self.composite_blend_stack_region,
            &config,
            source,
            mask,
            target,
            Some(bindings),
            Some(target_read),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_src_over_surface_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
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
        let needs_stack = !layer_stack.is_empty();
        let pipeline = if needs_stack {
            &self.composite_surface_stack_region
        } else {
            &self.composite_surface_direct_region
        };
        let bindings = if needs_stack { Some(bindings) } else { None };
        self.dispatch_with_target_read(
            commands,
            pipeline,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            bindings,
            Some(target_read),
        );
    }

    fn dispatch(
        &self,
        commands: &mut WgpuCommandBatch,
        pipeline: &FilterKernel,
        config: &FilterConfig,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        bindings: Option<&WgpuFilterBindings<'_>>,
    ) {
        self.dispatch_with_extra_target_read(
            commands, pipeline, config, source, aux, target, None, bindings, None, None, None,
            None, None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_with_target_read(
        &self,
        commands: &mut WgpuCommandBatch,
        pipeline: &FilterKernel,
        config: &FilterConfig,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        bindings: Option<&WgpuFilterBindings<'_>>,
        target_read: Option<&::wgpu::TextureView>,
    ) {
        self.dispatch_with_extra_target_read(
            commands,
            pipeline,
            config,
            source,
            aux,
            target,
            target_read,
            bindings,
            None,
            None,
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_with_transfer_target_read(
        &self,
        commands: &mut WgpuCommandBatch,
        pipeline: &FilterKernel,
        config: &FilterConfig,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: Option<&::wgpu::TextureView>,
        bindings: Option<&WgpuFilterBindings<'_>>,
        transfer_tables: Option<&::wgpu::Buffer>,
    ) {
        self.dispatch_with_extra_target_read(
            commands,
            pipeline,
            config,
            source,
            aux,
            target,
            target_read,
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
        commands: &mut WgpuCommandBatch,
        pipeline: &FilterKernel,
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
        self.dispatch_with_extra_target_read(
            commands,
            pipeline,
            config,
            source,
            aux,
            target,
            None,
            bindings,
            transfer_tables,
            brushes,
            convolve_kernels,
            turbulence_tables,
            path_bindings,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_with_extra_target_read(
        &self,
        commands: &mut WgpuCommandBatch,
        pipeline: &FilterKernel,
        config: &FilterConfig,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: Option<&::wgpu::TextureView>,
        bindings: Option<&WgpuFilterBindings<'_>>,
        transfer_tables: Option<&::wgpu::Buffer>,
        brushes: Option<&WgpuFilterBrushBindings<'_>>,
        convolve_kernels: Option<&::wgpu::Buffer>,
        turbulence_tables: Option<&WgpuFilterTurbulenceBindings<'_>>,
        path_bindings: Option<&WgpuFilterPathBindings<'_>>,
    ) {
        let profile_name = self.profile_name_for_pipeline(pipeline, config);
        let _profile_scope = start_cpu_scope(profile_name);
        if config.pixel_count == 0 {
            return;
        }

        let config_offset = commands.write_uniform_slot(
            "filter.config",
            &self.config,
            self.config_size,
            self.config_stride,
            self.config_slots,
            bytemuck::bytes_of(config),
        );
        let bind_group = self.create_bind_group(
            pipeline,
            commands.device(),
            config_offset,
            source,
            aux,
            target,
            target_read,
            bindings,
            transfer_tables,
            brushes,
            convolve_kernels,
            turbulence_tables,
            path_bindings,
        );

        let gpu_scope = start_gpu_scope(commands.device(), profile_name);
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some(profile_name),
                timestamp_writes,
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let workgroups = self.dispatch_workgroups_for_pipeline(pipeline, config);
            pass.dispatch_workgroups(workgroups.0, workgroups.1, workgroups.2);
        }
        finish_gpu_scope(encoder, gpu_scope);
    }

    fn dispatch_workgroups_for_pipeline(
        &self,
        pipeline: &FilterKernel,
        config: &FilterConfig,
    ) -> (u32, u32, u32) {
        if pipeline.shared_workgroups {
            (
                config.region_width.div_ceil(SHARED_BLUR_TILE_WIDTH),
                config.region_height.div_ceil(SHARED_BLUR_TILE_HEIGHT),
                1,
            )
        } else {
            (config.pixel_count.div_ceil(WORKGROUP_SIZE), 1, 1)
        }
    }

    fn profile_name_for_pipeline(
        &self,
        pipeline: &FilterKernel,
        config: &FilterConfig,
    ) -> &'static str {
        match pipeline.profile {
            FilterProfile::Clear => "filter.clear",
            FilterProfile::Copy => "filter.copy",
            FilterProfile::SourceAlpha => "filter.source_alpha",
            FilterProfile::SourceOver => "filter.source_over",
            FilterProfile::Tile => "filter.tile",
            FilterProfile::Offset => "filter.offset",
            FilterProfile::Flood => "filter.flood",
            FilterProfile::DropShadowMask => "filter.drop_shadow.mask",
            FilterProfile::MorphologyAxis => {
                if config.morphology_axis == 0 {
                    "filter.morphology.x"
                } else {
                    "filter.morphology.y"
                }
            }
            FilterProfile::Downsample => "filter.downsample",
            FilterProfile::Upsample => "filter.upsample",
            FilterProfile::UpsampleRectComposite => "filter.upsample.composite.rect",
            FilterProfile::Blur => {
                if config.blur_axis == 0 {
                    "filter.blur.x"
                } else {
                    "filter.blur.y"
                }
            }
            FilterProfile::SvgMaskCoverage => "filter.mask.svg_coverage",
            FilterProfile::ApplyRegionMask => "filter.mask.apply",
            FilterProfile::ColorFilter => profile_name_for_color_filter(config.filter_kind),
            FilterProfile::ColorMatrix => "filter.color_matrix",
            FilterProfile::ComponentTransfer => "filter.component_transfer",
            FilterProfile::ConvolveMatrix => "filter.convolve",
            FilterProfile::Lighting => {
                if config.lighting_output_kind == 0 {
                    "filter.lighting.diffuse"
                } else {
                    "filter.lighting.specular"
                }
            }
            FilterProfile::LiquidGlass => "filter.liquid_glass",
            FilterProfile::LiquidGlassRectComposite => "filter.liquid_glass.composite.rect",
            FilterProfile::Blend => "filter.blend",
            FilterProfile::CompositeInputs => "filter.composite",
            FilterProfile::DisplacementMap => "filter.displacement",
            FilterProfile::Turbulence => "filter.turbulence",
            FilterProfile::CompositeDropShadow => "filter.drop_shadow.composite",
            FilterProfile::LayerMask => "filter.mask.layer",
            FilterProfile::RectMask => "filter.mask.rect",
            FilterProfile::PathMask => "filter.mask.path",
            FilterProfile::CompositeDirect => "filter.composite.direct",
            FilterProfile::CompositeRectDirect => "filter.composite.rect_direct",
            FilterProfile::CompositeStack => "filter.stack.src_over",
            FilterProfile::CompositeBlendStack => "filter.stack.blend",
            FilterProfile::CompositeSurfaceDirect => "filter.composite.surface.direct",
            FilterProfile::CompositeSurfaceStack => "filter.stack.surface",
        }
    }

    fn create_bind_group(
        &self,
        kernel: &FilterKernel,
        device: &::wgpu::Device,
        config_offset: ::wgpu::BufferAddress,
        source: &::wgpu::TextureView,
        aux: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: Option<&::wgpu::TextureView>,
        bindings: Option<&WgpuFilterBindings<'_>>,
        transfer_tables: Option<&::wgpu::Buffer>,
        brushes: Option<&WgpuFilterBrushBindings<'_>>,
        convolve_kernels: Option<&::wgpu::Buffer>,
        turbulence_tables: Option<&WgpuFilterTurbulenceBindings<'_>>,
        path_bindings: Option<&WgpuFilterPathBindings<'_>>,
    ) -> ::wgpu::BindGroup {
        let fallback = WgpuFilterBindings {
            draw_records: &self.dummy_read,
            sdf_blob: &self.dummy_read,
            sdf_shadow_blob: &self.dummy_read,
            path_records: &self.dummy_read,
            backdrops: &self.dummy_read_write,
            segment_ranges: &self.dummy_read,
            segments: &self.dummy_read,
            layer_stack: &self.dummy_read,
        };
        let bindings = bindings.unwrap_or(&fallback);
        let brush_blob = brushes.map_or(&self.dummy_read, |brushes| brushes.blob);
        let image_resource_atlas = brushes.map_or(&self.dummy_texture_view, |brushes| {
            brushes.image_resource_atlas
        });
        let image_resource_sampler = brushes.map_or(&self.dummy_sampler, |brushes| {
            brushes.image_resource_sampler
        });
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
        let mut entries = vec![
            bind_config_buffer(0, &self.config, config_offset, self.config_size),
            bind_texture(1, source),
            bind_texture(2, aux),
            bind_texture(3, target),
        ];
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_DRAW_RECORDS,
            4,
            bindings.draw_records,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_SDF_BLOB,
            10,
            bindings.sdf_blob,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_SDF_SHADOW_BLOB,
            11,
            bindings.sdf_shadow_blob,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_PATH_RECORDS,
            28,
            bindings.path_records,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_BACKDROPS,
            29,
            bindings.backdrops,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_SEGMENT_RANGES,
            30,
            bindings.segment_ranges,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_SEGMENTS,
            32,
            bindings.segments,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_LAYER_STACK,
            33,
            bindings.layer_stack,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_TRANSFER_TABLES,
            36,
            transfer_tables.unwrap_or(&self.dummy_read),
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_BRUSH_BLOB,
            37,
            brush_blob,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_CONVOLVE_KERNELS,
            40,
            convolve_kernels,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_TURBULENCE_SELECTORS,
            41,
            turbulence_selectors,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_TURBULENCE_GRADIENTS,
            42,
            turbulence_gradients,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_PATH_RANGE_STARTS,
            43,
            path_range_starts,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_PATH_RANGE_ENDS,
            44,
            path_range_ends,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_PATH_P0X,
            45,
            path_p0x,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_PATH_P0Y,
            46,
            path_p0y,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_PATH_P1X,
            47,
            path_p1x,
        );
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_PATH_P1Y,
            48,
            path_p1y,
        );
        entries.push(bind_texture(
            filter_binding(
                kernel.portable_textures,
                kernel.resources,
                filter_layout::SOURCE_SAMPLE_TEXTURE_BINDING,
            ),
            source,
        ));
        entries.push(bind_texture(
            filter_binding(
                kernel.portable_textures,
                kernel.resources,
                filter_layout::AUX_SAMPLE_TEXTURE_BINDING,
            ),
            aux,
        ));
        entries.push(bind_sampler(
            filter_binding(
                kernel.portable_textures,
                kernel.resources,
                filter_layout::LINEAR_SAMPLER_BINDING,
            ),
            &self.dummy_sampler,
        ));
        if kernel.portable_textures {
            entries.push(bind_texture(
                filter_binding(kernel.portable_textures, kernel.resources, 55),
                target_read.unwrap_or(&self.dummy_texture_view),
            ));
        }
        if kernel.resources & FILTER_RES_BRUSH != 0 {
            entries.push(bind_texture(
                filter_binding(
                    kernel.portable_textures,
                    kernel.resources,
                    filter_layout::IMAGE_RESOURCE_ATLAS_BINDING,
                ),
                image_resource_atlas,
            ));
            entries.push(bind_sampler(
                filter_binding(
                    kernel.portable_textures,
                    kernel.resources,
                    filter_layout::IMAGE_RESOURCE_SAMPLER_BINDING,
                ),
                image_resource_sampler,
            ));
        }
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu filter bind group"),
            layout: &kernel.bind_group_layout,
            entries: &entries,
        })
    }
}

impl WgpuFilterPipeline {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn downsample_region(
        &self,
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        source_bounds: Bounds,
        target_bounds: Bounds,
        sampling: BlurSampling,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, target_bounds) else {
            return;
        };
        config.rect_x0 = source_bounds.x0 as f32;
        config.rect_y0 = source_bounds.y0 as f32;
        config.rect_x1 = source_bounds.x1 as f32;
        config.rect_y1 = source_bounds.y1 as f32;
        config.downsample = sampling.factor();
        config.downsample_filter = encode_blur_downsample_filter(sampling.downsample_filter);
        self.dispatch(
            commands,
            &self.downsample_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upsample_region(
        &self,
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        target_bounds: Bounds,
        source_bounds: Bounds,
        sampling: BlurSampling,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, target_bounds) else {
            return;
        };
        config.rect_x0 = source_bounds.x0 as f32;
        config.rect_y0 = source_bounds.y0 as f32;
        config.rect_x1 = source_bounds.x1 as f32;
        config.rect_y1 = source_bounds.y1 as f32;
        config.downsample = sampling.factor();
        config.upsample_filter = encode_blur_upsample_filter(sampling.upsample_filter);
        self.dispatch(
            commands,
            &self.upsample_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn upsample_rect_composite_region(
        &self,
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        target_bounds: Bounds,
        source_bounds: Bounds,
        sampling: BlurSampling,
        region: &Region,
    ) -> bool {
        let Region::Rect { rect, radius } = region else {
            return false;
        };
        let Some(mut config) = config_for_bounds(size, lengths, target_bounds) else {
            return true;
        };
        config.source_x0 = source_bounds.x0 as u32;
        config.source_y0 = source_bounds.y0 as u32;
        config.source_x1 = source_bounds.x1 as u32;
        config.source_y1 = source_bounds.y1 as u32;
        config.downsample = sampling.factor();
        config.upsample_filter = encode_blur_upsample_filter(sampling.upsample_filter);
        config.rect_x0 = rect.x0 as f32;
        config.rect_y0 = rect.y0 as f32;
        config.rect_x1 = rect.x1 as f32;
        config.rect_y1 = rect.y1 as f32;
        config.radius_top_left = radius.top_left;
        config.radius_top_right = radius.top_right;
        config.radius_bottom_left = radius.bottom_left;
        config.radius_bottom_right = radius.bottom_right;
        self.dispatch_with_target_read(
            commands,
            &self.upsample_rect_composite_region,
            &config,
            source,
            &self.dummy_texture_view,
            target,
            None,
            Some(target_read),
        );
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rect_liquid_glass_composite_region(
        &self,
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        blurred: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        target_read: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        target_bounds: Bounds,
        blurred_bounds: Bounds,
        sampling: BlurSampling,
        glass: RectLiquidGlass,
        region: RectLiquidGlassRegion,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, target_bounds) else {
            return;
        };
        configure_rect_liquid_glass(&mut config, glass, region);
        config.source_x0 = blurred_bounds.x0 as u32;
        config.source_y0 = blurred_bounds.y0 as u32;
        config.source_x1 = blurred_bounds.x1 as u32;
        config.source_y1 = blurred_bounds.y1 as u32;
        config.downsample = sampling.factor();
        config.upsample_filter = encode_blur_upsample_filter(sampling.upsample_filter);
        self.dispatch_with_target_read(
            commands,
            &self.liquid_glass_rect_composite_region,
            &config,
            source,
            blurred,
            target,
            None,
            Some(target_read),
        );
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

fn profile_name_for_color_filter(filter_kind: u32) -> &'static str {
    match filter_kind {
        FILTER_BRIGHTNESS => "filter.brightness",
        FILTER_CONTRAST => "filter.contrast",
        FILTER_GRAYSCALE => "filter.grayscale",
        FILTER_HUE_ROTATE => "filter.hue_rotate",
        FILTER_INVERT => "filter.invert",
        FILTER_OPACITY => "filter.opacity",
        FILTER_SATURATE => "filter.saturate",
        FILTER_SEPIA => "filter.sepia",
        _ => "filter.color",
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

fn encode_blur_downsample_filter(filter: BlurDownsampleFilter) -> u32 {
    match filter {
        BlurDownsampleFilter::Nearest => 0,
        BlurDownsampleFilter::Box => 1,
    }
}

fn encode_blur_upsample_filter(filter: BlurUpsampleFilter) -> u32 {
    match filter {
        BlurUpsampleFilter::Nearest => 0,
        BlurUpsampleFilter::Bilinear => 1,
    }
}

fn configure_rect_liquid_glass(
    config: &mut FilterConfig,
    glass: RectLiquidGlass,
    region: RectLiquidGlassRegion,
) {
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
}

fn shared_blur_radius(std_dev: f32) -> Option<u32> {
    let std_dev = std_dev.max(0.0);
    if !std_dev.is_finite() || std_dev <= 0.0 {
        return None;
    }
    let radius = (std_dev * 3.0).ceil().max(1.0) as u32;
    (radius <= SHARED_BLUR_MAX_RADIUS).then_some(radius)
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

fn create_kernel(
    device: &::wgpu::Device,
    shader_source: &'static str,
    portable_textures: bool,
    entry_point: &'static str,
    resources: u32,
    profile: FilterProfile,
    shared_workgroups: bool,
) -> FilterKernel {
    debug_assert!(filter_storage_binding_count(resources) <= STORAGE_BINDING_COUNT);
    let layout_entries = filter_layout_entries(portable_textures, resources);
    let bind_group_layout = device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
        label: Some(entry_point),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
        label: Some(entry_point),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(::wgpu::ShaderModuleDescriptor {
        label: Some(entry_point),
        source: ::wgpu::ShaderSource::Wgsl(
            remap_filter_shader_bindings(shader_source, portable_textures, resources).into(),
        ),
    });
    let pipeline = device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
        label: Some(entry_point),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some(entry_point),
        compilation_options: ::wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    FilterKernel {
        pipeline,
        bind_group_layout,
        resources,
        profile,
        shared_workgroups,
        portable_textures,
    }
}

fn filter_layout_entries(
    portable_textures: bool,
    resources: u32,
) -> Vec<::wgpu::BindGroupLayoutEntry> {
    let mut entries = vec![
        uniform_entry(0),
        read_texture_entry(1, portable_textures),
        read_texture_entry(2, portable_textures),
        write_texture_entry(3, portable_textures),
    ];
    push_storage_entry_if(&mut entries, resources, FILTER_RES_DRAW_RECORDS, 4, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_SDF_BLOB, 10, true);
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_SDF_SHADOW_BLOB,
        11,
        true,
    );
    push_storage_entry_if(&mut entries, resources, FILTER_RES_PATH_RECORDS, 28, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_BACKDROPS, 29, false);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_SEGMENT_RANGES, 30, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_SEGMENTS, 32, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_LAYER_STACK, 33, true);
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_TRANSFER_TABLES,
        36,
        true,
    );
    push_storage_entry_if(&mut entries, resources, FILTER_RES_BRUSH_BLOB, 37, true);
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_CONVOLVE_KERNELS,
        40,
        true,
    );
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_TURBULENCE_SELECTORS,
        41,
        true,
    );
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_TURBULENCE_GRADIENTS,
        42,
        true,
    );
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_PATH_RANGE_STARTS,
        43,
        true,
    );
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_PATH_RANGE_ENDS,
        44,
        true,
    );
    push_storage_entry_if(&mut entries, resources, FILTER_RES_PATH_P0X, 45, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_PATH_P0Y, 46, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_PATH_P1X, 47, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_PATH_P1Y, 48, true);
    entries.push(sampled_filterable_texture_entry(filter_binding(
        portable_textures,
        resources,
        filter_layout::SOURCE_SAMPLE_TEXTURE_BINDING,
    )));
    entries.push(sampled_filterable_texture_entry(filter_binding(
        portable_textures,
        resources,
        filter_layout::AUX_SAMPLE_TEXTURE_BINDING,
    )));
    entries.push(filtering_sampler_entry(filter_binding(
        portable_textures,
        resources,
        filter_layout::LINEAR_SAMPLER_BINDING,
    )));
    if portable_textures {
        entries.push(sampled_texture_entry(filter_binding(
            portable_textures,
            resources,
            55,
        )));
    }
    if resources & FILTER_RES_BRUSH != 0 {
        entries.push(sampled_filterable_texture_entry(filter_binding(
            portable_textures,
            resources,
            filter_layout::IMAGE_RESOURCE_ATLAS_BINDING,
        )));
        entries.push(filtering_sampler_entry(filter_binding(
            portable_textures,
            resources,
            filter_layout::IMAGE_RESOURCE_SAMPLER_BINDING,
        )));
    }
    entries
}

const FILTER_STORAGE_BINDINGS: [(u32, u32); 19] = [
    (FILTER_RES_DRAW_RECORDS, 4),
    (FILTER_RES_SDF_BLOB, 10),
    (FILTER_RES_SDF_SHADOW_BLOB, 11),
    (FILTER_RES_PATH_RECORDS, 28),
    (FILTER_RES_BACKDROPS, 29),
    (FILTER_RES_SEGMENT_RANGES, 30),
    (FILTER_RES_SEGMENTS, 32),
    (FILTER_RES_LAYER_STACK, 33),
    (FILTER_RES_TRANSFER_TABLES, 36),
    (FILTER_RES_BRUSH_BLOB, 37),
    (FILTER_RES_CONVOLVE_KERNELS, 40),
    (FILTER_RES_TURBULENCE_SELECTORS, 41),
    (FILTER_RES_TURBULENCE_GRADIENTS, 42),
    (FILTER_RES_PATH_RANGE_STARTS, 43),
    (FILTER_RES_PATH_RANGE_ENDS, 44),
    (FILTER_RES_PATH_P0X, 45),
    (FILTER_RES_PATH_P0Y, 46),
    (FILTER_RES_PATH_P1X, 47),
    (FILTER_RES_PATH_P1Y, 48),
];

const FILTER_TEXTURE_BINDINGS: [u32; 5] = [
    filter_layout::SOURCE_SAMPLE_TEXTURE_BINDING,
    filter_layout::AUX_SAMPLE_TEXTURE_BINDING,
    filter_layout::LINEAR_SAMPLER_BINDING,
    filter_layout::IMAGE_RESOURCE_ATLAS_BINDING,
    filter_layout::IMAGE_RESOURCE_SAMPLER_BINDING,
];

fn filter_binding(portable_textures: bool, resources: u32, old_binding: u32) -> u32 {
    filter_binding_remaps(portable_textures, resources)
        .into_iter()
        .find_map(|(old, new)| (old == old_binding).then_some(new))
        .unwrap_or(old_binding)
}

fn filter_binding_remaps(portable_textures: bool, resources: u32) -> Vec<(u32, u32)> {
    let mut ordered = vec![0, 1, 2, 3];
    for (flag, old_binding) in FILTER_STORAGE_BINDINGS {
        if resources & flag != 0 {
            ordered.push(old_binding);
        }
    }
    ordered.push(filter_layout::SOURCE_SAMPLE_TEXTURE_BINDING);
    ordered.push(filter_layout::AUX_SAMPLE_TEXTURE_BINDING);
    ordered.push(filter_layout::LINEAR_SAMPLER_BINDING);
    if portable_textures {
        ordered.push(55);
    }
    if resources & FILTER_RES_BRUSH != 0 {
        ordered.push(filter_layout::IMAGE_RESOURCE_ATLAS_BINDING);
        ordered.push(filter_layout::IMAGE_RESOURCE_SAMPLER_BINDING);
    }

    for (_, old_binding) in FILTER_STORAGE_BINDINGS {
        if !ordered.contains(&old_binding) {
            ordered.push(old_binding);
        }
    }
    for old_binding in FILTER_TEXTURE_BINDINGS {
        if !ordered.contains(&old_binding) {
            ordered.push(old_binding);
        }
    }
    if !ordered.contains(&55) {
        ordered.push(55);
    }

    ordered
        .into_iter()
        .enumerate()
        .map(|(new, old)| (old, new as u32))
        .collect()
}

fn remap_filter_shader_bindings(
    source: &'static str,
    portable_textures: bool,
    resources: u32,
) -> String {
    let remaps = filter_binding_remaps(portable_textures, resources);
    let mut out = source.to_owned();
    for (old, _) in &remaps {
        out = out.replace(
            &format!("@binding({old})"),
            &format!("@binding(__tileink_binding_{old}__)"),
        );
    }
    for (old, new) in remaps {
        out = out.replace(&format!("__tileink_binding_{old}__"), &new.to_string());
    }
    out
}

fn push_storage_entry_if(
    entries: &mut Vec<::wgpu::BindGroupLayoutEntry>,
    resources: u32,
    flag: u32,
    binding: u32,
    read_only: bool,
) {
    if resources & flag != 0 {
        entries.push(storage_entry(
            filter_binding(false, resources, binding),
            read_only,
        ));
    }
}

fn filter_storage_binding_count(resources: u32) -> u32 {
    resources.count_ones()
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

fn read_texture_entry(binding: u32, portable_textures: bool) -> ::wgpu::BindGroupLayoutEntry {
    if portable_textures {
        sampled_texture_entry(binding)
    } else {
        storage_texture_entry(binding, ::wgpu::StorageTextureAccess::ReadOnly)
    }
}

fn write_texture_entry(binding: u32, portable_textures: bool) -> ::wgpu::BindGroupLayoutEntry {
    storage_texture_entry(
        binding,
        if portable_textures {
            ::wgpu::StorageTextureAccess::WriteOnly
        } else {
            ::wgpu::StorageTextureAccess::ReadWrite
        },
    )
}

fn sampled_texture_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
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

fn sampled_filterable_texture_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Texture {
            sample_type: ::wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: ::wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn filtering_sampler_entry(binding: u32) -> ::wgpu::BindGroupLayoutEntry {
    ::wgpu::BindGroupLayoutEntry {
        binding,
        visibility: ::wgpu::ShaderStages::COMPUTE,
        ty: ::wgpu::BindingType::Sampler(::wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn filter_shader_source(portable_textures: bool) -> &'static str {
    if portable_textures {
        include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter_web.wgsl"))
    } else {
        include_str!(concat!(env!("OUT_DIR"), "/tileink_wgpu_filter.wgsl"))
    }
}

fn bind_buffer(binding: u32, buffer: &::wgpu::Buffer) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn push_buffer_if<'a>(
    entries: &mut Vec<::wgpu::BindGroupEntry<'a>>,
    resources: u32,
    flag: u32,
    binding: u32,
    buffer: &'a ::wgpu::Buffer,
) {
    if resources & flag != 0 {
        entries.push(bind_buffer(
            filter_binding(false, resources, binding),
            buffer,
        ));
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

fn bind_texture(binding: u32, view: &::wgpu::TextureView) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::TextureView(view),
    }
}

fn bind_sampler(binding: u32, sampler: &::wgpu::Sampler) -> ::wgpu::BindGroupEntry<'_> {
    ::wgpu::BindGroupEntry {
        binding,
        resource: ::wgpu::BindingResource::Sampler(sampler),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_kernel_storage_resource_sets_fit_wgpu_limit() {
        let resource_sets = [
            0,
            FILTER_RES_TRANSFER,
            FILTER_RES_BRUSH,
            FILTER_RES_CONVOLVE,
            FILTER_RES_TURBULENCE,
            FILTER_RES_PATH_MASK,
            FILTER_RES_SCENE_ALPHA,
            FILTER_RES_SCENE_STACK,
        ];
        for resources in resource_sets {
            assert!(
                filter_storage_binding_count(resources) <= STORAGE_BINDING_COUNT,
                "filter resource set has too many storage bindings: {resources:#x}"
            );
        }
        assert_eq!(filter_storage_binding_count(FILTER_RES_SCENE_STACK), 8);
    }

    #[test]
    fn filter_kernel_layouts_always_include_linear_sampling_resources() {
        for portable_textures in [false, true] {
            let entries = filter_layout_entries(portable_textures, 0);
            let bindings: Vec<u32> = entries.iter().map(|entry| entry.binding).collect();
            assert!(bindings.contains(&filter_binding(
                portable_textures,
                0,
                filter_layout::SOURCE_SAMPLE_TEXTURE_BINDING
            )));
            assert!(bindings.contains(&filter_binding(
                portable_textures,
                0,
                filter_layout::AUX_SAMPLE_TEXTURE_BINDING
            )));
            assert!(bindings.contains(&filter_binding(
                portable_textures,
                0,
                filter_layout::LINEAR_SAMPLER_BINDING
            )));
        }
    }

    #[test]
    fn filter_layout_entries_are_sorted_by_binding() {
        let resource_sets = [
            0,
            FILTER_RES_TRANSFER,
            FILTER_RES_BRUSH,
            FILTER_RES_CONVOLVE,
            FILTER_RES_TURBULENCE,
            FILTER_RES_PATH_MASK,
            FILTER_RES_SCENE_ALPHA,
            FILTER_RES_SCENE_STACK,
        ];
        for portable_textures in [false, true] {
            for resources in resource_sets {
                let entries = filter_layout_entries(portable_textures, resources);
                assert_sorted_by_binding(&entries);
            }
        }
    }

    #[test]
    fn filter_layout_entries_are_contiguous() {
        let resource_sets = [
            0,
            FILTER_RES_TRANSFER,
            FILTER_RES_BRUSH,
            FILTER_RES_CONVOLVE,
            FILTER_RES_TURBULENCE,
            FILTER_RES_PATH_MASK,
            FILTER_RES_SCENE_ALPHA,
            FILTER_RES_SCENE_STACK,
        ];
        for portable_textures in [false, true] {
            for resources in resource_sets {
                let entries = filter_layout_entries(portable_textures, resources);
                assert_contiguous_bindings(&entries);
            }
        }
    }

    #[test]
    fn filter_shader_binding_remap_matches_layout_entries() {
        let source = filter_shader_source(false);
        let remapped = remap_filter_shader_bindings(source, false, FILTER_RES_BRUSH);
        let bindings: Vec<u32> = filter_layout_entries(false, FILTER_RES_BRUSH)
            .iter()
            .map(|entry| entry.binding)
            .collect();
        for binding in bindings {
            assert!(
                remapped.contains(&format!("@binding({binding})")),
                "remapped shader is missing binding {binding}"
            );
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
}
