const SDF_NONE_REF: u32 = 0xffffffffu;

fn sdf_coverage_from_draw(draw: DrawRecord, x: f32, y: f32) -> f32 {
    let local = affine_record_point(draw.inverse_transform, vec2<f32>(x, y));
    if (draw.sdf_offset != SDF_NONE_REF) {
        return sdf_coverage_from_blob(draw.sdf_offset, false, local.x, local.y, draw.inverse_transform);
    }
    if (draw.sdf_shadow_offset != SDF_NONE_REF) {
        return sdf_coverage_from_blob(draw.sdf_shadow_offset, true, local.x, local.y, draw.inverse_transform);
    }
    return 0.0;
}

fn affine_record_point(transform: AffineRecord, point: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        transform.a * point.x + transform.c * point.y + transform.e,
        transform.b * point.x + transform.d * point.y + transform.f,
    );
}

struct SdfSample {
    distance: f32,
    normal: vec2<f32>,
}

// Analytic SDF geometry remains in local coordinates. Convert its signed distance along the
// local boundary normal to device pixels before applying the fixed one-pixel antialiasing ramp.
// Using the full inverse Jacobian is necessary for non-uniform scale and shear: no single affine
// scale factor is correct for every boundary orientation.
fn sdf_device_distance(sample: SdfSample, inverse_transform: AffineRecord) -> f32 {
    let device_gradient = vec2<f32>(
        inverse_transform.a * sample.normal.x + inverse_transform.b * sample.normal.y,
        inverse_transform.c * sample.normal.x + inverse_transform.d * sample.normal.y,
    );
    return sample.distance / max(length(device_gradient), 0.000001);
}

fn sdf_coverage_from_sample(sample: SdfSample, inverse_transform: AffineRecord) -> f32 {
    return sdf_coverage_from_dist(sdf_device_distance(sample, inverse_transform));
}

fn sdf_shadow_coverage_from_sample(
    sample: SdfSample,
    inverse_transform: AffineRecord,
    expand: f32,
    intensity: f32,
) -> f32 {
    let clamped_intensity = clamp(intensity, 0.0, 1.0);
    if (expand > 0.0) {
        // Shadow expansion belongs to the local geometry and must transform with it. Only the
        // fixed antialiasing ramp below is defined in device pixels.
        return clamp(exp(-max(sample.distance, 0.0) / expand) * clamped_intensity, 0.0, 1.0);
    }
    return sdf_coverage_from_sample(sample, inverse_transform) * clamped_intensity;
}

fn sdf_coverage_from_blob(
    base: u32,
    shadow_blob: bool,
    x: f32,
    y: f32,
    inverse_transform: AffineRecord,
) -> f32 {
    let kind = sdf_word(base, 0u, shadow_blob);
    let x0 = sdf_float(base, 1u, shadow_blob);
    let y0 = sdf_float(base, 2u, shadow_blob);
    let x1 = sdf_float(base, 3u, shadow_blob);
    let y1 = sdf_float(base, 4u, shadow_blob);
    let r0 = sdf_float(base, 5u, shadow_blob);
    let r1 = sdf_float(base, 6u, shadow_blob);
    let r2 = sdf_float(base, 7u, shadow_blob);
    let r3 = sdf_float(base, 8u, shadow_blob);
    let stroke_top = sdf_float(base, 9u, shadow_blob);
    let stroke_right = sdf_float(base, 10u, shadow_blob);
    let stroke_bottom = sdf_float(base, 11u, shadow_blob);
    let stroke_left = sdf_float(base, 12u, shadow_blob);
    let shadow_offset_x = sdf_float(base, 13u, shadow_blob);
    let shadow_offset_y = sdf_float(base, 14u, shadow_blob);
    let shadow_expand = sdf_float(base, 15u, shadow_blob);
    let shadow_intensity = sdf_float(base, 16u, shadow_blob);

    if (kind == GPU_SDF_RECT) {
        return sdf_coverage_from_sample(rect_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2, r3), inverse_transform);
    }
    if (kind == GPU_SDF_RECT_STROKE) {
        let half_top = max(stroke_top, 0.0);
        let half_right = max(stroke_right, 0.0);
        let half_bottom = max(stroke_bottom, 0.0);
        let half_left = max(stroke_left, 0.0);
        let rx0 = min(x0, x1);
        let ry0 = min(y0, y1);
        let rx1 = max(x0, x1);
        let ry1 = max(y0, y1);
        let outer = sdf_coverage_from_sample(rect_sdf_sample(
            x,
            y,
            rx0 - half_left,
            ry0 - half_top,
            rx1 + half_right,
            ry1 + half_bottom,
            r0 + max(half_top, half_left),
            r1 + max(half_top, half_right),
            r2 + max(half_bottom, half_left),
            r3 + max(half_bottom, half_right),
        ), inverse_transform);
        let inner_x0 = rx0 + half_left;
        let inner_y0 = ry0 + half_top;
        let inner_x1 = rx1 - half_right;
        let inner_y1 = ry1 - half_bottom;
        var inner = 0.0;
        if (inner_x0 < inner_x1 && inner_y0 < inner_y1) {
            inner = sdf_coverage_from_sample(rect_sdf_sample(
                x,
                y,
                inner_x0,
                inner_y0,
                inner_x1,
                inner_y1,
                max(r0 - max(half_top, half_left), 0.0),
                max(r1 - max(half_top, half_right), 0.0),
                max(r2 - max(half_bottom, half_left), 0.0),
                max(r3 - max(half_bottom, half_right), 0.0),
            ), inverse_transform);
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_RECT_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            rect_sdf_sample(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
            ),
            inverse_transform,
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CIRCLE) {
        return sdf_coverage_from_sample(circle_sdf_sample(x, y, x0, y0, x1), inverse_transform);
    }
    if (kind == GPU_SDF_CIRCLE_STROKE) {
        let half = max(stroke_top, 0.0);
        let radius = max(x1, 0.0);
        let outer = sdf_coverage_from_sample(circle_sdf_sample(x, y, x0, y0, radius + half), inverse_transform);
        var inner = 0.0;
        if (radius > half) {
            inner = sdf_coverage_from_sample(circle_sdf_sample(x, y, x0, y0, radius - half), inverse_transform);
        }
        return clamp(outer - inner, 0.0, 1.0);
    }
    if (kind == GPU_SDF_CIRCLE_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            circle_sdf_sample(x - shadow_offset_x, y - shadow_offset_y, x0, y0, x1),
            inverse_transform,
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_ARC) {
        return sdf_coverage_from_sample(arc_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2), inverse_transform);
    }
    if (kind == GPU_SDF_ARC_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            arc_sdf_sample(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
            ),
            inverse_transform,
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_CANDLESTICK) {
        return candlestick_sdf_coverage(x, y, x0, y0, x1, y1, r0, r1, r2, inverse_transform);
    }
    if (kind == GPU_SDF_LINE) {
        return sdf_coverage_from_sample(line_sdf_sample(x, y, x0, y0, x1, y1, r0, r1), inverse_transform);
    }
    if (kind == GPU_SDF_DASH_LINE) {
        return sdf_coverage_from_sample(
            dash_line_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2, r3, stroke_top),
            inverse_transform,
        );
    }
    if (kind == GPU_SDF_TRIANGLE) {
        return sdf_coverage_from_sample(
            triangle_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, r2),
            inverse_transform,
        );
    }
    if (kind == GPU_SDF_CHECKERBOARD) {
        return sdf_coverage_from_sample(
            checkerboard_sdf_sample(x, y, x0, y0, x1, y1, r0),
            inverse_transform,
        );
    }
    if (kind == GPU_SDF_STAR) {
        return sdf_coverage_from_sample(
            star_sdf_sample(x, y, x0, y0, x1, y1, r0, r1),
            inverse_transform,
        );
    }
    if (kind == GPU_SDF_STAR_STROKE) {
        return sdf_coverage_from_sample(
            star_stroke_sdf_sample(x, y, x0, y0, x1, y1, r0, r1, stroke_top),
            inverse_transform,
        );
    }
    if (kind == GPU_SDF_CALLOUT) {
        return sdf_coverage_from_sample(
            callout_sdf_sample(
                x,
                y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
                stroke_top,
                stroke_right,
                stroke_left,
            ),
            inverse_transform,
        );
    }
    if (kind == GPU_SDF_CALLOUT_STROKE) {
        return sdf_coverage_from_sample(
            callout_stroke_sdf_sample(
                x,
                y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
                stroke_top,
                stroke_right,
                stroke_bottom,
                stroke_left,
            ),
            inverse_transform,
        );
    }
    if (kind == GPU_SDF_CALLOUT_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            callout_sdf_sample(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
                r2,
                r3,
                stroke_top,
                stroke_right,
                stroke_left,
            ),
            inverse_transform,
            shadow_expand,
            shadow_intensity,
        );
    }
    if (kind == GPU_SDF_LINE_SHADOW) {
        return sdf_shadow_coverage_from_sample(
            line_sdf_sample(
                x - shadow_offset_x,
                y - shadow_offset_y,
                x0,
                y0,
                x1,
                y1,
                r0,
                r1,
            ),
            inverse_transform,
            shadow_expand,
            shadow_intensity,
        );
    }
    return 0.0;
}

fn sdf_word(base: u32, index: u32, shadow_blob: bool) -> u32 {
    return sdf_storage_word(base + index, shadow_blob);
}

fn sdf_float(base: u32, index: u32, shadow_blob: bool) -> f32 {
    return bitcast<f32>(sdf_word(base, index, shadow_blob));
}

#include "sdf_primitives.wgsl"
