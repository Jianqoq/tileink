use std::cell::RefCell;

use usvg::filter::EdgeMode;
use wgpu::Device;

use crate::{
    shared::{
        brush::Brush,
        layer::{blend::Blend, filter::Filter, mask::MaskMode},
    },
    wgpu::buffer::{GpuImageBuffer, WgpuBuffer},
};

pub(crate) struct SvgLightingParams {
    pub(crate) mode: u32,
    pub(crate) light_kind: u32,
    pub(crate) has_cone_angle: u32,
    pub(crate) _pad0: u32,
    pub(crate) surface_scale: f32,
    pub(crate) constant: f32,
    pub(crate) primitive_exponent: f32,
    pub(crate) spot_exponent: f32,
    pub(crate) cone_cos: f32,
    pub(crate) _pad1: [f32; 3],
    pub(crate) lighting_color: [f32; 4],
    pub(crate) light_base: [f32; 4],
    pub(crate) light_target: [f32; 4],
}

pub(crate) struct SvgTurbulenceParams {
    pub(crate) base_freq_x: f32,
    pub(crate) base_freq_y: f32,
    pub(crate) offset_x: f32,
    pub(crate) offset_y: f32,
    pub(crate) scale_x: f32,
    pub(crate) scale_y: f32,
    pub(crate) num_octaves: u32,
    pub(crate) fractal_sum: u32,
    pub(crate) stitch_tiles: u32,
    pub(crate) _pad: u32,
}

pub struct FilterGpuPipeline {
    _device: Device,
    keepalive: RefCell<Vec<()>>,
}

impl FilterGpuPipeline {
    pub fn new(device: &Device) -> Self {
        Self {
            _device: device.clone(),
            keepalive: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn clear_dispatch_keepalive(&self) {
        self.keepalive.borrow_mut().clear();
    }

    pub(crate) fn execute_filter(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _image: &mut GpuImageBuffer,
        _filter: Filter,
    ) {
    }

    pub(crate) fn execute_composite(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
    ) {
    }

    pub(crate) fn execute_composite_at(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _offset: (i32, i32),
    ) {
    }

    pub(crate) fn execute_backdrop_composite(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _filtered_backdrop: &GpuImageBuffer,
        _mask: &GpuImageBuffer,
    ) {
    }

    pub(crate) fn execute_mask_composite(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _content: &GpuImageBuffer,
        _mask: &GpuImageBuffer,
        _mode: MaskMode,
    ) {
    }

    pub(crate) fn execute_mask_composite_at(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _content: &GpuImageBuffer,
        _mask: &GpuImageBuffer,
        _offset: (i32, i32),
        _mode: MaskMode,
    ) {
    }

    pub(crate) fn execute_blend_composite(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _blend: &Blend,
    ) {
    }

    pub(crate) fn execute_blend_composite_at(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _offset: (i32, i32),
        _blend: &Blend,
    ) {
    }

    pub(crate) fn execute_copy(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
    ) {
    }

    pub(crate) fn execute_copy_rect(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _src_offset: (i32, i32),
    ) {
    }

    pub(crate) fn execute_color_space_conversion(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _kind: u32,
    ) {
    }

    pub(crate) fn execute_extract_alpha(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
    ) {
    }

    pub(crate) fn execute_svg_color_matrix(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _matrix: &[f32; 20],
    ) {
    }

    pub(crate) fn execute_component_transfer(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _table: &[u8],
    ) {
    }

    pub(crate) fn execute_tile(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _tile_origin: (i32, i32),
        _tile_size: (u32, u32),
    ) {
    }

    pub(crate) fn execute_arithmetic_composite(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _aux: &GpuImageBuffer,
        _coefficients: [f32; 4],
    ) {
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_convolve_matrix(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _columns: u32,
        _rows: u32,
        _target_x: u32,
        _target_y: u32,
        _edge_mode: EdgeMode,
        _preserve_alpha: bool,
        _divisor: f32,
        _bias: f32,
        _matrix: &[f32],
    ) {
    }

    pub(crate) fn execute_svg_lighting(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _params: SvgLightingParams,
    ) {
    }

    pub(crate) fn execute_svg_morphology(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _operator: u32,
        _columns: u32,
        _rows: u32,
    ) {
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_shadow_composite(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _src: &GpuImageBuffer,
        _mask: &GpuImageBuffer,
        _offset_x: i32,
        _offset_y: i32,
        _brush: &Brush,
    ) {
    }

    pub(crate) fn execute_svg_turbulence(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _dst: &mut GpuImageBuffer,
        _params: SvgTurbulenceParams,
        _lattice_selector: &[u32],
        _gradient: &WgpuBuffer<[f32; 2]>,
    ) {
    }

    pub(crate) fn execute_blur_xy(
        &self,
        _encoder: &mut wgpu::CommandEncoder,
        _image: &mut GpuImageBuffer,
        _sigma_x: f32,
        _sigma_y: f32,
    ) {
    }
}

pub(crate) fn build_turbulence_tables(_seed: i32) -> ([u32; 514], [[[f32; 2]; 514]; 4]) {
    ([0; 514], [[[0.0; 2]; 514]; 4])
}
