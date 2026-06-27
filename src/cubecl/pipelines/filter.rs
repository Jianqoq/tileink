use ::cubecl::prelude::*;

use crate::{
    cubecl::{
        brush::{
            GPU_BRUSH_FOUR_CORNER, GPU_BRUSH_LINEAR, GPU_BRUSH_PARAM_STRIDE, GPU_BRUSH_PATTERN,
            GPU_BRUSH_RADIAL, GPU_BRUSH_SWEEP, GPU_BRUSH_U32_STRIDE, GPU_EXTEND_REFLECT,
            GPU_EXTEND_REPEAT, GpuBrushResources,
        },
        buffer::CubeBuffer,
        renderer::{ScanBuffers, SceneBuffers},
        types::{
            CUBE_DRAW_BLEND, CUBE_DRAW_BRUSH, CUBE_DRAW_CLIP, CUBE_DRAW_OPACITY, CUBE_LAYER_BLEND,
            CUBE_LAYER_CLIP, CUBE_LAYER_OPACITY,
        },
    },
    shared::bounds::Bounds,
};

const FILTER_WORKGROUP_SIZE: u32 = 256;

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
    let shadow_color = sample_filter_brush(
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
fn sample_filter_brush(
    brush_index: u32,
    x: f32,
    y: f32,
    brush_data: &Array<u32>,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let data_base = (brush_index * GPU_BRUSH_U32_STRIDE as u32) as usize;
    let kind = brush_data[data_base];
    let extend = brush_data[data_base + 1];
    let payload_offset = brush_data[data_base + 2];
    let payload_len = brush_data[data_base + 3];
    let base = (brush_index * GPU_BRUSH_PARAM_STRIDE as u32) as usize;
    let mut color = brush_data[data_base + 4];

    if kind == GPU_BRUSH_LINEAR {
        let sx = brush_params[base];
        let sy = brush_params[base + 1];
        let ex = brush_params[base + 2];
        let ey = brush_params[base + 3];
        let dx = ex - sx;
        let dy = ey - sy;
        let denominator = dx * dx + dy * dy;
        let mut t = 0.0;
        if denominator > f32::new(0.000_000_119_209_29_f32) {
            t = ((x - sx) * dx + (y - sy) * dy) / denominator;
        }
        color = sample_filter_ramp(brush_payloads, payload_offset, payload_len, t, extend);
    } else if kind == GPU_BRUSH_RADIAL {
        color = sample_filter_radial(
            x,
            y,
            base,
            extend,
            payload_offset,
            payload_len,
            brush_params,
            brush_payloads,
        );
    } else if kind == GPU_BRUSH_SWEEP {
        let cx = brush_params[base];
        let cy = brush_params[base + 1];
        let start_angle = brush_params[base + 2];
        let end_angle = brush_params[base + 3];
        let span = end_angle - start_angle;
        let mut t = 0.0;
        if span.abs() > f32::new(0.000_000_119_209_29_f32) {
            let tau = f32::new(6.283_185_5_f32);
            let mut angle = (y - cy).atan2(x - cx);
            if span > 0.0 {
                while angle < start_angle {
                    angle += tau;
                }
            } else {
                while angle > start_angle {
                    angle -= tau;
                }
            }
            t = (angle - start_angle) / span;
        }
        color = sample_filter_ramp(brush_payloads, payload_offset, payload_len, t, extend);
    } else if kind == GPU_BRUSH_FOUR_CORNER {
        color = sample_filter_four_corner(x, y, base, payload_offset, brush_params, brush_payloads);
    } else if kind == GPU_BRUSH_PATTERN {
        color = sample_filter_pattern(
            x,
            y,
            base,
            payload_offset,
            payload_len,
            brush_data[data_base + 5],
            brush_data[data_base + 6],
            brush_data[data_base + 7],
            brush_params,
            brush_payloads,
        );
    }

    color
}

#[cube]
fn sample_filter_radial(
    x: f32,
    y: f32,
    base: usize,
    extend: u32,
    payload_offset: u32,
    payload_len: u32,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let tx = brush_params[base + 6] * x + brush_params[base + 8] * y + brush_params[base + 10];
    let ty = brush_params[base + 7] * x + brush_params[base + 9] * y + brush_params[base + 11];
    let sx = brush_params[base];
    let sy = brush_params[base + 1];
    let ex = brush_params[base + 2];
    let ey = brush_params[base + 3];
    let start_radius = brush_params[base + 4];
    let end_radius = brush_params[base + 5];
    let qx = tx - sx;
    let qy = ty - sy;
    let dcx = ex - sx;
    let dcy = ey - sy;
    let dr = end_radius - start_radius;
    let a = dcx * dcx + dcy * dcy - dr * dr;
    let b = -2.0 * (qx * dcx + qy * dcy + start_radius * dr);
    let c = qx * qx + qy * qy - start_radius * start_radius;
    let mut has_t = false;
    let mut t = 0.0;

    if a.abs() <= f32::new(0.000001_f32) {
        if b.abs() > f32::new(0.000001_f32) {
            let candidate = -c / b;
            if start_radius + candidate * dr >= 0.0 {
                has_t = true;
                t = candidate;
            }
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            let t0 = (-b - root) / (2.0 * a);
            let t1 = (-b + root) / (2.0 * a);
            let valid0 = start_radius + t0 * dr >= 0.0;
            let valid1 = start_radius + t1 * dr >= 0.0;
            if valid0 {
                has_t = true;
                if valid1 {
                    t = t0.max(t1);
                } else {
                    t = t0;
                }
            } else if valid1 {
                has_t = true;
                t = t1;
            }
        }
    }

    let mut color = 0u32;
    if has_t {
        color = sample_filter_ramp(brush_payloads, payload_offset, payload_len, t, extend);
    }
    color
}

#[cube]
fn sample_filter_four_corner(
    x: f32,
    y: f32,
    base: usize,
    payload_offset: u32,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let x0 = brush_params[base];
    let y0 = brush_params[base + 1];
    let x1 = brush_params[base + 2];
    let y1 = brush_params[base + 3];
    let width = x1 - x0;
    let height = y1 - y0;
    let mut u = 0.0;
    let mut v = 0.0;
    if width.abs() > f32::new(0.000_000_119_209_29_f32) {
        u = ((x - x0) / width).clamp(0.0, 1.0);
    }
    if height.abs() > f32::new(0.000_000_119_209_29_f32) {
        v = ((y - y0) / height).clamp(0.0, 1.0);
    }
    let tl = brush_payloads[payload_offset as usize];
    let tr = brush_payloads[(payload_offset + 1) as usize];
    let br = brush_payloads[(payload_offset + 2) as usize];
    let bl = brush_payloads[(payload_offset + 3) as usize];
    let top = lerp_premul_u8(tl, tr, u);
    let bottom = lerp_premul_u8(bl, br, u);
    lerp_premul_u8(top, bottom, v)
}

#[cube]
fn sample_filter_pattern(
    x: f32,
    y: f32,
    base: usize,
    payload_offset: u32,
    payload_len: u32,
    width: u32,
    height: u32,
    opacity: u32,
    brush_params: &Array<f32>,
    brush_payloads: &Array<u32>,
) -> u32 {
    let mut color = 0u32;
    if payload_len > 0 && width > 0 && height > 0 {
        let tx = brush_params[base] * x + brush_params[base + 2] * y + brush_params[base + 4];
        let ty = brush_params[base + 1] * x + brush_params[base + 3] * y + brush_params[base + 5];
        let local_x = repeat_coord_i32(tx.floor() as i32, width);
        let local_y = repeat_coord_i32(ty.floor() as i32, height);
        let local_ix = (local_y * width + local_x).min(payload_len - 1);
        color = scale_premul_u8(
            brush_payloads[(payload_offset + local_ix) as usize],
            opacity,
        );
    }
    color
}

#[cube]
fn sample_filter_ramp(
    brush_payloads: &Array<u32>,
    payload_offset: u32,
    payload_len: u32,
    t: f32,
    extend: u32,
) -> u32 {
    let mut color = 0u32;
    if payload_len > 0 {
        let last = payload_len - 1;
        let position = apply_filter_extend(t, extend) * last as f32;
        let left_ix = position.floor() as u32;
        let right_ix = (left_ix + 1).min(last);
        let frac = position - left_ix as f32;
        let left = brush_payloads[(payload_offset + left_ix) as usize];
        let right = brush_payloads[(payload_offset + right_ix) as usize];
        if frac <= f32::new(0.000_000_119_209_29_f32) || left_ix == right_ix {
            color = left;
        } else {
            color = lerp_premul_u8(left, right, frac);
        }
    }
    color
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
fn apply_filter_extend(t: f32, extend: u32) -> f32 {
    let mut out = t.clamp(0.0, 1.0);
    if extend == GPU_EXTEND_REPEAT {
        out = rem_euclid_f32(t, 1.0);
    } else if extend == GPU_EXTEND_REFLECT {
        let value = rem_euclid_f32(t, 2.0);
        if value <= 1.0 {
            out = value;
        } else {
            out = 2.0 - value;
        }
    }
    out
}

#[cube]
fn rem_euclid_f32(value: f32, modulus: f32) -> f32 {
    value - (value / modulus).floor() * modulus
}

#[cube]
fn repeat_coord_i32(value: i32, size: u32) -> u32 {
    let size_i = size as i32;
    let mut out = value % size_i;
    if out < 0 {
        out += size_i;
    }
    out as u32
}

#[cube]
fn lerp_premul_u8(a: u32, b: u32, t: f32) -> u32 {
    let inv = 1.0 / 255.0;
    let ar = (a & 255) as f32 * inv;
    let ag = ((a >> 8) & 255) as f32 * inv;
    let ab = ((a >> 16) & 255) as f32 * inv;
    let aa = ((a >> 24) & 255) as f32 * inv;
    let br = (b & 255) as f32 * inv;
    let bg = ((b >> 8) & 255) as f32 * inv;
    let bb = ((b >> 16) & 255) as f32 * inv;
    let ba = ((b >> 24) & 255) as f32 * inv;
    pack_premul_rgba8(
        ar + (br - ar) * t,
        ag + (bg - ag) * t,
        ab + (bb - ab) * t,
        aa + (ba - aa) * t,
    )
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
fn lum(r: f32, g: f32, b: f32) -> f32 {
    r * 0.2126 + g * 0.7152 + b * 0.0722
}

#[cube]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cube]
fn pack_premul_rgba8(r: f32, g: f32, b: f32, a: f32) -> u32 {
    let pr = (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    let pg = (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    let pb = (b.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    let pa = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    pr | (pg << 8) | (pb << 16) | (pa << 24)
}

#[cube]
fn src_over_premul_u8(dst: u32, src: u32) -> u32 {
    let sa = src >> 24;
    let mut out = dst;
    if sa != 0 {
        if sa == 255 {
            out = src;
        } else {
            let inv = 255 - sa;
            let r = (src & 255) + mul_div255(dst & 255, inv);
            let g = ((src >> 8) & 255) + mul_div255((dst >> 8) & 255, inv);
            let b = ((src >> 16) & 255) + mul_div255((dst >> 16) & 255, inv);
            let a = sa + mul_div255((dst >> 24) & 255, inv);
            out = r | (g << 8) | (b << 16) | (a << 24);
        }
    }
    out
}

#[cube]
fn blend_premul_u8(dst: u32, src: u32, mode: u32) -> u32 {
    let mix = mode & 255;
    let compose = (mode >> 8) & 255;
    let inv = 1.0 / 255.0;
    let sr = (src & 255) as f32 * inv;
    let sg = ((src >> 8) & 255) as f32 * inv;
    let sb = ((src >> 16) & 255) as f32 * inv;
    let sa = ((src >> 24) & 255) as f32 * inv;
    let dr = (dst & 255) as f32 * inv;
    let dg = ((dst >> 8) & 255) as f32 * inv;
    let db = ((dst >> 16) & 255) as f32 * inv;
    let da = ((dst >> 24) & 255) as f32 * inv;

    let mut out_r = sr + dr * (1.0 - sa);
    let mut out_g = sg + dg * (1.0 - sa);
    let mut out_b = sb + db * (1.0 - sa);
    let mut out_a = sa + da * (1.0 - sa);

    if mix == 0 && compose == 3 {
    } else if mix == 0 && compose == 2 {
        out_r = dr;
        out_g = dg;
        out_b = db;
        out_a = da;
    } else if mix == 0 && compose == 0 {
        out_r = 0.0;
        out_g = 0.0;
        out_b = 0.0;
        out_a = 0.0;
    } else if mix == 0 && compose == 1 {
        out_r = sr;
        out_g = sg;
        out_b = sb;
        out_a = sa;
    } else if mix == 0 {
        let src_factor = compose_src_factor(compose, sa, da);
        let dst_factor = compose_dst_factor(compose, sa, da);
        out_r = sr * src_factor + dr * dst_factor;
        out_g = sg * src_factor + dg * dst_factor;
        out_b = sb * src_factor + db * dst_factor;
        out_a = sa * src_factor + da * dst_factor;
        if compose == 13 {
            out_r = out_r.min(1.0);
            out_g = out_g.min(1.0);
            out_b = out_b.min(1.0);
            out_a = out_a.min(1.0);
        }
    } else if compose == 3 && mix == 6 {
        out_r = color_dodge_premul(sr, dr, sa, da);
        out_g = color_dodge_premul(sg, dg, sa, da);
        out_b = color_dodge_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else if compose == 3 && mix == 7 {
        out_r = color_burn_premul(sr, dr, sa, da);
        out_g = color_burn_premul(sg, dg, sa, da);
        out_b = color_burn_premul(sb, db, sa, da);
        out_a = sa + da * (1.0 - sa);
    } else {
        let src_alpha = sa.clamp(0.0, 1.0);
        let dst_alpha = da.clamp(0.0, 1.0);
        let src_r = unpremul_channel(sr, src_alpha);
        let src_g = unpremul_channel(sg, src_alpha);
        let src_b = unpremul_channel(sb, src_alpha);
        let dst_r = unpremul_channel(dr, dst_alpha);
        let dst_g = unpremul_channel(dg, dst_alpha);
        let dst_b = unpremul_channel(db, dst_alpha);
        let mixed_r = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 0);
        let mixed_g = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 1);
        let mixed_b = mix_rgb_channel(dst_r, dst_g, dst_b, src_r, src_g, src_b, mix, 2);
        let effective_r = src_alpha * ((1.0 - dst_alpha) * src_r + dst_alpha * mixed_r);
        let effective_g = src_alpha * ((1.0 - dst_alpha) * src_g + dst_alpha * mixed_g);
        let effective_b = src_alpha * ((1.0 - dst_alpha) * src_b + dst_alpha * mixed_b);
        let src_factor = compose_src_factor(compose, src_alpha, dst_alpha);
        let dst_factor = compose_dst_factor(compose, src_alpha, dst_alpha);
        out_r = effective_r * src_factor + dr * dst_factor;
        out_g = effective_g * src_factor + dg * dst_factor;
        out_b = effective_b * src_factor + db * dst_factor;
        out_a = src_alpha * src_factor + da * dst_factor;
    }

    pack_premul_rgba8(out_r, out_g, out_b, out_a)
}

#[cube]
fn compose_src_factor(compose: u32, _src_alpha: f32, dst_alpha: f32) -> f32 {
    let mut factor = 1.0;
    if compose == 0 || compose == 2 || compose == 6 || compose == 8 {
        factor = 0.0;
    } else if compose == 4 {
        factor = 1.0 - dst_alpha;
    } else if compose == 5 || compose == 9 {
        factor = dst_alpha;
    } else if compose == 7 || compose == 10 || compose == 11 {
        factor = 1.0 - dst_alpha;
    }
    factor
}

#[cube]
fn compose_dst_factor(compose: u32, src_alpha: f32, _dst_alpha: f32) -> f32 {
    let mut factor = 1.0 - src_alpha;
    if compose == 0 || compose == 1 || compose == 5 || compose == 7 {
        factor = 0.0;
    } else if compose == 2 || compose == 4 {
        factor = 1.0;
    } else if compose == 6 || compose == 10 {
        factor = src_alpha;
    } else if compose == 8 || compose == 9 || compose == 11 {
        factor = 1.0 - src_alpha;
    } else if compose == 12 || compose == 13 {
        factor = 1.0;
    }
    factor
}

#[cube]
fn unpremul_channel(value: f32, alpha: f32) -> f32 {
    let mut out = 0.0;
    if alpha > 0.0 {
        out = value / alpha;
    }
    out
}

#[cube]
fn mix_rgb_channel(
    dst_r: f32,
    dst_g: f32,
    dst_b: f32,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    mix: u32,
    channel: u32,
) -> f32 {
    let mut r = src_r;
    let mut g = src_g;
    let mut b = src_b;
    if mix == 1 {
        r = dst_r * src_r;
        g = dst_g * src_g;
        b = dst_b * src_b;
    } else if mix == 2 {
        r = dst_r + src_r - dst_r * src_r;
        g = dst_g + src_g - dst_g * src_g;
        b = dst_b + src_b - dst_b * src_b;
    } else if mix == 3 {
        r = overlay(dst_r, src_r);
        g = overlay(dst_g, src_g);
        b = overlay(dst_b, src_b);
    } else if mix == 4 {
        r = dst_r.min(src_r);
        g = dst_g.min(src_g);
        b = dst_b.min(src_b);
    } else if mix == 5 {
        r = dst_r.max(src_r);
        g = dst_g.max(src_g);
        b = dst_b.max(src_b);
    } else if mix == 6 {
        r = color_dodge(dst_r, src_r);
        g = color_dodge(dst_g, src_g);
        b = color_dodge(dst_b, src_b);
    } else if mix == 7 {
        r = color_burn(dst_r, src_r);
        g = color_burn(dst_g, src_g);
        b = color_burn(dst_b, src_b);
    } else if mix == 8 {
        r = overlay(src_r, dst_r);
        g = overlay(src_g, dst_g);
        b = overlay(src_b, dst_b);
    } else if mix == 9 {
        r = soft_light(dst_r, src_r);
        g = soft_light(dst_g, src_g);
        b = soft_light(dst_b, src_b);
    } else if mix == 10 {
        r = (dst_r - src_r).abs();
        g = (dst_g - src_g).abs();
        b = (dst_b - src_b).abs();
    } else if mix == 11 {
        r = dst_r + src_r - 2.0 * dst_r * src_r;
        g = dst_g + src_g - 2.0 * dst_g * src_g;
        b = dst_b + src_b - 2.0 * dst_b * src_b;
    } else if mix == 12 {
        let sat_dst = sat3(dst_r, dst_g, dst_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let sr = set_sat_channel(src_r, src_g, src_b, sat_dst, 0);
        let sg = set_sat_channel(src_r, src_g, src_b, sat_dst, 1);
        let sb = set_sat_channel(src_r, src_g, src_b, sat_dst, 2);
        r = set_lum_channel(sr, sg, sb, lum_dst, 0);
        g = set_lum_channel(sr, sg, sb, lum_dst, 1);
        b = set_lum_channel(sr, sg, sb, lum_dst, 2);
    } else if mix == 13 {
        let sat_src = sat3(src_r, src_g, src_b);
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        let dr = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 0);
        let dg = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 1);
        let db = set_sat_channel(dst_r, dst_g, dst_b, sat_src, 2);
        r = set_lum_channel(dr, dg, db, lum_dst, 0);
        g = set_lum_channel(dr, dg, db, lum_dst, 1);
        b = set_lum_channel(dr, dg, db, lum_dst, 2);
    } else if mix == 14 {
        let lum_dst = lum3(dst_r, dst_g, dst_b);
        r = set_lum_channel(src_r, src_g, src_b, lum_dst, 0);
        g = set_lum_channel(src_r, src_g, src_b, lum_dst, 1);
        b = set_lum_channel(src_r, src_g, src_b, lum_dst, 2);
    } else if mix == 15 {
        let lum_src = lum3(src_r, src_g, src_b);
        r = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 0);
        g = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 1);
        b = set_lum_channel(dst_r, dst_g, dst_b, lum_src, 2);
    }

    if channel == 0 {
        r
    } else if channel == 1 {
        g
    } else {
        b
    }
}

#[cube]
fn overlay(dst: f32, src: f32) -> f32 {
    if dst <= 0.5 {
        2.0 * dst * src
    } else {
        1.0 - 2.0 * (1.0 - dst) * (1.0 - src)
    }
}

#[cube]
fn color_dodge(dst: f32, src: f32) -> f32 {
    let mut out = 1.0;
    if src < 1.0 {
        out = (dst / (1.0 - src)).min(1.0);
    }
    out
}

#[cube]
fn color_burn(dst: f32, src: f32) -> f32 {
    let mut out = 0.0;
    if src > 0.0 {
        out = 1.0 - ((1.0 - dst) / src).min(1.0);
    }
    out
}

#[cube]
fn color_dodge_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    let mut out = src * (1.0 - dst_alpha);
    if dst > 0.0 {
        if src >= src_alpha {
            out = src + dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * dst_alpha.min((dst * src_alpha) / (src_alpha - src))
                + src * (1.0 - dst_alpha)
                + dst * (1.0 - src_alpha);
        }
    }
    out
}

#[cube]
fn color_burn_premul(src: f32, dst: f32, src_alpha: f32, dst_alpha: f32) -> f32 {
    let mut out = dst + src * (1.0 - dst_alpha);
    if dst < dst_alpha {
        if src <= 0.0 {
            out = dst * (1.0 - src_alpha);
        } else {
            out = src_alpha * (dst_alpha - dst_alpha.min(((dst_alpha - dst) * src_alpha) / src))
                + src * (1.0 - dst_alpha)
                + dst * (1.0 - src_alpha);
        }
    }
    out
}

#[cube]
fn soft_light(dst: f32, src: f32) -> f32 {
    let mut out = dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    if src > 0.5 {
        let mut d = dst.sqrt();
        if dst <= 0.25 {
            d = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
        }
        out = dst + (2.0 * src - 1.0) * (d - dst);
    }
    out
}

#[cube]
fn lum3(r: f32, g: f32, b: f32) -> f32 {
    0.3 * r + 0.59 * g + 0.11 * b
}

#[cube]
fn sat3(r: f32, g: f32, b: f32) -> f32 {
    r.max(g).max(b) - r.min(g).min(b)
}

#[cube]
fn set_lum_channel(r: f32, g: f32, b: f32, lum: f32, channel: u32) -> f32 {
    let d = lum - lum3(r, g, b);
    clip_color_channel(r + d, g + d, b + d, channel)
}

#[cube]
fn clip_color_channel(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    let lum = lum3(r, g, b);
    let min_c = r.min(g).min(b);
    let max_c = r.max(g).max(b);
    let mut out_r = r;
    let mut out_g = g;
    let mut out_b = b;
    if min_c < 0.0 {
        out_r = lum + (out_r - lum) * lum / (lum - min_c);
        out_g = lum + (out_g - lum) * lum / (lum - min_c);
        out_b = lum + (out_b - lum) * lum / (lum - min_c);
    }
    if max_c > 1.0 {
        out_r = lum + (out_r - lum) * (1.0 - lum) / (max_c - lum);
        out_g = lum + (out_g - lum) * (1.0 - lum) / (max_c - lum);
        out_b = lum + (out_b - lum) * (1.0 - lum) / (max_c - lum);
    }
    if channel == 0 {
        out_r
    } else if channel == 1 {
        out_g
    } else {
        out_b
    }
}

#[cube]
fn set_sat_channel(r: f32, g: f32, b: f32, sat: f32, channel: u32) -> f32 {
    let mut min_ix = 0u32;
    if r <= g && r <= b {
    } else if g <= b {
        min_ix = 1;
    } else {
        min_ix = 2;
    }

    let mut max_ix = 0u32;
    if r >= g && r >= b {
    } else if g >= b {
        max_ix = 1;
    } else {
        max_ix = 2;
    }

    let mut out_r = 0.0;
    let mut out_g = 0.0;
    let mut out_b = 0.0;
    if min_ix != max_ix {
        let mid_ix = 3 - min_ix - max_ix;
        let min_v = channel_value(r, g, b, min_ix);
        let mid_v = channel_value(r, g, b, mid_ix);
        let max_v = channel_value(r, g, b, max_ix);
        let mut new_mid = 0.0;
        let mut new_max = 0.0;
        if max_v > min_v {
            new_mid = (mid_v - min_v) * sat / (max_v - min_v);
            new_max = sat;
        }
        out_r = set_channel_value(out_r, new_mid, mid_ix, 0);
        out_g = set_channel_value(out_g, new_mid, mid_ix, 1);
        out_b = set_channel_value(out_b, new_mid, mid_ix, 2);
        out_r = set_channel_value(out_r, new_max, max_ix, 0);
        out_g = set_channel_value(out_g, new_max, max_ix, 1);
        out_b = set_channel_value(out_b, new_max, max_ix, 2);
    }

    if channel == 0 {
        out_r
    } else if channel == 1 {
        out_g
    } else {
        out_b
    }
}

#[cube]
fn channel_value(r: f32, g: f32, b: f32, channel: u32) -> f32 {
    if channel == 0 {
        r
    } else if channel == 1 {
        g
    } else {
        b
    }
}

#[cube]
fn set_channel_value(current: f32, value: f32, src_channel: u32, dst_channel: u32) -> f32 {
    if src_channel == dst_channel {
        value
    } else {
        current
    }
}

#[cube]
fn scale_premul_u8(px: u32, alpha: u32) -> u32 {
    let r = mul_div255(px & 255, alpha);
    let g = mul_div255((px >> 8) & 255, alpha);
    let b = mul_div255((px >> 16) & 255, alpha);
    let a = mul_div255((px >> 24) & 255, alpha);
    r | (g << 8) | (b << 16) | (a << 24)
}

#[cube]
fn combine_alpha(a: u32, b: u32) -> u32 {
    (a * b + 127) / 255
}

#[cube]
fn mul_div255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}
