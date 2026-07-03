use std::sync::Arc;

use peniko::{
    Extend, Gradient, GradientKind, InterpolationAlphaSpace,
    color::{PremulColor, Srgb},
    kurbo,
};

use crate::shared::{
    image::Image,
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
    pub(crate) image: Arc<Image>,
    pub(crate) transform: [f32; 6],
    pub(crate) extend: Extend,
    pub(crate) sampling: PatternSampling,
    pub(crate) opacity: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternSampling {
    /// Point sampling for SVG patterns and explicit image-rendering speed/crisp hints.
    Nearest,
    /// Center-aligned bilinear sampling for default SVG raster image rendering.
    Bilinear,
}

impl Brush {
    /// Creates an image brush that scales `image` into `rect` with bilinear sampling.
    ///
    /// Returns `None` for empty images, empty rectangles, or non-finite
    /// rectangle coordinates. The brush uses pad extend, matching ordinary
    /// image drawing semantics.
    pub fn from_image(image: impl Into<Arc<Image>>, rect: kurbo::Rect) -> Option<Self> {
        Self::from_image_with_sampling(image, rect, PatternSampling::Bilinear)
    }

    /// Creates an image brush that scales `image` into `rect` with explicit sampling.
    pub fn from_image_with_sampling(
        image: impl Into<Arc<Image>>,
        rect: kurbo::Rect,
        sampling: PatternSampling,
    ) -> Option<Self> {
        Self::from_image_with_options(image, rect, Extend::Pad, sampling, 255)
    }

    /// Creates an image brush with explicit extend, sampling, and opacity.
    pub fn from_image_with_options(
        image: impl Into<Arc<Image>>,
        rect: kurbo::Rect,
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        PatternBrush::for_rect(image, rect, extend, sampling, opacity).map(Self::Pattern)
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
    pub(crate) fn sample(&self, x: f32, y: f32) -> u32 {
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
            Self::Pattern(pattern) => pattern.sample(x, y),
        }
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
    /// The transform maps scene coordinates to image pixel coordinates. Images
    /// are rejected when either dimension is zero because both CPU and wgpu
    /// samplers require at least one valid texel.
    pub fn new(
        image: impl Into<Arc<Image>>,
        transform: [f32; 6],
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        let image = image.into();
        (image.width > 0 && image.height > 0).then_some(Self {
            image,
            transform,
            extend,
            sampling,
            opacity,
        })
    }

    /// Creates a pattern brush that maps the image exactly into `rect`.
    pub fn for_rect(
        image: impl Into<Arc<Image>>,
        rect: kurbo::Rect,
        extend: Extend,
        sampling: PatternSampling,
        opacity: u8,
    ) -> Option<Self> {
        if !rect_is_valid_image_target(rect) {
            return None;
        }
        let image = image.into();
        if image.width == 0 || image.height == 0 {
            return None;
        }
        let sx = image.width as f32 / rect.width() as f32;
        let sy = image.height as f32 / rect.height() as f32;
        Self::new(
            image,
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

    fn sample(&self, x: f32, y: f32) -> u32 {
        let [x, y] = transform_point(self.transform, x, y);
        let pixel = match self.sampling {
            PatternSampling::Nearest => {
                let local_x = extend_coord(x.floor() as i32, self.image.width, self.extend);
                let local_y = extend_coord(y.floor() as i32, self.image.height, self.extend);
                self.image.pixels[(local_y * self.image.width + local_x) as usize]
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
                let tl = self.pixel_at(x0, y0);
                let tr = self.pixel_at(x0 + 1, y0);
                let bl = self.pixel_at(x0, y0 + 1);
                let br = self.pixel_at(x0 + 1, y0 + 1);
                lerp_premul_u8(lerp_premul_u8(tl, tr, tx), lerp_premul_u8(bl, br, tx), ty)
            }
        };
        scale_premul_u8(pixel, self.opacity)
    }

    fn pixel_at(&self, x: i32, y: i32) -> u32 {
        let local_x = extend_coord(x, self.image.width, self.extend);
        let local_y = extend_coord(y, self.image.height, self.extend);
        self.image.pixels[(local_y * self.image.width + local_x) as usize]
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

    fn two_pixel_pattern(extend: Extend, sampling: PatternSampling) -> PatternBrush {
        PatternBrush {
            image: Arc::new(Image {
                width: 2,
                height: 1,
                pixels: vec![rgba8_pack([255, 0, 0, 255]), rgba8_pack([0, 0, 255, 255])],
            }),
            transform: IDENTITY_TRANSFORM,
            extend,
            sampling,
            opacity: 255,
        }
    }

    #[test]
    fn pattern_bilinear_interpolates_premultiplied_pixels() {
        let pattern = two_pixel_pattern(Extend::Pad, PatternSampling::Bilinear);

        assert_eq!(unpack_rgba8(pattern.sample(1.0, 0.5)), [128, 0, 128, 255]);
    }

    #[test]
    fn pattern_bilinear_respects_pad_extend() {
        let pattern = two_pixel_pattern(Extend::Pad, PatternSampling::Bilinear);

        assert_eq!(unpack_rgba8(pattern.sample(0.25, 0.5)), [255, 0, 0, 255]);
        assert_eq!(unpack_rgba8(pattern.sample(1.75, 0.5)), [0, 0, 255, 255]);
    }

    #[test]
    fn pattern_nearest_keeps_repeat_extend() {
        let pattern = two_pixel_pattern(Extend::Repeat, PatternSampling::Nearest);

        assert_eq!(unpack_rgba8(pattern.sample(-0.1, 0.5)), [0, 0, 255, 255]);
        assert_eq!(unpack_rgba8(pattern.sample(2.1, 0.5)), [255, 0, 0, 255]);
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
}
