//! Scalar f64 SVG noise oracle, independent of GPU upload and shader helpers.
//! Uses the reference Park–Miller generator and bilinear cubic interpolation.
//! <https://www.w3.org/TR/SVG11/filters.html#feTurbulenceElement>
pub struct Noise {
    permutation: [usize; 256],
    gradients: [[[f64; 2]; 256]; 4],
}

impl Noise {
    pub fn new(seed: i32) -> Self {
        let mut state = if seed <= 0 {
            -(i64::from(seed) % 2_147_483_646) + 1
        } else {
            i64::from(seed).min(2_147_483_646)
        };
        let mut random = || {
            state = state * 16_807 % 2_147_483_647;
            state
        };
        let mut gradients = [[[0.0; 2]; 256]; 4];
        for channel in &mut gradients {
            for gradient in channel {
                let x = (random() % 512 - 256) as f64;
                let y = (random() % 512 - 256) as f64;
                let length = x.hypot(y);
                if length != 0.0 {
                    *gradient = [x / length, y / length];
                }
            }
        }
        let mut permutation = std::array::from_fn(|index| index);
        for i in (1..256).rev() {
            permutation.swap(i, random() as usize % 256);
        }
        Self {
            permutation,
            gradients,
        }
    }

    fn perlin(&self, channel: usize, point: [f64; 2]) -> f64 {
        let cell = point.map(|v| v.floor() as i32);
        let relative = [point[0] - f64::from(cell[0]), point[1] - f64::from(cell[1])];
        let weights = relative.map(|t| t * t * (3.0 - 2.0 * t));
        let mut result = 0.0;
        for dy in 0..2 {
            for dx in 0..2 {
                let column = self.permutation[((cell[0] + dx) & 255) as usize];
                let selector = self.permutation[(column + ((cell[1] + dy) & 255) as usize) % 256];
                let [gx, gy] = self.gradients[channel][selector];
                let dot = gx * (relative[0] - f64::from(dx)) + gy * (relative[1] - f64::from(dy));
                let wx = if dx == 0 {
                    1.0 - weights[0]
                } else {
                    weights[0]
                };
                let wy = if dy == 0 {
                    1.0 - weights[1]
                } else {
                    weights[1]
                };
                result += dot * wx * wy;
            }
        }
        result
    }

    pub fn pixel(
        &self,
        point: [f64; 2],
        frequency: f64,
        octaves: u32,
        fractal: bool,
        linear: bool,
    ) -> [u8; 4] {
        let mut value = [0.0; 4];
        let mut frequency = frequency;
        let mut amplitude = 1.0;
        for _ in 0..octaves {
            for (channel, accumulated) in value.iter_mut().enumerate() {
                let noise = self.perlin(channel, point.map(|v| v * frequency));
                *accumulated += if fractal {
                    noise * amplitude
                } else {
                    noise.abs() * amplitude
                };
            }
            frequency *= 2.0;
            amplitude *= 0.5;
        }
        value = value.map(|v| (if fractal { v * 0.5 + 0.5 } else { v }).clamp(0.0, 1.0));
        let alpha = value[3];
        for channel in &mut value[..3] {
            if linear {
                *channel = if *channel > 0.0031308 {
                    1.055 * channel.powf(1.0 / 2.4) - 0.055
                } else {
                    *channel * 12.92
                };
            }
            *channel *= alpha;
        }
        value.map(|v| (v * 255.0).round() as u8)
    }
}

#[test]
fn noise_lattice_and_constant_cases_have_analytic_colors() {
    // Park–Miller seed 1 starts with 16807, 282475249: the first
    // unnormalized SVG gradient is (167, -15).
    let first = Noise::new(0).gradients[0][0];
    let length = 167.0f64.hypot(-15.0);
    assert_eq!(first, [167.0 / length, -15.0 / length]);
    for seed in [i32::MIN, -7, 0, 123, i32::MAX] {
        let noise = Noise::new(seed);
        for point in [[0.0, 0.0], [-17.0, 23.0], [256.0, -256.0]] {
            assert_eq!(noise.pixel(point, 1.0, 5, false, false), [0; 4]);
            assert_eq!(noise.pixel(point, 1.0, 5, true, false), [64, 64, 64, 128]);
        }
        assert_eq!(noise.pixel([0.25, 0.75], 0.05, 0, false, true), [0; 4]);
        assert_eq!(
            noise.pixel([0.25, 0.75], 0.05, 0, true, false),
            [64, 64, 64, 128]
        );
    }
}

#[test]
fn unit_gradients_have_closed_form_interpolation_and_octave_colors() {
    let mut noise = Noise::new(1);
    noise.gradients = [[[1.0, 0.0]; 256]; 4];
    // Identical gradients reduce Perlin noise to x - smoothstep(x).
    // Doubling x=.25/.75 reaches .5 and then integers, adding zero.
    for octaves in [1, 3] {
        for x in [0.25, 0.75] {
            assert_eq!(
                noise.pixel([x, -0.25], 1.0, octaves, false, false),
                [2, 2, 2, 24]
            );
        }
        assert_eq!(
            noise.pixel([0.25, -0.25], 1.0, octaves, true, false),
            [76, 76, 76, 139]
        );
        assert_eq!(
            noise.pixel([0.75, -0.25], 1.0, octaves, true, false),
            [52, 52, 52, 116]
        );
    }
}
