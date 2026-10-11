mod mesh;
pub use mesh::{MeshGradient, MeshGradientError, MeshInterpolation};

use std::rc::Rc;

use peniko::{
    Extend, Gradient, GradientKind, InterpolationAlphaSpace,
    color::{PremulColor, Srgb},
    kurbo,
};

use crate::shared::{
    gpu_layout::brush::{
        GPU_BRUSH_FOUR_CORNER, GPU_BRUSH_LINEAR, GPU_BRUSH_MESH, GPU_BRUSH_PARAM_STRIDE,
        GPU_BRUSH_PATTERN_RESOURCE, GPU_BRUSH_PROGRAM, GPU_BRUSH_RADIAL, GPU_BRUSH_SOLID,
        GPU_BRUSH_SWEEP, GPU_BRUSH_U32_STRIDE, GPU_EXTEND_PAD, GPU_EXTEND_REFLECT,
        GPU_EXTEND_REPEAT, GPU_PATTERN_BILINEAR, GPU_PATTERN_NEAREST,
    },
    image::unpack_rgba8,
    image_resource::{ImageKey, ImageResourceId},
    pixel::premul_f32_to_u32,
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
    Mesh(MeshGradient),
    Program(super::shader::ProgramBrush),
    Pattern(PatternBrush),
}

#[derive(Clone, Debug)]
pub struct LinearGradient {
    pub(crate) start: [f32; 2],
    pub(crate) end: [f32; 2],
    pub(crate) transform: [f32; 6],
    pub(crate) extend: Extend,
    pub(crate) ramp: Rc<[u32]>,
}

#[derive(Clone, Debug)]
pub struct RadialGradient {
    pub(crate) start_center: [f32; 2],
    pub(crate) end_center: [f32; 2],
    pub(crate) start_radius: f32,
    pub(crate) end_radius: f32,
    pub(crate) transform: [f32; 6],
    pub(crate) extend: Extend,
    pub(crate) ramp: Rc<[u32]>,
}

#[derive(Clone, Debug)]
pub struct SweepGradient {
    pub(crate) center: [f32; 2],
    /// Orientation in [-PI, PI]; GPU evaluation needs at most one turn correction.
    pub(crate) start_angle: f32,
    /// Kept separately so normalizing the orientation cannot change the sweep.
    pub(crate) angle_span: f32,
    pub(crate) extend: Extend,
    pub(crate) ramp: Rc<[u32]>,
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

    /// Creates a gradient brush with an automatically sized color ramp.
    ///
    /// # Panics
    /// Panics if a sweep gradient has a nonfinite start or end angle.
    pub fn from_gradient(gradient: &Gradient) -> Self {
        Self::from_gradient_with_ramp_size(gradient, estimate_gradient_ramp_size(gradient))
    }

    /// Creates a gradient brush with the requested color ramp size.
    ///
    /// # Panics
    /// Panics if a sweep gradient has a nonfinite start or end angle.
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
            GradientKind::Sweep(position) => {
                let start = f64::from(position.start_angle);
                let end = f64::from(position.end_angle);
                assert!(
                    start.is_finite() && end.is_finite(),
                    "sweep gradient angles must be finite"
                );
                // Reduce once in wide precision, rather than iterating toward a
                // potentially huge start on every GPU pixel. Keep the original
                // signed span; subtracting normalized endpoints loses full turns.
                let orientation = start.rem_euclid(std::f64::consts::TAU);
                let orientation = if orientation > std::f64::consts::PI {
                    orientation - std::f64::consts::TAU
                } else {
                    orientation
                };
                Self::Sweep(SweepGradient {
                    center: [position.center.x as f32, position.center.y as f32],
                    start_angle: orientation as f32,
                    angle_span: (end - start) as f32,
                    extend: gradient.extend,
                    ramp,
                })
            }
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
            params[3] = gradient.angle_span.to_bits();
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
        Brush::Mesh(gradient) => {
            data[0] = GPU_BRUSH_MESH;
            data[4] = gradient.bins[0];
            data[5] = gradient.bins[1];
            data[6] = gradient.patch_base;
            data[7] = u32::from(gradient.interpolation == MeshInterpolation::Smooth);
            copy_f32_bits(&mut params[..6], &gradient.transform);
            copy_f32_bits(&mut params[6..10], &gradient.bounds);
            set_local_payload(&mut data, gradient.payload.len());
            push_encoded_header(blob, data, params);
            blob.extend_from_slice(&gradient.payload);
            return (offset, blob.len() as u32 - offset);
        }
        Brush::Program(brush) => return brush.0.encode(blob),
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
            Brush::Mesh(gradient) => gradient.payload.len(),
            Brush::Program(brush) => brush.0.word_len() - ENCODED_BRUSH_HEADER_WORDS,
            Brush::Pattern(_) => 3,
        }
}

pub(crate) fn decode_encoded_brush(blob: &[u32], offset: u32, len: u32) -> Option<Brush> {
    let start = offset as usize;
    let record = blob.get(start..start.checked_add(len as usize)?)?;
    let header = record.get(..ENCODED_BRUSH_HEADER_WORDS)?;
    let data = &header[..GPU_BRUSH_U32_STRIDE];
    // Header validation guarantees every fixed parameter. Variable payloads and
    // encoded descriptor values still require validation below: unknown values are
    // malformed records, not alternate spellings of the default enum variant.
    let params: [f32; GPU_BRUSH_PARAM_STRIDE] =
        std::array::from_fn(|i| f32::from_bits(header[GPU_BRUSH_U32_STRIDE + i]));
    let payload_start = data[2] as usize;
    let payload = record.get(payload_start..payload_start.checked_add(data[3] as usize)?)?;
    match data[0] {
        GPU_BRUSH_SOLID => Some(Brush::Solid(color_from_premul_rgba8(data[4]))),
        GPU_BRUSH_LINEAR => Some(Brush::Linear(LinearGradient {
            start: [params[0], params[1]],
            end: [params[2], params[3]],
            transform: std::array::from_fn(|i| params[4 + i]),
            extend: decode_gpu_extend(data[1])?,
            ramp: Rc::from(payload),
        })),
        GPU_BRUSH_RADIAL => Some(Brush::Radial(RadialGradient {
            start_center: [params[0], params[1]],
            end_center: [params[2], params[3]],
            start_radius: params[4],
            end_radius: params[5],
            transform: std::array::from_fn(|i| params[6 + i]),
            extend: decode_gpu_extend(data[1])?,
            ramp: Rc::from(payload),
        })),
        GPU_BRUSH_SWEEP => {
            if !params[2].is_finite()
                || params[2].abs() > std::f32::consts::PI
                || params[3].is_nan()
            {
                return None;
            }
            Some(Brush::Sweep(SweepGradient {
                center: [params[0], params[1]],
                start_angle: params[2],
                angle_span: params[3],
                extend: decode_gpu_extend(data[1])?,
                ramp: Rc::from(payload),
            }))
        }
        GPU_BRUSH_FOUR_CORNER => Some(Brush::FourCorner(FourCornerGradient {
            bounds: std::array::from_fn(|i| params[i]),
            colors: payload.try_into().ok()?,
        })),
        GPU_BRUSH_MESH => {
            let bins = [data[4], data[5]];
            let bin_count = bins[0].checked_mul(bins[1])? as usize;
            let patch_base = data[6] as usize;
            if bins
                .iter()
                .any(|&v| v == 0 || v > mesh::MESH_MAX_BINS as u32)
                || patch_base < bin_count + 1
                || patch_base > payload.len()
                || !(payload.len() - patch_base).is_multiple_of(mesh::MESH_PATCH_WORDS)
                || payload.first().copied()? as usize != bin_count + 1
                || payload.get(bin_count).copied()? as usize != patch_base
            {
                return None;
            }
            Some(Brush::Mesh(MeshGradient {
                transform: std::array::from_fn(|i| params[i]),
                bounds: std::array::from_fn(|i| params[6 + i]),
                bins,
                patch_base: data[6],
                interpolation: match data[7] {
                    0 => MeshInterpolation::Bilinear,
                    1 => MeshInterpolation::Smooth,
                    _ => return None,
                },
                payload: Rc::from(payload),
            }))
        }
        GPU_BRUSH_PROGRAM => Some(Brush::Program(super::shader::ProgramBrush(
            super::shader::BrushInstance::decode(payload, std::array::from_fn(|i| params[i]))?,
        ))),
        GPU_BRUSH_PATTERN_RESOURCE => {
            if payload.len() != 3 {
                return None;
            }
            Some(Brush::Pattern(PatternBrush {
                image: PatternImage::Resource(ImageResourceId::decode(
                    payload[0], payload[1], payload[2],
                )?),
                transform: std::array::from_fn(|i| params[i]),
                extend: decode_gpu_extend(data[1])?,
                sampling: decode_gpu_pattern_sampling(data[8])?,
                opacity: data[7].try_into().ok()?,
            }))
        }
        _ => None,
    }
}

impl From<MeshGradient> for Brush {
    fn from(value: MeshGradient) -> Self {
        Self::Mesh(value)
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
    /// are rejected when either dimension is zero because sampled resources
    /// require at least one valid texel.
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

fn build_ramp(gradient: &Gradient, ramp_size: usize) -> Rc<[u32]> {
    let ramp_size = ramp_size.max(2);
    let mut ramp = vec![0_u32; ramp_size];
    if gradient.stops.is_empty() {
        return Rc::from(ramp.into_boxed_slice());
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
    Rc::from(ramp.into_boxed_slice())
}

#[inline]
fn premul_color_to_u32(color: PremulColor<Srgb>) -> u32 {
    premul_f32_to_u32(color.components)
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

fn decode_gpu_extend(extend: u32) -> Option<Extend> {
    match extend {
        GPU_EXTEND_PAD => Some(Extend::Pad),
        GPU_EXTEND_REPEAT => Some(Extend::Repeat),
        GPU_EXTEND_REFLECT => Some(Extend::Reflect),
        _ => None,
    }
}

pub(crate) fn encode_gpu_pattern_sampling(sampling: PatternSampling) -> u32 {
    match sampling {
        PatternSampling::Nearest => GPU_PATTERN_NEAREST,
        PatternSampling::Bilinear => GPU_PATTERN_BILINEAR,
    }
}

fn decode_gpu_pattern_sampling(sampling: u32) -> Option<PatternSampling> {
    match sampling {
        GPU_PATTERN_NEAREST => Some(PatternSampling::Nearest),
        GPU_PATTERN_BILINEAR => Some(PatternSampling::Bilinear),
        _ => None,
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
mod tests;
