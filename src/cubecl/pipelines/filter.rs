use ::cubecl::prelude::*;

use crate::{cubecl::buffer::CubeBuffer, shared::bounds::Bounds};

const FILTER_WORKGROUP_SIZE: u32 = 256;

pub(crate) const FILTER_BRIGHTNESS: u32 = 1;
pub(crate) const FILTER_CONTRAST: u32 = 2;
pub(crate) const FILTER_GRAYSCALE: u32 = 3;
pub(crate) const FILTER_HUE_ROTATE: u32 = 4;
pub(crate) const FILTER_INVERT: u32 = 5;
pub(crate) const FILTER_OPACITY: u32 = 6;
pub(crate) const FILTER_SATURATE: u32 = 7;
pub(crate) const FILTER_SEPIA: u32 = 8;

pub(crate) const FILTER_BRUSH_U32_STRIDE: usize = 8;
pub(crate) const FILTER_BRUSH_PARAM_STRIDE: usize = 12;
pub(crate) const FILTER_BRUSH_SOLID: u32 = 1;
pub(crate) const FILTER_BRUSH_LINEAR: u32 = 2;
pub(crate) const FILTER_BRUSH_RADIAL: u32 = 3;
pub(crate) const FILTER_BRUSH_SWEEP: u32 = 4;
pub(crate) const FILTER_BRUSH_FOUR_CORNER: u32 = 5;
pub(crate) const FILTER_BRUSH_PATTERN: u32 = 6;

pub(crate) const FILTER_EXTEND_PAD: u32 = 0;
pub(crate) const FILTER_EXTEND_REPEAT: u32 = 1;
pub(crate) const FILTER_EXTEND_REFLECT: u32 = 2;

pub(crate) struct FilterBrushResources<'a> {
    pub(crate) data: &'a CubeBuffer<u32>,
    pub(crate) params: &'a CubeBuffer<f32>,
    pub(crate) payloads: &'a CubeBuffer<u32>,
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
        brushes: FilterBrushResources<'_>,
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

    let scale = if sum > 0.0 {
        1.0 / (255.0 * sum)
    } else {
        f32::new(0.0_f32)
    };
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
    let data_base = (brush_index * FILTER_BRUSH_U32_STRIDE as u32) as usize;
    let kind = brush_data[data_base];
    let extend = brush_data[data_base + 1];
    let payload_offset = brush_data[data_base + 2];
    let payload_len = brush_data[data_base + 3];
    let base = (brush_index * FILTER_BRUSH_PARAM_STRIDE as u32) as usize;
    let mut color = brush_data[data_base + 4];

    if kind == FILTER_BRUSH_LINEAR {
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
    } else if kind == FILTER_BRUSH_RADIAL {
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
    } else if kind == FILTER_BRUSH_SWEEP {
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
    } else if kind == FILTER_BRUSH_FOUR_CORNER {
        color = sample_filter_four_corner(x, y, base, payload_offset, brush_params, brush_payloads);
    } else if kind == FILTER_BRUSH_PATTERN {
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
            if valid0 && valid1 {
                has_t = true;
                t = t0.max(t1);
            } else if valid0 {
                has_t = true;
                t = t0;
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
        color = if frac <= f32::new(0.000_000_119_209_29_f32) || left_ix == right_ix {
            left
        } else {
            lerp_premul_u8(left, right, frac)
        };
    }
    color
}

#[cube]
fn apply_filter_extend(t: f32, extend: u32) -> f32 {
    let mut out = t.clamp(0.0, 1.0);
    if extend == FILTER_EXTEND_REPEAT {
        out = rem_euclid_f32(t, 1.0);
    } else if extend == FILTER_EXTEND_REFLECT {
        let value = rem_euclid_f32(t, 2.0);
        out = if value <= 1.0 { value } else { 2.0 - value };
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
fn scale_premul_u8(px: u32, alpha: u32) -> u32 {
    let r = mul_div255(px & 255, alpha);
    let g = mul_div255((px >> 8) & 255, alpha);
    let b = mul_div255((px >> 16) & 255, alpha);
    let a = mul_div255((px >> 24) & 255, alpha);
    r | (g << 8) | (b << 16) | (a << 24)
}

#[cube]
fn mul_div255(a: u32, b: u32) -> u32 {
    let t = a * b + 128;
    (t + (t >> 8)) >> 8
}
