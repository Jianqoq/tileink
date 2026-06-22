use std::{cell::RefCell, num::NonZeroU64};

use usvg::filter::EdgeMode;
use wgpu::{Device, Queue, util::DeviceExt};

use crate::{
    memory::{Allocation, Memory},
    shared::{
        brush::Brush,
        layer::{blend::Blend, filter::Filter, mask::MaskMode},
    },
    wgpu::{memory::Memory as WgpuMemory, types::brush::GpuBrush},
};

const OP_COLOR: u32 = 0;
const OP_BLUR_H: u32 = 1;
const OP_BLUR_V: u32 = 2;
const OP_COMPOSITE: u32 = 3;
const OP_SHADOW_COLOR: u32 = 4;
const OP_SHADOW_COMPOSITE: u32 = 5;
const OP_BACKDROP_COMPOSITE: u32 = 6;
const OP_COPY: u32 = 7;
const OP_MASK_COMPOSITE: u32 = 8;
const OP_BLEND_COMPOSITE: u32 = 9;
const OP_COPY_RECT: u32 = 10;
const OP_COLOR_SPACE: u32 = 11;
const OP_SVG_COLOR_MATRIX: u32 = 12;
const OP_COMPONENT_TRANSFER: u32 = 13;
const OP_TILE: u32 = 14;
const OP_ARITHMETIC_COMPOSITE: u32 = 15;
const OP_CONVOLVE_MATRIX: u32 = 16;
const OP_SVG_LIGHTING: u32 = 17;
const OP_GAUSSIAN_BLUR_H: u32 = 18;
const OP_GAUSSIAN_BLUR_V: u32 = 19;
const OP_MORPHOLOGY: u32 = 20;
const OP_TURBULENCE: u32 = 21;

const BOX_BLUR_SIGMA_THRESHOLD: f32 = 2.0;
const BOX_BLUR_STEPS: usize = 5;
const TURB_RAND_M: i32 = 2_147_483_647;
const TURB_RAND_A: i32 = 16_807;
const TURB_RAND_Q: i32 = 127_773;
const TURB_RAND_R: i32 = 2_836;
const TURB_B_SIZE: usize = 0x100;
const TURB_B_SIZE_I32: i32 = 0x100;
const TURB_B_LEN: usize = TURB_B_SIZE + TURB_B_SIZE + 2;

const FILTER_SHADER: &str = concat!(
    include_str!("../../wgpu/shaders/brush.wgsl"),
    "\n",
    include_str!("../../wgpu/shaders/blend.wgsl"),
    "\n",
    include_str!("../../wgpu/shaders/filter.wgsl")
);

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FilterParams {
    width: u32,
    height: u32,
    dst_width: u32,
    dst_height: u32,
    src_off: u32,
    dst_off: u32,
    aux_off: u32,
    op: u32,
    kind: u32,
    radius: u32,
    amount: f32,
    offset_x: i32,
    offset_y: i32,
    brush_off: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SvgConvolveMatrixParams {
    columns: u32,
    rows: u32,
    target_x: u32,
    target_y: u32,
    edge_mode: u32,
    preserve_alpha: u32,
    divisor: f32,
    bias: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
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

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
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
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    keepalive: RefCell<Vec<DispatchKeepalive>>,
}

struct DispatchKeepalive {
    params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl FilterGpuPipeline {
    pub fn new(device: &Device, queue: &Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("filter_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<FilterParams>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filter"),
            source: wgpu::ShaderSource::Wgsl(FILTER_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("filter_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("filter"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        Self {
            pipeline,
            layout,
            keepalive: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn clear_dispatch_keepalive(&self) {
        self.keepalive.borrow_mut().clear();
    }

    pub(crate) fn execute_filter(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        image: Allocation,
        width: u32,
        height: u32,
        filter: Filter,
    ) {
        match filter {
            Filter::Blur(radius) => {
                self.execute_filter_blur(memory, encoder, image, width, height, radius)
            }
            Filter::DropShadow {
                offset_x,
                offset_y,
                radius,
                brush,
            } => {
                let brush_off = upload_brush(memory, &brush);
                let shadow = memory.allocate_image(width, height, peniko::Color::TRANSPARENT);
                let scratch = memory.allocate_image(width, height, peniko::Color::TRANSPARENT);
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: shadow.offset as u32,
                        aux_off: 0,
                        op: OP_SHADOW_COLOR,
                        kind: 0,
                        radius: 0,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                self.execute_filter_blur_between(
                    memory, encoder, shadow, scratch, width, height, radius, radius,
                );
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: image.offset as u32,
                        aux_off: shadow.offset as u32,
                        op: OP_SHADOW_COMPOSITE,
                        kind: 0,
                        radius: 0,
                        amount: 0.0,
                        offset_x: offset_x.round() as i32,
                        offset_y: offset_y.round() as i32,
                        brush_off,
                    },
                );
            }
            Filter::LiquidGlass(_) => {
                panic!("liquid glass is currently implemented only by the classic CPU renderer")
            }
            other => {
                let (kind, amount) = color_filter(other);
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: image.offset as u32,
                        aux_off: 0,
                        op: OP_COLOR,
                        kind,
                        radius: 0,
                        amount,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
            }
        }
    }

    pub(crate) fn execute_composite(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_COMPOSITE,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_composite_at(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        dst_size: (u32, u32),
        src: Allocation,
        src_size: (u32, u32),
        offset: (i32, i32),
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width: src_size.0,
                height: src_size.1,
                dst_width: dst_size.0,
                dst_height: dst_size.1,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_COMPOSITE,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: offset.0,
                offset_y: offset.1,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_backdrop_composite(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        filtered_backdrop: Allocation,
        mask: Allocation,
        width: u32,
        height: u32,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: filtered_backdrop.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: mask.offset as u32,
                op: OP_BACKDROP_COMPOSITE,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_mask_composite(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        content: Allocation,
        mask: Allocation,
        width: u32,
        height: u32,
        mode: MaskMode,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: content.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: mask.offset as u32,
                op: OP_MASK_COMPOSITE,
                kind: match mode {
                    MaskMode::Alpha => 0,
                    MaskMode::Luminance => 1,
                },
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_mask_composite_at(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        dst_size: (u32, u32),
        content: Allocation,
        mask: Allocation,
        layer_size: (u32, u32),
        offset: (i32, i32),
        mode: MaskMode,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width: layer_size.0,
                height: layer_size.1,
                dst_width: dst_size.0,
                dst_height: dst_size.1,
                src_off: content.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: mask.offset as u32,
                op: OP_MASK_COMPOSITE,
                kind: match mode {
                    MaskMode::Alpha => 0,
                    MaskMode::Luminance => 1,
                },
                radius: 0,
                amount: 0.0,
                offset_x: offset.0,
                offset_y: offset.1,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_blend_composite(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        blend: &Blend,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_BLEND_COMPOSITE,
                kind: blend.mode.mix as u32,
                radius: blend.mode.compose as u32,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_blend_composite_at(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        dst_size: (u32, u32),
        src: Allocation,
        src_size: (u32, u32),
        offset: (i32, i32),
        blend: &Blend,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width: src_size.0,
                height: src_size.1,
                dst_width: dst_size.0,
                dst_height: dst_size.1,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_BLEND_COMPOSITE,
                kind: blend.mode.mix as u32,
                radius: blend.mode.compose as u32,
                amount: 0.0,
                offset_x: offset.0,
                offset_y: offset.1,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_copy(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_COPY,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_copy_rect(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        dst_size: (u32, u32),
        src: Allocation,
        src_size: (u32, u32),
        src_offset: (i32, i32),
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width: dst_size.0,
                height: dst_size.1,
                dst_width: dst_size.0,
                dst_height: dst_size.1,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_COPY_RECT,
                kind: src_size.0,
                radius: src_size.1,
                amount: 0.0,
                offset_x: src_offset.0,
                offset_y: src_offset.1,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_color_space_conversion(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        kind: u32,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_COLOR_SPACE,
                kind,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_extract_alpha(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_SHADOW_COLOR,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_svg_color_matrix(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        matrix: &[f32; 20],
    ) {
        let aux = upload_filter_data(memory, bytemuck::cast_slice(matrix));
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: aux,
                op: OP_SVG_COLOR_MATRIX,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_component_transfer(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        table: &[u8],
    ) {
        let expanded: Vec<u32> = table.iter().map(|&v| v as u32).collect();
        let aux = upload_filter_data(memory, bytemuck::cast_slice(&expanded));
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: aux,
                op: OP_COMPONENT_TRANSFER,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_tile(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        tile_origin: (i32, i32),
        tile_size: (u32, u32),
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_TILE,
                kind: tile_size.0,
                radius: tile_size.1,
                amount: 0.0,
                offset_x: tile_origin.0,
                offset_y: tile_origin.1,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_arithmetic_composite(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        aux: Allocation,
        width: u32,
        height: u32,
        coefficients: [f32; 4],
    ) {
        let coeffs = upload_filter_data(memory, bytemuck::cast_slice(&coefficients));
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: aux.offset as u32,
                op: OP_ARITHMETIC_COMPOSITE,
                kind: coeffs,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_convolve_matrix(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        columns: u32,
        rows: u32,
        target_x: u32,
        target_y: u32,
        edge_mode: EdgeMode,
        preserve_alpha: bool,
        divisor: f32,
        bias: f32,
        matrix: &[f32],
    ) {
        let header = SvgConvolveMatrixParams {
            columns,
            rows,
            target_x,
            target_y,
            edge_mode: match edge_mode {
                EdgeMode::None => 0,
                EdgeMode::Duplicate => 1,
                EdgeMode::Wrap => 2,
            },
            preserve_alpha: u32::from(preserve_alpha),
            divisor,
            bias,
        };
        let mut data = Vec::with_capacity(
            std::mem::size_of::<SvgConvolveMatrixParams>() + std::mem::size_of_val(matrix),
        );
        data.extend_from_slice(bytemuck::bytes_of(&header));
        data.extend_from_slice(bytemuck::cast_slice(matrix));
        let aux = upload_filter_data(memory, &data);
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: aux,
                op: OP_CONVOLVE_MATRIX,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_svg_lighting(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        params: SvgLightingParams,
    ) {
        let aux = upload_filter_data(memory, bytemuck::bytes_of(&params));
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: aux,
                op: OP_SVG_LIGHTING,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    fn execute_filter_blur(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        image: Allocation,
        width: u32,
        height: u32,
        radius: f32,
    ) {
        let scratch = memory.allocate_image(width, height, peniko::Color::TRANSPARENT);
        self.execute_filter_blur_between(
            memory, encoder, image, scratch, width, height, radius, radius,
        );
    }

    fn execute_filter_blur_between(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        image: Allocation,
        scratch: Allocation,
        width: u32,
        height: u32,
        radius_x: f32,
        radius_y: f32,
    ) {
        let radius_x = radius_x.ceil().max(0.0) as u32;
        let radius_y = radius_y.ceil().max(0.0) as u32;
        match (radius_x > 0, radius_y > 0) {
            (true, true) => {
                let kernel_x = gaussian_kernel_for_radius(radius_x);
                let aux_x = upload_filter_data(memory, bytemuck::cast_slice(&kernel_x));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: scratch.offset as u32,
                        aux_off: aux_x,
                        op: OP_GAUSSIAN_BLUR_H,
                        kind: 0,
                        radius: radius_x,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                let kernel_y = gaussian_kernel_for_radius(radius_y);
                let aux_y = upload_filter_data(memory, bytemuck::cast_slice(&kernel_y));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: scratch.offset as u32,
                        dst_off: image.offset as u32,
                        aux_off: aux_y,
                        op: OP_GAUSSIAN_BLUR_V,
                        kind: 0,
                        radius: radius_y,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
            }
            (true, false) => {
                let kernel_x = gaussian_kernel_for_radius(radius_x);
                let aux_x = upload_filter_data(memory, bytemuck::cast_slice(&kernel_x));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: scratch.offset as u32,
                        aux_off: aux_x,
                        op: OP_GAUSSIAN_BLUR_H,
                        kind: 0,
                        radius: radius_x,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                self.execute_copy(memory, encoder, image, scratch, width, height);
            }
            (false, true) => {
                let kernel_y = gaussian_kernel_for_radius(radius_y);
                let aux_y = upload_filter_data(memory, bytemuck::cast_slice(&kernel_y));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: scratch.offset as u32,
                        aux_off: aux_y,
                        op: OP_GAUSSIAN_BLUR_V,
                        kind: 0,
                        radius: radius_y,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                self.execute_copy(memory, encoder, image, scratch, width, height);
            }
            (false, false) => {}
        }
    }

    fn execute_blur_between(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        image: Allocation,
        scratch: Allocation,
        width: u32,
        height: u32,
        sigma_x: f32,
        sigma_y: f32,
    ) {
        if sigma_x <= 0.0 && sigma_y <= 0.0 {
            return;
        }
        if sigma_x >= BOX_BLUR_SIGMA_THRESHOLD || sigma_y >= BOX_BLUR_SIGMA_THRESHOLD {
            let boxes_x = create_box_gauss(sigma_x);
            let boxes_y = create_box_gauss(sigma_y);
            for (&box_x, &box_y) in boxes_x.iter().zip(boxes_y.iter()) {
                let radius_x = ((box_x - 1) / 2).max(0) as u32;
                let radius_y = ((box_y - 1) / 2).max(0) as u32;
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: scratch.offset as u32,
                        aux_off: 0,
                        op: OP_BLUR_V,
                        kind: 0,
                        radius: radius_y,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: scratch.offset as u32,
                        dst_off: image.offset as u32,
                        aux_off: 0,
                        op: OP_BLUR_H,
                        kind: 0,
                        radius: radius_x,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
            }
            return;
        }
        match (sigma_x > 0.0, sigma_y > 0.0) {
            (true, true) => {
                let kernel_x = gaussian_kernel(sigma_x);
                let aux_x = upload_filter_data(memory, bytemuck::cast_slice(&kernel_x));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: scratch.offset as u32,
                        aux_off: aux_x,
                        op: OP_GAUSSIAN_BLUR_H,
                        kind: 0,
                        radius: (kernel_x.len() / 2) as u32,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                let kernel_y = gaussian_kernel(sigma_y);
                let aux_y = upload_filter_data(memory, bytemuck::cast_slice(&kernel_y));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: scratch.offset as u32,
                        dst_off: image.offset as u32,
                        aux_off: aux_y,
                        op: OP_GAUSSIAN_BLUR_V,
                        kind: 0,
                        radius: (kernel_y.len() / 2) as u32,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
            }
            (true, false) => {
                let kernel_x = gaussian_kernel(sigma_x);
                let aux_x = upload_filter_data(memory, bytemuck::cast_slice(&kernel_x));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: scratch.offset as u32,
                        aux_off: aux_x,
                        op: OP_GAUSSIAN_BLUR_H,
                        kind: 0,
                        radius: (kernel_x.len() / 2) as u32,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                self.execute_copy(memory, encoder, image, scratch, width, height);
            }
            (false, true) => {
                let kernel_y = gaussian_kernel(sigma_y);
                let aux_y = upload_filter_data(memory, bytemuck::cast_slice(&kernel_y));
                self.dispatch(
                    memory,
                    encoder,
                    FilterParams {
                        width,
                        height,
                        dst_width: width,
                        dst_height: height,
                        src_off: image.offset as u32,
                        dst_off: scratch.offset as u32,
                        aux_off: aux_y,
                        op: OP_GAUSSIAN_BLUR_V,
                        kind: 0,
                        radius: (kernel_y.len() / 2) as u32,
                        amount: 0.0,
                        offset_x: 0,
                        offset_y: 0,
                        brush_off: 0,
                    },
                );
                self.execute_copy(memory, encoder, image, scratch, width, height);
            }
            (false, false) => {}
        }
    }

    pub(crate) fn execute_svg_morphology(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        width: u32,
        height: u32,
        operator: u32,
        columns: u32,
        rows: u32,
    ) {
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: 0,
                op: OP_MORPHOLOGY,
                kind: operator,
                radius: columns,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: rows,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn execute_shadow_composite(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        src: Allocation,
        mask: Allocation,
        width: u32,
        height: u32,
        offset_x: i32,
        offset_y: i32,
        brush: &Brush,
    ) {
        let brush_off = upload_brush(memory, brush);
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: src.offset as u32,
                dst_off: dst.offset as u32,
                aux_off: mask.offset as u32,
                op: OP_SHADOW_COMPOSITE,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x,
                offset_y,
                brush_off,
            },
        );
    }

    pub(crate) fn execute_svg_turbulence(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        dst: Allocation,
        width: u32,
        height: u32,
        params: SvgTurbulenceParams,
        lattice_selector: &[u32; TURB_B_LEN],
        gradient: &[[[f32; 2]; TURB_B_LEN]; 4],
    ) {
        let mut data = Vec::with_capacity(
            std::mem::size_of::<SvgTurbulenceParams>()
                + std::mem::size_of_val(lattice_selector)
                + std::mem::size_of_val(gradient),
        );
        data.extend_from_slice(bytemuck::bytes_of(&params));
        data.extend_from_slice(bytemuck::cast_slice(lattice_selector));
        data.extend_from_slice(bytemuck::cast_slice(gradient));
        let aux = upload_filter_data(memory, &data);
        self.dispatch(
            memory,
            encoder,
            FilterParams {
                width,
                height,
                dst_width: width,
                dst_height: height,
                src_off: 0,
                dst_off: dst.offset as u32,
                aux_off: aux,
                op: OP_TURBULENCE,
                kind: 0,
                radius: 0,
                amount: 0.0,
                offset_x: 0,
                offset_y: 0,
                brush_off: 0,
            },
        );
    }

    pub(crate) fn execute_blur_xy(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        image: Allocation,
        width: u32,
        height: u32,
        sigma_x: f32,
        sigma_y: f32,
    ) {
        let scratch = memory.allocate_image(width, height, peniko::Color::TRANSPARENT);
        self.execute_blur_between(
            memory, encoder, image, scratch, width, height, sigma_x, sigma_y,
        );
    }

    fn dispatch(
        &self,
        memory: &mut WgpuMemory,
        encoder: &mut wgpu::CommandEncoder,
        params: FilterParams,
    ) {
        let device = memory.device();
        let bump = memory.bump();
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("filter_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = memory.with_buffer(|buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("filter_bind_group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer,
                            offset: 0,
                            size: NonZeroU64::new(bump as u64),
                        }),
                    },
                ],
            })
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("filter"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(params.width.div_ceil(16), params.height.div_ceil(16), 1);
        drop(pass);
        self.keepalive.borrow_mut().push(DispatchKeepalive {
            params: params_buffer,
            bind_group,
        });
    }
}

fn create_box_gauss(sigma: f32) -> [i32; BOX_BLUR_STEPS] {
    if sigma <= 0.0 {
        return [1; BOX_BLUR_STEPS];
    }

    let n = BOX_BLUR_STEPS as f32;
    let w_ideal = (12.0 * sigma * sigma / n).sqrt() + 1.0;
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n)
        / (-4.0 * wl as f32 - 4.0);
    let m = m_ideal.round() as usize;
    let mut sizes = [0; BOX_BLUR_STEPS];
    for (i, size) in sizes.iter_mut().enumerate() {
        *size = if i < m { wl } else { wu };
    }
    sizes
}

fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let radius = (sigma * 3.0).ceil().max(1.0) as i32;
    let mut kernel = Vec::with_capacity((radius * 2 + 1) as usize);
    let denom = 2.0 * sigma * sigma;
    let mut sum = 0.0;

    for i in -radius..=radius {
        let value = (-(i * i) as f32 / denom).exp();
        kernel.push(value);
        sum += value;
    }
    for value in &mut kernel {
        *value /= sum;
    }
    kernel
}

fn gaussian_kernel_for_radius(radius: u32) -> Vec<f32> {
    let sigma = (radius as f32 / 3.0).max(1.0e-3);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let mut kernel = Vec::with_capacity((radius * 2 + 1) as usize);
    let mut sum = 0.0;

    for i in -(radius as i32)..=(radius as i32) {
        let x = i as f32;
        let weight = (-x * x / two_sigma_sq).exp();
        kernel.push(weight);
        sum += weight;
    }
    for weight in &mut kernel {
        *weight /= sum;
    }

    kernel
}

fn upload_brush(memory: &mut WgpuMemory, brush: &Brush) -> u32 {
    let ramp_off = if let Some(payload) = brush.gpu_payload() {
        let allocation = memory.allocate(payload.len() * 4, 4);
        memory.write_at(allocation.offset, bytemuck::cast_slice(payload));
        allocation.offset as u32
    } else {
        0
    };
    let record = GpuBrush::from_brush(brush, ramp_off, brush.gpu_payload_len());
    let allocation = memory.allocate(std::mem::size_of::<GpuBrush>(), 4);
    memory.write_at(allocation.offset, bytemuck::bytes_of(&record));
    allocation.offset as u32
}

fn upload_filter_data(memory: &mut WgpuMemory, data: &[u8]) -> u32 {
    let allocation = memory.allocate(data.len(), 4);
    memory.write_at(allocation.offset, data);
    allocation.offset as u32
}

fn color_filter(filter: Filter) -> (u32, f32) {
    match filter {
        Filter::Brightness(v) => (0, v),
        Filter::Contrast(v) => (1, v),
        Filter::Grayscale(v) => (2, v),
        Filter::HueRotate(v) => (3, v),
        Filter::Invert(v) => (4, v),
        Filter::Opacity(v) => (5, v),
        Filter::Saturate(v) => (6, v),
        Filter::Sepia(v) => (7, v),
        _ => unreachable!(),
    }
}

pub(crate) fn build_turbulence_tables(
    mut seed: i32,
) -> ([u32; TURB_B_LEN], [[[f32; 2]; TURB_B_LEN]; 4]) {
    let mut lattice_selector = [0u32; TURB_B_LEN];
    let mut gradient = [[[0.0; 2]; TURB_B_LEN]; 4];

    if seed <= 0 {
        seed = -seed % (TURB_RAND_M - 1) + 1;
    }
    if seed > TURB_RAND_M - 1 {
        seed = TURB_RAND_M - 1;
    }

    for channel_gradient in &mut gradient {
        for (i, selector) in lattice_selector.iter_mut().enumerate().take(TURB_B_SIZE) {
            *selector = i as u32;
            for component in &mut channel_gradient[i] {
                seed = turbulence_random(seed);
                *component = ((seed % (TURB_B_SIZE_I32 + TURB_B_SIZE_I32)) - TURB_B_SIZE_I32)
                    as f32
                    / TURB_B_SIZE_I32 as f32;
            }

            let length = (channel_gradient[i][0] * channel_gradient[i][0]
                + channel_gradient[i][1] * channel_gradient[i][1])
                .sqrt();
            channel_gradient[i][0] /= length;
            channel_gradient[i][1] /= length;
        }
    }

    for i in (1..TURB_B_SIZE).rev() {
        let k = lattice_selector[i];
        seed = turbulence_random(seed);
        let j = (seed % TURB_B_SIZE_I32) as usize;
        lattice_selector[i] = lattice_selector[j];
        lattice_selector[j] = k;
    }

    for i in 0..TURB_B_SIZE + 2 {
        lattice_selector[TURB_B_SIZE + i] = lattice_selector[i];
        for channel_gradient in &mut gradient {
            channel_gradient[TURB_B_SIZE + i] = channel_gradient[i];
        }
    }

    (lattice_selector, gradient)
}

fn turbulence_random(seed: i32) -> i32 {
    let mut result = TURB_RAND_A * (seed % TURB_RAND_Q) - TURB_RAND_R * (seed / TURB_RAND_Q);
    if result <= 0 {
        result += TURB_RAND_M;
    }
    result
}
