use super::ProgressiveBlurQuality;

pub(super) struct Level {
    pub size: [u32; 2],
    pub scale: f32,
    pub variance: f32,
    pub step: u32,
    pub kernel: Vec<[f32; 2]>,
}

/// Build a denser scale space before discarding spatial resolution. Keeping the
/// prefilter wider than a reduced texel suppresses the phase-dependent edge
/// shapes of the former binomial pyramid. Sigma <= 1 is evaluated directly.
pub(super) fn levels(size: [u32; 2], sigma: f32, quality: ProgressiveBlurQuality) -> Vec<Level> {
    let (octave_steps, min_sigma, support) = match quality {
        ProgressiveBlurQuality::Balanced => (2.0, 1.5, 3.0),
        ProgressiveBlurQuality::High => (3.0, 2.0, 4.0),
    };
    let mut levels = vec![Level {
        size,
        scale: 1.0,
        variance: 0.0,
        step: 1,
        kernel: vec![],
    }];
    if sigma <= 1.0 {
        return levels;
    }
    let mut raw_variance = 0.0;
    let mut target_sigma = 1.0_f32;
    while levels.last().unwrap().variance < sigma * sigma {
        let previous = levels.last().unwrap();
        let step = if target_sigma >= min_sigma * previous.scale * 2.0 {
            2
        } else {
            1
        };
        let scale = previous.scale * step as f32;
        let reconstruction = if scale == 1.0 {
            0.0
        } else {
            (2.0 * scale * scale + 1.0) / 12.0
        };
        let incremental = ((target_sigma * target_sigma - reconstruction - raw_variance)
            / (previous.scale * previous.scale))
            .sqrt();
        let (kernel, variance) = gaussian_kernel(incremental, step, support);
        raw_variance += variance * previous.scale * previous.scale;
        levels.push(Level {
            size: previous.size.map(|n| n.div_ceil(step)),
            scale,
            variance: raw_variance + reconstruction,
            step,
            kernel,
        });
        target_sigma *= 2.0_f32.powf(1.0 / octave_steps);
    }
    levels
}

/// Offsets are relative to the integer source base. Decimation uses half-pixel
/// centers; the measured discrete variance calibrates both kernels consistently.
fn gaussian_kernel(sigma: f32, step: u32, support: f32) -> (Vec<[f32; 2]>, f32) {
    let radius = (sigma * support).ceil() as i32;
    let center = (step - 1) as f32 * 0.5;
    let mut taps: Vec<[f32; 2]> = (-radius..=radius + step as i32 - 1)
        .map(|x| {
            [
                x as f32,
                (-0.5 * ((x as f32 - center) / sigma).powi(2)).exp(),
            ]
        })
        .collect();
    let total: f32 = taps.iter().map(|p| p[1]).sum();
    let mut variance = 0.0;
    for tap in &mut taps {
        tap[1] /= total;
        variance += tap[1] * (tap[0] - center).powi(2);
    }
    // A hardware bilinear fetch exactly combines adjacent positive weights.
    // Pair only after measuring the unpaired discrete kernel's variance.
    let paired = taps
        .chunks(2)
        .map(|pair| {
            let weight: f32 = pair.iter().map(|t| t[1]).sum();
            let offset = pair.iter().map(|t| t[0] * t[1]).sum::<f32>() / weight;
            [offset, weight]
        })
        .collect();
    (paired, variance)
}
