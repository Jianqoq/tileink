#![allow(clippy::too_many_arguments)]

use std::sync::{
    OnceLock,
    atomic::{AtomicU32, Ordering},
};

use peniko::{BlendMode, Compose, Mix};

use crate::shared::{
    bounds::Bounds,
    gpu_layout::filter as filter_layout,
    gpu_plan::GpuBufferLengths,
    layer::{
        filter::{
            BlurSampling, CompositeOperator, ConvolveMatrix, DiffuseLighting, DisplacementMap,
            LightSource, RectLiquidGlass, RectLiquidGlassRegion, SpecularLighting, Turbulence,
        },
        mask::MaskKind,
        region::Region,
    },
};

use super::{
    canvas::{WgpuFilterBindings, WgpuImageResourceBindings},
    commands::{
        WGPU_CONFIG_SLOTS, WgpuCommandBatch, aligned_uniform_stride, uniform_slots_buffer_size,
    },
    filter_work::FilterTileWork,
    image_resources::{
        create_image_resource_bind_group, create_image_resource_bind_group_layout,
        large_texture_table_len, patch_image_resource_shader_source,
    },
    lazy::PipelineCompilationTracker,
    profile::{finish_gpu_scope, start_cpu_scope, start_gpu_scope},
};

pub(crate) const FILTER_BRIGHTNESS: u32 = crate::shared::gpu_constants::FILTER_BRIGHTNESS;
pub(crate) const FILTER_CONTRAST: u32 = crate::shared::gpu_constants::FILTER_CONTRAST;
pub(crate) const FILTER_GRAYSCALE: u32 = crate::shared::gpu_constants::FILTER_GRAYSCALE;
pub(crate) const FILTER_HUE_ROTATE: u32 = crate::shared::gpu_constants::FILTER_HUE_ROTATE;
pub(crate) const FILTER_INVERT: u32 = crate::shared::gpu_constants::FILTER_INVERT;
pub(crate) const FILTER_OPACITY: u32 = crate::shared::gpu_constants::FILTER_OPACITY;
pub(crate) const FILTER_SATURATE: u32 = crate::shared::gpu_constants::FILTER_SATURATE;
pub(crate) const FILTER_SEPIA: u32 = crate::shared::gpu_constants::FILTER_SEPIA;
pub(crate) const SVG_MASK_ALPHA: u32 = 0;
pub(crate) const SVG_MASK_LUMINANCE: u32 = crate::shared::gpu_constants::SVG_MASK_LUMINANCE;

use crate::shared::gpu_constants::{
    FILTER_WORKGROUP_SIZE, SHARED_BLUR_MAX_RADIUS, SHARED_BLUR_TILE_HEIGHT, SHARED_BLUR_TILE_WIDTH,
};
const STORAGE_BINDING_COUNT: u32 = filter_layout::MAX_STORAGE_BUFFER_COUNT;

const FILTER_RES_DRAW_RECORDS: u32 = 1 << 0;
const FILTER_RES_PAINT_BLOB: u32 = 1 << 1;
const FILTER_RES_PATH_RECORDS: u32 = 1 << 2;
const FILTER_RES_BACKDROPS: u32 = 1 << 3;
const FILTER_RES_SEGMENT_RANGES: u32 = 1 << 4;
const FILTER_RES_SEGMENTS: u32 = 1 << 5;
const FILTER_RES_LAYER_STACK: u32 = 1 << 6;
const FILTER_RES_TRANSFER_TABLES: u32 = 1 << 7;
const FILTER_RES_BRUSH_BLOB: u32 = 1 << 8;
const FILTER_RES_CONVOLVE_KERNELS: u32 = 1 << 9;
const FILTER_RES_TURBULENCE_SELECTORS: u32 = 1 << 10;
const FILTER_RES_TURBULENCE_GRADIENTS: u32 = 1 << 11;
const FILTER_RES_PATH_RANGE_STARTS: u32 = 1 << 12;
const FILTER_RES_PATH_RANGE_ENDS: u32 = 1 << 13;
const FILTER_RES_PATH_P0X: u32 = 1 << 14;
const FILTER_RES_PATH_P0Y: u32 = 1 << 15;
const FILTER_RES_PATH_P1X: u32 = 1 << 16;
const FILTER_RES_PATH_P1Y: u32 = 1 << 17;
const FILTER_RES_ACTIVE_TILES: u32 = 1 << 18;
const ACTIVE_TILES_BINDING: u32 = 52;

const FILTER_RES_SCENE_ALPHA: u32 = FILTER_RES_DRAW_RECORDS
    | FILTER_RES_PAINT_BLOB
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

use crate::shared::filter_config::FilterConfig;
use crate::shared::filter_parameters::configure_color_matrix;
use crate::shared::filter_parameters::configure_convolve;
use crate::shared::filter_parameters::configure_displacement;
use crate::shared::filter_parameters::configure_lighting;
use crate::shared::filter_parameters::configure_turbulence;
use crate::shared::filter_parameters::{
    composite_arithmetic, configure_rect_liquid_glass, encode_blur_downsample_filter,
    encode_blur_upsample_filter, encode_composite_operator,
};

const FILTER_KERNEL_RESOURCE_SETS: [u32; 8] = [
    0,
    FILTER_RES_BRUSH,
    FILTER_RES_TRANSFER,
    FILTER_RES_CONVOLVE,
    FILTER_RES_TURBULENCE,
    FILTER_RES_SCENE_ALPHA,
    FILTER_RES_PATH_MASK,
    FILTER_RES_SCENE_STACK,
];

pub(crate) struct WgpuFilterPipeline {
    clear_region: LazyFilterKernel,
    copy_region: LazyFilterKernel,
    source_alpha_region: LazyFilterKernel,
    source_over_region: LazyFilterKernel,
    tile_region: LazyFilterKernel,
    offset_region: LazyFilterKernel,
    flood_region: LazyFilterKernel,
    drop_shadow_mask_region: LazyFilterKernel,
    morphology_axis_region: LazyFilterKernel,
    downsample_region: LazyFilterKernel,
    upsample_region: LazyFilterKernel,
    upsample_rect_composite_region: LazyFilterKernel,
    blur_region: LazyFilterKernel,
    blur_shared_region: LazyFilterKernel,
    svg_mask_coverage_region: LazyFilterKernel,
    apply_region_mask: LazyFilterKernel,
    color_filter_region: LazyFilterKernel,
    color_matrix_region: LazyFilterKernel,
    component_transfer_region: LazyFilterKernel,
    convolve_matrix_region: LazyFilterKernel,
    lighting_region: LazyFilterKernel,
    liquid_glass_region: LazyFilterKernel,
    liquid_glass_rect_composite_region: LazyFilterKernel,
    blend_region: LazyFilterKernel,
    composite_inputs_region: LazyFilterKernel,
    displacement_map_region: LazyFilterKernel,
    turbulence_region: LazyFilterKernel,
    composite_drop_shadow_region: LazyFilterKernel,
    layer_mask_region: LazyFilterKernel,
    rect_mask_region: LazyFilterKernel,
    path_mask_region: LazyFilterKernel,
    composite_direct_region: LazyFilterKernel,
    composite_rect_direct_region: LazyFilterKernel,
    composite_stack_region: LazyFilterKernel,
    composite_blend_stack_region: LazyFilterKernel,
    composite_surface_direct_region: LazyFilterKernel,
    composite_surface_stack_region: LazyFilterKernel,
    config: ::wgpu::Buffer,
    config_size: ::wgpu::BufferAddress,
    config_stride: ::wgpu::BufferAddress,
    config_slots: u64,
    _dummy_texture: ::wgpu::Texture,
    dummy_texture_view: ::wgpu::TextureView,
    _dummy_atlas_texture: ::wgpu::Texture,
    dummy_atlas_view: ::wgpu::TextureView,
    dummy_sampler: ::wgpu::Sampler,
    dummy_read: ::wgpu::Buffer,
    image_bind_group_layout: ::wgpu::BindGroupLayout,
    // The owner fixes device, shader source, texture mode and image-table variant.
    // Only resource remapping varies between its kernels. Sharing those modules
    // fixes repeated WGSL parsing/validation without making pipelines eager.
    shader_modules: [(u32, super::lazy::LazyShaderModule); 8],
    #[cfg(test)]
    created_shader_modules: AtomicU32,
    shader_source: &'static str,
    portable_textures: bool,
    large_texture_table_len: u32,
    pipeline_cache: Option<::wgpu::PipelineCache>,
    compilation_tracker: PipelineCompilationTracker,
    active_tile_work: Option<FilterTileWork>,
    dispatch_count: AtomicU32,
    compact_dispatch_count: AtomicU32,
}

struct LazyFilterKernel {
    kernel: OnceLock<FilterKernel>,
    entry_point: &'static str,
    resources: u32,
    profile: FilterProfile,
    shared_workgroups: bool,
}

impl LazyFilterKernel {
    fn new(
        entry_point: &'static str,
        resources: u32,
        profile: FilterProfile,
        shared_workgroups: bool,
    ) -> Self {
        Self {
            kernel: OnceLock::new(),
            entry_point,
            // All region kernels share the coordinate remap helper. Keeping
            // the worklist binding uniform across kernels avoids a parallel
            // family of compact pipelines and keeps lazy compilation intact.
            resources: resources | FILTER_RES_ACTIVE_TILES,
            profile,
            shared_workgroups,
        }
    }
}

struct FilterKernel {
    pipeline: ::wgpu::ComputePipeline,
    bind_group_layout: ::wgpu::BindGroupLayout,
    resources: u32,
    shared_workgroups: bool,
    portable_textures: bool,
    order_write_only: bool,
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
    pub(crate) image_resource_texture_views: &'a [::wgpu::TextureView],
    pub(crate) image_resource_dummy_texture: &'a ::wgpu::TextureView,
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
    pub(crate) fn new(
        device: &::wgpu::Device,
        pipeline_cache: Option<&::wgpu::PipelineCache>,
        compilation_tracker: &PipelineCompilationTracker,
    ) -> Option<Self> {
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
        let dummy_atlas_texture = device.create_texture(&::wgpu::TextureDescriptor {
            label: Some("tileink wgpu filter dummy image atlas texture"),
            size: ::wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: ::wgpu::TextureDimension::D2,
            format: ::wgpu::TextureFormat::Rgba8Unorm,
            usage: ::wgpu::TextureUsages::TEXTURE_BINDING | ::wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let dummy_atlas_view = dummy_atlas_texture.create_view(&::wgpu::TextureViewDescriptor {
            label: Some("tileink wgpu filter dummy image atlas view"),
            dimension: Some(::wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let dummy_sampler = device.create_sampler(&::wgpu::SamplerDescriptor {
            label: Some("tileink wgpu filter dummy sampler"),
            mag_filter: ::wgpu::FilterMode::Linear,
            min_filter: ::wgpu::FilterMode::Linear,
            mipmap_filter: ::wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let large_texture_table_len = large_texture_table_len(device);
        let image_bind_group_layout =
            create_image_resource_bind_group_layout(device, large_texture_table_len);
        Some(Self {
            clear_region: LazyFilterKernel::new(
                "filter_clear_region",
                0,
                FilterProfile::Clear,
                false,
            ),
            copy_region: LazyFilterKernel::new("filter_copy_region", 0, FilterProfile::Copy, false),
            source_alpha_region: LazyFilterKernel::new(
                "filter_source_alpha_region",
                0,
                FilterProfile::SourceAlpha,
                false,
            ),
            source_over_region: LazyFilterKernel::new(
                "filter_source_over_region",
                0,
                FilterProfile::SourceOver,
                false,
            ),
            tile_region: LazyFilterKernel::new("filter_tile_region", 0, FilterProfile::Tile, false),
            offset_region: LazyFilterKernel::new(
                "filter_offset_region",
                0,
                FilterProfile::Offset,
                false,
            ),
            flood_region: LazyFilterKernel::new(
                "filter_flood_region",
                FILTER_RES_BRUSH,
                FilterProfile::Flood,
                false,
            ),
            drop_shadow_mask_region: LazyFilterKernel::new(
                "filter_drop_shadow_mask_region",
                0,
                FilterProfile::DropShadowMask,
                false,
            ),
            morphology_axis_region: LazyFilterKernel::new(
                "filter_morphology_axis_region",
                0,
                FilterProfile::MorphologyAxis,
                false,
            ),
            downsample_region: LazyFilterKernel::new(
                "filter_downsample_region",
                0,
                FilterProfile::Downsample,
                false,
            ),
            upsample_region: LazyFilterKernel::new(
                "filter_upsample_region",
                0,
                FilterProfile::Upsample,
                false,
            ),
            upsample_rect_composite_region: LazyFilterKernel::new(
                "filter_upsample_rect_composite_region",
                0,
                FilterProfile::UpsampleRectComposite,
                false,
            ),
            blur_region: LazyFilterKernel::new("filter_blur_region", 0, FilterProfile::Blur, false),
            blur_shared_region: LazyFilterKernel::new(
                "filter_blur_shared_region",
                0,
                FilterProfile::Blur,
                true,
            ),
            svg_mask_coverage_region: LazyFilterKernel::new(
                "filter_svg_mask_coverage_region",
                0,
                FilterProfile::SvgMaskCoverage,
                false,
            ),
            apply_region_mask: LazyFilterKernel::new(
                "filter_apply_region_mask",
                0,
                FilterProfile::ApplyRegionMask,
                false,
            ),
            color_filter_region: LazyFilterKernel::new(
                "filter_color_region",
                0,
                FilterProfile::ColorFilter,
                false,
            ),
            color_matrix_region: LazyFilterKernel::new(
                "filter_color_matrix_region",
                0,
                FilterProfile::ColorMatrix,
                false,
            ),
            component_transfer_region: LazyFilterKernel::new(
                "filter_component_transfer_region",
                FILTER_RES_TRANSFER,
                FilterProfile::ComponentTransfer,
                false,
            ),
            convolve_matrix_region: LazyFilterKernel::new(
                "filter_convolve_matrix_region",
                FILTER_RES_CONVOLVE,
                FilterProfile::ConvolveMatrix,
                false,
            ),
            lighting_region: LazyFilterKernel::new(
                "filter_lighting_region",
                0,
                FilterProfile::Lighting,
                false,
            ),
            liquid_glass_region: LazyFilterKernel::new(
                "filter_liquid_glass_region",
                0,
                FilterProfile::LiquidGlass,
                false,
            ),
            liquid_glass_rect_composite_region: LazyFilterKernel::new(
                "filter_liquid_glass_rect_composite_region",
                0,
                FilterProfile::LiquidGlassRectComposite,
                false,
            ),
            blend_region: LazyFilterKernel::new(
                "filter_blend_region",
                0,
                FilterProfile::Blend,
                false,
            ),
            composite_inputs_region: LazyFilterKernel::new(
                "filter_composite_inputs_region",
                0,
                FilterProfile::CompositeInputs,
                false,
            ),
            displacement_map_region: LazyFilterKernel::new(
                "filter_displacement_map_region",
                0,
                FilterProfile::DisplacementMap,
                false,
            ),
            turbulence_region: LazyFilterKernel::new(
                "filter_turbulence_region",
                FILTER_RES_TURBULENCE,
                FilterProfile::Turbulence,
                false,
            ),
            composite_drop_shadow_region: LazyFilterKernel::new(
                "filter_composite_drop_shadow_region",
                FILTER_RES_BRUSH,
                FilterProfile::CompositeDropShadow,
                false,
            ),
            layer_mask_region: LazyFilterKernel::new(
                "filter_layer_mask_region",
                FILTER_RES_SCENE_ALPHA,
                FilterProfile::LayerMask,
                false,
            ),
            rect_mask_region: LazyFilterKernel::new(
                "filter_rect_mask_region",
                0,
                FilterProfile::RectMask,
                false,
            ),
            path_mask_region: LazyFilterKernel::new(
                "filter_path_mask_region",
                FILTER_RES_PATH_MASK,
                FilterProfile::PathMask,
                false,
            ),
            composite_direct_region: LazyFilterKernel::new(
                "filter_composite_direct_region",
                0,
                FilterProfile::CompositeDirect,
                false,
            ),
            composite_rect_direct_region: LazyFilterKernel::new(
                "filter_composite_rect_direct_region",
                0,
                FilterProfile::CompositeRectDirect,
                false,
            ),
            composite_stack_region: LazyFilterKernel::new(
                "filter_composite_stack_region",
                FILTER_RES_SCENE_STACK,
                FilterProfile::CompositeStack,
                false,
            ),
            composite_blend_stack_region: LazyFilterKernel::new(
                "filter_composite_blend_stack_region",
                FILTER_RES_SCENE_STACK,
                FilterProfile::CompositeBlendStack,
                false,
            ),
            composite_surface_direct_region: LazyFilterKernel::new(
                "filter_composite_surface_direct_region",
                0,
                FilterProfile::CompositeSurfaceDirect,
                false,
            ),
            composite_surface_stack_region: LazyFilterKernel::new(
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
            _dummy_atlas_texture: dummy_atlas_texture,
            dummy_atlas_view,
            dummy_sampler,
            dummy_read,
            image_bind_group_layout,
            shader_modules: FILTER_KERNEL_RESOURCE_SETS.map(|resources| {
                (
                    resources | FILTER_RES_ACTIVE_TILES,
                    super::lazy::LazyShaderModule::new("tileink filter shared module"),
                )
            }),
            #[cfg(test)]
            created_shader_modules: AtomicU32::new(0),
            shader_source,
            portable_textures,
            large_texture_table_len,
            pipeline_cache: pipeline_cache.cloned(),
            compilation_tracker: compilation_tracker.clone(),
            active_tile_work: None,
            dispatch_count: AtomicU32::new(0),
            compact_dispatch_count: AtomicU32::new(0),
        })
    }

    pub(crate) fn reset_dispatch_counts(&self) {
        self.dispatch_count.store(0, Ordering::Relaxed);
        self.compact_dispatch_count.store(0, Ordering::Relaxed);
    }

    pub(crate) fn dispatch_counts(&self) -> (u32, u32) {
        (
            self.dispatch_count.load(Ordering::Relaxed),
            self.compact_dispatch_count.load(Ordering::Relaxed),
        )
    }

    /// Restores dense rectangle execution for subsequent filter stages.
    pub(crate) fn clear_active_tile_work(&mut self) {
        self.active_tile_work = None;
    }

    pub(crate) fn active_tile_work(&self) -> Option<FilterTileWork> {
        self.active_tile_work.clone()
    }

    pub(crate) fn restore_active_tile_work(&mut self, work: Option<FilterTileWork>) {
        self.active_tile_work = work;
    }

    fn kernel<'a>(
        &'a self,
        device: &::wgpu::Device,
        lazy: &'a LazyFilterKernel,
    ) -> &'a FilterKernel {
        lazy.kernel.get_or_init(|| {
            // This lookup runs only when a pipeline is first requested. A warm
            // dispatch reads its kernel OnceLock without touching the module cache.
            let module = self
                .shader_modules
                .iter()
                .find(|(resources, _)| *resources == lazy.resources)
                .expect("filter kernel resource set must have a module slot")
                .1
                .get(device, || {
                    let source = patch_image_resource_shader_source(
                        self.shader_source,
                        self.large_texture_table_len > 0,
                    );
                    let source = remap_filter_shader_bindings(
                        &source,
                        self.portable_textures,
                        lazy.resources,
                    );
                    #[cfg(test)]
                    self.created_shader_modules.fetch_add(1, Ordering::Relaxed);
                    ::wgpu::ShaderSource::Wgsl(source.into())
                });
            let kernel = create_filter_kernel(
                device,
                module,
                self.portable_textures,
                &self.image_bind_group_layout,
                lazy.entry_point,
                lazy.resources,
                lazy.shared_workgroups,
                self.pipeline_cache.as_ref(),
            );
            self.compilation_tracker.record();
            kernel
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
        let Some(mut config) =
            config_for_bounds_dense(size, lengths, Bounds::canvas(size.0, size.1))
        else {
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
        configure_displacement(&mut config, displacement);
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
        configure_turbulence(&mut config, turbulence, table_index);
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

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn blur_region_partial(
        &self,
        commands: &mut WgpuCommandBatch,
        source: &::wgpu::TextureView,
        target: &::wgpu::TextureView,
        size: (u32, u32),
        lengths: GpuBufferLengths,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        std_dev: f32,
        axis: u32,
    ) {
        let Some(mut config) = config_for_bounds(size, lengths, output_bounds) else {
            return;
        };
        config.amount = std_dev;
        config.blur_axis = axis;
        config.source_x0 = sample_bounds.x0.max(0) as u32;
        config.source_y0 = sample_bounds.y0.max(0) as u32;
        config.source_x1 = sample_bounds.x1.max(0) as u32;
        config.source_y1 = sample_bounds.y1.max(0) as u32;
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
        configure_color_matrix(&mut config, matrix);
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
        configure_convolve(&mut config, matrix, kernel_offset);
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
        configure_lighting(
            &mut config,
            output_kind,
            surface_scale,
            light_constant,
            specular_exponent,
            lighting_color,
            light_source,
            surface_origin,
        );
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
        pipeline: &LazyFilterKernel,
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
        pipeline: &LazyFilterKernel,
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
        pipeline: &LazyFilterKernel,
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
        pipeline: &LazyFilterKernel,
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
        pipeline: &LazyFilterKernel,
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
        let mut config = *config;
        if config.compact_tiles != 0 {
            if let Some(work) = &self.active_tile_work {
                config.active_tile_count = work.count;
                config.pixel_count = work.count.saturating_mul(FILTER_WORKGROUP_SIZE);
            } else {
                config.compact_tiles = 0;
            }
        }
        if let Some(scene_bindings) = bindings {
            config.paint_sdf_shadow_base = scene_bindings.paint_sdf_shadow_base;
        }
        let profile_name = self.profile_name_for_pipeline(pipeline, &config);
        let _profile_scope = start_cpu_scope(profile_name);
        if config.pixel_count == 0 {
            return;
        }
        self.dispatch_count.fetch_add(1, Ordering::Relaxed);
        if config.compact_tiles != 0 {
            self.compact_dispatch_count.fetch_add(1, Ordering::Relaxed);
        }

        let kernel = self.kernel(commands.device(), pipeline);
        let workgroups = self.dispatch_workgroups_for_pipeline(
            kernel,
            &config,
            commands
                .device()
                .limits()
                .max_compute_workgroups_per_dimension,
        );
        config.dispatch_width = workgroups.0;
        let config_offset = commands.write_uniform_slot(
            &self.config,
            self.config_size,
            self.config_stride,
            self.config_slots,
            bytemuck::bytes_of(&config),
        );
        let bind_group = self.create_bind_group(
            kernel,
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
        let image_bind_group = self.create_image_resource_bind_group(commands.device(), brushes);

        let gpu_scope = start_gpu_scope(commands.device(), profile_name);
        let timestamp_writes = gpu_scope.as_ref().map(|scope| scope.timestamp_writes());
        let encoder = commands.encoder();
        if kernel.order_write_only {
            super::texture_order::prepare_write(encoder, target.texture());
        }
        {
            let mut pass = encoder.begin_compute_pass(&::wgpu::ComputePassDescriptor {
                label: Some(profile_name),
                timestamp_writes,
            });
            pass.set_pipeline(&kernel.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(1, &image_bind_group, &[]);
            pass.dispatch_workgroups(workgroups.0, workgroups.1, workgroups.2);
        }
        finish_gpu_scope(encoder, gpu_scope);
    }

    fn dispatch_workgroups_for_pipeline(
        &self,
        pipeline: &FilterKernel,
        config: &FilterConfig,
        max_workgroups: u32,
    ) -> (u32, u32, u32) {
        if config.compact_tiles == 0 && pipeline.shared_workgroups {
            (
                config.region_width.div_ceil(SHARED_BLUR_TILE_WIDTH),
                config.region_height.div_ceil(SHARED_BLUR_TILE_HEIGHT),
                1,
            )
        } else {
            // Linear filters have the same device limit as fine; a 4096-square clear
            // already needs 65,536 groups. Split rows instead of issuing an invalid dispatch.
            let groups = if config.compact_tiles != 0 {
                config.active_tile_count
            } else {
                config.pixel_count.div_ceil(FILTER_WORKGROUP_SIZE)
            };
            let (x, y) = crate::render::dispatch::dispatch_2d(groups, max_workgroups);
            (x, y, 1)
        }
    }

    fn profile_name_for_pipeline(
        &self,
        pipeline: &LazyFilterKernel,
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
            paint_blob: &self.dummy_read,
            paint_sdf_shadow_base: 0,
            path_records: &self.dummy_read,
            backdrops: &self.dummy_read,
            segment_ranges: &self.dummy_read,
            segments: &self.dummy_read,
            layer_stack: &self.dummy_read,
        };
        let bindings = bindings.unwrap_or(&fallback);
        let brush_blob = brushes.map_or(&self.dummy_read, |brushes| brushes.blob);
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
            bind_texture(filter_layout::SOURCE_TEXTURE_BINDING, source),
            bind_texture(filter_layout::AUX_TEXTURE_BINDING, aux),
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
            FILTER_RES_PAINT_BLOB,
            10,
            bindings.paint_blob,
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
        push_buffer_if(
            &mut entries,
            kernel.resources,
            FILTER_RES_ACTIVE_TILES,
            ACTIVE_TILES_BINDING,
            self.active_tile_work
                .as_ref()
                .map_or(&self.dummy_read, |work| &work.buffer),
        );
        if kernel.portable_textures {
            entries.push(bind_texture(
                filter_binding(kernel.portable_textures, kernel.resources, 55),
                target_read.unwrap_or(&self.dummy_texture_view),
            ));
        }
        device.create_bind_group(&::wgpu::BindGroupDescriptor {
            label: Some("tileink wgpu filter bind group"),
            layout: &kernel.bind_group_layout,
            entries: &entries,
        })
    }

    fn create_image_resource_bind_group(
        &self,
        device: &::wgpu::Device,
        brushes: Option<&WgpuFilterBrushBindings<'_>>,
    ) -> ::wgpu::BindGroup {
        let empty_textures: &[::wgpu::TextureView] = &[];
        let bindings = brushes.map_or(
            WgpuImageResourceBindings {
                atlas: &self.dummy_atlas_view,
                sampler: &self.dummy_sampler,
                texture_views: empty_textures,
                dummy_texture: &self.dummy_texture_view,
            },
            |brushes| WgpuImageResourceBindings {
                atlas: brushes.image_resource_atlas,
                sampler: brushes.image_resource_sampler,
                texture_views: brushes.image_resource_texture_views,
                dummy_texture: brushes.image_resource_dummy_texture,
            },
        );
        create_image_resource_bind_group(
            device,
            &self.image_bind_group_layout,
            &bindings,
            self.large_texture_table_len,
        )
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
        // Retained callers install a projected worklist in the downsampled
        // coordinate space before this stage. Non-retained callers have no
        // worklist and automatically use the same single dense dispatch.
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
        // Request compact execution. The dispatcher falls back to the dense
        // rectangle when no retained active-tile worklist is installed.
        compact_tiles: 1,
        ..FilterConfig::default()
    })
}

fn config_for_bounds_dense(
    size: (u32, u32),
    lengths: GpuBufferLengths,
    bounds: Bounds,
) -> Option<FilterConfig> {
    let mut config = config_for_bounds(size, lengths, bounds)?;
    config.compact_tiles = 0;
    Some(config)
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

fn create_filter_kernel(
    device: &::wgpu::Device,
    shader: &::wgpu::ShaderModule,
    portable_textures: bool,
    image_bind_group_layout: &::wgpu::BindGroupLayout,
    entry_point: &'static str,
    resources: u32,
    shared_workgroups: bool,
    pipeline_cache: Option<&::wgpu::PipelineCache>,
) -> FilterKernel {
    debug_assert!(filter_storage_binding_count(resources) <= STORAGE_BINDING_COUNT);
    let layout_entries = filter_layout_entries(portable_textures, resources);
    let bind_group_layout = device.create_bind_group_layout(&::wgpu::BindGroupLayoutDescriptor {
        label: Some(entry_point),
        entries: &layout_entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&::wgpu::PipelineLayoutDescriptor {
        label: Some(entry_point),
        bind_group_layouts: &[Some(&bind_group_layout), Some(image_bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&::wgpu::ComputePipelineDescriptor {
        label: Some(entry_point),
        layout: Some(&pipeline_layout),
        module: shader,
        entry_point: Some(entry_point),
        compilation_options: ::wgpu::PipelineCompilationOptions::default(),
        cache: pipeline_cache,
    });
    FilterKernel {
        pipeline,
        bind_group_layout,
        resources,
        shared_workgroups,
        portable_textures,
        order_write_only: portable_textures
            && device.adapter_info().backend == ::wgpu::Backend::Dx12,
    }
}

fn filter_layout_entries(
    portable_textures: bool,
    resources: u32,
) -> Vec<::wgpu::BindGroupLayoutEntry> {
    // A single sampled input supports texel loads for every filter. Binding the
    // same input as storage too would combine incompatible DX12 UAV/SRV states.
    let mut entries = vec![
        uniform_entry(0),
        sampled_texture_entry(filter_layout::SOURCE_TEXTURE_BINDING),
        sampled_texture_entry(filter_layout::AUX_TEXTURE_BINDING),
        write_texture_entry(3, portable_textures),
    ];
    push_storage_entry_if(&mut entries, resources, FILTER_RES_DRAW_RECORDS, 4, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_PAINT_BLOB, 10, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_PATH_RECORDS, 28, true);
    push_storage_entry_if(&mut entries, resources, FILTER_RES_BACKDROPS, 29, true);
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
    push_storage_entry_if(
        &mut entries,
        resources,
        FILTER_RES_ACTIVE_TILES,
        ACTIVE_TILES_BINDING,
        true,
    );
    if portable_textures {
        entries.push(sampled_texture_entry(filter_binding(
            portable_textures,
            resources,
            55,
        )));
    }
    entries
}

const FILTER_STORAGE_BINDINGS: [(u32, u32); 19] = [
    (FILTER_RES_DRAW_RECORDS, 4),
    (FILTER_RES_PAINT_BLOB, 10),
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
    (FILTER_RES_ACTIVE_TILES, ACTIVE_TILES_BINDING),
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
    if portable_textures {
        ordered.push(55);
    }
    for (_, old_binding) in FILTER_STORAGE_BINDINGS {
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

fn remap_filter_shader_bindings(source: &str, portable_textures: bool, resources: u32) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_kernel_storage_resource_sets_fit_wgpu_limit() {
        for resources in
            FILTER_KERNEL_RESOURCE_SETS.map(|resources| resources | FILTER_RES_ACTIVE_TILES)
        {
            assert!(
                filter_storage_binding_count(resources) <= STORAGE_BINDING_COUNT,
                "filter resource set has too many storage bindings: {resources:#x}"
            );
        }
        assert_eq!(
            filter_storage_binding_count(FILTER_RES_SCENE_STACK | FILTER_RES_ACTIVE_TILES),
            8
        );
    }

    #[test]
    fn filter_config_keeps_matrix_vec4_alignment() {
        assert_eq!(std::mem::offset_of!(FilterConfig, matrix_r) % 16, 0);
        assert_eq!(std::mem::size_of::<FilterConfig>(), 512);
    }

    #[test]
    fn filter_kernel_inputs_use_sampled_textures_without_storage_aliases() {
        for portable_textures in [false, true] {
            let entries = filter_layout_entries(portable_textures, 0);
            // Input texel loads use an SRV. A storage
            // alias causes an invalid UAV | SRV state transition on DX12.
            for binding in [1, 2] {
                let input = entries
                    .iter()
                    .find(|entry| entry.binding == binding)
                    .unwrap();
                assert!(matches!(
                    input.ty,
                    ::wgpu::BindingType::Texture {
                        sample_type: ::wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: ::wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }
                ));
            }
            assert_eq!(entries.len(), if portable_textures { 5 } else { 4 });
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

#[cfg(test)]
#[path = "filter/lazy_tests.rs"]
mod lazy_tests;

#[cfg(feature = "bench-internals")]
mod benchmark;
#[cfg(feature = "bench-internals")]
pub use benchmark::FilterCompilationBenchmark;
