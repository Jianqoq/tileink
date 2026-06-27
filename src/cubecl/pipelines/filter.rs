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
fn lum(r: f32, g: f32, b: f32) -> f32 {
    r * 0.2126 + g * 0.7152 + b * 0.0722
}

#[cube]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
