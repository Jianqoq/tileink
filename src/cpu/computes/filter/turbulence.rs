use super::*;

pub(super) fn image(
    width: u32,
    height: u32,
    bounds: Bounds,
    region: Bounds,
    turbulence: &Turbulence,
) -> Image {
    let mut image = Image::new(width, height, peniko::Color::TRANSPARENT);
    if region.is_empty() {
        return image;
    }

    let lattice = turbulence_lattice(turbulence.seed);
    for y in region.y0..region.y1 {
        let local_y = (y - bounds.y0) as u32;
        for x in region.x0..region.x1 {
            let local_x = (x - bounds.x0) as u32;
            image.pixels[(local_y * width + local_x) as usize] =
                pixel(x as f32, y as f32, turbulence, &lattice);
        }
    }
    image
}

fn pixel(
    x: f32,
    y: f32,
    turbulence: &Turbulence,
    lattice: &crate::shared::layer::filter::TurbulenceLattice,
) -> u32 {
    if turbulence.scale_x.abs() <= f32::EPSILON || turbulence.scale_y.abs() <= f32::EPSILON {
        return 0;
    }
    let sample_x = (x - turbulence.transform_x) / turbulence.scale_x;
    let sample_y = (y - turbulence.transform_y) / turbulence.scale_y;
    let tile_x = x - turbulence.tile_x;
    let tile_y = y - turbulence.tile_y;
    let mut stitch = turbulence
        .stitch_tiles
        .then(|| Stitch::new(tile_x, tile_y, turbulence));
    let mut frequency_x = stitch
        .as_ref()
        .map_or(turbulence.base_frequency_x, |stitch| stitch.frequency_x);
    let mut frequency_y = stitch
        .as_ref()
        .map_or(turbulence.base_frequency_y, |stitch| stitch.frequency_y);
    let mut ratio = 1.0;
    let mut out = [0.0; 4];

    for _ in 0..turbulence.num_octaves {
        let sample_x = sample_x * frequency_x;
        let sample_y = sample_y * frequency_y;
        for (channel, out_channel) in out.iter_mut().enumerate() {
            let noise = noise2(channel, sample_x, sample_y, stitch.as_ref(), lattice);
            *out_channel += match turbulence.kind {
                TurbulenceKind::Turbulence => noise.abs() * ratio,
                TurbulenceKind::FractalNoise => noise * ratio,
            };
        }
        frequency_x *= 2.0;
        frequency_y *= 2.0;
        ratio *= 0.5;
        if let Some(stitch) = &mut stitch {
            stitch.double_frequency();
        }
    }

    if turbulence.kind == TurbulenceKind::FractalNoise {
        for channel in &mut out {
            *channel = *channel * 0.5 + 0.5;
        }
    }

    for channel in &mut out {
        *channel = channel.clamp(0.0, 1.0);
    }
    if turbulence.linear_rgb {
        for channel in &mut out[..3] {
            *channel = linear_rgb_to_srgb(*channel);
        }
    }
    let alpha = out[3];
    pack_premul_rgba8([out[0] * alpha, out[1] * alpha, out[2] * alpha, alpha])
}

fn linear_rgb_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

#[derive(Clone, Copy)]
struct Stitch {
    frequency_x: f32,
    frequency_y: f32,
    width: i32,
    height: i32,
    wrap_x: i32,
    wrap_y: i32,
}

impl Stitch {
    fn new(tile_x: f32, tile_y: f32, turbulence: &Turbulence) -> Self {
        let tile_width = turbulence.tile_width.max(1.0);
        let tile_height = turbulence.tile_height.max(1.0);
        let frequency_x = stitch_frequency(turbulence.base_frequency_x, tile_width);
        let frequency_y = stitch_frequency(turbulence.base_frequency_y, tile_height);
        let width = (tile_width * frequency_x + 0.5) as i32;
        let height = (tile_height * frequency_y + 0.5) as i32;
        Self {
            frequency_x,
            frequency_y,
            width,
            height,
            wrap_x: (tile_x * frequency_x + 4096.0 + width as f32) as i32,
            wrap_y: (tile_y * frequency_y + 4096.0 + height as f32) as i32,
        }
    }

    fn double_frequency(&mut self) {
        self.width *= 2;
        self.height *= 2;
        self.wrap_x = 2 * self.wrap_x - 4096;
        self.wrap_y = 2 * self.wrap_y - 4096;
    }
}

fn stitch_frequency(frequency: f32, tile_size: f32) -> f32 {
    if frequency <= 0.0 || tile_size <= 0.0 {
        return 0.0;
    }
    let low = (tile_size * frequency).floor() / tile_size;
    let high = (tile_size * frequency).ceil() / tile_size;
    if low != 0.0 && frequency / low < high / frequency {
        low
    } else {
        high
    }
}

fn noise2(
    channel: usize,
    x: f32,
    y: f32,
    stitch: Option<&Stitch>,
    lattice: &crate::shared::layer::filter::TurbulenceLattice,
) -> f32 {
    let (bx0, bx1, rx0, rx1) =
        turbulence_axis(x, stitch.map(|stitch| (stitch.wrap_x, stitch.width)));
    let (by0, by1, ry0, ry1) =
        turbulence_axis(y, stitch.map(|stitch| (stitch.wrap_y, stitch.height)));
    let i = lattice.selectors[bx0 as usize] as usize;
    let j = lattice.selectors[bx1 as usize] as usize;
    let b00 = lattice.selectors[i + by0 as usize] as usize;
    let b10 = lattice.selectors[j + by0 as usize] as usize;
    let b01 = lattice.selectors[i + by1 as usize] as usize;
    let b11 = lattice.selectors[j + by1 as usize] as usize;
    let sx = turbulence_curve(rx0);
    let sy = turbulence_curve(ry0);
    let a = lerp(
        gradient_dot(&lattice.gradients, channel, b00, rx0, ry0),
        gradient_dot(&lattice.gradients, channel, b10, rx1, ry0),
        sx,
    );
    let b = lerp(
        gradient_dot(&lattice.gradients, channel, b01, rx0, ry1),
        gradient_dot(&lattice.gradients, channel, b11, rx1, ry1),
        sx,
    );
    lerp(a, b, sy)
}

fn turbulence_axis(value: f32, stitch: Option<(i32, i32)>) -> (i32, i32, f32, f32) {
    let t = value + 4096.0;
    let mut b0 = t.floor() as i32;
    let mut b1 = b0 + 1;
    let r0 = t - b0 as f32;
    let r1 = r0 - 1.0;
    if let Some((wrap, width)) = stitch {
        if b0 >= wrap {
            b0 -= width;
        }
        if b1 >= wrap {
            b1 -= width;
        }
    }
    (
        b0 & (TURBULENCE_LATTICE_SIZE as i32 - 1),
        b1 & (TURBULENCE_LATTICE_SIZE as i32 - 1),
        r0,
        r1,
    )
}

fn turbulence_curve(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn gradient_dot(gradients: &[f32], channel: usize, selector: usize, x: f32, y: f32) -> f32 {
    debug_assert!(selector < TURBULENCE_TABLE_LEN);
    let ix = turbulence_gradient_index(channel, selector);
    gradients[ix] * x + gradients[ix + 1] * y
}
