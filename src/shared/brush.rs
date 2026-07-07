use std::sync::Arc;

use peniko::{
    Extend, Gradient, GradientKind, InterpolationAlphaSpace,
    color::{PremulColor, Srgb},
    kurbo,
};

use crate::shared::{
    gpu_layout::brush::{
        GPU_BRUSH_FOUR_CORNER, GPU_BRUSH_LINEAR, GPU_BRUSH_PARAM_STRIDE,
        GPU_BRUSH_PATTERN_RESOURCE, GPU_BRUSH_RADIAL, GPU_BRUSH_SOLID, GPU_BRUSH_SWEEP,
        GPU_BRUSH_U32_STRIDE, GPU_EXTEND_PAD, GPU_EXTEND_REFLECT, GPU_EXTEND_REPEAT,
        GPU_PATTERN_BILINEAR, GPU_PATTERN_NEAREST,
    },
    image::{Image, unpack_rgba8},
    image_resource::{ImageKey, ImageResourceId, ImageResourceResolver},
    pixel::{pack_premul_rgba8, premul_f32_to_u32, scale_premul_u8, unpack_premul_rgba8},
};

// Default ramp size chosen for high-quality SVG/filter parity. Individual
// gradients can override this at construction time.
pub const DEFAULT_GRADIENT_RAMP_SIZE: usize = 4096;
pub(crate) const MIN_GRADIENT_RAMP_SIZE: usize = 64;
const GRADIENT_SAMPLES_PER_STOP_SEGMENT: usize = 16;
const GRADIENT_SPAN_QUALITY: f32 = 2.0;

#[derive(Clone, Debug)]
pub enum Brush {
    Solid(peniko::Color),
    Linear(LinearGradient),
    Radial(RadialGradient),
    Sweep(SweepGradient),
    FourCorner(FourCornerGradient),
    Pattern(PatternBrush),
}

#[derive(Clone, Debug)]
pub struct LinearGradient {
    pub(crate) start: [f32; 2],
    pub(crate) end: [f32; 2],
    pub(crate) transform: [f32; 6],
    pub(crate) extend: Extend,
    pub(crate) ramp: Arc<[u32]>,
}

#[derive(Clone, Debug)]
pub struct RadialGradient {
    pub(crate) start_center: [f32; 2],
    pub(crate) end_center: [f32; 2],
    pub(crate) start_radius: f32,
    pub(crate) end_radius: f32,
    pub(crate) transform: [f32; 6],
    pub(crate) extend: Extend,
    pub(crate) ramp: Arc<[u32]>,
}

#[derive(Clone, Debug)]
pub struct SweepGradient {
    pub(crate) center: [f32; 2],
    pub(crate) start_angle: f32,
    pub(crate) end_angle: f32,
    pub(crate) extend: Extend,
    pub(crate) ramp: Arc<[u32]>,
}

#[derive(Clone, Debug)]
pub struct FourCornerGradient {
    pub(crate) bounds: [f32; 4],
    /// Top-left, top-right, bottom-right, bottom-left.
    pub(crate) colors: [u32; 4],
}

#[derive(Clone, Debug)]
pub struct PatternBrush {
    pub(crate) image: PatternImage,
    pub(crate) transform: [f32; 6],
    pub(crate) extend: Extend,
    pub(crate) sampling: PatternSampling,
    pub(crate) opacity: u8,
}

#[derive(Clone, Debug)]
pub(crate) enum PatternImage {
    Resource(ImageResourceId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternSampling {
    /// Point sampling for SVG patterns and explicit image-rendering speed/crisp hints.
    Nearest,
    /// Center-aligned bilinear sampling for default SVG raster image rendering.
    Bilinear,
}

pub(crate) const ENCODED_BRUSH_HEADER_WORDS: usize = GPU_BRUSH_U32_STRIDE + GPU_BRUSH_PARAM_STRIDE;

impl Brush {
    /// Creates an image brush from a renderer-owned image resource key with explicit sampling.
    pub fn from_image_key(
        key: ImageKey,
        rect: kurbo::Rect,
        sampling: PatternSampling,
    ) -> Option<Self> {
        Self::from_image_key_with_options(key, rect, Extend::Pad, sampling, 255)
    }

    /// Creates a renderer-resource image brush with explicit extend, sampling, and opacity.
    pub fn from_image_key_with_options(
        key: ImageKey,
        rect: kurbo::Rect,
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        PatternBrush::for_rect_resource(
            ImageResourceId::renderer(key),
            rect,
            extend,
            sampling,
            opacity,
        )
        .map(Self::Pattern)
    }

    pub(crate) fn from_scene_image_key_with_options(
        key: ImageKey,
        rect: kurbo::Rect,
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        PatternBrush::for_rect_resource(
            ImageResourceId::scene(key),
            rect,
            extend,
            sampling,
            opacity,
        )
        .map(Self::Pattern)
    }

    /// Draws a renderer-owned image at 1:1 canvas pixels starting at `origin`.
    pub fn from_image_key_natural(
        key: ImageKey,
        origin: [f32; 2],
        image_size: (u32, u32),
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        PatternBrush::for_origin_resource(key, origin, image_size, extend, sampling, opacity)
            .map(Self::Pattern)
    }

    pub fn from_gradient(gradient: &Gradient) -> Self {
        Self::from_gradient_with_ramp_size(gradient, estimate_gradient_ramp_size(gradient))
    }

    pub fn from_gradient_with_ramp_size(gradient: &Gradient, ramp_size: usize) -> Self {
        let ramp = build_ramp(gradient, ramp_size);
        match gradient.kind {
            GradientKind::Linear(position) => Self::Linear(LinearGradient {
                start: [position.start.x as f32, position.start.y as f32],
                end: [position.end.x as f32, position.end.y as f32],
                transform: IDENTITY_TRANSFORM,
                extend: gradient.extend,
                ramp,
            }),
            GradientKind::Radial(position) => Self::Radial(RadialGradient {
                start_center: [
                    position.start_center.x as f32,
                    position.start_center.y as f32,
                ],
                end_center: [position.end_center.x as f32, position.end_center.y as f32],
                start_radius: position.start_radius,
                end_radius: position.end_radius,
                transform: IDENTITY_TRANSFORM,
                extend: gradient.extend,
                ramp,
            }),
            GradientKind::Sweep(position) => Self::Sweep(SweepGradient {
                center: [position.center.x as f32, position.center.y as f32],
                start_angle: position.start_angle,
                end_angle: position.end_angle,
                extend: gradient.extend,
                ramp,
            }),
        }
    }

    pub fn four_corner(bounds: kurbo::Rect, colors: [peniko::Color; 4]) -> Self {
        Self::FourCorner(FourCornerGradient {
            bounds: [
                bounds.x0 as f32,
                bounds.y0 as f32,
                bounds.x1 as f32,
                bounds.y1 as f32,
            ],
            colors: colors.map(|color| premul_f32_to_u32(color.premultiply().components)),
        })
    }

    #[inline]
    pub(crate) fn solid_color(&self) -> Option<peniko::Color> {
        match self {
            Self::Solid(color) => Some(*color),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn sample_with_resources<'a>(
        &self,
        x: f32,
        y: f32,
        image_resources: impl Into<ImageResourceResolver<'a>>,
    ) -> u32 {
        let image_resources = image_resources.into();
        match self {
            Self::Solid(color) => premul_f32_to_u32(color.premultiply().components),
            Self::Linear(gradient) => {
                sample_ramp(&gradient.ramp, gradient.t(x, y), gradient.extend)
            }
            Self::Radial(gradient) => gradient
                .t(x, y)
                .map(|t| sample_ramp(&gradient.ramp, t, gradient.extend))
                .unwrap_or(0),
            Self::Sweep(gradient) => sample_ramp(&gradient.ramp, gradient.t(x, y), gradient.extend),
            Self::FourCorner(gradient) => gradient.sample(x, y),
            Self::Pattern(pattern) => pattern.sample_with_resources(x, y, image_resources),
        }
    }
}

pub(crate) fn push_encoded_brush(blob: &mut Vec<u32>, brush: &Brush) -> (u32, u32) {
    let offset = blob.len() as u32;
    let mut data = [0; GPU_BRUSH_U32_STRIDE];
    let mut params = [0; GPU_BRUSH_PARAM_STRIDE];
    data[0] = GPU_BRUSH_SOLID;
    data[1] = GPU_EXTEND_PAD;
    data[7] = 255;
    data[8] = GPU_PATTERN_NEAREST;

    match brush {
        Brush::Solid(color) => {
            data[4] = premul_f32_to_u32(color.premultiply().components);
        }
        Brush::Linear(gradient) => {
            data[0] = GPU_BRUSH_LINEAR;
            data[1] = encode_gpu_extend(gradient.extend);
            params[0] = gradient.start[0].to_bits();
            params[1] = gradient.start[1].to_bits();
            params[2] = gradient.end[0].to_bits();
            params[3] = gradient.end[1].to_bits();
            copy_f32_bits(&mut params[4..10], &gradient.transform);
            set_local_payload(&mut data, gradient.ramp.len());
            push_encoded_header(blob, data, params);
            blob.extend_from_slice(&gradient.ramp);
            return (offset, blob.len() as u32 - offset);
        }
        Brush::Radial(gradient) => {
            data[0] = GPU_BRUSH_RADIAL;
            data[1] = encode_gpu_extend(gradient.extend);
            params[0] = gradient.start_center[0].to_bits();
            params[1] = gradient.start_center[1].to_bits();
            params[2] = gradient.end_center[0].to_bits();
            params[3] = gradient.end_center[1].to_bits();
            params[4] = gradient.start_radius.to_bits();
            params[5] = gradient.end_radius.to_bits();
            copy_f32_bits(&mut params[6..12], &gradient.transform);
            set_local_payload(&mut data, gradient.ramp.len());
            push_encoded_header(blob, data, params);
            blob.extend_from_slice(&gradient.ramp);
            return (offset, blob.len() as u32 - offset);
        }
        Brush::Sweep(gradient) => {
            data[0] = GPU_BRUSH_SWEEP;
            data[1] = encode_gpu_extend(gradient.extend);
            params[0] = gradient.center[0].to_bits();
            params[1] = gradient.center[1].to_bits();
            params[2] = gradient.start_angle.to_bits();
            params[3] = gradient.end_angle.to_bits();
            set_local_payload(&mut data, gradient.ramp.len());
            push_encoded_header(blob, data, params);
            blob.extend_from_slice(&gradient.ramp);
            return (offset, blob.len() as u32 - offset);
        }
        Brush::FourCorner(gradient) => {
            data[0] = GPU_BRUSH_FOUR_CORNER;
            copy_f32_bits(&mut params[0..4], &gradient.bounds);
            set_local_payload(&mut data, gradient.colors.len());
            push_encoded_header(blob, data, params);
            blob.extend_from_slice(&gradient.colors);
            return (offset, blob.len() as u32 - offset);
        }
        Brush::Pattern(pattern) => {
            data[1] = encode_gpu_extend(pattern.extend);
            copy_f32_bits(&mut params[0..6], &pattern.transform);
            let (width, height) = pattern.image_size();
            data[5] = width;
            data[6] = height;
            data[7] = pattern.opacity as u32;
            data[8] = encode_gpu_pattern_sampling(pattern.sampling);
            let PatternImage::Resource(id) = pattern.image;
            let (scope, low, high) = id.encode();
            data[0] = GPU_BRUSH_PATTERN_RESOURCE;
            set_local_payload(&mut data, 3);
            push_encoded_header(blob, data, params);
            blob.push(scope);
            blob.push(low);
            blob.push(high);
            return (offset, blob.len() as u32 - offset);
        }
    }

    push_encoded_header(blob, data, params);
    (offset, blob.len() as u32 - offset)
}

pub(crate) fn encoded_brush_word_len(brush: &Brush) -> usize {
    ENCODED_BRUSH_HEADER_WORDS
        + match brush {
            Brush::Solid(_) => 0,
            Brush::Linear(gradient) => gradient.ramp.len(),
            Brush::Radial(gradient) => gradient.ramp.len(),
            Brush::Sweep(gradient) => gradient.ramp.len(),
            Brush::FourCorner(gradient) => gradient.colors.len(),
            Brush::Pattern(_) => 3,
        }
}

pub(crate) fn encoded_brush_payload(blob: &[u32], offset: u32, len: u32) -> Option<&[u32]> {
    let record = encoded_brush_words(blob, offset, len)?;
    let data: &[u32; GPU_BRUSH_U32_STRIDE] = record[..GPU_BRUSH_U32_STRIDE].try_into().ok()?;
    let start = data[2] as usize;
    let end = start.checked_add(data[3] as usize)?;
    record.get(start..end)
}

pub(crate) fn decode_encoded_brush(blob: &[u32], offset: u32, len: u32) -> Option<Brush> {
    let record = encoded_brush_words(blob, offset, len)?;
    let data: &[u32; GPU_BRUSH_U32_STRIDE] = record[..GPU_BRUSH_U32_STRIDE].try_into().ok()?;
    let params = decode_params(&record[GPU_BRUSH_U32_STRIDE..ENCODED_BRUSH_HEADER_WORDS]);
    let payload = encoded_brush_payload(blob, offset, len)?;
    match data[0] {
        GPU_BRUSH_SOLID => Some(Brush::Solid(color_from_premul_rgba8(data[4]))),
        GPU_BRUSH_LINEAR => Some(Brush::Linear(LinearGradient {
            start: [params[0], params[1]],
            end: [params[2], params[3]],
            transform: params[4..10].try_into().ok()?,
            extend: decode_gpu_extend(data[1]),
            ramp: Arc::from(payload),
        })),
        GPU_BRUSH_RADIAL => Some(Brush::Radial(RadialGradient {
            start_center: [params[0], params[1]],
            end_center: [params[2], params[3]],
            start_radius: params[4],
            end_radius: params[5],
            transform: params[6..12].try_into().ok()?,
            extend: decode_gpu_extend(data[1]),
            ramp: Arc::from(payload),
        })),
        GPU_BRUSH_SWEEP => Some(Brush::Sweep(SweepGradient {
            center: [params[0], params[1]],
            start_angle: params[2],
            end_angle: params[3],
            extend: decode_gpu_extend(data[1]),
            ramp: Arc::from(payload),
        })),
        GPU_BRUSH_FOUR_CORNER => Some(Brush::FourCorner(FourCornerGradient {
            bounds: params[0..4].try_into().ok()?,
            colors: payload.try_into().ok()?,
        })),
        GPU_BRUSH_PATTERN_RESOURCE => {
            if payload.len() < 3 {
                return None;
            }
            Some(Brush::Pattern(PatternBrush {
                image: PatternImage::Resource(ImageResourceId::decode(
                    payload[0], payload[1], payload[2],
                )),
                transform: params[0..6].try_into().ok()?,
                extend: decode_gpu_extend(data[1]),
                sampling: decode_gpu_pattern_sampling(data[8]),
                opacity: data[7].min(255) as u8,
            }))
        }
        _ => None,
    }
}

impl From<&Gradient> for Brush {
    fn from(value: &Gradient) -> Self {
        Self::from_gradient(value)
    }
}

impl From<peniko::Color> for Brush {
    fn from(value: peniko::Color) -> Self {
        Self::Solid(value)
    }
}

impl LinearGradient {
    fn t(&self, x: f32, y: f32) -> f32 {
        let [x, y] = transform_point(self.transform, x, y);
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        let denominator = dx * dx + dy * dy;
        if denominator <= f32::EPSILON {
            0.0
        } else {
            ((x - self.start[0]) * dx + (y - self.start[1]) * dy) / denominator
        }
    }
}

impl RadialGradient {
    fn t(&self, x: f32, y: f32) -> Option<f32> {
        let [x, y] = transform_point(self.transform, x, y);
        let qx = x - self.start_center[0];
        let qy = y - self.start_center[1];
        let dcx = self.end_center[0] - self.start_center[0];
        let dcy = self.end_center[1] - self.start_center[1];
        let dr = self.end_radius - self.start_radius;
        let a = dcx * dcx + dcy * dcy - dr * dr;
        let b = -2.0 * (qx * dcx + qy * dcy + self.start_radius * dr);
        let c = qx * qx + qy * qy - self.start_radius * self.start_radius;
        if a.abs() <= 1e-6 {
            if b.abs() <= 1e-6 {
                None
            } else {
                let t = -c / b;
                (self.start_radius + t * dr >= 0.0).then_some(t)
            }
        } else {
            let discriminant = b * b - 4.0 * a * c;
            if discriminant < 0.0 {
                None
            } else {
                let root = discriminant.sqrt();
                let t0 = (-b - root) / (2.0 * a);
                let t1 = (-b + root) / (2.0 * a);
                choose_radial_root(t0, t1, self.start_radius, dr)
            }
        }
    }
}

impl SweepGradient {
    fn t(&self, x: f32, y: f32) -> f32 {
        let mut angle = (y - self.center[1]).atan2(x - self.center[0]);
        let tau = std::f32::consts::TAU;
        let span = self.end_angle - self.start_angle;
        if span.abs() <= f32::EPSILON {
            0.0
        } else {
            if span > 0.0 {
                while angle < self.start_angle {
                    angle += tau;
                }
            } else {
                while angle > self.start_angle {
                    angle -= tau;
                }
            }
            (angle - self.start_angle) / span
        }
    }
}

impl FourCornerGradient {
    fn sample(&self, x: f32, y: f32) -> u32 {
        let width = self.bounds[2] - self.bounds[0];
        let height = self.bounds[3] - self.bounds[1];
        let u = if width.abs() <= f32::EPSILON {
            0.0
        } else {
            ((x - self.bounds[0]) / width).clamp(0.0, 1.0)
        };
        let v = if height.abs() <= f32::EPSILON {
            0.0
        } else {
            ((y - self.bounds[1]) / height).clamp(0.0, 1.0)
        };
        let tl = unpack_premul_rgba8(self.colors[0]);
        let tr = unpack_premul_rgba8(self.colors[1]);
        let br = unpack_premul_rgba8(self.colors[2]);
        let bl = unpack_premul_rgba8(self.colors[3]);
        let mut result = [0.0; 4];
        for channel in 0..4 {
            let top = tl[channel] + (tr[channel] - tl[channel]) * u;
            let bottom = bl[channel] + (br[channel] - bl[channel]) * u;
            result[channel] = top + (bottom - top) * v;
        }
        pack_premul_rgba8(result)
    }
}

pub(crate) fn estimate_gradient_ramp_size(gradient: &Gradient) -> usize {
    match gradient.kind {
        GradientKind::Linear(position) => estimate_linear_ramp_size(
            [position.start.x as f32, position.start.y as f32],
            [position.end.x as f32, position.end.y as f32],
            gradient.stops.len(),
        ),
        GradientKind::Radial(position) => estimate_radial_ramp_size(
            [
                position.start_center.x as f32,
                position.start_center.y as f32,
            ],
            [position.end_center.x as f32, position.end_center.y as f32],
            position.start_radius,
            position.end_radius,
            1.0,
            gradient.stops.len(),
        ),
        GradientKind::Sweep(position) => estimate_sweep_ramp_size(
            position.start_angle,
            position.end_angle,
            gradient.stops.len(),
        ),
    }
}

pub(crate) fn estimate_linear_ramp_size(
    start: [f32; 2],
    end: [f32; 2],
    stop_count: usize,
) -> usize {
    let span = (end[0] - start[0]).hypot(end[1] - start[1]);
    quantize_ramp_size(span, stop_count)
}

pub(crate) fn estimate_radial_ramp_size(
    start_center: [f32; 2],
    end_center: [f32; 2],
    start_radius: f32,
    end_radius: f32,
    scale: f32,
    stop_count: usize,
) -> usize {
    let center_span = (end_center[0] - start_center[0]).hypot(end_center[1] - start_center[1]);
    let radius_span = start_radius.abs() + end_radius.abs();
    let span = (center_span + radius_span).max((end_radius - start_radius).abs()) * scale.abs();
    quantize_ramp_size(span, stop_count)
}

pub(crate) fn estimate_sweep_ramp_size(
    start_angle: f32,
    end_angle: f32,
    stop_count: usize,
) -> usize {
    let turns = ((end_angle - start_angle).abs() / std::f32::consts::TAU).max(0.25);
    quantize_ramp_size(turns * 256.0, stop_count)
}

impl PatternBrush {
    /// Creates a pattern brush with a caller-provided world-to-image transform.
    ///
    /// The transform maps canvas coordinates to image pixel coordinates. Images
    /// are rejected when either dimension is zero because both CPU and wgpu
    /// samplers require at least one valid texel.
    pub(crate) fn new_resource(
        id: ImageResourceId,
        transform: [f32; 6],
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        Some(Self {
            image: PatternImage::Resource(id),
            transform,
            extend,
            sampling,
            opacity,
        })
    }

    /// Creates a renderer-resource pattern brush that maps the image exactly into `rect`.
    pub(crate) fn for_rect_resource(
        id: ImageResourceId,
        rect: kurbo::Rect,
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        if !rect_is_valid_image_target(rect) {
            return None;
        }
        let sx = 1.0 / rect.width() as f32;
        let sy = 1.0 / rect.height() as f32;
        Self::new_resource(
            id,
            [
                sx,
                0.0,
                0.0,
                sy,
                -(rect.x0 as f32) * sx,
                -(rect.y0 as f32) * sy,
            ],
            extend,
            sampling,
            opacity,
        )
    }

    /// Creates a renderer-resource pattern brush that maps image pixels 1:1 to canvas
    /// coordinates.
    pub fn for_origin_resource(
        key: ImageKey,
        origin: [f32; 2],
        image_size: (u32, u32),
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        let (width, height) = image_size;
        if width == 0 || height == 0 {
            return None;
        }
        let sx = 1.0 / width as f32;
        let sy = 1.0 / height as f32;
        Self::new_resource(
            ImageResourceId::renderer(key),
            [sx, 0.0, 0.0, sy, -origin[0] * sx, -origin[1] * sy],
            extend,
            sampling,
            opacity,
        )
    }

    #[cfg(test)]
    pub(crate) fn image_key(&self) -> Option<ImageKey> {
        match self.image {
            PatternImage::Resource(ImageResourceId::Renderer(key)) => Some(key),
            PatternImage::Resource(ImageResourceId::Scene(_)) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn image_resource_id(&self) -> ImageResourceId {
        match self.image {
            PatternImage::Resource(id) => id,
        }
    }

    pub(crate) fn image_size(&self) -> (u32, u32) {
        (0, 0)
    }

    fn sample_with_resources(
        &self,
        x: f32,
        y: f32,
        image_resources: ImageResourceResolver<'_>,
    ) -> u32 {
        let PatternImage::Resource(id) = self.image;
        let Some(image) = image_resources.resolve(id) else {
            return 0;
        };
        let [mut x, mut y] = transform_point(self.transform, x, y);
        x *= image.width as f32;
        y *= image.height as f32;
        let pixel = match self.sampling {
            PatternSampling::Nearest => {
                let local_x = extend_coord(x.floor() as i32, image.width, self.extend);
                let local_y = extend_coord(y.floor() as i32, image.height, self.extend);
                image.pixels[(local_y * image.width + local_x) as usize]
            }
            PatternSampling::Bilinear => {
                let x = x - 0.5;
                let y = y - 0.5;
                let x0 = x.floor();
                let y0 = y.floor();
                let tx = x - x0;
                let ty = y - y0;
                let x0 = x0 as i32;
                let y0 = y0 as i32;
                let tl = self.pixel_at(image, x0, y0);
                let tr = self.pixel_at(image, x0 + 1, y0);
                let bl = self.pixel_at(image, x0, y0 + 1);
                let br = self.pixel_at(image, x0 + 1, y0 + 1);
                lerp_premul_u8(lerp_premul_u8(tl, tr, tx), lerp_premul_u8(bl, br, tx), ty)
            }
        };
        scale_premul_u8(pixel, self.opacity)
    }

    fn pixel_at(&self, image: &Image, x: i32, y: i32) -> u32 {
        let local_x = extend_coord(x, image.width, self.extend);
        let local_y = extend_coord(y, image.height, self.extend);
        image.pixels[(local_y * image.width + local_x) as usize]
    }
}

fn rect_is_valid_image_target(rect: kurbo::Rect) -> bool {
    rect.x0.is_finite()
        && rect.y0.is_finite()
        && rect.x1.is_finite()
        && rect.y1.is_finite()
        && rect.width() > 0.0
        && rect.height() > 0.0
}

pub(crate) const IDENTITY_TRANSFORM: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[inline]
pub(crate) fn transform_point(transform: [f32; 6], x: f32, y: f32) -> [f32; 2] {
    [
        transform[0] * x + transform[2] * y + transform[4],
        transform[1] * x + transform[3] * y + transform[5],
    ]
}

fn build_ramp(gradient: &Gradient, ramp_size: usize) -> Arc<[u32]> {
    let ramp_size = ramp_size.max(2);
    let mut ramp = vec![0_u32; ramp_size];
    if gradient.stops.is_empty() {
        return Arc::from(ramp.into_boxed_slice());
    }
    let mut stops = gradient.stops.iter().copied().collect::<Vec<_>>();
    stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    for (ix, output) in ramp.iter_mut().enumerate() {
        let t = ix as f32 / (ramp_size - 1) as f32;
        let upper = stops.partition_point(|stop| stop.offset < t);
        let (left, right) = match upper {
            0 => (stops[0], stops[0]),
            n if n >= stops.len() => {
                let stop = stops[stops.len() - 1];
                (stop, stop)
            }
            n => (stops[n - 1], stops[n]),
        };
        let span = right.offset - left.offset;
        let local_t = if span.abs() <= f32::EPSILON {
            0.0
        } else {
            ((t - left.offset) / span).clamp(0.0, 1.0)
        };
        let color = match gradient.interpolation_alpha_space {
            InterpolationAlphaSpace::Premultiplied => left
                .color
                .interpolate(
                    right.color,
                    gradient.interpolation_cs,
                    gradient.hue_direction,
                )
                .eval(local_t)
                .to_alpha_color::<Srgb>()
                .premultiply(),
            InterpolationAlphaSpace::Unpremultiplied => left
                .color
                .interpolate_unpremultiplied(
                    right.color,
                    gradient.interpolation_cs,
                    gradient.hue_direction,
                )
                .eval(local_t)
                .to_alpha_color::<Srgb>()
                .premultiply(),
        };
        *output = premul_color_to_u32(color);
    }
    Arc::from(ramp.into_boxed_slice())
}

fn choose_radial_root(t0: f32, t1: f32, r0: f32, dr: f32) -> Option<f32> {
    let valid0 = r0 + t0 * dr >= 0.0;
    let valid1 = r0 + t1 * dr >= 0.0;
    match (valid0, valid1) {
        (true, true) => Some(t0.max(t1)),
        (true, false) => Some(t0),
        (false, true) => Some(t1),
        (false, false) => None,
    }
}

#[inline]
fn sample_ramp(ramp: &[u32], t: f32, extend: Extend) -> u32 {
    if ramp.is_empty() {
        return 0;
    }
    let t = apply_extend(t, extend);
    let last = ramp.len() - 1;
    let position = t * last as f32;
    let left_ix = position.floor() as usize;
    let right_ix = (left_ix + 1).min(last);
    let frac = position - left_ix as f32;
    if frac <= f32::EPSILON || left_ix == right_ix {
        return ramp[left_ix];
    }

    lerp_premul_u8(ramp[left_ix], ramp[right_ix], frac)
}

#[inline]
fn apply_extend(t: f32, extend: Extend) -> f32 {
    match extend {
        Extend::Pad => t.clamp(0.0, 1.0),
        Extend::Repeat => t.rem_euclid(1.0),
        Extend::Reflect => {
            let value = t.rem_euclid(2.0);
            if value <= 1.0 { value } else { 2.0 - value }
        }
    }
}

#[inline]
fn premul_color_to_u32(color: PremulColor<Srgb>) -> u32 {
    premul_f32_to_u32(color.components)
}

fn extend_coord(value: i32, size: u32, extend: Extend) -> u32 {
    match extend {
        Extend::Pad => value.clamp(0, size as i32 - 1) as u32,
        Extend::Repeat => value.rem_euclid(size as i32) as u32,
        Extend::Reflect => reflect_coord(value, size),
    }
}

fn reflect_coord(value: i32, size: u32) -> u32 {
    if size <= 1 {
        return 0;
    }
    let period = size as i32 * 2;
    let value = value.rem_euclid(period);
    if value < size as i32 {
        value as u32
    } else {
        (period - value - 1) as u32
    }
}

fn lerp_premul_u8(a: u32, b: u32, t: f32) -> u32 {
    let a = unpack_premul_rgba8(a);
    let b = unpack_premul_rgba8(b);
    pack_premul_rgba8([
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ])
}

fn push_encoded_header(
    blob: &mut Vec<u32>,
    data: [u32; GPU_BRUSH_U32_STRIDE],
    params: [u32; GPU_BRUSH_PARAM_STRIDE],
) {
    blob.extend_from_slice(&data);
    blob.extend_from_slice(&params);
}

fn set_local_payload(data: &mut [u32; GPU_BRUSH_U32_STRIDE], len: usize) {
    data[2] = ENCODED_BRUSH_HEADER_WORDS as u32;
    data[3] = len as u32;
}

fn copy_f32_bits(dst: &mut [u32], src: &[f32]) {
    for (dst, src) in dst.iter_mut().zip(src) {
        *dst = src.to_bits();
    }
}

fn encoded_brush_words(blob: &[u32], offset: u32, len: u32) -> Option<&[u32]> {
    let start = offset as usize;
    let end = start.checked_add(len as usize)?;
    let record = blob.get(start..end)?;
    (record.len() >= ENCODED_BRUSH_HEADER_WORDS).then_some(record)
}

fn decode_params(words: &[u32]) -> [f32; GPU_BRUSH_PARAM_STRIDE] {
    let mut params = [0.0; GPU_BRUSH_PARAM_STRIDE];
    for (dst, src) in params.iter_mut().zip(words) {
        *dst = f32::from_bits(*src);
    }
    params
}

fn color_from_premul_rgba8(color: u32) -> peniko::Color {
    let [r, g, b, a] = unpack_rgba8(color);
    if a == 0 {
        return peniko::Color::TRANSPARENT;
    }
    let unpremul =
        |channel: u8| ((u16::from(channel) * 255 + u16::from(a) / 2) / u16::from(a)) as u8;
    peniko::Color::from_rgba8(unpremul(r), unpremul(g), unpremul(b), a)
}

pub(crate) fn encode_gpu_extend(extend: Extend) -> u32 {
    match extend {
        Extend::Pad => GPU_EXTEND_PAD,
        Extend::Repeat => GPU_EXTEND_REPEAT,
        Extend::Reflect => GPU_EXTEND_REFLECT,
    }
}

fn decode_gpu_extend(extend: u32) -> Extend {
    match extend {
        GPU_EXTEND_REPEAT => Extend::Repeat,
        GPU_EXTEND_REFLECT => Extend::Reflect,
        _ => Extend::Pad,
    }
}

pub(crate) fn encode_gpu_pattern_sampling(sampling: PatternSampling) -> u32 {
    match sampling {
        PatternSampling::Nearest => GPU_PATTERN_NEAREST,
        PatternSampling::Bilinear => GPU_PATTERN_BILINEAR,
    }
}

fn decode_gpu_pattern_sampling(sampling: u32) -> PatternSampling {
    match sampling {
        GPU_PATTERN_BILINEAR => PatternSampling::Bilinear,
        _ => PatternSampling::Nearest,
    }
}

fn quantize_ramp_size(span: f32, stop_count: usize) -> usize {
    let span_samples = (span.max(1.0) * GRADIENT_SPAN_QUALITY).ceil() as usize;
    let stop_samples = stop_count
        .max(2)
        .saturating_sub(1)
        .saturating_mul(GRADIENT_SAMPLES_PER_STOP_SEGMENT);
    let target = span_samples
        .max(stop_samples)
        .clamp(MIN_GRADIENT_RAMP_SIZE, DEFAULT_GRADIENT_RAMP_SIZE);
    target.next_power_of_two().min(DEFAULT_GRADIENT_RAMP_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::image::{rgba8_pack, unpack_rgba8};
    use crate::shared::image_resource::{ImageResourceResolver, ImageResourceStore};
    use peniko::{
        ColorStop, ColorStops, Gradient, GradientKind, LinearGradientPosition,
        color::{AlphaColor, ColorSpaceTag, HueDirection},
    };

    fn linear_gradient() -> Gradient {
        let stops = [
            ColorStop::from((0.0, AlphaColor::from_rgb8(255, 0, 0))),
            ColorStop::from((1.0, AlphaColor::from_rgb8(0, 0, 255))),
        ];
        Gradient {
            kind: GradientKind::Linear(LinearGradientPosition::new((0.0, 0.0), (1.0, 0.0))),
            extend: Extend::Pad,
            stops: ColorStops::from(stops.as_slice()),
            interpolation_cs: ColorSpaceTag::Srgb,
            interpolation_alpha_space: InterpolationAlphaSpace::Premultiplied,
            hue_direction: HueDirection::Shorter,
        }
    }

    fn two_pixel_pattern(
        extend: Extend,
        sampling: PatternSampling,
    ) -> (PatternBrush, ImageResourceStore) {
        let mut resources = ImageResourceStore::default();
        let key = ImageKey::new(1);
        resources.insert(
            key,
            Image {
                width: 2,
                height: 1,
                pixels: vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 0, 255, 255])],
            },
        );
        (
            PatternBrush::new_resource(
                ImageResourceId::renderer(key),
                [0.5, 0.0, 0.0, 1.0, 0.0, 0.0],
                extend,
                sampling,
                255,
            )
            .unwrap(),
            resources,
        )
    }

    #[test]
    fn pattern_bilinear_interpolates_premultiplied_pixels() {
        let (pattern, resources) = two_pixel_pattern(Extend::Pad, PatternSampling::Bilinear);
        let resolver = ImageResourceResolver::new(Some(&resources), None);

        assert_eq!(
            unpack_rgba8(pattern.sample_with_resources(1.0, 0.5, resolver)),
            [128, 0, 128, 255]
        );
    }

    #[test]
    fn pattern_bilinear_respects_pad_extend() {
        let (pattern, resources) = two_pixel_pattern(Extend::Pad, PatternSampling::Bilinear);
        let resolver = ImageResourceResolver::new(Some(&resources), None);

        assert_eq!(
            unpack_rgba8(pattern.sample_with_resources(0.25, 0.5, resolver)),
            [255, 0, 0, 255]
        );
        assert_eq!(
            unpack_rgba8(pattern.sample_with_resources(1.75, 0.5, resolver)),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn pattern_nearest_keeps_repeat_extend() {
        let (pattern, resources) = two_pixel_pattern(Extend::Repeat, PatternSampling::Nearest);
        let resolver = ImageResourceResolver::new(Some(&resources), None);

        assert_eq!(
            unpack_rgba8(pattern.sample_with_resources(-0.1, 0.5, resolver)),
            [0, 0, 255, 255]
        );
        assert_eq!(
            unpack_rgba8(pattern.sample_with_resources(2.1, 0.5, resolver)),
            [255, 0, 0, 255]
        );
    }

    #[test]
    fn from_gradient_with_ramp_size_uses_dynamic_len() {
        let brush = Brush::from_gradient_with_ramp_size(&linear_gradient(), 33);
        let Brush::Linear(gradient) = brush else {
            panic!("expected linear gradient");
        };
        assert_eq!(gradient.ramp.len(), 33);
    }

    #[test]
    fn from_gradient_automatically_estimates_small_linear_ramp() {
        let brush = Brush::from_gradient(&linear_gradient());
        let Brush::Linear(gradient) = brush else {
            panic!("expected linear gradient");
        };
        assert_eq!(gradient.ramp.len(), MIN_GRADIENT_RAMP_SIZE);
    }

    #[test]
    fn estimated_ramp_size_grows_with_span() {
        let small = estimate_linear_ramp_size([0.0, 0.0], [8.0, 0.0], 2);
        let large = estimate_linear_ramp_size([0.0, 0.0], [512.0, 0.0], 2);
        assert!(large > small, "small={small} large={large}");
    }

    #[test]
    fn estimated_ramp_size_respects_stop_density() {
        let sparse = estimate_linear_ramp_size([0.0, 0.0], [8.0, 0.0], 2);
        let dense = estimate_linear_ramp_size([0.0, 0.0], [8.0, 0.0], 12);
        assert!(dense > sparse, "sparse={sparse} dense={dense}");
    }

    #[test]
    fn encoded_brush_blob_round_trips_variable_payload_brushes() {
        let renderer_key = ImageKey::new(0x1234_5678_9abc_def0);
        let scene_key = ImageKey::new(0x2234_5678_9abc_def0);
        let brushes = [
            Brush::Solid(peniko::Color::from_rgba8(64, 128, 255, 128)),
            Brush::Pattern(
                PatternBrush::new_resource(
                    ImageResourceId::renderer(renderer_key),
                    [2.0, 0.0, 0.0, 3.0, 5.0, 7.0],
                    Extend::Reflect,
                    PatternSampling::Bilinear,
                    200,
                )
                .unwrap(),
            ),
            Brush::Pattern(
                PatternBrush::new_resource(
                    ImageResourceId::scene(scene_key),
                    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    Extend::Repeat,
                    PatternSampling::Nearest,
                    255,
                )
                .unwrap(),
            ),
        ];
        let mut blob = Vec::new();
        let ranges = brushes
            .iter()
            .map(|brush| push_encoded_brush(&mut blob, brush))
            .collect::<Vec<_>>();

        assert_eq!(
            decode_encoded_brush(&blob, ranges[0].0, ranges[0].1)
                .and_then(|brush| brush.solid_color()),
            Some(peniko::Color::from_rgba8(64, 128, 255, 128))
        );
        let Some(Brush::Pattern(resource)) = decode_encoded_brush(&blob, ranges[1].0, ranges[1].1)
        else {
            panic!("expected renderer resource pattern");
        };
        assert_eq!(resource.image_key(), Some(renderer_key));
        assert_eq!(resource.transform, [2.0, 0.0, 0.0, 3.0, 5.0, 7.0]);
        assert_eq!(resource.extend, Extend::Reflect);
        assert_eq!(resource.sampling, PatternSampling::Bilinear);
        assert_eq!(resource.opacity, 200);

        let Some(Brush::Pattern(scene)) = decode_encoded_brush(&blob, ranges[2].0, ranges[2].1)
        else {
            panic!("expected scene resource pattern");
        };
        assert_eq!(scene.image_resource_id(), ImageResourceId::scene(scene_key));
        assert_eq!(scene.extend, Extend::Repeat);
    }

    #[test]
    fn natural_pattern_maps_canvas_pixels_one_to_one() {
        let mut resources = ImageResourceStore::default();
        let key = ImageKey::new(2);
        resources.insert(
            key,
            Image {
                width: 2,
                height: 1,
                pixels: vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 255, 0, 255])],
            },
        );
        let pattern = PatternBrush::for_origin_resource(
            key,
            [10.0, 20.0],
            (2, 1),
            Extend::Pad,
            PatternSampling::Nearest,
            255,
        )
        .unwrap();
        let resolver = ImageResourceResolver::new(Some(&resources), None);
        assert_eq!(
            unpack_rgba8(pattern.sample_with_resources(10.0, 20.0, resolver)),
            [255, 0, 0, 255]
        );
        assert_eq!(
            unpack_rgba8(pattern.sample_with_resources(11.0, 20.0, resolver)),
            [0, 255, 0, 255]
        );
    }
}
