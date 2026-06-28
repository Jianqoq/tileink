use ::cubecl::prelude::*;

use crate::{
    cubecl::{
        brush::GpuBrushResources,
        buffer::CubeBuffer,
        pipelines::common::{
            blend_premul_u8, combine_alpha, pack_premul_rgba8, sample_brush, scale_premul_u8,
            src_over_premul_u8,
        },
        renderer::{ScanBuffers, SceneBuffers},
        types::{
            CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_OPACITY, CUBE_LAYER_BLEND,
            CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY,
        },
    },
    shared::bounds::Bounds,
};

const FILTER_WORKGROUP_SIZE: u32 = 256;
const COMPONENT_TRANSFER_TABLE_SIZE_U32: u32 =
    crate::shared::layer::filter::COMPONENT_TRANSFER_TABLE_SIZE as u32;
const COMPONENT_TRANSFER_TABLE_LEN_U32: u32 =
    crate::shared::layer::filter::COMPONENT_TRANSFER_TABLE_LEN as u32;

pub(crate) const FILTER_BRIGHTNESS: u32 = 1;
pub(crate) const FILTER_CONTRAST: u32 = 2;
pub(crate) const FILTER_GRAYSCALE: u32 = 3;
pub(crate) const FILTER_HUE_ROTATE: u32 = 4;
pub(crate) const FILTER_INVERT: u32 = 5;
pub(crate) const FILTER_OPACITY: u32 = 6;
pub(crate) const FILTER_SATURATE: u32 = 7;
pub(crate) const FILTER_SEPIA: u32 = 8;

pub(crate) struct FilterPathResources<'a> {
    pub(crate) range_starts: &'a CubeBuffer<u32>,
    pub(crate) range_ends: &'a CubeBuffer<u32>,
    pub(crate) p0x: &'a CubeBuffer<i32>,
    pub(crate) p0y: &'a CubeBuffer<i32>,
    pub(crate) p1x: &'a CubeBuffer<i32>,
    pub(crate) p1y: &'a CubeBuffer<i32>,
}

pub(crate) struct FilterPipeline;

impl FilterPipeline {
    pub(crate) fn clear_region<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_clear_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            unsafe { target.arg() },
        );
    }

    pub(crate) fn copy_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_copy_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn source_alpha_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_source_alpha_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn source_over_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_source_over_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn blend_region<R: Runtime>(
        client: &ComputeClient<R>,
        input1: &CubeBuffer<u32>,
        input2: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        mode: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_blend_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            mode,
            unsafe { input1.arg() },
            unsafe { input2.arg() },
            unsafe { target.arg() },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_inputs_region<R: Runtime>(
        client: &ComputeClient<R>,
        input1: &CubeBuffer<u32>,
        input2: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        operator: u32,
        arithmetic: [f32; 4],
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_composite_inputs_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            operator,
            arithmetic[0],
            arithmetic[1],
            arithmetic[2],
            arithmetic[3],
            unsafe { input1.arg() },
            unsafe { input2.arg() },
            unsafe { target.arg() },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn morphology_axis_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        radius: u32,
        operator: u32,
        axis: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_morphology_axis_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            size.1,
            radius,
            operator,
            axis,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn apply_color_filter<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        filter_kind: u32,
        amount: f32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_color_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            filter_kind,
            amount,
            unsafe { target.arg() },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_color_matrix<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        matrix: [f32; 20],
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_color_matrix_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            matrix[0],
            matrix[1],
            matrix[2],
            matrix[3],
            matrix[4],
            matrix[5],
            matrix[6],
            matrix[7],
            matrix[8],
            matrix[9],
            matrix[10],
            matrix[11],
            matrix[12],
            matrix[13],
            matrix[14],
            matrix[15],
            matrix[16],
            matrix[17],
            matrix[18],
            matrix[19],
            unsafe { target.arg() },
        );
    }

    pub(crate) fn apply_component_transfer<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        table_index: u32,
        transfer_tables: &CubeBuffer<u32>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_component_transfer_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            table_index,
            unsafe { transfer_tables.arg() },
            unsafe { target.arg() },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn convolve_matrix_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        kernels: &CubeBuffer<f32>,
        kernel_offset: u32,
        columns: u32,
        rows: u32,
        target_x: u32,
        target_y: u32,
        divisor: f32,
        bias: f32,
        edge_mode: u32,
        preserve_alpha: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_convolve_matrix_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.height,
            region.x0,
            region.y0,
            size.0,
            kernel_offset,
            columns,
            rows,
            target_x,
            target_y,
            divisor,
            bias,
            edge_mode,
            preserve_alpha,
            unsafe { kernels.arg() },
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn offset_region<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_offset_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.height,
            region.x0,
            region.y0,
            size.0,
            dx,
            dy,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn flood_region<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        brush_index: u32,
        brushes: GpuBrushResources<'_>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_flood_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            brush_index,
            unsafe { brushes.data.arg() },
            unsafe { brushes.params.arg() },
            unsafe { brushes.payloads.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn composite_src_over_region<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        source: &CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_composite_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_src_over_stack_region<R: Runtime>(
        client: &ComputeClient<R>,
        scene: &SceneBuffers,
        scan: &ScanBuffers,
        target: &mut CubeBuffer<u32>,
        source: &CubeBuffer<u32>,
        mask: Option<&CubeBuffer<u32>>,
        size: (u32, u32),
        bounds: Bounds,
        layer_stack_start: u32,
        layer_stack_end: u32,
        group_stack_capacity: usize,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        let mask_enabled = u32::from(mask.is_some());
        let mask = mask.unwrap_or(source);
        filter_composite_stack_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            FILTER_WORKGROUP_SIZE as usize,
            group_stack_capacity.max(1),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            size.0.div_ceil(16),
            size.1.div_ceil(16),
            layer_stack_start,
            layer_stack_end,
            mask_enabled,
            unsafe { source.arg() },
            unsafe { mask.arg() },
            unsafe { scene.draw_path_ids.arg() },
            unsafe { scene.draw_tags.arg() },
            unsafe { scene.draw_fill_rules.arg() },
            unsafe { scene.draw_pixel_x0.arg() },
            unsafe { scene.draw_pixel_y0.arg() },
            unsafe { scene.draw_pixel_x1.arg() },
            unsafe { scene.draw_pixel_y1.arg() },
            unsafe { scene.backdrop_data_offsets.arg() },
            unsafe { scene.backdrop_tile_x0.arg() },
            unsafe { scene.backdrop_tile_y0.arg() },
            unsafe { scene.backdrop_tile_x1.arg() },
            unsafe { scene.backdrop_tile_y1.arg() },
            unsafe { scan.backdrops.arg() },
            unsafe { scan.tile_segment_range_starts.arg() },
            unsafe { scan.tile_segment_range_ends.arg() },
            unsafe { scan.segment_p0x.arg() },
            unsafe { scan.segment_p0y.arg() },
            unsafe { scan.segment_p1x.arg() },
            unsafe { scan.segment_p1y.arg() },
            unsafe { scan.segment_y_edge.arg() },
            unsafe { scene.plan_layer_stack_tags.arg() },
            unsafe { scene.plan_layer_stack_draws.arg() },
            unsafe { scene.plan_layer_stack_payloads.arg() },
            unsafe { target.arg() },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rasterize_rect_mask<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        rect: (f32, f32, f32, f32),
        radius: (f32, f32, f32, f32),
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_rect_mask_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            radius.0,
            radius.1,
            radius.2,
            radius.3,
            unsafe { target.arg() },
        );
    }

    pub(crate) fn rasterize_path_mask<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        path_index: u32,
        paths: FilterPathResources<'_>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_path_mask_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            path_index,
            unsafe { paths.range_starts.arg() },
            unsafe { paths.range_ends.arg() },
            unsafe { paths.p0x.arg() },
            unsafe { paths.p0y.arg() },
            unsafe { paths.p1x.arg() },
            unsafe { paths.p1y.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn blur_pass<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        radius: f32,
        axis: u32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_blur_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.height,
            region.x0,
            region.y0,
            size.0,
            radius,
            axis,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn build_drop_shadow_mask<R: Runtime>(
        client: &ComputeClient<R>,
        source: &CubeBuffer<u32>,
        target: &mut CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_drop_shadow_mask_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.height,
            region.x0,
            region.y0,
            size.0,
            dx,
            dy,
            unsafe { source.arg() },
            unsafe { target.arg() },
        );
    }

    pub(crate) fn composite_drop_shadow<R: Runtime>(
        client: &ComputeClient<R>,
        target: &mut CubeBuffer<u32>,
        shadow_mask: &CubeBuffer<u32>,
        size: (u32, u32),
        bounds: Bounds,
        brush_index: u32,
        brushes: GpuBrushResources<'_>,
    ) {
        let Some(region) = FilterRegion::new(size, bounds) else {
            return;
        };
        filter_composite_drop_shadow_region::launch::<R>(
            client,
            cube_count(region.pixel_count),
            CubeDim::new_1d(FILTER_WORKGROUP_SIZE),
            region.pixel_count,
            region.width,
            region.x0,
            region.y0,
            size.0,
            brush_index,
            unsafe { brushes.data.arg() },
            unsafe { brushes.params.arg() },
            unsafe { brushes.payloads.arg() },
            unsafe { shadow_mask.arg() },
            unsafe { target.arg() },
        );
    }
}

#[derive(Clone, Copy)]
struct FilterRegion {
    x0: u32,
    y0: u32,
    width: u32,
    height: u32,
    pixel_count: u32,
}

impl FilterRegion {
    fn new(size: (u32, u32), bounds: Bounds) -> Option<Self> {
        let canvas = Bounds::canvas(size.0, size.1);
        let bounds = bounds.intersect(canvas);
        if bounds.is_empty() {
            return None;
        }
        let width = bounds.width();
        let height = bounds.height();
        Some(Self {
            x0: bounds.x0 as u32,
            y0: bounds.y0 as u32,
            width,
            height,
            pixel_count: width * height,
        })
    }
}

fn cube_count(items: u32) -> CubeCount {
    CubeCount::Static(items.div_ceil(FILTER_WORKGROUP_SIZE), 1, 1)
}

#[cube(launch)]
fn filter_clear_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = 0;
}

#[cube(launch)]
fn filter_copy_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = source[ix];
}

#[cube(launch)]
fn filter_source_alpha_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = source[ix] & 0xff00_0000u32;
}

#[cube(launch)]
fn filter_source_over_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = blend_premul_u8(target[ix], source[ix], 3 << 8);
}

#[cube(launch)]
fn filter_blend_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    mode: u32,
    input1: &Array<u32>,
    input2: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = blend_premul_u8(input2[ix], input1[ix], mode);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
fn filter_composite_inputs_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    operator: u32,
    k1: f32,
    k2: f32,
    k3: f32,
    k4: f32,
    input1: &Array<u32>,
    input2: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = composite_inputs_pixel(input1[ix], input2[ix], operator, k1, k2, k3, k4);
}

#[cube(launch)]
fn filter_morphology_axis_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    image_height: u32,
    radius: u32,
    operator: u32,
    axis: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let pos = if axis == 0 { x } else { y };
    let line_len = if axis == 0 { image_width } else { image_height };

    if operator == 0 && (pos < radius || pos + radius >= line_len) {
        target[ix] = 0;
        terminate!();
    }

    let mut out_r = 1.0;
    let mut out_g = 1.0;
    let mut out_b = 1.0;
    let mut out_a = 1.0;
    if operator == 1 {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    }

    let mut start = 0;
    if pos > radius {
        start = pos - radius;
    }
    let mut end = line_len - 1;
    if pos + radius < end {
        end = pos + radius;
    }

    let mut sample_pos = start;
    while sample_pos <= end {
        let sx = if axis == 0 { sample_pos } else { x };
        let sy = if axis == 0 { y } else { sample_pos };
        let sample = source[(sy * image_width + sx) as usize];
        let alpha = (sample >> 24) & 255;
        let sample_r = straight_channel(sample & 255, alpha);
        let sample_g = straight_channel((sample >> 8) & 255, alpha);
        let sample_b = straight_channel((sample >> 16) & 255, alpha);
        let sample_a = alpha as f32 / 255.0;

        if operator == 1 {
            if sample_r > out_r {
                out_r = sample_r;
            }
            if sample_g > out_g {
                out_g = sample_g;
            }
            if sample_b > out_b {
                out_b = sample_b;
            }
            if sample_a > out_a {
                out_a = sample_a;
            }
        } else {
            if sample_r < out_r {
                out_r = sample_r;
            }
            if sample_g < out_g {
                out_g = sample_g;
            }
            if sample_b < out_b {
                out_b = sample_b;
            }
            if sample_a < out_a {
                out_a = sample_a;
            }
        }
        sample_pos += 1;
    }

    target[ix] = pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a);
}

#[cube(launch)]
fn filter_color_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    filter_kind: u32,
    amount: f32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = apply_color_filter_pixel(target[ix], filter_kind, amount);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
fn filter_color_matrix_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    m00: f32,
    m01: f32,
    m02: f32,
    m03: f32,
    m04: f32,
    m10: f32,
    m11: f32,
    m12: f32,
    m13: f32,
    m14: f32,
    m20: f32,
    m21: f32,
    m22: f32,
    m23: f32,
    m24: f32,
    m30: f32,
    m31: f32,
    m32: f32,
    m33: f32,
    m34: f32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = apply_color_matrix_pixel(
        target[ix], m00, m01, m02, m03, m04, m10, m11, m12, m13, m14, m20, m21, m22, m23, m24, m30,
        m31, m32, m33, m34,
    );
}

#[cube(launch)]
fn filter_component_transfer_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    table_index: u32,
    transfer_tables: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = apply_component_transfer_pixel(target[ix], table_index, transfer_tables);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
fn filter_convolve_matrix_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    kernel_offset: u32,
    columns: u32,
    rows: u32,
    target_x: u32,
    target_y: u32,
    divisor: f32,
    bias: f32,
    edge_mode: u32,
    preserve_alpha: u32,
    kernels: &Array<f32>,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let dst_ix = (y * image_width + x) as usize;

    if columns == 0 || rows == 0 || divisor == 0.0 {
        target[dst_ix] = source[dst_ix];
        terminate!();
    }

    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    let mut out_r = 0.0;
    let mut out_g = 0.0;
    let mut out_b = 0.0;
    let mut out_a = 0.0;
    let mut ky = 0;
    while ky < rows {
        let mut kx = 0;
        while kx < columns {
            let kernel_ix = kernel_offset + (rows - 1 - ky) * columns + (columns - 1 - kx);
            let weight = kernels[kernel_ix as usize];
            let mut sx = x as i32 + kx as i32 - target_x as i32;
            let mut sy = y as i32 + ky as i32 - target_y as i32;
            let mut sample = u32::new(0);
            if edge_mode == 1 {
                sx = sx.clamp(region_x0 as i32, region_x1 - 1);
                sy = sy.clamp(region_y0 as i32, region_y1 - 1);
                sample = source[(sy as u32 * image_width + sx as u32) as usize];
            } else if edge_mode == 2 {
                while sx < region_x0 as i32 {
                    sx += region_width as i32;
                }
                while sx >= region_x1 {
                    sx -= region_width as i32;
                }
                while sy < region_y0 as i32 {
                    sy += region_height as i32;
                }
                while sy >= region_y1 {
                    sy -= region_height as i32;
                }
                sample = source[(sy as u32 * image_width + sx as u32) as usize];
            } else if sx >= region_x0 as i32
                && sx < region_x1
                && sy >= region_y0 as i32
                && sy < region_y1
            {
                sample = source[(sy as u32 * image_width + sx as u32) as usize];
            }

            let alpha = (sample >> 24) & 255;
            out_r += straight_channel(sample & 255, alpha) * weight;
            out_g += straight_channel((sample >> 8) & 255, alpha) * weight;
            out_b += straight_channel((sample >> 16) & 255, alpha) * weight;
            out_a += (alpha as f32 / 255.0) * weight;
            kx += 1;
        }
        ky += 1;
    }

    let base_alpha = (source[dst_ix] >> 24) as f32 / 255.0;
    let mut alpha = (out_a / divisor + bias).clamp(0.0, 1.0);
    if preserve_alpha == 1 {
        alpha = base_alpha;
    }
    let r = (out_r / divisor + bias).clamp(0.0, 1.0);
    let g = (out_g / divisor + bias).clamp(0.0, 1.0);
    let b = (out_b / divisor + bias).clamp(0.0, 1.0);
    target[dst_ix] = pack_premul_rgba8(r * alpha, g * alpha, b * alpha, alpha);
}

#[cube(launch)]
fn filter_offset_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    dx: i32,
    dy: i32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let sx = x as i32 - dx;
    let sy = y as i32 - dy;
    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    let ix = (y * image_width + x) as usize;
    let mut pixel = 0u32;
    if sx >= region_x0 as i32 && sx < region_x1 && sy >= region_y0 as i32 && sy < region_y1 {
        pixel = source[(sy as u32 * image_width + sx as u32) as usize];
    }
    target[ix] = pixel;
}

#[cube(launch)]
fn filter_flood_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    brush_index: u32,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = sample_brush(
        brush_index,
        x as f32 + 0.5,
        y as f32 + 0.5,
        brush_data,
        brush_params,
        brush_payloads,
    );
}

#[cube(launch)]
fn filter_composite_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    target[ix] = src_over_premul_u8(target[ix], source[ix]);
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
// Keep runtime bool conditions as nested branches here. Combined `&&`/`||`
// expressions have produced incorrect wgpu shader output in this CubeCL path.
#[allow(clippy::collapsible_if)]
fn filter_composite_stack_region(
    #[comptime] workgroup_size: usize,
    #[comptime] group_stack_capacity: usize,
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    tiles_width: u32,
    tiles_height: u32,
    layer_stack_start: u32,
    layer_stack_end: u32,
    mask_enabled: u32,
    source: &Array<u32>,
    mask: &Array<u32>,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_fill_rules: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
    layer_stack_tags: &Array<u32>,
    layer_stack_draws: &Array<u32>,
    layer_stack_payloads: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let tile_x = x / 16;
    let tile_y = y / 16;
    let local_x = x - tile_x * 16;
    let local_y = y - tile_y * 16;

    let mut pixel = target[ix];
    let mut clip_mask = 255u32;
    let mut group_depth = 0u32;
    let mut group_kinds = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_pixels = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_parent_clips = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_layer_alphas = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);
    let mut group_payloads = SharedMemory::<u32>::new(workgroup_size * group_stack_capacity);

    let mut stack_ix = layer_stack_start;
    while stack_ix < layer_stack_end {
        let stack_i = stack_ix as usize;
        let tag = layer_stack_tags[stack_i];
        let alpha = layer_stack_alpha_at(
            layer_stack_draws[stack_i],
            tile_x,
            tile_y,
            local_x,
            local_y,
            tiles_width,
            tiles_height,
            draw_path_ids,
            draw_tags,
            draw_fill_rules,
            draw_pixel_x0,
            draw_pixel_y0,
            draw_pixel_x1,
            draw_pixel_y1,
            backdrop_data_offsets,
            backdrop_tile_x0,
            backdrop_tile_y0,
            backdrop_tile_x1,
            backdrop_tile_y1,
            backdrops,
            segment_starts,
            segment_ends,
            segment_p0x,
            segment_p0y,
            segment_p1x,
            segment_p1y,
            segment_y_edge,
        );

        if tag == CUBE_LAYER_CLIP {
            clip_mask = combine_alpha(clip_mask, alpha);
        } else {
            let mut is_group = false;
            if tag == CUBE_LAYER_OPACITY {
                is_group = true;
            }
            if tag == CUBE_LAYER_BLEND {
                is_group = true;
            }
            if is_group {
                if group_depth < group_stack_capacity as u32 {
                    let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
                    group_kinds[group_ix] = tag;
                    group_parent_pixels[group_ix] = pixel;
                    group_parent_clips[group_ix] = clip_mask;
                    group_layer_alphas[group_ix] = alpha;
                    group_payloads[group_ix] = layer_stack_payloads[stack_i];
                    group_depth += 1;
                    pixel = 0;
                }
            }
        }
        stack_ix += 1;
    }

    let mut source_alpha = clip_mask;
    if mask_enabled == 1 {
        source_alpha = combine_alpha(source_alpha, mask[ix] >> 24);
    }
    pixel = src_over_premul_u8(pixel, scale_premul_u8(source[ix], source_alpha));

    while group_depth > 0 {
        group_depth -= 1;
        let group_ix = (group_depth * workgroup_size as u32 + UNIT_POS) as usize;
        let parent = group_parent_pixels[group_ix];
        let parent_clip = group_parent_clips[group_ix];
        let layer_alpha = group_layer_alphas[group_ix];
        let payload = group_payloads[group_ix];
        let group_kind = group_kinds[group_ix];
        let mut alpha = combine_alpha(layer_alpha, parent_clip);
        if group_kind == CUBE_LAYER_OPACITY {
            alpha = combine_alpha(alpha, payload);
            pixel = src_over_premul_u8(parent, scale_premul_u8(pixel, alpha));
        } else {
            let src = scale_premul_u8(pixel, alpha);
            if src >> 24 == 0 {
                pixel = parent;
            } else {
                pixel = blend_premul_u8(parent, src, payload);
            }
        }
    }

    target[ix] = pixel;
}

#[cube(launch)]
#[allow(clippy::too_many_arguments)]
fn filter_rect_mask_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let dist = rect_signed_distance(
        px,
        py,
        x0.min(x1),
        y0.min(y1),
        x0.max(x1),
        y0.max(y1),
        radius_top_left,
        radius_top_right,
        radius_bottom_left,
        radius_bottom_right,
    );
    let alpha = ((0.5 - dist).clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    let ix = (y * image_width + x) as usize;
    target[ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
}

#[cube(launch)]
// Keep runtime bool conditions as nested branches here. Combined `&&`/`||`
// expressions have produced incorrect wgpu shader output in this CubeCL path.
#[allow(clippy::collapsible_if)]
fn filter_path_mask_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    path_index: u32,
    path_range_starts: &Array<u32>,
    path_range_ends: &Array<u32>,
    path_p0x: &Array<i32>,
    path_p0y: &Array<i32>,
    path_p1x: &Array<i32>,
    path_p1y: &Array<i32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let px = x as f32 + 0.5;
    let py = y as f32 + 0.5;
    let path_i = path_index as usize;
    let mut winding = i32::new(0);
    if path_i < path_range_starts.len() {
        let mut line_ix = path_range_starts[path_i];
        let line_end = path_range_ends[path_i];
        while line_ix < line_end {
            let i = line_ix as usize;
            let inv_scale = f32::new(0.003_906_25_f32);
            let y0 = path_p0y[i] as f32 * inv_scale;
            let y1 = path_p1y[i] as f32 * inv_scale;
            let mut winding_delta = i32::new(0);
            if y0 <= py {
                if y1 > py {
                    winding_delta = i32::new(1);
                }
            }
            if y1 <= py {
                if y0 > py {
                    winding_delta = i32::new(-1);
                }
            }
            if winding_delta != 0 {
                let x0 = path_p0x[i] as f32 * inv_scale;
                let x1 = path_p1x[i] as f32 * inv_scale;
                let t = (py - y0) / (y1 - y0);
                let x_cross = x0 + (x1 - x0) * t;
                if x_cross > px {
                    winding += winding_delta;
                }
            }
            line_ix += 1;
        }
    }

    let mut alpha = u32::new(0);
    if winding != 0 {
        alpha = 255u32;
    }
    let ix = (y * image_width + x) as usize;
    target[ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
}

#[cube(launch)]
fn filter_blur_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    radius: f32,
    axis: u32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let radius = radius.max(0.0);
    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let dst_ix = (y * image_width + x) as usize;
    if radius <= 0.0 {
        target[dst_ix] = source[dst_ix];
        terminate!();
    }

    let half_width = (radius * 3.0).ceil().max(1.0) as i32;
    let sigma = radius.max(0.0001);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    let base_x = x as i32;
    let base_y = y as i32;
    let mut sum = 0.0;
    let mut r = 0.0;
    let mut g = 0.0;
    let mut b = 0.0;
    let mut a = 0.0;
    let mut d = -half_width;
    while d <= half_width {
        let df = d as f32;
        let weight = (-(df * df) / two_sigma_sq).exp();
        sum += weight;
        let mut sample_x = base_x;
        let mut sample_y = base_y;
        if axis == 0 {
            sample_x += d;
        } else {
            sample_y += d;
        }
        if sample_x >= region_x0 as i32
            && sample_x < region_x1
            && sample_y >= region_y0 as i32
            && sample_y < region_y1
        {
            let sample_ix = (sample_y as u32 * image_width + sample_x as u32) as usize;
            let px = source[sample_ix];
            r += (px & 255) as f32 * weight;
            g += ((px >> 8) & 255) as f32 * weight;
            b += ((px >> 16) & 255) as f32 * weight;
            a += ((px >> 24) & 255) as f32 * weight;
        }
        d += 1;
    }

    let mut scale = f32::new(0.0_f32);
    if sum > 0.0 {
        scale = 1.0 / (255.0 * sum);
    }
    target[dst_ix] = pack_premul_rgba8(r * scale, g * scale, b * scale, a * scale);
}

#[cube(launch)]
fn filter_drop_shadow_mask_region(
    pixel_count: u32,
    region_width: u32,
    region_height: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    dx: i32,
    dy: i32,
    source: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let source_ix = (y * image_width + x) as usize;
    let alpha = source[source_ix] >> 24;
    if alpha == 0 {
        terminate!();
    }

    let tx = x as i32 + dx;
    let ty = y as i32 + dy;
    let region_x1 = (region_x0 + region_width) as i32;
    let region_y1 = (region_y0 + region_height) as i32;
    if tx >= region_x0 as i32 && tx < region_x1 && ty >= region_y0 as i32 && ty < region_y1 {
        let target_ix = (ty as u32 * image_width + tx as u32) as usize;
        target[target_ix] = alpha | (alpha << 8) | (alpha << 16) | (alpha << 24);
    }
}

#[cube(launch)]
fn filter_composite_drop_shadow_region(
    pixel_count: u32,
    region_width: u32,
    region_x0: u32,
    region_y0: u32,
    image_width: u32,
    brush_index: u32,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
    shadow_mask: &Array<u32>,
    target: &mut Array<u32>,
) {
    let region_ix = ABSOLUTE_POS as u32;
    if region_ix >= pixel_count {
        terminate!();
    }

    let x = region_x0 + region_ix % region_width;
    let y = region_y0 + region_ix / region_width;
    let ix = (y * image_width + x) as usize;
    let alpha = shadow_mask[ix] >> 24;
    let shadow_color = sample_brush(
        brush_index,
        x as f32 + 0.5,
        y as f32 + 0.5,
        brush_data,
        brush_params,
        brush_payloads,
    );
    let shadow = scale_premul_u8(shadow_color, alpha);
    target[ix] = src_over_premul_u8(shadow, target[ix]);
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn layer_stack_alpha_at(
    draw_ix: u32,
    tile_x: u32,
    tile_y: u32,
    local_x: u32,
    local_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_fill_rules: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
    backdrops: &Array<Atomic<i32>>,
    segment_starts: &Array<u32>,
    segment_ends: &Array<u32>,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
) -> u32 {
    let invalid = u32::new(-1);
    let backdrop_ix = filter_draw_backdrop_ix(
        draw_ix,
        tile_x,
        tile_y,
        tiles_width,
        tiles_height,
        draw_path_ids,
        draw_tags,
        draw_pixel_x0,
        draw_pixel_y0,
        draw_pixel_x1,
        draw_pixel_y1,
        backdrop_data_offsets,
        backdrop_tile_x0,
        backdrop_tile_y0,
        backdrop_tile_x1,
        backdrop_tile_y1,
    );
    let mut alpha = 0u32;
    if backdrop_ix != invalid {
        let i = backdrop_ix as usize;
        alpha = filter_fill_alpha_at(
            backdrops[i].load(),
            draw_fill_rules[draw_ix as usize],
            segment_starts[i],
            segment_ends[i],
            local_x,
            local_y,
            segment_p0x,
            segment_p0y,
            segment_p1x,
            segment_p1y,
            segment_y_edge,
        );
    }
    alpha
}

#[cube]
#[allow(clippy::too_many_arguments)]
// Keep runtime bool conditions as nested branches here. Combined `&&`/`||`
// expressions have produced incorrect wgpu shader output in this CubeCL path.
#[allow(clippy::collapsible_if)]
fn filter_draw_backdrop_ix(
    draw_ix: u32,
    tile_x: u32,
    tile_y: u32,
    tiles_width: u32,
    tiles_height: u32,
    draw_path_ids: &Array<u32>,
    draw_tags: &Array<u32>,
    draw_pixel_x0: &Array<i32>,
    draw_pixel_y0: &Array<i32>,
    draw_pixel_x1: &Array<i32>,
    draw_pixel_y1: &Array<i32>,
    backdrop_data_offsets: &Array<u32>,
    backdrop_tile_x0: &Array<u32>,
    backdrop_tile_y0: &Array<u32>,
    backdrop_tile_x1: &Array<u32>,
    backdrop_tile_y1: &Array<u32>,
) -> u32 {
    let invalid = u32::new(-1);
    let draw_i = draw_ix as usize;
    let path_id = draw_path_ids[draw_i];
    let draw_tag = draw_tags[draw_i];
    let mut result = invalid;

    let mut valid_draw = false;
    if draw_tag == CUBE_DRAW_BRUSH {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_CLIP {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_OPACITY {
        valid_draw = true;
    }
    if draw_tag == CUBE_DRAW_BLEND {
        valid_draw = true;
    }

    if path_id != invalid {
        if valid_draw {
            let draw_x0 = filter_pixel_tile_min(draw_pixel_x0[draw_i], tiles_width);
            let draw_y0 = filter_pixel_tile_min(draw_pixel_y0[draw_i], tiles_height);
            let draw_x1 = filter_pixel_tile_max(draw_pixel_x1[draw_i], tiles_width);
            let draw_y1 = filter_pixel_tile_max(draw_pixel_y1[draw_i], tiles_height);
            let mut tile_in_draw = false;
            if tile_x >= draw_x0 {
                if tile_x < draw_x1 {
                    if tile_y >= draw_y0 {
                        if tile_y < draw_y1 {
                            tile_in_draw = true;
                        }
                    }
                }
            }
            if tile_in_draw {
                let path_i = path_id as usize;
                if path_i < backdrop_data_offsets.len() {
                    let bx0 = backdrop_tile_x0[path_i];
                    let by0 = backdrop_tile_y0[path_i];
                    let bx1 = backdrop_tile_x1[path_i];
                    let by1 = backdrop_tile_y1[path_i];
                    let stride = bx1 - bx0;
                    let mut tile_in_backdrop = false;
                    if stride > 0 {
                        if tile_x >= bx0 {
                            if tile_x < bx1 {
                                if tile_y >= by0 {
                                    if tile_y < by1 {
                                        tile_in_backdrop = true;
                                    }
                                }
                            }
                        }
                    }
                    if tile_in_backdrop {
                        result =
                            backdrop_data_offsets[path_i] + (tile_y - by0) * stride + tile_x - bx0;
                    }
                }
            }
        }
    }

    result
}

#[cube]
fn filter_pixel_tile_min(value: i32, limit: u32) -> u32 {
    let mut tile = 0u32;
    if value > 0 {
        tile = (value as u32 / 16).min(limit);
    }
    tile
}

#[cube]
fn filter_pixel_tile_max(value: i32, limit: u32) -> u32 {
    let mut tile = 0u32;
    if value > 0 {
        tile = (value as u32).div_ceil(16).min(limit);
    }
    tile
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn filter_fill_alpha_at(
    backdrop: i32,
    fill_rule: u32,
    segment_start: u32,
    segment_end: u32,
    x: u32,
    y: u32,
    segment_p0x: &Array<f32>,
    segment_p0y: &Array<f32>,
    segment_p1x: &Array<f32>,
    segment_p1y: &Array<f32>,
    segment_y_edge: &Array<f32>,
) -> u32 {
    let mut coverage = backdrop as f32;
    let mut segment_ix = segment_start;
    while segment_ix < segment_end {
        let i = segment_ix as usize;
        coverage += filter_segment_coverage_at(
            segment_p0x[i],
            segment_p0y[i],
            segment_p1x[i],
            segment_p1y[i],
            segment_y_edge[i],
            x,
            y,
        );
        segment_ix += 1;
    }
    filter_coverage_to_alpha(coverage, fill_rule)
}

#[cube]
fn filter_segment_coverage_at(
    p0x: f32,
    p0y: f32,
    p1x: f32,
    p1y: f32,
    y_edge: f32,
    x: u32,
    y: u32,
) -> f32 {
    let delta_x = p1x - p0x;
    let delta_y = p1y - p0y;
    let row_y = y as f32;
    let local_y = p0y - row_y;
    let y0 = local_y.clamp(0.0, 1.0);
    let y1 = (local_y + delta_y).clamp(0.0, 1.0);
    let dy = y0 - y1;
    let x_sign = filter_signum_f32(delta_x);
    let mut coverage = x_sign * (row_y - y_edge + 1.0).clamp(0.0, 1.0);

    if dy != 0.0 {
        let recip = 1.0 / delta_y;
        let t0 = (y0 - local_y) * recip;
        let t1 = (y1 - local_y) * recip;
        let sx0 = p0x + t0 * delta_x;
        let sx1 = p1x + (t1 - 1.0) * delta_x;
        let pixel_x = x as f32;
        let xmin = sx0.min(sx1) - pixel_x;
        let xmax = sx0.max(sx1) - pixel_x;
        let a_min = xmin.min(1.0) - f32::new(0.000001_f32);
        let b = xmax.min(1.0);
        let c = b.max(0.0);
        let d = a_min.max(0.0);
        let area = (b + f32::new(0.5_f32) * (d * d - c * c) - a_min) / (xmax - a_min);
        coverage += area * dy;
    }

    coverage
}

#[cube]
fn filter_signum_f32(value: f32) -> f32 {
    // Match CPU f32::signum semantics for coverage: vertical edges add no y-edge term.
    let mut out = 0.0;
    if value > 0.0 {
        out = 1.0;
    } else if value < 0.0 {
        out = -1.0;
    }
    out
}

#[cube]
fn filter_coverage_to_alpha(value: f32, fill_rule: u32) -> u32 {
    let mut alpha = value.abs().min(1.0);
    if fill_rule == 1 {
        alpha = (value - f32::new(2.0_f32) * (f32::new(0.5_f32) * value).round()).abs();
    }
    (alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u32
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn rect_signed_distance(
    x: f32,
    y: f32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius_top_left: f32,
    radius_top_right: f32,
    radius_bottom_left: f32,
    radius_bottom_right: f32,
) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = ((x1 - x0) * 0.5).max(0.0);
    let hy = ((y1 - y0) * 0.5).max(0.0);
    let px = x - cx;
    let py = y - cy;
    let mut r = radius_top_left;
    if px >= 0.0 {
        if py <= 0.0 {
            r = radius_top_right;
        } else {
            r = radius_bottom_right;
        }
    } else if py > 0.0 {
        r = radius_bottom_left;
    }
    r = r.min(hx).min(hy).max(0.0);

    let ax = px.abs();
    let ay = py.abs();
    if r <= 0.0 {
        let dx = ax - hx;
        let dy = ay - hy;
        dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0)
    } else {
        let qx = ax - hx + r;
        let qy = ay - hy + r;
        qx.max(qy).min(0.0) + (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt() - r
    }
}

#[cube]
fn apply_color_filter_pixel(px: u32, filter_kind: u32, amount: f32) -> u32 {
    let inv_255 = 1.0 / 255.0;
    let mut r = (px & 255) as f32 * inv_255;
    let mut g = ((px >> 8) & 255) as f32 * inv_255;
    let mut b = ((px >> 16) & 255) as f32 * inv_255;
    let mut a = ((px >> 24) & 255) as f32 * inv_255;

    if filter_kind == FILTER_OPACITY {
        let opacity = amount.clamp(0.0, 1.0);
        r *= opacity;
        g *= opacity;
        b *= opacity;
        a *= opacity;
    } else if a > 0.0 {
        let alpha = a;
        let mut ur = r / alpha;
        let mut ug = g / alpha;
        let mut ub = b / alpha;

        if filter_kind == FILTER_BRIGHTNESS {
            ur *= amount;
            ug *= amount;
            ub *= amount;
        } else if filter_kind == FILTER_CONTRAST {
            ur = (ur - 0.5) * amount + 0.5;
            ug = (ug - 0.5) * amount + 0.5;
            ub = (ub - 0.5) * amount + 0.5;
        } else if filter_kind == FILTER_GRAYSCALE {
            let t = amount.clamp(0.0, 1.0);
            let l = lum(ur, ug, ub);
            ur = lerp(ur, l, t);
            ug = lerp(ug, l, t);
            ub = lerp(ub, l, t);
        } else if filter_kind == FILTER_HUE_ROTATE {
            let angle = amount * f32::new(0.017_453_292_f32);
            let co = angle.cos();
            let si = angle.sin();
            let nr = (0.213 + co * 0.787 - si * 0.213) * ur
                + (0.715 - co * 0.715 - si * 0.715) * ug
                + (0.072 - co * 0.072 + si * 0.928) * ub;
            let ng = (0.213 - co * 0.213 + si * 0.143) * ur
                + (0.715 + co * 0.285 + si * 0.140) * ug
                + (0.072 - co * 0.072 - si * 0.283) * ub;
            let nb = (0.213 - co * 0.213 - si * 0.787) * ur
                + (0.715 - co * 0.715 + si * 0.715) * ug
                + (0.072 + co * 0.928 + si * 0.072) * ub;
            ur = nr;
            ug = ng;
            ub = nb;
        } else if filter_kind == FILTER_INVERT {
            let t = amount.clamp(0.0, 1.0);
            ur = lerp(ur, 1.0 - ur, t);
            ug = lerp(ug, 1.0 - ug, t);
            ub = lerp(ub, 1.0 - ub, t);
        } else if filter_kind == FILTER_SATURATE {
            let l = lum(ur, ug, ub);
            ur = l + (ur - l) * amount;
            ug = l + (ug - l) * amount;
            ub = l + (ub - l) * amount;
        } else if filter_kind == FILTER_SEPIA {
            let t = amount.clamp(0.0, 1.0);
            let sr = ur * 0.393 + ug * 0.769 + ub * 0.189;
            let sg = ur * 0.349 + ug * 0.686 + ub * 0.168;
            let sb = ur * 0.272 + ug * 0.534 + ub * 0.131;
            ur = lerp(ur, sr, t);
            ug = lerp(ug, sg, t);
            ub = lerp(ub, sb, t);
        }

        r = ur.clamp(0.0, 1.0) * alpha;
        g = ug.clamp(0.0, 1.0) * alpha;
        b = ub.clamp(0.0, 1.0) * alpha;
    }

    pack_premul_rgba8(r, g, b, a)
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn apply_color_matrix_pixel(
    px: u32,
    m00: f32,
    m01: f32,
    m02: f32,
    m03: f32,
    m04: f32,
    m10: f32,
    m11: f32,
    m12: f32,
    m13: f32,
    m14: f32,
    m20: f32,
    m21: f32,
    m22: f32,
    m23: f32,
    m24: f32,
    m30: f32,
    m31: f32,
    m32: f32,
    m33: f32,
    m34: f32,
) -> u32 {
    // SVG filter matrices operate on straight RGBA, while render buffers are premultiplied.
    let inv_255 = 1.0 / 255.0;
    let premul_r = (px & 255) as f32 * inv_255;
    let premul_g = ((px >> 8) & 255) as f32 * inv_255;
    let premul_b = ((px >> 16) & 255) as f32 * inv_255;
    let a = ((px >> 24) & 255) as f32 * inv_255;

    let mut r = 0.0;
    let mut g = 0.0;
    let mut b = 0.0;
    if a > 0.0 {
        r = premul_r / a;
        g = premul_g / a;
        b = premul_b / a;
    }

    let out_r = m00 * r + m01 * g + m02 * b + m03 * a + m04;
    let out_g = m10 * r + m11 * g + m12 * b + m13 * a + m14;
    let out_b = m20 * r + m21 * g + m22 * b + m23 * a + m24;
    let out_a = (m30 * r + m31 * g + m32 * b + m33 * a + m34).clamp(0.0, 1.0);
    pack_premul_rgba8(
        out_r.clamp(0.0, 1.0) * out_a,
        out_g.clamp(0.0, 1.0) * out_a,
        out_b.clamp(0.0, 1.0) * out_a,
        out_a,
    )
}

#[cube]
fn apply_component_transfer_pixel(px: u32, table_index: u32, transfer_tables: &Array<u32>) -> u32 {
    let alpha = (px >> 24) & 255;
    let base = table_index * COMPONENT_TRANSFER_TABLE_LEN_U32;
    let r_index = straight_component_index(px & 255, alpha);
    let g_index = straight_component_index((px >> 8) & 255, alpha);
    let b_index = straight_component_index((px >> 16) & 255, alpha);
    let inv_255 = 1.0 / 255.0;
    let r = transfer_tables[(base + r_index) as usize] as f32 * inv_255;
    let g = transfer_tables[(base + COMPONENT_TRANSFER_TABLE_SIZE_U32 + g_index) as usize] as f32
        * inv_255;
    let b = transfer_tables[(base + 2 * COMPONENT_TRANSFER_TABLE_SIZE_U32 + b_index) as usize]
        as f32
        * inv_255;
    let a = transfer_tables[(base + 3 * COMPONENT_TRANSFER_TABLE_SIZE_U32 + alpha) as usize] as f32
        * inv_255;
    pack_premul_rgba8(r * a, g * a, b * a, a)
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn composite_inputs_pixel(
    input1: u32,
    input2: u32,
    operator: u32,
    k1: f32,
    k2: f32,
    k3: f32,
    k4: f32,
) -> u32 {
    let mut out = blend_premul_u8(input2, input1, 3 << 8);
    if operator == 1 {
        out = blend_premul_u8(input2, input1, 5 << 8);
    } else if operator == 2 {
        out = blend_premul_u8(input2, input1, 7 << 8);
    } else if operator == 3 {
        out = blend_premul_u8(input2, input1, 9 << 8);
    } else if operator == 4 {
        out = blend_premul_u8(input2, input1, 11 << 8);
    } else if operator == 5 {
        out = arithmetic_composite_pixel(input1, input2, k1, k2, k3, k4);
    }
    out
}

#[cube]
#[allow(clippy::too_many_arguments)]
fn arithmetic_composite_pixel(input1: u32, input2: u32, k1: f32, k2: f32, k3: f32, k4: f32) -> u32 {
    let a_r = straight_channel(input1 & 255, (input1 >> 24) & 255);
    let a_g = straight_channel((input1 >> 8) & 255, (input1 >> 24) & 255);
    let a_b = straight_channel((input1 >> 16) & 255, (input1 >> 24) & 255);
    let a_a = ((input1 >> 24) & 255) as f32 / 255.0;
    let b_r = straight_channel(input2 & 255, (input2 >> 24) & 255);
    let b_g = straight_channel((input2 >> 8) & 255, (input2 >> 24) & 255);
    let b_b = straight_channel((input2 >> 16) & 255, (input2 >> 24) & 255);
    let b_a = ((input2 >> 24) & 255) as f32 / 255.0;

    let out_r = arithmetic_channel(a_r, b_r, k1, k2, k3, k4);
    let out_g = arithmetic_channel(a_g, b_g, k1, k2, k3, k4);
    let out_b = arithmetic_channel(a_b, b_b, k1, k2, k3, k4);
    let out_a = arithmetic_channel(a_a, b_a, k1, k2, k3, k4);
    pack_premul_rgba8(out_r * out_a, out_g * out_a, out_b * out_a, out_a)
}

#[cube]
fn straight_channel(premul: u32, alpha: u32) -> f32 {
    let mut out = 0.0;
    if alpha != 0 {
        out = premul as f32 / alpha as f32;
    }
    out
}

#[cube]
fn arithmetic_channel(a: f32, b: f32, k1: f32, k2: f32, k3: f32, k4: f32) -> f32 {
    (k1 * a * b + k2 * a + k3 * b + k4).clamp(0.0, 1.0)
}

#[cube]
fn straight_component_index(premul: u32, alpha: u32) -> u32 {
    let safe_alpha = alpha.max(1);
    let mut index = ((premul * 255 + safe_alpha / 2) / safe_alpha).min(255);
    if alpha == 0 {
        index = 0;
    }
    index
}

#[cube]
fn lum(r: f32, g: f32, b: f32) -> f32 {
    r * 0.2126 + g * 0.7152 + b * 0.0722
}

#[cube]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
