use std::{collections::HashMap, error::Error, fmt::Write as _, fs, path::Path};

use peniko::Color;
use tileink::{Image, TextSubpixelMode};

use super::matrix::{
    CORE_THRESHOLD, HEIGHT, MatrixTier, MatrixWeight, QualityCase, WIDTH, subpixel_name,
};

#[derive(Clone, Copy, Debug)]
struct InkBounds {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl InkBounds {
    fn width(self) -> u32 {
        self.x1 - self.x0
    }

    fn height(self) -> u32 {
        self.y1 - self.y0
    }
}

#[derive(Debug)]
pub(crate) struct Metrics {
    pub(crate) name: String,
    pub(crate) foreground: &'static str,
    pub(crate) background: &'static str,
    pub(crate) font_size: u8,
    pub(crate) weight: MatrixWeight,
    pub(crate) opacity_percent: u8,
    pub(crate) phase_thirds: u8,
    pub(crate) subpixel: TextSubpixelMode,
    pub(crate) tile_core: usize,
    pub(crate) reference_core: usize,
    pub(crate) core_ratio: f64,
    pub(crate) tile_ink: f64,
    pub(crate) reference_ink: f64,
    pub(crate) coverage_mae: f64,
    pub(crate) rgb_mae: f64,
    pub(crate) tile_fringe: f64,
    pub(crate) reference_fringe: f64,
}

#[derive(Default)]
struct CohortTotals {
    tile_core: usize,
    reference_core: usize,
    tile_ink: f64,
    reference_ink: f64,
}

pub(crate) struct CohortRanges {
    pub(crate) core_min: f64,
    pub(crate) core_max: f64,
    pub(crate) ink_min: f64,
    pub(crate) ink_max: f64,
}

pub(crate) fn cohort_ratio_ranges(metrics: &[Metrics]) -> CohortRanges {
    let mut cohorts: HashMap<(&str, &str, u8), CohortTotals> = HashMap::new();
    for item in metrics {
        let totals = cohorts
            .entry((item.foreground, item.background, item.opacity_percent))
            .or_default();
        totals.tile_core += item.tile_core;
        totals.reference_core += item.reference_core;
        totals.tile_ink += item.tile_ink;
        totals.reference_ink += item.reference_ink;
    }

    let core_ratios = cohorts
        .values()
        .filter(|totals| totals.reference_core > 0)
        .map(|totals| totals.tile_core as f64 / totals.reference_core as f64)
        .collect::<Vec<_>>();
    let ink_ratios = cohorts
        .values()
        .filter(|totals| totals.reference_ink > 1.0)
        .map(|totals| totals.tile_ink / totals.reference_ink)
        .collect::<Vec<_>>();
    CohortRanges {
        core_min: core_ratios.iter().copied().fold(f64::INFINITY, f64::min),
        core_max: core_ratios
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max),
        ink_min: ink_ratios.iter().copied().fold(f64::INFINITY, f64::min),
        ink_max: ink_ratios.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    }
}

pub(crate) fn percentile(
    metrics: &[Metrics],
    percentile: f64,
    value: impl Fn(&Metrics) -> f64,
) -> f64 {
    let mut values = metrics.iter().map(value).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    let index = ((values.len().saturating_sub(1)) as f64 * percentile).floor() as usize;
    values[index]
}

pub(crate) fn compare(
    case: &QualityCase,
    tileink: &Image,
    directwrite: &Image,
) -> (Metrics, Image) {
    let background = case.background.rgb;
    let foreground = case.foreground.rgb;
    let detected_tile_bounds = ink_bounds(tileink, background);
    let detected_reference_bounds = ink_bounds(directwrite, background);
    let measurable = detected_tile_bounds.is_some() && detected_reference_bounds.is_some();
    // Extremely low-contrast translucent pairs can differ from the background by at most one
    // channel level. Keep them in raw RGB metrics, but use the full surface for both renderers so
    // an undetectable bbox on one side cannot create a false alignment advantage.
    let (tile_bounds, reference_bounds) = match (detected_tile_bounds, detected_reference_bounds) {
        (Some(tile), Some(reference)) => (tile, reference),
        _ => {
            let full = InkBounds {
                x0: 0,
                y0: 0,
                x1: WIDTH,
                y1: HEIGHT,
            };
            (full, full)
        }
    };
    let width = tile_bounds.width().max(reference_bounds.width());
    let height = tile_bounds.height().max(reference_bounds.height());
    let mut tile_core = 0;
    let mut reference_core = 0;
    let mut tile_ink = 0.0;
    let mut reference_ink = 0.0;
    let mut coverage_error = 0.0;
    let mut rgb_error = 0.0;
    let mut tile_fringe = 0.0;
    let mut reference_fringe = 0.0;
    let mut diff = Image::new(WIDTH, HEIGHT, Color::from_rgb8(24, 24, 27));
    let mut samples = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let tile = aligned_pixel(tileink, tile_bounds, x, y, background);
            let reference = aligned_pixel(directwrite, reference_bounds, x, y, background);
            let tile_coverage = projected_coverage(tile, foreground, background);
            let reference_coverage = projected_coverage(reference, foreground, background);
            samples.push((x, y, tile, reference, tile_coverage, reference_coverage));
        }
    }

    // Compare every translucent raster against the same requested source opacity. Normalizing each
    // renderer by its own strongest pixel would conceal a globally weak or over-bold result.
    let opacity = f64::from(case.opacity_percent) / 100.0;
    for (x, y, tile, reference, raw_tile_coverage, raw_reference_coverage) in samples {
        let tile_coverage = if !measurable {
            0.0
        } else {
            (raw_tile_coverage / opacity).clamp(0.0, 1.0)
        };
        let reference_coverage = if !measurable {
            0.0
        } else {
            (raw_reference_coverage / opacity).clamp(0.0, 1.0)
        };
        tile_core += usize::from(tile_coverage >= CORE_THRESHOLD);
        reference_core += usize::from(reference_coverage >= CORE_THRESHOLD);
        tile_ink += tile_coverage;
        reference_ink += reference_coverage;
        coverage_error += (tile_coverage - reference_coverage).abs();
        rgb_error += tile
            .iter()
            .zip(reference)
            .map(|(left, right)| (*left as f64 - right as f64).abs() / 255.0)
            .sum::<f64>()
            / 3.0;
        tile_fringe += fringe(tile, foreground, background, raw_tile_coverage);
        reference_fringe += fringe(reference, foreground, background, raw_reference_coverage);

        if x < WIDTH && y < HEIGHT {
            let amplified: [u8; 3] = std::array::from_fn(|channel| {
                (tile[channel].abs_diff(reference[channel]) as u16 * 4).min(255) as u8
            });
            diff.pixels[(y * WIDTH + x) as usize] =
                pack_rgba8([amplified[0], amplified[1], amplified[2], 255]);
        }
    }

    let count = (width * height) as f64;
    let core_ratio = if reference_core == 0 {
        1.0
    } else {
        tile_core as f64 / reference_core as f64
    };
    (
        Metrics {
            name: case.id(),
            foreground: case.foreground.name,
            background: case.background.name,
            font_size: case.font_size,
            weight: case.weight,
            opacity_percent: case.opacity_percent,
            phase_thirds: case.phase_thirds,
            subpixel: case.subpixel,
            tile_core,
            reference_core,
            core_ratio,
            tile_ink,
            reference_ink,
            coverage_mae: coverage_error / count,
            rgb_mae: rgb_error / count,
            tile_fringe: tile_fringe / count,
            reference_fringe: reference_fringe / count,
        },
        diff,
    )
}

fn ink_bounds(image: &Image, background: [u8; 3]) -> Option<InkBounds> {
    let mut bounds = InkBounds {
        x0: image.width,
        y0: image.height,
        x1: 0,
        y1: 0,
    };
    let mut found = false;
    for y in 0..image.height {
        for x in 0..image.width {
            let pixel = rgb_at(image, x, y);
            let distance = pixel
                .iter()
                .zip(background)
                .map(|(channel, bg)| channel.abs_diff(bg) as u32)
                .sum::<u32>();
            if distance > 1 {
                found = true;
                bounds.x0 = bounds.x0.min(x);
                bounds.y0 = bounds.y0.min(y);
                bounds.x1 = bounds.x1.max(x + 1);
                bounds.y1 = bounds.y1.max(y + 1);
            }
        }
    }
    found.then_some(bounds)
}

fn aligned_pixel(image: &Image, bounds: InkBounds, x: u32, y: u32, background: [u8; 3]) -> [u8; 3] {
    if x >= bounds.width() || y >= bounds.height() {
        return background;
    }
    rgb_at(image, bounds.x0 + x, bounds.y0 + y)
}

fn rgb_at(image: &Image, x: u32, y: u32) -> [u8; 3] {
    let rgba = image.rgba8_at(x, y);
    [rgba[0], rgba[1], rgba[2]]
}

fn projected_coverage(pixel: [u8; 3], foreground: [u8; 3], background: [u8; 3]) -> f64 {
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for channel in 0..3 {
        let axis = foreground[channel] as f64 - background[channel] as f64;
        numerator += (pixel[channel] as f64 - background[channel] as f64) * axis;
        denominator += axis * axis;
    }
    if denominator == 0.0 {
        0.0
    } else {
        (numerator / denominator).clamp(0.0, 1.0)
    }
}

fn fringe(pixel: [u8; 3], foreground: [u8; 3], background: [u8; 3], coverage: f64) -> f64 {
    let squared = (0..3)
        .map(|channel| {
            let expected = background[channel] as f64
                + coverage * (foreground[channel] as f64 - background[channel] as f64);
            let residual = pixel[channel] as f64 - expected;
            residual * residual
        })
        .sum::<f64>();
    (squared / 3.0).sqrt() / 255.0
}

pub(crate) fn contact_row(tileink: &Image, directwrite: &Image, diff: &Image) -> Image {
    let gap = 2;
    let width = WIDTH * 3 + gap * 2;
    let mut row = Image::new(width, HEIGHT, Color::from_rgb8(82, 82, 91));
    blit(&mut row, tileink, 0, 0);
    blit(&mut row, directwrite, WIDTH + gap, 0);
    blit(&mut row, diff, WIDTH * 2 + gap * 2, 0);
    row
}

pub(crate) fn stack_rows(rows: &[Image]) -> Image {
    let gap = 2;
    let width = rows.first().map_or(0, |row| row.width);
    let height = rows.len() as u32 * HEIGHT + rows.len().saturating_sub(1) as u32 * gap;
    let mut sheet = Image::new(width, height, Color::from_rgb8(63, 63, 70));
    for (index, row) in rows.iter().enumerate() {
        blit(&mut sheet, row, 0, index as u32 * (HEIGHT + gap));
    }
    sheet
}

fn blit(destination: &mut Image, source: &Image, x: u32, y: u32) {
    for source_y in 0..source.height {
        let source_start = (source_y * source.width) as usize;
        let destination_start = ((y + source_y) * destination.width + x) as usize;
        destination.pixels[destination_start..destination_start + source.width as usize]
            .copy_from_slice(&source.pixels[source_start..source_start + source.width as usize]);
    }
}

pub(crate) fn write_metrics(
    output_dir: &Path,
    tier: MatrixTier,
    metrics: &[Metrics],
    embolden: Option<f32>,
) -> Result<(), Box<dyn Error>> {
    let mut csv = String::from(
        "case,foreground,background,font_size,weight,opacity_percent,phase_thirds,subpixel,tile_core,directwrite_core,core_ratio,tile_ink,directwrite_ink,coverage_mae,rgb_mae,tile_fringe,directwrite_fringe\n",
    );
    for item in metrics {
        writeln!(
            csv,
            "{},{},{},{},{},{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
            item.name,
            item.foreground,
            item.background,
            item.font_size,
            item.weight.name(),
            item.opacity_percent,
            item.phase_thirds,
            subpixel_name(item.subpixel),
            item.tile_core,
            item.reference_core,
            item.core_ratio,
            item.tile_ink,
            item.reference_ink,
            item.coverage_mae,
            item.rgb_mae,
            item.tile_fringe,
            item.reference_fringe,
        )?;
    }
    if let Some(value) = embolden {
        writeln!(csv, "# TILEINK_TEXT_EMBOLDEN={value}")?;
    }
    writeln!(csv, "# TILEINK_TEXT_MATRIX_TIER={}", tier.name())?;
    fs::write(output_dir.join("metrics.csv"), csv)?;
    Ok(())
}

fn pack_rgba8(rgba: [u8; 4]) -> u32 {
    u32::from_le_bytes(rgba)
}
