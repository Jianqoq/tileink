//! Generates a text raster quality corpus against Windows DirectWrite.
//!
//! The harness keeps Tileink and DirectWrite on the same explicit font family,
//! then compares rendered pixels after visual-bbox alignment. That separates
//! raster quality issues from normal layout overhang and baseline differences.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet, VecDeque},
    env, fs,
    path::{Path, PathBuf},
};

use peniko::{Color, kurbo::Point};
use rayon::prelude::*;
use tileink::{
    CpuRenderer, CubeWgpuRenderer, Image, Scene, TextAttrs, TextCompositeMode, TextContext,
    TextCoverageParams, TextFamily, TextLayoutOptions, TextRasterOptions, TextSubpixelMode,
};

#[cfg(not(all(windows, feature = "directwrite-reference")))]
compile_error!("text_quality requires Windows and --features directwrite-reference");

#[cfg(all(windows, feature = "directwrite-reference"))]
mod directwrite {
    use super::{Image, QualityCase, TextSubpixelMode, pack_rgba8};
    use std::ptr;
    use windows::{
        Win32::{
            Graphics::{
                Direct2D::{
                    Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
                    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
                    D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
                    D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                    D2D1CreateFactory, ID2D1Factory,
                },
                DirectWrite::{
                    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_PIXEL_GEOMETRY_BGR,
                    DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_PIXEL_GEOMETRY_RGB,
                    DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
                    DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC, DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
                },
                Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                Imaging::{
                    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
                    WICBitmapCacheOnLoad,
                },
            },
            System::Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            },
        },
        core::PCWSTR,
    };
    use windows_numerics::Vector2;

    pub struct DirectWriteReference {
        d2d: ID2D1Factory,
        dwrite: IDWriteFactory,
        wic: IWICImagingFactory,
    }

    impl DirectWriteReference {
        pub fn new() -> windows::core::Result<Self> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
                Ok(Self {
                    d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                    dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                    wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?,
                })
            }
        }

        pub fn render(
            &self,
            case: &QualityCase,
            font_family: &str,
            width: u32,
            height: u32,
        ) -> windows::core::Result<Image> {
            unsafe {
                let bitmap = self.wic.CreateBitmap(
                    width,
                    height,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapCacheOnLoad,
                )?;
                let target_props = D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                    minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                };
                let target = self
                    .d2d
                    .CreateWicBitmapRenderTarget(&bitmap, &target_props)?;

                let bg = color_to_d2d(case.background);
                let fg = color_to_d2d(case.foreground);
                let brush = target.CreateSolidColorBrush(&fg, None)?;
                let font_family = wide_null(font_family);
                let locale = wide_null("en-us");
                let text = wide_no_null(case.text);
                let format = self.dwrite.CreateTextFormat(
                    PCWSTR(font_family.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    case.font_size,
                    PCWSTR(locale.as_ptr()),
                )?;
                format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
                format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                let layout = self.dwrite.CreateTextLayout(
                    &text,
                    &format,
                    case.layout_width,
                    height as f32,
                )?;

                let (pixel_geometry, antialias_mode, rendering_mode, cleartype_level) =
                    match case.subpixel {
                        TextSubpixelMode::Rgb => (
                            DWRITE_PIXEL_GEOMETRY_RGB,
                            D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
                            DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
                            1.0,
                        ),
                        TextSubpixelMode::Bgr => (
                            DWRITE_PIXEL_GEOMETRY_BGR,
                            D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
                            DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
                            1.0,
                        ),
                        TextSubpixelMode::None => (
                            DWRITE_PIXEL_GEOMETRY_FLAT,
                            D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                            DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                            0.0,
                        ),
                    };
                let rendering_params = self.dwrite.CreateCustomRenderingParams(
                    2.2,
                    1.0,
                    cleartype_level,
                    pixel_geometry,
                    rendering_mode,
                )?;

                target.BeginDraw();
                target.SetTextAntialiasMode(antialias_mode);
                target.SetTextRenderingParams(&rendering_params);
                target.Clear(Some(&bg));
                target.DrawTextLayout(
                    Vector2 {
                        X: case.margin + case.origin_x,
                        Y: case.margin + case.origin_y,
                    },
                    &layout,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
                target.EndDraw(None, None)?;

                let stride = width * 4;
                let mut bgra = vec![0; (stride * height) as usize];
                bitmap.CopyPixels(ptr::null(), stride, &mut bgra)?;
                let pixels = bgra
                    .chunks_exact(4)
                    .map(|px| pack_rgba8([px[2], px[1], px[0], px[3]]))
                    .collect();
                Ok(Image {
                    width,
                    height,
                    pixels,
                })
            }
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }

    fn wide_no_null(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    pub fn color_to_d2d(color: peniko::Color) -> D2D1_COLOR_F {
        let [r, g, b, a] = color.components;
        D2D1_COLOR_F { r, g, b, a }
    }
}

const WIDTH: u32 = 640;
const HEIGHT: u32 = 104;
const TEXT: &str = "Illi Hnm 0123456789 Tileink text";
const DEFAULT_FONT: &str = "Segoe UI";

#[derive(Clone, Copy, Debug)]
enum Backend {
    Cpu,
    Cubecl,
    Both,
}

#[derive(Clone, Debug)]
struct ColorCase {
    name: Cow<'static, str>,
    foreground: Color,
    background: Color,
}

#[derive(Clone, Copy, Debug)]
struct OriginCase {
    name: &'static str,
    x: f32,
    y: f32,
}

#[derive(Clone, Debug)]
struct QualityCase {
    name: String,
    text: &'static str,
    font_size: f32,
    layout_width: f32,
    margin: f32,
    origin_x: f32,
    origin_y: f32,
    foreground: Color,
    background: Color,
    subpixel: TextSubpixelMode,
}

#[derive(Clone, Copy, Debug, Default)]
struct Bounds {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct ImageStats {
    bbox: Option<Bounds>,
    ink: f64,
    fringe: f64,
    fringe_pixels: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct DiffStats {
    mae_luma: f64,
    rmse_luma: f64,
    mae_rgb: f64,
    max_rgb: u8,
    ink_ratio: f64,
    tileink_ink: f64,
    reference_ink: f64,
    fringe_delta: f64,
    tileink_bbox: Option<Bounds>,
    reference_bbox: Option<Bounds>,
}

#[derive(Clone, Copy, Debug, Default)]
struct GrayAlignedDiffStats {
    pixels: u64,
    tileink_sum: f64,
    reference_sum: f64,
    ratio: f64,
    mae: f64,
    rmse: f64,
    max_delta: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct CoverageFitStats {
    pixels: u64,
    current_sum_error: f64,
    current_rmse: f64,
    scale: f64,
    scale_sum_error: f64,
    scale_rmse: f64,
    scale_rmse_reduction: f64,
    axis_scale: f64,
    axis_bias: f64,
    axis_sum_error: f64,
    axis_rmse: f64,
    axis_rmse_reduction: f64,
    exponent: f64,
    exponent_scale: f64,
    exponent_sum_error: f64,
    exponent_rmse: f64,
    exponent_rmse_reduction: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct ExponentFitAccum {
    sum: f64,
    sum_sq: f64,
    sum_ref: f64,
}

// Streaming residual model used by the quality harness. It estimates how much
// error remains if a whole color group is allowed to use an ideal coverage
// scale, axis affine correction, or exponent curve; a poor residual means the
// failure is likely glyph shape or placement instead of global compositing.
#[derive(Clone, Debug, Default)]
struct CoverageFitAccum {
    pixels: u64,
    sum_tile: f64,
    sum_reference: f64,
    sum_tile_sq: f64,
    sum_reference_sq: f64,
    sum_tile_reference: f64,
    exponent: [ExponentFitAccum; COVERAGE_FIT_EXPONENTS.len()],
}

#[derive(Clone, Debug)]
struct MetricRow {
    backend: String,
    font_size: String,
    subpixel: String,
    fg: String,
    bg: String,
    stats: DiffStats,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct SummaryKey {
    backend: String,
    font_size: String,
    subpixel: String,
    fg: String,
    bg: String,
}

#[derive(Clone, Debug)]
struct SummaryRow {
    key: SummaryKey,
    cases: usize,
    avg_ink_ratio: f64,
    avg_abs_ink_error: f64,
    avg_mae_luma: f64,
    avg_rmse_luma: f64,
    avg_mae_rgb: f64,
    max_rgb: u8,
    avg_fringe_delta: f64,
}

#[derive(Default)]
struct SummaryAccum {
    cases: usize,
    ink_ratio: f64,
    abs_ink_error: f64,
    mae_luma: f64,
    rmse_luma: f64,
    mae_rgb: f64,
    max_rgb: u8,
    fringe_delta: f64,
}

#[derive(Debug)]
struct Options {
    backend: Backend,
    out_dir: PathBuf,
    font_family: String,
    full: bool,
    color_sweep: bool,
    tune_coverage: bool,
    tune_iterations: usize,
    tune_seed: u64,
    tune_candidates: Option<PathBuf>,
    tune_parallelism: usize,
    tune_color_groups: usize,
    tune_target_score: Option<f64>,
    dump_case: Option<String>,
    dump_coverage_results: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse()?;
    let cases = if options.tune_coverage {
        build_cases_with_color_group_target(options.full, true, Some(options.tune_color_groups))
    } else {
        build_cases(options.full, options.color_sweep)
    };
    fs::create_dir_all(&options.out_dir)?;

    let directwrite = directwrite::DirectWriteReference::new()?;
    if let Some(case_name) = &options.dump_case {
        let cases = build_cases_with_color_group_target(
            options.full,
            true,
            Some(options.tune_color_groups),
        );
        return run_case_dump(&options, &cases, &directwrite, case_name);
    }
    if options.tune_coverage {
        return run_coverage_tuning(&options, &cases, &directwrite);
    }

    let tileink_dir = options.out_dir.join(match options.backend {
        Backend::Cpu => "tileink_cpu",
        Backend::Cubecl => "tileink_cubecl",
        Backend::Both => "tileink_cpu",
    });
    let cubecl_dir = options.out_dir.join("tileink_cubecl");
    let reference_dir = options.out_dir.join("reference");
    let diff_dir = options.out_dir.join("diff");
    for dir in [&tileink_dir, &cubecl_dir, &reference_dir, &diff_dir] {
        fs::create_dir_all(dir)?;
    }

    let mut csv = String::from(
        "case,backend,font_size,subpixel,fg,bg,origin,tileink_ink,reference_ink,ink_ratio,mae_luma,rmse_luma,mae_rgb,max_rgb,fringe_delta,tileink_bbox,reference_bbox\n",
    );
    let mut metrics = Vec::new();
    let backend_count = match options.backend {
        Backend::Both => 2,
        Backend::Cpu | Backend::Cubecl => 1,
    };
    let mut contact_sheet = ContactSheet::new(cases.len() * backend_count);
    let mut output_state = OutputState {
        diff_dir: &diff_dir,
        csv: &mut csv,
        metrics: &mut metrics,
        contact_sheet: &mut contact_sheet,
    };

    for case in &cases {
        let reference = directwrite.render(case, &options.font_family, WIDTH, HEIGHT)?;
        let reference_path = reference_dir.join(format!("{}.png", case.name));
        reference.save(&reference_path)?;

        match options.backend {
            Backend::Cpu | Backend::Both => {
                let tileink = render_tileink(case, &options.font_family, Backend::Cpu);
                let diff = diff_images(&tileink, &reference, case.background);
                output_state.write_case(&tileink, &reference, &diff, case, "cpu", &tileink_dir)?;
            }
            Backend::Cubecl => {}
        }

        match options.backend {
            Backend::Cubecl | Backend::Both => {
                let tileink = render_tileink(case, &options.font_family, Backend::Cubecl);
                let diff = diff_images(&tileink, &reference, case.background);
                output_state.write_case(
                    &tileink,
                    &reference,
                    &diff,
                    case,
                    "cubecl",
                    &cubecl_dir,
                )?;
            }
            Backend::Cpu => {}
        }
    }

    fs::write(
        options.out_dir.join("metrics.csv"),
        output_state.csv.as_str(),
    )?;
    let summary = summarize_metrics(output_state.metrics);
    fs::write(options.out_dir.join("summary.csv"), summary_csv(&summary))?;
    output_state
        .contact_sheet
        .save(&options.out_dir.join("contact_sheet.png"))?;
    println!(
        "Wrote {} cases to {}",
        cases.len(),
        options.out_dir.display()
    );
    println!("Glyph raster backend: swash");
    println!("Metrics: {}", options.out_dir.join("metrics.csv").display());
    println!("Summary: {}", options.out_dir.join("summary.csv").display());
    println!("Reference: {}", reference_dir.display());
    println!(
        "Contact sheet: {}",
        options.out_dir.join("contact_sheet.png").display()
    );
    print_summary(&summary);
    Ok(())
}

fn run_case_dump(
    options: &Options,
    cases: &[QualityCase],
    directwrite: &directwrite::DirectWriteReference,
    case_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let case = cases
        .iter()
        .find(|case| case.name == case_name)
        .ok_or_else(|| format!("dump case {case_name:?} was not generated"))?;
    let coverage_params = dump_coverage_params(options)?;
    let dump_dir = options.out_dir.join(case_name);
    fs::create_dir_all(&dump_dir)?;

    let reference = directwrite.render(case, &options.font_family, WIDTH, HEIGHT)?;
    let mut context = TextContext::new();
    let tileink = render_tileink_with_context(
        case,
        &options.font_family,
        Backend::Cpu,
        &mut context,
        coverage_params,
    );
    let diff = diff_images(&tileink, &reference, case.background);
    save_dump_image_set(
        &dump_dir,
        "reference",
        &reference,
        diff.stats.reference_bbox,
    )?;
    save_dump_image_set(&dump_dir, "tileink", &tileink, diff.stats.tileink_bbox)?;
    save_dump_image_set(&dump_dir, "diff", &diff.image, dump_bounds(diff.stats))?;
    let aligned_reference = aligned_reference_to_tileink(&reference, diff.stats, case.background);
    save_dump_image_set(
        &dump_dir,
        "reference_aligned_to_tileink",
        &aligned_reference,
        diff.stats.tileink_bbox,
    )?;

    let reference_coverage = apparent_coverage_image(&reference, case.foreground, case.background);
    let tileink_coverage = apparent_coverage_image(&tileink, case.foreground, case.background);
    let coverage_delta = coverage_delta_image(&tileink_coverage, &reference_coverage, diff.stats);
    let aligned_reference_coverage =
        aligned_reference_to_tileink(&reference_coverage, diff.stats, Color::BLACK);
    let coverage_stats =
        gray_aligned_diff_stats(&tileink_coverage, &reference_coverage, diff.stats);
    save_dump_image_set(
        &dump_dir,
        "reference_apparent_coverage",
        &reference_coverage,
        diff.stats.reference_bbox,
    )?;
    save_dump_image_set(
        &dump_dir,
        "tileink_apparent_coverage",
        &tileink_coverage,
        diff.stats.tileink_bbox,
    )?;
    save_dump_image_set(
        &dump_dir,
        "reference_apparent_coverage_aligned_to_tileink",
        &aligned_reference_coverage,
        diff.stats.tileink_bbox,
    )?;
    save_dump_image_set(
        &dump_dir,
        "coverage_delta_over_red_under_blue",
        &coverage_delta,
        dump_bounds(diff.stats),
    )?;

    let neutral_case = QualityCase {
        foreground: Color::WHITE,
        background: Color::BLACK,
        ..case.clone()
    };
    let neutral_reference =
        directwrite.render(&neutral_case, &options.font_family, WIDTH, HEIGHT)?;
    let neutral_tileink = render_tileink_with_context(
        &neutral_case,
        &options.font_family,
        Backend::Cpu,
        &mut context,
        coverage_params,
    );
    let neutral_diff = diff_images(&neutral_tileink, &neutral_reference, Color::BLACK);
    let aligned_neutral_reference =
        aligned_reference_to_tileink(&neutral_reference, neutral_diff.stats, Color::BLACK);
    let neutral_gray_stats =
        gray_aligned_diff_stats(&neutral_tileink, &neutral_reference, neutral_diff.stats);
    save_dump_image_set(
        &dump_dir,
        "reference_white_on_black",
        &neutral_reference,
        neutral_diff.stats.reference_bbox,
    )?;
    save_dump_image_set(
        &dump_dir,
        "tileink_white_on_black",
        &neutral_tileink,
        neutral_diff.stats.tileink_bbox,
    )?;
    save_dump_image_set(
        &dump_dir,
        "reference_white_on_black_aligned_to_tileink",
        &aligned_neutral_reference,
        neutral_diff.stats.tileink_bbox,
    )?;
    save_dump_image_set(
        &dump_dir,
        "white_on_black_diff",
        &neutral_diff.image,
        dump_bounds(neutral_diff.stats),
    )?;

    fs::write(
        dump_dir.join("metrics.txt"),
        format!(
            "case: {case_name}\nfont: {}\nfont_size: {:.1}\nsubpixel: {}\nfg: {}\nbg: {}\norigin: {}\ncoverage_params: {}\n\noriginal tileink_ink: {:.6}\noriginal reference_ink: {:.6}\noriginal robust_ink_error: {:.8}\noriginal mae_luma: {:.8}\noriginal mae_rgb: {:.8}\noriginal max_rgb: {}\noriginal tileink_bbox: {}\noriginal reference_bbox: {}\noriginal raw_bbox_delta: {}\noriginal aligned_apparent_coverage_pixels: {}\noriginal aligned_apparent_coverage_tileink_sum: {:.6}\noriginal aligned_apparent_coverage_reference_sum: {:.6}\noriginal aligned_apparent_coverage_error: {:.8}\noriginal aligned_apparent_coverage_ratio: {:.6}\noriginal aligned_apparent_coverage_mae: {:.8}\noriginal aligned_apparent_coverage_rmse: {:.8}\noriginal aligned_apparent_coverage_max_delta: {}\n\nwhite_on_black tileink_ink: {:.6}\nwhite_on_black reference_ink: {:.6}\nwhite_on_black robust_ink_error: {:.8}\nwhite_on_black mae_luma: {:.8}\nwhite_on_black mae_rgb: {:.8}\nwhite_on_black max_rgb: {}\nwhite_on_black tileink_bbox: {}\nwhite_on_black reference_bbox: {}\nwhite_on_black raw_bbox_delta: {}\nwhite_on_black aligned_gray_pixels: {}\nwhite_on_black aligned_gray_tileink_sum: {:.6}\nwhite_on_black aligned_gray_reference_sum: {:.6}\nwhite_on_black aligned_gray_error: {:.8}\nwhite_on_black aligned_gray_ratio: {:.6}\nwhite_on_black aligned_gray_mae: {:.8}\nwhite_on_black aligned_gray_rmse: {:.8}\nwhite_on_black aligned_gray_max_delta: {}\n",
            options.font_family,
            case.font_size,
            mode_name(case.subpixel),
            color_name(case.foreground),
            color_name(case.background),
            origin_name(case.origin_x, case.origin_y),
            options
                .dump_coverage_results
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "default".to_string()),
            diff.stats.tileink_ink,
            diff.stats.reference_ink,
            tuning_ink_error(diff.stats),
            diff.stats.mae_luma,
            diff.stats.mae_rgb,
            diff.stats.max_rgb,
            bbox_csv(diff.stats.tileink_bbox),
            bbox_csv(diff.stats.reference_bbox),
            bbox_delta_csv(diff.stats),
            coverage_stats.pixels,
            coverage_stats.tileink_sum,
            coverage_stats.reference_sum,
            aligned_sum_error(coverage_stats),
            coverage_stats.ratio,
            coverage_stats.mae,
            coverage_stats.rmse,
            coverage_stats.max_delta,
            neutral_diff.stats.tileink_ink,
            neutral_diff.stats.reference_ink,
            tuning_ink_error(neutral_diff.stats),
            neutral_diff.stats.mae_luma,
            neutral_diff.stats.mae_rgb,
            neutral_diff.stats.max_rgb,
            bbox_csv(neutral_diff.stats.tileink_bbox),
            bbox_csv(neutral_diff.stats.reference_bbox),
            bbox_delta_csv(neutral_diff.stats),
            neutral_gray_stats.pixels,
            neutral_gray_stats.tileink_sum,
            neutral_gray_stats.reference_sum,
            aligned_sum_error(neutral_gray_stats),
            neutral_gray_stats.ratio,
            neutral_gray_stats.mae,
            neutral_gray_stats.rmse,
            neutral_gray_stats.max_delta,
        ),
    )?;

    println!("Dumped {case_name} to {}", dump_dir.display());
    println!("Metrics: {}", dump_dir.join("metrics.txt").display());
    Ok(())
}

fn dump_coverage_params(
    options: &Options,
) -> Result<TextCoverageParams, Box<dyn std::error::Error>> {
    let Some(path) = &options.dump_coverage_results else {
        return Ok(TextCoverageParams::DEFAULT);
    };
    let results = load_coverage_tuning_results(path)?;
    best_coverage_result(&results)
        .map(|result| result.candidate.params)
        .ok_or_else(|| format!("{} has no coverage tuning rows", path.display()).into())
}

struct OutputState<'a> {
    diff_dir: &'a Path,
    csv: &'a mut String,
    metrics: &'a mut Vec<MetricRow>,
    contact_sheet: &'a mut ContactSheet,
}

impl OutputState<'_> {
    fn write_case(
        &mut self,
        tileink: &Image,
        reference: &Image,
        diff: &DiffResult,
        case: &QualityCase,
        backend: &str,
        tileink_dir: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let suffix = if backend == "cpu" {
            case.name.clone()
        } else {
            format!("{}_{}", backend, case.name)
        };
        tileink.save(tileink_dir.join(format!("{suffix}.png")))?;
        diff.image
            .save(self.diff_dir.join(format!("{}_{}.png", backend, case.name)))?;
        append_metrics_csv(self.csv, case, backend, &diff.stats);
        self.metrics.push(MetricRow {
            backend: backend.to_string(),
            font_size: format!("{:.1}", case.font_size),
            subpixel: mode_name(case.subpixel).to_string(),
            fg: color_name(case.foreground),
            bg: color_name(case.background),
            stats: diff.stats,
        });
        self.contact_sheet.push(tileink, reference, &diff.image);
        Ok(())
    }
}

fn summarize_metrics(rows: &[MetricRow]) -> Vec<SummaryRow> {
    let mut groups = BTreeMap::<SummaryKey, SummaryAccum>::new();
    for row in rows {
        let key = SummaryKey {
            backend: row.backend.clone(),
            font_size: row.font_size.clone(),
            subpixel: row.subpixel.clone(),
            fg: row.fg.clone(),
            bg: row.bg.clone(),
        };
        let accum = groups.entry(key).or_default();
        accum.cases += 1;
        accum.ink_ratio += row.stats.ink_ratio;
        accum.abs_ink_error += (row.stats.ink_ratio - 1.0).abs();
        accum.mae_luma += row.stats.mae_luma;
        accum.rmse_luma += row.stats.rmse_luma;
        accum.mae_rgb += row.stats.mae_rgb;
        accum.max_rgb = accum.max_rgb.max(row.stats.max_rgb);
        accum.fringe_delta += row.stats.fringe_delta;
    }

    groups
        .into_iter()
        .map(|(key, accum)| {
            let cases = accum.cases as f64;
            SummaryRow {
                key,
                cases: accum.cases,
                avg_ink_ratio: accum.ink_ratio / cases,
                avg_abs_ink_error: accum.abs_ink_error / cases,
                avg_mae_luma: accum.mae_luma / cases,
                avg_rmse_luma: accum.rmse_luma / cases,
                avg_mae_rgb: accum.mae_rgb / cases,
                max_rgb: accum.max_rgb,
                avg_fringe_delta: accum.fringe_delta / cases,
            }
        })
        .collect()
}

fn summary_csv(rows: &[SummaryRow]) -> String {
    let mixed_ranks = worst_mixed_ranks(rows);
    let mut csv = String::from(
        "backend,font_size,subpixel,fg,bg,cases,avg_ink_ratio,avg_abs_ink_error,avg_mae_luma,avg_rmse_luma,avg_mae_rgb,max_rgb,avg_fringe_delta,color_group,worst_mixed_rank\n",
    );
    for row in rows {
        let mixed_rank = mixed_ranks
            .get(&row.key)
            .map(|rank| rank.to_string())
            .unwrap_or_default();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{:.6},{:.6},{:.8},{:.8},{:.8},{},{:.8},{},{}\n",
            row.key.backend.as_str(),
            row.key.font_size.as_str(),
            row.key.subpixel.as_str(),
            row.key.fg.as_str(),
            row.key.bg.as_str(),
            row.cases,
            row.avg_ink_ratio,
            row.avg_abs_ink_error,
            row.avg_mae_luma,
            row.avg_rmse_luma,
            row.avg_mae_rgb,
            row.max_rgb,
            row.avg_fringe_delta,
            color_group(row),
            mixed_rank,
        ));
    }
    csv
}

fn worst_mixed_ranks(rows: &[SummaryRow]) -> BTreeMap<SummaryKey, usize> {
    let mut mixed = rows
        .iter()
        .filter(|row| is_mixed_color_group(row))
        .collect::<Vec<_>>();
    mixed.sort_by(|a, b| {
        b.avg_abs_ink_error
            .partial_cmp(&a.avg_abs_ink_error)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.key.cmp(&b.key))
    });
    mixed
        .into_iter()
        .enumerate()
        .map(|(ix, row)| (row.key.clone(), ix + 1))
        .collect()
}

fn color_group(row: &SummaryRow) -> &'static str {
    if is_mixed_color_group(row) {
        "mixed"
    } else {
        "other"
    }
}

fn is_mixed_color_group(row: &SummaryRow) -> bool {
    is_mixed_color_key(&row.key)
}

fn is_mixed_color_key(key: &SummaryKey) -> bool {
    !is_reference_neutral_color(&key.fg) && !is_reference_neutral_color(&key.bg)
}

fn is_reference_neutral_color(color: &str) -> bool {
    matches!(color, "#000000ff" | "#ffffffff" | "#e0e0e0ff" | "#303030ff")
}

fn print_summary(rows: &[SummaryRow]) {
    let mut worst = rows.to_vec();
    worst.sort_by(|a, b| {
        b.avg_mae_luma
            .partial_cmp(&a.avg_mae_luma)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    println!("Worst summary groups by mae_luma:");
    for row in worst.iter().take(8) {
        println!(
            "  {backend:>5} {size:>4}px {subpixel:>4} {fg} on {bg}: ink={ink:.3} mae={mae:.5} rgb={rgb:.5} max={max}",
            backend = row.key.backend.as_str(),
            size = row.key.font_size.as_str(),
            subpixel = row.key.subpixel.as_str(),
            fg = row.key.fg.as_str(),
            bg = row.key.bg.as_str(),
            ink = row.avg_ink_ratio,
            mae = row.avg_mae_luma,
            rgb = row.avg_mae_rgb,
            max = row.max_rgb,
        );
    }

    let mut worst_mixed = rows
        .iter()
        .filter(|row| is_mixed_color_group(row))
        .collect::<Vec<_>>();
    worst_mixed.sort_by(|a, b| {
        b.avg_abs_ink_error
            .partial_cmp(&a.avg_abs_ink_error)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.key.cmp(&b.key))
    });
    println!("Worst mixed summary groups by abs_ink_error:");
    if worst_mixed.is_empty() {
        println!("  (none)");
    }
    for row in worst_mixed.iter().take(8) {
        println!(
            "  {backend:>5} {size:>4}px {subpixel:>4} {fg} on {bg}: abs_ink={abs_ink:.3} ink={ink:.3} mae={mae:.5} rgb={rgb:.5} max={max}",
            backend = row.key.backend.as_str(),
            size = row.key.font_size.as_str(),
            subpixel = row.key.subpixel.as_str(),
            fg = row.key.fg.as_str(),
            bg = row.key.bg.as_str(),
            abs_ink = row.avg_abs_ink_error,
            ink = row.avg_ink_ratio,
            mae = row.avg_mae_luma,
            rgb = row.avg_mae_rgb,
            max = row.max_rgb,
        );
    }
}

#[derive(Clone)]
struct ReferenceCase {
    case: QualityCase,
    reference: Image,
}

#[derive(Clone)]
struct CoverageCandidate {
    name: String,
    source: String,
    params: TextCoverageParams,
}

#[derive(Clone)]
struct CoverageTuneResult {
    candidate: CoverageCandidate,
    score: f64,
    case_count: usize,
    mixed_group_count: usize,
    avg_mae_luma: f64,
    avg_mae_rgb: f64,
    avg_abs_ink_error: f64,
    mixed_avg_abs_ink_error: f64,
    worst_mixed_abs_ink_error: f64,
    worst_group_abs_ink_error: f64,
    avg_abs_fringe_delta: f64,
    max_rgb: u8,
}

#[derive(Default)]
struct CoverageGroupAccum {
    cases: usize,
    robust_ink_error: f64,
    mae_luma: f64,
    mae_rgb: f64,
    tileink_ink: f64,
    reference_ink: f64,
    abs_fringe_delta: f64,
    max_rgb: u8,
    reference_floor_hits: usize,
    fg_luma: f64,
    bg_luma: f64,
    fg_chroma: f64,
    bg_chroma: f64,
    fit: CoverageFitAccum,
    abs_bbox_dx: f64,
    abs_bbox_dy: f64,
    worst_case: String,
    worst_case_robust_ink_error: f64,
    worst_case_tileink_ink: f64,
    worst_case_reference_ink: f64,
    worst_case_bbox_dx: i32,
    worst_case_bbox_dy: i32,
}

#[derive(Clone)]
struct CoverageGroupDetail {
    key: SummaryKey,
    cases: usize,
    avg_robust_ink_error: f64,
    avg_mae_luma: f64,
    avg_mae_rgb: f64,
    avg_tileink_ink: f64,
    avg_reference_ink: f64,
    avg_abs_fringe_delta: f64,
    max_rgb: u8,
    reference_floor_hit_rate: f64,
    fg_luma: f64,
    bg_luma: f64,
    fg_chroma: f64,
    bg_chroma: f64,
    fit: CoverageFitStats,
    avg_abs_bbox_dx: f64,
    avg_abs_bbox_dy: f64,
    worst_case: String,
    worst_case_robust_ink_error: f64,
    worst_case_tileink_ink: f64,
    worst_case_reference_ink: f64,
    worst_case_bbox_dx: i32,
    worst_case_bbox_dy: i32,
}

#[derive(Clone, Copy)]
struct CoverageObjectiveMetrics {
    avg_mae_luma: f64,
    avg_mae_rgb: f64,
    avg_abs_ink_error: f64,
    mixed_avg_abs_ink_error: f64,
    worst_mixed_abs_ink_error: f64,
    worst_group_abs_ink_error: f64,
    avg_abs_fringe_delta: f64,
    max_rgb: u8,
}

#[derive(Clone, Copy)]
struct CoverageParamRange {
    name: &'static str,
    min: f32,
    max: f32,
}

const COVERAGE_PARAM_COUNT: usize = 25;
const COVERAGE_PARAM_RANGES: [CoverageParamRange; COVERAGE_PARAM_COUNT] = [
    CoverageParamRange {
        name: "dark_on_light_coverage_strength",
        min: 0.45,
        max: 0.95,
    },
    CoverageParamRange {
        name: "dark_on_light_luma_base",
        min: 0.95,
        max: 1.95,
    },
    CoverageParamRange {
        name: "dark_on_light_luma_taper",
        min: 0.25,
        max: 1.15,
    },
    CoverageParamRange {
        name: "dark_on_light_chroma_boost",
        min: 0.0,
        max: 1.0,
    },
    CoverageParamRange {
        name: "source_chroma_coverage_boost",
        min: 0.0,
        max: 1.2,
    },
    CoverageParamRange {
        name: "source_chroma_coverage_contrast_limit",
        min: 0.08,
        max: 0.45,
    },
    CoverageParamRange {
        name: "light_on_dark_coverage_reduction",
        min: 0.03,
        max: 0.35,
    },
    CoverageParamRange {
        name: "light_on_dark_black_luma_limit",
        min: 0.005,
        max: 0.06,
    },
    CoverageParamRange {
        name: "light_on_dark_chroma_reduction",
        min: 0.04,
        max: 0.55,
    },
    CoverageParamRange {
        name: "light_on_dark_high_luma_chroma_reduction",
        min: 0.0,
        max: 0.45,
    },
    CoverageParamRange {
        name: "light_on_dark_high_luma_threshold",
        min: 0.25,
        max: 0.80,
    },
    CoverageParamRange {
        name: "light_on_colored_dark_chroma_reduction",
        min: 0.25,
        max: 1.35,
    },
    CoverageParamRange {
        name: "light_on_colored_dark_luma_limit",
        min: 0.08,
        max: 0.45,
    },
    CoverageParamRange {
        name: "alpha_mask_chroma_scale",
        min: 1.0,
        max: 2.6,
    },
    CoverageParamRange {
        name: "subpixel_mask_chroma_scale",
        min: 0.65,
        max: 1.35,
    },
    CoverageParamRange {
        name: "alpha_mask_apparent_axis_strength",
        min: 0.0,
        max: 2.5,
    },
    CoverageParamRange {
        name: "alpha_mask_apparent_axis_luma_limit",
        min: 0.05,
        max: 0.75,
    },
    CoverageParamRange {
        name: "subpixel_mask_apparent_axis_strength",
        min: 0.0,
        max: 1.5,
    },
    CoverageParamRange {
        name: "subpixel_mask_apparent_axis_luma_limit",
        min: 0.05,
        max: 0.75,
    },
    CoverageParamRange {
        name: "alpha_mask_embolden",
        min: 0.0,
        max: 0.18,
    },
    CoverageParamRange {
        name: "subpixel_mask_embolden",
        min: 0.0,
        max: 0.10,
    },
    CoverageParamRange {
        name: "alpha_mask_low_luma_chroma_reduction",
        min: 0.0,
        max: 12.0,
    },
    CoverageParamRange {
        name: "alpha_mask_low_luma_contrast_limit",
        min: 0.03,
        max: 0.30,
    },
    CoverageParamRange {
        name: "subpixel_mask_low_luma_chroma_reduction",
        min: 0.0,
        max: 6.0,
    },
    CoverageParamRange {
        name: "subpixel_mask_low_luma_contrast_limit",
        min: 0.03,
        max: 0.30,
    },
];
const TUNING_INK_REFERENCE_FLOOR: f64 = 100.0;
// Apparent coverage is a good ink proxy for neutral and high-luma-contrast
// text. Low-luma-contrast saturated color pairs are mostly separated by hue,
// so tiny RGB differences can look like large coverage-ratio errors.
const LOW_LUMA_CONTRAST_FLOOR: f64 = 0.04;
const LOW_LUMA_CONTRAST_CEILING: f64 = 0.30;
const CHROMATIC_COVERAGE_FLOOR: f64 = 0.18;
const CHROMATIC_COVERAGE_CEILING: f64 = 0.55;
const MIN_COVERAGE_RELIABILITY: f64 = 0.22;
// Small fixed search grid keeps residual diagnostics deterministic and cheap
// while still exposing whether a simple coverage curve could explain a group.
const COVERAGE_FIT_EXPONENTS: [f64; 15] = [
    0.45, 0.55, 0.65, 0.75, 0.85, 0.95, 1.0, 1.1, 1.25, 1.45, 1.7, 2.0, 2.4, 2.9, 3.5,
];

fn run_coverage_tuning(
    options: &Options,
    cases: &[QualityCase],
    directwrite: &directwrite::DirectWriteReference,
) -> Result<(), Box<dyn std::error::Error>> {
    let references = build_tuning_references(cases, directwrite, &options.font_family)?;
    let reference_mib =
        references.len() as f64 * f64::from(WIDTH) * f64::from(HEIGHT) * 4.0 / (1024.0 * 1024.0);
    println!(
        "Coverage tuning: {} cases, target {} evaluations, {} workers, reference cache ~= {:.1} MiB",
        references.len(),
        options.tune_iterations,
        options.tune_parallelism,
        reference_mib
    );
    println!("Glyph raster backend: swash");
    if let Some(target_score) = options.tune_target_score {
        println!("Target score: {target_score:.6}");
    }
    println!("Candidate images are rendered, diffed, and dropped inside each worker.");

    let mut seeds = builtin_coverage_candidates();
    if let Some(path) = &options.tune_candidates {
        for candidate in load_coverage_candidates(path)? {
            seeds.push_back(candidate);
        }
    }
    let target = options.tune_iterations;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.tune_parallelism)
        .build()?;
    let mut rng = SmallRng::new(options.tune_seed);
    let csv_path = options.out_dir.join("coverage_tuning.csv");
    let best_path = options.out_dir.join("coverage_best_params.rs");
    let mut results = if csv_path.exists() {
        let loaded = dedupe_coverage_results(load_coverage_tuning_results(&csv_path)?);
        if !loaded.is_empty() {
            println!(
                "Resuming {} evaluated candidates from {}",
                loaded.len(),
                csv_path.display()
            );
        }
        loaded
    } else {
        Vec::new()
    };
    let mut seen_params = results
        .iter()
        .map(|result| result.candidate.params)
        .collect::<HashSet<_>>();

    while results.len() < target && !coverage_target_reached(&results, options.tune_target_score) {
        let batch_len = (options.tune_parallelism * 2)
            .clamp(1, 16)
            .min(target - results.len());
        let mut batch = Vec::with_capacity(batch_len);
        while batch.len() < batch_len {
            if let Some(candidate) = seeds.pop_front() {
                if seen_params.insert(candidate.params) {
                    batch.push(candidate);
                }
                continue;
            }
            let ix = results.len() + batch.len() + 1;
            let mut candidate = propose_coverage_candidate(ix, &results, &mut rng);
            while !seen_params.insert(candidate.params) {
                candidate = propose_coverage_candidate(ix, &results, &mut rng);
            }
            batch.push(candidate);
        }

        let evaluated = pool.install(|| {
            batch
                .par_iter()
                .map(|candidate| {
                    evaluate_coverage_candidate(candidate, &references, &options.font_family)
                })
                .collect::<Vec<_>>()
        });
        results.extend(evaluated);
        fs::write(&csv_path, coverage_tuning_csv(&results))?;
        if let Some(best) = best_coverage_result(&results) {
            fs::write(&best_path, coverage_params_rust(best.candidate.params))?;
            println!(
                "  evaluated {:>4}/{target}: best score={:.6} name={} source={} mixed_coverage={:.5} worst_mixed={:.5} avg_coverage={:.5}",
                results.len(),
                best.score,
                best.candidate.name,
                best.candidate.source,
                best.mixed_avg_abs_ink_error,
                best.worst_mixed_abs_ink_error,
                best.avg_abs_ink_error
            );
        }
    }

    println!("Coverage tuning CSV: {}", csv_path.display());
    println!("Best params: {}", best_path.display());
    if coverage_target_reached(&results, options.tune_target_score) {
        println!("Target score reached.");
    }
    print_coverage_tuning_top(&results, 8);
    if let Some(best) = best_coverage_result(&results) {
        let details = collect_coverage_group_details(best, &references, &options.font_family);
        let path = options.out_dir.join("coverage_best_worst_mixed_groups.csv");
        fs::write(&path, coverage_worst_mixed_groups_csv(best, &details))?;
        println!("Worst mixed groups: {}", path.display());
        print_worst_mixed_group_details(&details, 12);
    }
    Ok(())
}

fn coverage_target_reached(results: &[CoverageTuneResult], target_score: Option<f64>) -> bool {
    target_score.is_some_and(|target| {
        best_coverage_result(results).is_some_and(|best| best.score <= target)
    })
}

fn build_tuning_references(
    cases: &[QualityCase],
    directwrite: &directwrite::DirectWriteReference,
    font_family: &str,
) -> Result<Vec<ReferenceCase>, Box<dyn std::error::Error>> {
    let mut references = Vec::with_capacity(cases.len());
    for case in cases {
        references.push(ReferenceCase {
            case: case.clone(),
            reference: directwrite.render(case, font_family, WIDTH, HEIGHT)?,
        });
    }
    Ok(references)
}

fn evaluate_coverage_candidate(
    candidate: &CoverageCandidate,
    references: &[ReferenceCase],
    font_family: &str,
) -> CoverageTuneResult {
    let mut context = TextContext::new();
    let mut avg_abs_ink_error = 0.0;
    let mut avg_mae_luma = 0.0;
    let mut avg_mae_rgb = 0.0;
    let mut avg_abs_fringe_delta = 0.0;
    let mut max_rgb = 0;
    let mut group_ink_errors = BTreeMap::<SummaryKey, (f64, usize)>::new();

    for reference in references {
        let tileink = render_tileink_with_context(
            &reference.case,
            font_family,
            Backend::Cpu,
            &mut context,
            candidate.params,
        );
        let diff = diff_images(&tileink, &reference.reference, reference.case.background);
        let coverage_error =
            tuning_case_coverage_error(&reference.case, &tileink, &reference.reference, diff.stats);
        avg_abs_ink_error += coverage_error;
        avg_mae_luma += diff.stats.mae_luma;
        avg_mae_rgb += diff.stats.mae_rgb;
        avg_abs_fringe_delta += diff.stats.fringe_delta.abs();
        max_rgb = max_rgb.max(diff.stats.max_rgb);
        let summary_key = coverage_summary_key(&reference.case);
        let group = group_ink_errors.entry(summary_key).or_default();
        group.0 += coverage_error;
        group.1 += 1;
    }

    let case_count = references.len();
    let inv_cases = 1.0 / case_count as f64;
    avg_abs_ink_error *= inv_cases;
    avg_mae_luma *= inv_cases;
    avg_mae_rgb *= inv_cases;
    avg_abs_fringe_delta *= inv_cases;

    let mut mixed_group_count = 0;
    let mut mixed_group_sum = 0.0;
    let mut worst_mixed_abs_ink_error = 0.0;
    let mut worst_group_abs_ink_error = 0.0;
    for (key, (sum, count)) in &group_ink_errors {
        let group_error = sum / *count as f64;
        worst_group_abs_ink_error = f64::max(worst_group_abs_ink_error, group_error);
        if is_mixed_color_key(key) {
            mixed_group_count += 1;
            mixed_group_sum += group_error;
            worst_mixed_abs_ink_error = f64::max(worst_mixed_abs_ink_error, group_error);
        }
    }
    let mixed_avg_abs_ink_error = if mixed_group_count == 0 {
        avg_abs_ink_error
    } else {
        mixed_group_sum / mixed_group_count as f64
    };
    let score = coverage_objective(CoverageObjectiveMetrics {
        avg_mae_luma,
        avg_mae_rgb,
        avg_abs_ink_error,
        mixed_avg_abs_ink_error,
        worst_mixed_abs_ink_error,
        worst_group_abs_ink_error,
        avg_abs_fringe_delta,
        max_rgb,
    });

    CoverageTuneResult {
        candidate: candidate.clone(),
        score,
        case_count,
        mixed_group_count,
        avg_mae_luma,
        avg_mae_rgb,
        avg_abs_ink_error,
        mixed_avg_abs_ink_error,
        worst_mixed_abs_ink_error,
        worst_group_abs_ink_error,
        avg_abs_fringe_delta,
        max_rgb,
    }
}

fn collect_coverage_group_details(
    best: &CoverageTuneResult,
    references: &[ReferenceCase],
    font_family: &str,
) -> Vec<CoverageGroupDetail> {
    let mut context = TextContext::new();
    let mut groups = BTreeMap::<SummaryKey, CoverageGroupAccum>::new();
    for reference in references {
        let tileink = render_tileink_with_context(
            &reference.case,
            font_family,
            Backend::Cpu,
            &mut context,
            best.candidate.params,
        );
        let diff = diff_images(&tileink, &reference.reference, reference.case.background);
        let coverage = apparent_coverage_aligned_diff_stats(
            &tileink,
            &reference.reference,
            reference.case.foreground,
            reference.case.background,
            diff.stats,
        );
        let key = coverage_summary_key(&reference.case);
        let group = groups.entry(key).or_default();
        accumulate_apparent_coverage_fit(
            &tileink,
            &reference.reference,
            reference.case.foreground,
            reference.case.background,
            diff.stats,
            &mut group.fit,
        );
        let (bbox_dx, bbox_dy) = bbox_alignment_delta(diff.stats);
        let robust_ink_error =
            tuning_case_coverage_error(&reference.case, &tileink, &reference.reference, diff.stats);
        if group.cases == 0 {
            let fg = color_to_rgba8(reference.case.foreground);
            let bg = color_to_rgba8(reference.case.background);
            group.fg_luma = luma(fg);
            group.bg_luma = luma(bg);
            group.fg_chroma = rgb_chroma(fg);
            group.bg_chroma = rgb_chroma(bg);
        }
        group.cases += 1;
        group.robust_ink_error += robust_ink_error;
        group.mae_luma += diff.stats.mae_luma;
        group.mae_rgb += diff.stats.mae_rgb;
        group.tileink_ink += coverage.tileink_sum;
        group.reference_ink += coverage.reference_sum;
        group.abs_fringe_delta += diff.stats.fringe_delta.abs();
        group.max_rgb = group.max_rgb.max(diff.stats.max_rgb);
        group.reference_floor_hits +=
            usize::from(coverage.reference_sum < TUNING_INK_REFERENCE_FLOOR);
        group.abs_bbox_dx += f64::from(bbox_dx.abs());
        group.abs_bbox_dy += f64::from(bbox_dy.abs());
        if robust_ink_error > group.worst_case_robust_ink_error {
            group.worst_case = reference.case.name.clone();
            group.worst_case_robust_ink_error = robust_ink_error;
            group.worst_case_tileink_ink = coverage.tileink_sum;
            group.worst_case_reference_ink = coverage.reference_sum;
            group.worst_case_bbox_dx = bbox_dx;
            group.worst_case_bbox_dy = bbox_dy;
        }
    }

    let mut details = groups
        .into_iter()
        .filter(|(key, _)| is_mixed_color_key(key))
        .map(|(key, group)| {
            let inv_cases = 1.0 / group.cases as f64;
            CoverageGroupDetail {
                key,
                cases: group.cases,
                avg_robust_ink_error: group.robust_ink_error * inv_cases,
                avg_mae_luma: group.mae_luma * inv_cases,
                avg_mae_rgb: group.mae_rgb * inv_cases,
                avg_tileink_ink: group.tileink_ink * inv_cases,
                avg_reference_ink: group.reference_ink * inv_cases,
                avg_abs_fringe_delta: group.abs_fringe_delta * inv_cases,
                max_rgb: group.max_rgb,
                reference_floor_hit_rate: group.reference_floor_hits as f64 * inv_cases,
                fg_luma: group.fg_luma,
                bg_luma: group.bg_luma,
                fg_chroma: group.fg_chroma,
                bg_chroma: group.bg_chroma,
                fit: group
                    .fit
                    .finish(TUNING_INK_REFERENCE_FLOOR * group.cases as f64),
                avg_abs_bbox_dx: group.abs_bbox_dx * inv_cases,
                avg_abs_bbox_dy: group.abs_bbox_dy * inv_cases,
                worst_case: group.worst_case,
                worst_case_robust_ink_error: group.worst_case_robust_ink_error,
                worst_case_tileink_ink: group.worst_case_tileink_ink,
                worst_case_reference_ink: group.worst_case_reference_ink,
                worst_case_bbox_dx: group.worst_case_bbox_dx,
                worst_case_bbox_dy: group.worst_case_bbox_dy,
            }
        })
        .collect::<Vec<_>>();
    details.sort_by(|a, b| {
        b.avg_robust_ink_error
            .partial_cmp(&a.avg_robust_ink_error)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.key.cmp(&b.key))
    });
    details
}

fn coverage_summary_key(case: &QualityCase) -> SummaryKey {
    SummaryKey {
        backend: "cpu".to_string(),
        font_size: format!("{:.1}", case.font_size),
        subpixel: mode_name(case.subpixel).to_string(),
        fg: color_name(case.foreground),
        bg: color_name(case.background),
    }
}

fn tuning_ink_error(stats: DiffStats) -> f64 {
    (stats.tileink_ink - stats.reference_ink).abs()
        / stats.reference_ink.max(TUNING_INK_REFERENCE_FLOOR)
}

fn tuning_case_coverage_error(
    case: &QualityCase,
    tileink: &Image,
    reference: &Image,
    stats: DiffStats,
) -> f64 {
    let coverage = apparent_coverage_aligned_diff_stats(
        tileink,
        reference,
        case.foreground,
        case.background,
        stats,
    );
    if coverage.reference_sum > 0.0 {
        aligned_sum_error(coverage)
            * apparent_coverage_reliability(case.foreground, case.background)
    } else {
        tuning_ink_error(stats)
    }
}

fn apparent_coverage_reliability(foreground: Color, background: Color) -> f64 {
    let fg = color_to_rgba8(foreground);
    let bg = color_to_rgba8(background);
    let luma_contrast = (luma(fg) - luma(bg)).abs();
    let chroma = rgb_chroma(fg).max(rgb_chroma(bg));
    let contrast_reliability = smoothstep(
        LOW_LUMA_CONTRAST_FLOOR,
        LOW_LUMA_CONTRAST_CEILING,
        luma_contrast,
    );
    let neutral_reliability =
        1.0 - smoothstep(CHROMATIC_COVERAGE_FLOOR, CHROMATIC_COVERAGE_CEILING, chroma);
    MIN_COVERAGE_RELIABILITY
        + (1.0 - MIN_COVERAGE_RELIABILITY) * contrast_reliability.max(neutral_reliability)
}

fn smoothstep(edge0: f64, edge1: f64, value: f64) -> f64 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl CoverageFitAccum {
    fn add(&mut self, tileink: f64, reference: f64) {
        self.pixels += 1;
        self.sum_tile += tileink;
        self.sum_reference += reference;
        self.sum_tile_sq += tileink * tileink;
        self.sum_reference_sq += reference * reference;
        self.sum_tile_reference += tileink * reference;
        for (accum, &exponent) in self.exponent.iter_mut().zip(&COVERAGE_FIT_EXPONENTS) {
            let powered = tileink.powf(exponent);
            accum.sum += powered;
            accum.sum_sq += powered * powered;
            accum.sum_ref += powered * reference;
        }
    }

    fn finish(&self, reference_floor: f64) -> CoverageFitStats {
        if self.pixels == 0 {
            return CoverageFitStats::default();
        }

        let pixels = self.pixels as f64;
        let denom = self.sum_reference.max(reference_floor).max(1.0e-9);
        let current_sse = self.sum_tile_sq - 2.0 * self.sum_tile_reference + self.sum_reference_sq;
        let current_rmse = rmse(current_sse, pixels);
        let current_sum_error = (self.sum_tile - self.sum_reference).abs() / denom;

        let scale = fit_scale(self.sum_tile_sq, self.sum_tile_reference);
        let scale_sse = scale * scale * self.sum_tile_sq - 2.0 * scale * self.sum_tile_reference
            + self.sum_reference_sq;
        let scale_rmse = rmse(scale_sse, pixels);
        let scale_sum_error = (scale * self.sum_tile - self.sum_reference).abs() / denom;

        let (axis_scale, axis_bias) = fit_axis_affine(
            pixels,
            self.sum_tile,
            self.sum_reference,
            self.sum_tile_sq,
            self.sum_tile_reference,
        );
        let axis_sse = axis_scale * axis_scale * self.sum_tile_sq
            + 2.0 * axis_scale * axis_bias * self.sum_tile
            + axis_bias * axis_bias * pixels
            - 2.0 * axis_scale * self.sum_tile_reference
            - 2.0 * axis_bias * self.sum_reference
            + self.sum_reference_sq;
        let axis_rmse = rmse(axis_sse, pixels);
        let axis_sum_error =
            (axis_scale * self.sum_tile + axis_bias * pixels - self.sum_reference).abs() / denom;

        let (exponent, exponent_scale, exponent_sum_error, exponent_rmse) =
            self.best_exponent_fit(denom, pixels);

        CoverageFitStats {
            pixels: self.pixels,
            current_sum_error,
            current_rmse,
            scale,
            scale_sum_error,
            scale_rmse,
            scale_rmse_reduction: fit_reduction(current_rmse, scale_rmse),
            axis_scale,
            axis_bias,
            axis_sum_error,
            axis_rmse,
            axis_rmse_reduction: fit_reduction(current_rmse, axis_rmse),
            exponent,
            exponent_scale,
            exponent_sum_error,
            exponent_rmse,
            exponent_rmse_reduction: fit_reduction(current_rmse, exponent_rmse),
        }
    }

    fn best_exponent_fit(&self, denom: f64, pixels: f64) -> (f64, f64, f64, f64) {
        let mut best = (
            1.0,
            1.0,
            self.sum_reference_sq,
            self.current_identity_sum_error(denom),
        );
        for (accum, &exponent) in self.exponent.iter().zip(&COVERAGE_FIT_EXPONENTS) {
            let scale = fit_scale(accum.sum_sq, accum.sum_ref);
            let sse =
                scale * scale * accum.sum_sq - 2.0 * scale * accum.sum_ref + self.sum_reference_sq;
            if sse < best.2 {
                let sum_error = (scale * accum.sum - self.sum_reference).abs() / denom;
                best = (exponent, scale, sse, sum_error);
            }
        }
        (best.0, best.1, best.3, rmse(best.2, pixels))
    }

    fn current_identity_sum_error(&self, denom: f64) -> f64 {
        (self.sum_tile - self.sum_reference).abs() / denom
    }
}

fn fit_scale(sum_x_sq: f64, sum_x_ref: f64) -> f64 {
    if sum_x_sq <= 1.0e-12 {
        0.0
    } else {
        (sum_x_ref / sum_x_sq).clamp(0.0, 4.0)
    }
}

fn fit_axis_affine(
    pixels: f64,
    sum_tile: f64,
    sum_reference: f64,
    sum_tile_sq: f64,
    sum_tile_reference: f64,
) -> (f64, f64) {
    let denom = pixels * sum_tile_sq - sum_tile * sum_tile;
    if denom.abs() <= 1.0e-12 {
        return (fit_scale(sum_tile_sq, sum_tile_reference), 0.0);
    }
    let scale = ((pixels * sum_tile_reference - sum_tile * sum_reference) / denom).clamp(-4.0, 4.0);
    let bias = ((sum_reference - scale * sum_tile) / pixels).clamp(-1.0, 1.0);
    (scale, bias)
}

fn rmse(sse: f64, pixels: f64) -> f64 {
    (sse.max(0.0) / pixels.max(1.0)).sqrt()
}

fn fit_reduction(current: f64, fitted: f64) -> f64 {
    if current <= 1.0e-9 {
        0.0
    } else {
        ((current - fitted) / current).clamp(-1.0, 1.0)
    }
}

fn coverage_objective(metrics: CoverageObjectiveMetrics) -> f64 {
    metrics.avg_mae_luma * 10.0
        + metrics.avg_mae_rgb * 5.0
        + metrics.avg_abs_ink_error * 0.40
        + metrics.mixed_avg_abs_ink_error * 0.90
        + metrics.worst_mixed_abs_ink_error * 0.35
        + metrics.worst_group_abs_ink_error * 0.15
        + metrics.avg_abs_fringe_delta * 0.05
        + f64::from(metrics.max_rgb) * 0.00002
}

fn builtin_coverage_candidates() -> VecDeque<CoverageCandidate> {
    let default = TextCoverageParams::DEFAULT;
    let mut candidates = VecDeque::new();
    candidates.push_back(coverage_candidate("current", "builtin", default));

    let mut params = default;
    params.light_on_colored_dark_chroma_reduction = 1.05;
    params.alpha_mask_chroma_scale = 2.10;
    candidates.push_back(coverage_candidate(
        "strong_mixed_alpha_reduction",
        "builtin",
        params,
    ));

    let mut params = default;
    params.light_on_colored_dark_chroma_reduction = 1.20;
    params.light_on_colored_dark_luma_limit = 0.32;
    params.alpha_mask_chroma_scale = 2.30;
    candidates.push_back(coverage_candidate(
        "wide_colored_dark_reduction",
        "builtin",
        params,
    ));

    let mut params = default;
    params.dark_on_light_coverage_strength = 0.62;
    params.dark_on_light_luma_base = 1.25;
    candidates.push_back(coverage_candidate(
        "softer_dark_on_light",
        "builtin",
        params,
    ));

    let mut params = default;
    params.dark_on_light_coverage_strength = 0.86;
    params.dark_on_light_chroma_boost = 0.75;
    candidates.push_back(coverage_candidate(
        "strong_color_edges_on_light",
        "builtin",
        params,
    ));

    let mut params = default;
    params.light_on_dark_chroma_reduction = 0.34;
    params.light_on_dark_high_luma_chroma_reduction = 0.26;
    params.light_on_dark_high_luma_threshold = 0.42;
    candidates.push_back(coverage_candidate(
        "strong_light_color_reduction",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_apparent_axis_strength = 1.0;
    params.alpha_mask_apparent_axis_luma_limit = 0.30;
    candidates.push_back(coverage_candidate(
        "alpha_apparent_axis_compensation",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_apparent_axis_strength = 1.45;
    params.alpha_mask_apparent_axis_luma_limit = 0.36;
    params.subpixel_mask_apparent_axis_strength = 0.45;
    params.subpixel_mask_apparent_axis_luma_limit = 0.28;
    candidates.push_back(coverage_candidate(
        "wide_apparent_axis_compensation",
        "builtin",
        params,
    ));

    let mut params = default;
    params.light_on_dark_chroma_reduction = 0.34;
    params.light_on_dark_high_luma_chroma_reduction = 0.26;
    params.light_on_dark_high_luma_threshold = 0.42;
    params.alpha_mask_apparent_axis_strength = 1.2;
    params.alpha_mask_apparent_axis_luma_limit = 0.34;
    candidates.push_back(coverage_candidate(
        "light_color_apparent_axis_compensation",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_chroma_scale = 1.45;
    params.subpixel_mask_chroma_scale = 0.90;
    candidates.push_back(coverage_candidate("lower_chroma_scales", "builtin", params));

    let mut params = default;
    params.alpha_mask_embolden = 0.04;
    candidates.push_back(coverage_candidate(
        "light_alpha_embolden",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_embolden = 0.08;
    params.subpixel_mask_embolden = 0.03;
    candidates.push_back(coverage_candidate("small_text_embolden", "builtin", params));

    let mut params = default;
    params.alpha_mask_embolden = 0.12;
    params.subpixel_mask_embolden = 0.05;
    params.alpha_mask_apparent_axis_strength = 1.2;
    params.subpixel_mask_apparent_axis_strength = 0.9;
    candidates.push_back(coverage_candidate(
        "strong_text_embolden",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_chroma_scale = 2.40;
    params.subpixel_mask_chroma_scale = 1.15;
    candidates.push_back(coverage_candidate(
        "higher_chroma_scales",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_low_luma_chroma_reduction = 0.75;
    params.alpha_mask_low_luma_contrast_limit = 0.10;
    candidates.push_back(coverage_candidate(
        "alpha_low_luma_chroma_gate",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_low_luma_chroma_reduction = 1.20;
    params.alpha_mask_low_luma_contrast_limit = 0.12;
    candidates.push_back(coverage_candidate(
        "strong_alpha_low_luma_chroma_gate",
        "builtin",
        params,
    ));

    let mut params = default;
    params.alpha_mask_low_luma_chroma_reduction = 1.60;
    params.alpha_mask_low_luma_contrast_limit = 0.16;
    params.alpha_mask_chroma_scale = 1.55;
    candidates.push_back(coverage_candidate(
        "wide_alpha_low_luma_chroma_gate",
        "builtin",
        params,
    ));

    let mut params = default;
    params.dark_on_light_chroma_boost = 0.0;
    params.alpha_mask_chroma_scale = 2.10;
    params.alpha_mask_low_luma_chroma_reduction = 4.0;
    params.alpha_mask_low_luma_contrast_limit = 0.24;
    params.subpixel_mask_low_luma_chroma_reduction = 0.8;
    params.subpixel_mask_low_luma_contrast_limit = 0.18;
    candidates.push_back(coverage_candidate(
        "alpha_subpixel_low_luma_gate",
        "builtin",
        params,
    ));

    let mut params = default;
    params.dark_on_light_chroma_boost = 0.0;
    params.alpha_mask_chroma_scale = 2.10;
    params.alpha_mask_low_luma_chroma_reduction = 4.0;
    params.alpha_mask_low_luma_contrast_limit = 0.24;
    params.subpixel_mask_low_luma_chroma_reduction = 1.6;
    params.subpixel_mask_low_luma_contrast_limit = 0.22;
    candidates.push_back(coverage_candidate(
        "strong_alpha_subpixel_low_luma_gate",
        "builtin",
        params,
    ));

    let mut params = default;
    params.source_chroma_coverage_boost = 0.20;
    params.source_chroma_coverage_contrast_limit = 0.35;
    candidates.push_back(coverage_candidate("source_chroma_boost", "builtin", params));

    let mut params = default;
    params.source_chroma_coverage_boost = 0.50;
    params.source_chroma_coverage_contrast_limit = 0.40;
    candidates.push_back(coverage_candidate(
        "strong_source_chroma_boost",
        "builtin",
        params,
    ));

    let mut params = default;
    params.dark_on_light_chroma_boost = 0.0;
    params.source_chroma_coverage_boost = 0.50;
    params.source_chroma_coverage_contrast_limit = 0.40;
    params.alpha_mask_chroma_scale = 2.10;
    params.alpha_mask_low_luma_chroma_reduction = 8.0;
    params.alpha_mask_low_luma_contrast_limit = 0.24;
    params.subpixel_mask_low_luma_chroma_reduction = 2.0;
    params.subpixel_mask_low_luma_contrast_limit = 0.22;
    candidates.push_back(coverage_candidate(
        "balanced_source_and_low_luma_gate",
        "builtin",
        params,
    ));

    candidates
}

fn coverage_candidate(
    name: impl Into<String>,
    source: impl Into<String>,
    params: TextCoverageParams,
) -> CoverageCandidate {
    CoverageCandidate {
        name: name.into(),
        source: source.into(),
        params,
    }
}

fn load_coverage_candidates(
    path: &Path,
) -> Result<Vec<CoverageCandidate>, Box<dyn std::error::Error>> {
    parse_coverage_candidates_csv(&fs::read_to_string(path)?)
        .map_err(|err| format!("{}: {err}", path.display()).into())
}

fn load_coverage_tuning_results(
    path: &Path,
) -> Result<Vec<CoverageTuneResult>, Box<dyn std::error::Error>> {
    parse_coverage_tuning_results_csv(&fs::read_to_string(path)?)
        .map_err(|err| format!("{}: {err}", path.display()).into())
}

fn parse_coverage_tuning_results_csv(
    csv: &str,
) -> Result<Vec<CoverageTuneResult>, Box<dyn std::error::Error>> {
    let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
    let Some(header) = lines.next() else {
        return Ok(Vec::new());
    };
    let headers = header
        .split(',')
        .map(|field| field.trim())
        .collect::<Vec<_>>();
    let mut results = Vec::new();
    for (line_ix, line) in lines.enumerate() {
        let fields = line
            .split(',')
            .map(|field| field.trim())
            .collect::<Vec<_>>();
        let mut values = coverage_params_to_values(TextCoverageParams::DEFAULT);
        for (param_ix, range) in COVERAGE_PARAM_RANGES.iter().enumerate() {
            if csv_field(&headers, &fields, range.name).is_some() {
                values[param_ix] = csv_f32(&headers, &fields, range.name)
                    .map_err(|err| format!("line {}: {err}", line_ix + 2))?;
            }
        }
        results.push(CoverageTuneResult {
            candidate: coverage_candidate(
                csv_string(&headers, &fields, "name").unwrap_or_else(|| {
                    format!(
                        "resumed_{}",
                        csv_usize(&headers, &fields, "rank").unwrap_or(line_ix + 1)
                    )
                }),
                csv_string(&headers, &fields, "source").unwrap_or_else(|| "resume".to_string()),
                coverage_params_from_values(values),
            ),
            score: csv_f64(&headers, &fields, "score")
                .map_err(|err| format!("line {}: {err}", line_ix + 2))?,
            case_count: csv_usize(&headers, &fields, "cases").unwrap_or_default(),
            mixed_group_count: csv_usize(&headers, &fields, "mixed_groups").unwrap_or_default(),
            avg_mae_luma: csv_f64(&headers, &fields, "avg_mae_luma").unwrap_or_default(),
            avg_mae_rgb: csv_f64(&headers, &fields, "avg_mae_rgb").unwrap_or_default(),
            avg_abs_ink_error: csv_f64_any(
                &headers,
                &fields,
                &[
                    "avg_robust_coverage_error",
                    "avg_robust_ink_error",
                    "avg_abs_ink_error",
                ],
            )
            .unwrap_or_default(),
            mixed_avg_abs_ink_error: csv_f64_any(
                &headers,
                &fields,
                &[
                    "mixed_avg_robust_coverage_error",
                    "mixed_avg_robust_ink_error",
                    "mixed_avg_abs_ink_error",
                ],
            )
            .unwrap_or_default(),
            worst_mixed_abs_ink_error: csv_f64_any(
                &headers,
                &fields,
                &[
                    "worst_mixed_robust_coverage_error",
                    "worst_mixed_robust_ink_error",
                    "worst_mixed_abs_ink_error",
                ],
            )
            .unwrap_or_default(),
            worst_group_abs_ink_error: csv_f64_any(
                &headers,
                &fields,
                &[
                    "worst_group_robust_coverage_error",
                    "worst_group_robust_ink_error",
                    "worst_group_abs_ink_error",
                ],
            )
            .unwrap_or_default(),
            avg_abs_fringe_delta: csv_f64(&headers, &fields, "avg_abs_fringe_delta")
                .unwrap_or_default(),
            max_rgb: csv_u8(&headers, &fields, "max_rgb").unwrap_or_default(),
        });
    }
    Ok(results)
}

fn dedupe_coverage_results(mut results: Vec<CoverageTuneResult>) -> Vec<CoverageTuneResult> {
    results.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    results
        .into_iter()
        .filter(|result| seen.insert(result.candidate.params))
        .collect()
}

fn parse_coverage_candidates_csv(
    csv: &str,
) -> Result<Vec<CoverageCandidate>, Box<dyn std::error::Error>> {
    let mut lines = csv
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    let Some((_, header)) = lines.next() else {
        return Ok(Vec::new());
    };
    let headers = header
        .split(',')
        .map(|field| field.trim())
        .collect::<Vec<_>>();
    let name_ix = headers.iter().position(|header| *header == "name");
    let mut candidates = Vec::new();
    for (line_ix, line) in lines {
        let fields = line
            .split(',')
            .map(|field| field.trim())
            .collect::<Vec<_>>();
        let mut values = coverage_params_to_values(TextCoverageParams::DEFAULT);
        for (param_ix, range) in COVERAGE_PARAM_RANGES.iter().enumerate() {
            if let Some(field_ix) = headers.iter().position(|header| *header == range.name) {
                let Some(field) = fields.get(field_ix) else {
                    continue;
                };
                if !field.is_empty() {
                    values[param_ix] = field.parse::<f32>().map_err(|_| {
                        format!(
                            "line {} field {} must be a float, got {field:?}",
                            line_ix + 1,
                            range.name
                        )
                    })?;
                }
            }
        }
        let name = name_ix
            .and_then(|ix| fields.get(ix))
            .filter(|name| !name.is_empty())
            .map(|name| (*name).to_string())
            .unwrap_or_else(|| format!("input_line_{}", line_ix + 1));
        candidates.push(coverage_candidate(
            name,
            "input",
            coverage_params_from_values(values),
        ));
    }
    Ok(candidates)
}

fn csv_field<'a>(headers: &[&str], fields: &'a [&str], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .position(|header| *header == name)
        .and_then(|ix| fields.get(ix).copied())
        .filter(|field| !field.is_empty())
}

fn csv_string(headers: &[&str], fields: &[&str], name: &str) -> Option<String> {
    csv_field(headers, fields, name).map(ToString::to_string)
}

fn csv_f64(headers: &[&str], fields: &[&str], name: &str) -> Result<f64, String> {
    csv_field(headers, fields, name)
        .ok_or_else(|| format!("missing {name}"))
        .and_then(|field| {
            field
                .parse()
                .map_err(|_| format!("{name} must be a float, got {field:?}"))
        })
}

fn csv_f64_any(headers: &[&str], fields: &[&str], names: &[&str]) -> Result<f64, String> {
    for name in names {
        if csv_field(headers, fields, name).is_some() {
            return csv_f64(headers, fields, name);
        }
    }
    Err(format!("missing one of {}", names.join(", ")))
}

fn csv_f32(headers: &[&str], fields: &[&str], name: &str) -> Result<f32, String> {
    csv_field(headers, fields, name)
        .ok_or_else(|| format!("missing {name}"))
        .and_then(|field| {
            field
                .parse()
                .map_err(|_| format!("{name} must be a float, got {field:?}"))
        })
}

fn csv_usize(headers: &[&str], fields: &[&str], name: &str) -> Result<usize, String> {
    csv_field(headers, fields, name)
        .ok_or_else(|| format!("missing {name}"))
        .and_then(|field| {
            field
                .parse()
                .map_err(|_| format!("{name} must be an integer, got {field:?}"))
        })
}

fn csv_u8(headers: &[&str], fields: &[&str], name: &str) -> Result<u8, String> {
    csv_field(headers, fields, name)
        .ok_or_else(|| format!("missing {name}"))
        .and_then(|field| {
            field
                .parse()
                .map_err(|_| format!("{name} must be an integer, got {field:?}"))
        })
}

fn propose_coverage_candidate(
    ix: usize,
    results: &[CoverageTuneResult],
    rng: &mut SmallRng,
) -> CoverageCandidate {
    let source = if results.len() < 16 {
        "random"
    } else {
        "bayes"
    };
    let params = if results.len() < 16 {
        sample_uniform_coverage_params(rng)
    } else {
        sample_tpe_coverage_params(results, rng)
    };
    coverage_candidate(format!("{source}_{ix:04}"), source, params)
}

fn sample_uniform_coverage_params(rng: &mut SmallRng) -> TextCoverageParams {
    let mut values = [0.0; COVERAGE_PARAM_COUNT];
    for (value, range) in values.iter_mut().zip(COVERAGE_PARAM_RANGES) {
        *value = range.min + rng.f32() * (range.max - range.min);
    }
    coverage_params_from_values(values)
}

fn sample_tpe_coverage_params(
    results: &[CoverageTuneResult],
    rng: &mut SmallRng,
) -> TextCoverageParams {
    let mut sorted = results.to_vec();
    sorted.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let focused = sorted.len() >= 64;
    let good_count = if focused {
        (sorted.len() / 8).clamp(8, sorted.len().saturating_sub(1))
    } else {
        (sorted.len() / 4).clamp(4, sorted.len().saturating_sub(1))
    };
    let (good, bad) = sorted.split_at(good_count);
    let good_stats = coverage_distribution_stats(good);
    let bad_stats = coverage_distribution_stats(bad);
    let mut best_values = coverage_params_to_values(good[0].candidate.params);
    let mut best_acquisition = f64::NEG_INFINITY;
    let proposal_count = if focused { 256 } else { 64 };
    let std_floor_scale = if focused { 0.015 } else { 0.04 };

    for _ in 0..proposal_count {
        let mut values = [0.0; COVERAGE_PARAM_RANGES.len()];
        for ix in 0..COVERAGE_PARAM_RANGES.len() {
            let range = COVERAGE_PARAM_RANGES[ix];
            let std = good_stats[ix]
                .1
                .max((range.max - range.min) * std_floor_scale);
            values[ix] = (good_stats[ix].0 + rng.normal_f32() * std).clamp(range.min, range.max);
        }
        let acquisition = values
            .iter()
            .enumerate()
            .map(|(ix, &value)| {
                log_gaussian(value, good_stats[ix].0, good_stats[ix].1)
                    - log_gaussian(value, bad_stats[ix].0, bad_stats[ix].1)
            })
            .sum::<f64>();
        if acquisition > best_acquisition {
            best_acquisition = acquisition;
            best_values = values;
        }
    }

    coverage_params_from_values(best_values)
}

fn coverage_distribution_stats(
    results: &[CoverageTuneResult],
) -> [(f32, f32); COVERAGE_PARAM_COUNT] {
    let mut stats = [(0.0, 0.0); COVERAGE_PARAM_COUNT];
    if results.is_empty() {
        for (ix, range) in COVERAGE_PARAM_RANGES.iter().enumerate() {
            stats[ix] = (
                (range.min + range.max) * 0.5,
                (range.max - range.min) * 0.25,
            );
        }
        return stats;
    }

    for ix in 0..COVERAGE_PARAM_RANGES.len() {
        let mean = results
            .iter()
            .map(|result| coverage_params_to_values(result.candidate.params)[ix])
            .sum::<f32>()
            / results.len() as f32;
        let variance = results
            .iter()
            .map(|result| {
                let delta = coverage_params_to_values(result.candidate.params)[ix] - mean;
                delta * delta
            })
            .sum::<f32>()
            / results.len() as f32;
        let range = COVERAGE_PARAM_RANGES[ix];
        stats[ix] = (mean, variance.sqrt().max((range.max - range.min) * 0.03));
    }
    stats
}

fn log_gaussian(value: f32, mean: f32, std: f32) -> f64 {
    let std = f64::from(std.max(1.0e-6));
    let z = (f64::from(value) - f64::from(mean)) / std;
    -0.5 * z * z - std.ln()
}

fn best_coverage_result(results: &[CoverageTuneResult]) -> Option<&CoverageTuneResult> {
    results.iter().min_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn print_coverage_tuning_top(results: &[CoverageTuneResult], count: usize) {
    let mut sorted = results.to_vec();
    sorted.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    println!("Best coverage candidates:");
    for (rank, result) in sorted.iter().take(count).enumerate() {
        println!(
            "  #{:<2} score={:.6} {:<28} {:<7} mixed_coverage={:.5} worst_mixed={:.5} avg_coverage={:.5} mae={:.5} rgb={:.5}",
            rank + 1,
            result.score,
            result.candidate.name,
            result.candidate.source,
            result.mixed_avg_abs_ink_error,
            result.worst_mixed_abs_ink_error,
            result.avg_abs_ink_error,
            result.avg_mae_luma,
            result.avg_mae_rgb,
        );
    }
}

fn coverage_tuning_csv(results: &[CoverageTuneResult]) -> String {
    let mut sorted = results.to_vec();
    sorted.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut csv = String::from(
        "rank,name,source,score,cases,mixed_groups,avg_mae_luma,avg_mae_rgb,avg_robust_coverage_error,mixed_avg_robust_coverage_error,worst_mixed_robust_coverage_error,worst_group_robust_coverage_error,avg_abs_fringe_delta,max_rgb",
    );
    for range in COVERAGE_PARAM_RANGES {
        csv.push(',');
        csv.push_str(range.name);
    }
    csv.push('\n');

    for (rank, result) in sorted.iter().enumerate() {
        csv.push_str(&format!(
            "{},{},{},{:.8},{},{},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{}",
            rank + 1,
            result.candidate.name,
            result.candidate.source,
            result.score,
            result.case_count,
            result.mixed_group_count,
            result.avg_mae_luma,
            result.avg_mae_rgb,
            result.avg_abs_ink_error,
            result.mixed_avg_abs_ink_error,
            result.worst_mixed_abs_ink_error,
            result.worst_group_abs_ink_error,
            result.avg_abs_fringe_delta,
            result.max_rgb,
        ));
        for value in coverage_params_to_values(result.candidate.params) {
            csv.push_str(&format!(",{value:.8}"));
        }
        csv.push('\n');
    }
    csv
}

fn coverage_worst_mixed_groups_csv(
    best: &CoverageTuneResult,
    details: &[CoverageGroupDetail],
) -> String {
    let mut csv = String::from(
        "rank,candidate,score,font_size,subpixel,fg,bg,cases,avg_robust_coverage_error,worst_case_robust_coverage_error,worst_case,worst_case_reference_coverage,worst_case_tileink_coverage,avg_reference_coverage,avg_tileink_coverage,reference_floor_hit_rate,coverage_fit_pixels,current_coverage_sum_error,current_coverage_rmse,best_scale,scale_sum_error,scale_rmse,scale_rmse_reduction,axis_scale,axis_bias,axis_sum_error,axis_rmse,axis_rmse_reduction,best_exponent,best_exponent_scale,exponent_sum_error,exponent_rmse,exponent_rmse_reduction,avg_abs_bbox_dx,avg_abs_bbox_dy,worst_case_bbox_dx,worst_case_bbox_dy,avg_mae_luma,avg_mae_rgb,max_rgb,avg_abs_fringe_delta,fg_luma,bg_luma,luma_contrast,fg_chroma,bg_chroma\n",
    );
    for (rank, detail) in details.iter().enumerate() {
        csv.push_str(&format!(
            "{},{},{:.8},{},{},{},{},{},{:.8},{:.8},{},{:.8},{:.8},{:.8},{:.8},{:.8},{},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8},{},{},{:.8},{:.8},{},{:.8},{:.8},{:.8},{:.8},{:.8},{:.8}\n",
            rank + 1,
            best.candidate.name,
            best.score,
            detail.key.font_size,
            detail.key.subpixel,
            detail.key.fg,
            detail.key.bg,
            detail.cases,
            detail.avg_robust_ink_error,
            detail.worst_case_robust_ink_error,
            detail.worst_case,
            detail.worst_case_reference_ink,
            detail.worst_case_tileink_ink,
            detail.avg_reference_ink,
            detail.avg_tileink_ink,
            detail.reference_floor_hit_rate,
            detail.fit.pixels,
            detail.fit.current_sum_error,
            detail.fit.current_rmse,
            detail.fit.scale,
            detail.fit.scale_sum_error,
            detail.fit.scale_rmse,
            detail.fit.scale_rmse_reduction,
            detail.fit.axis_scale,
            detail.fit.axis_bias,
            detail.fit.axis_sum_error,
            detail.fit.axis_rmse,
            detail.fit.axis_rmse_reduction,
            detail.fit.exponent,
            detail.fit.exponent_scale,
            detail.fit.exponent_sum_error,
            detail.fit.exponent_rmse,
            detail.fit.exponent_rmse_reduction,
            detail.avg_abs_bbox_dx,
            detail.avg_abs_bbox_dy,
            detail.worst_case_bbox_dx,
            detail.worst_case_bbox_dy,
            detail.avg_mae_luma,
            detail.avg_mae_rgb,
            detail.max_rgb,
            detail.avg_abs_fringe_delta,
            detail.fg_luma,
            detail.bg_luma,
            (detail.fg_luma - detail.bg_luma).abs(),
            detail.fg_chroma,
            detail.bg_chroma,
        ));
    }
    csv
}

fn print_worst_mixed_group_details(details: &[CoverageGroupDetail], count: usize) {
    println!("Worst mixed groups for best candidate:");
    for (rank, detail) in details.iter().take(count).enumerate() {
        println!(
            "  #{rank:<2} {size:>4}px {subpixel:>4} {fg} on {bg}: coverage={robust:.4} ref_cov={ref_ink:.2} tile_cov={tile_ink:.2} fit_rmse={rmse:.4}->{scale_rmse:.4}/{exp_rmse:.4} scale={scale:.3} exp={exp:.2} contrast={contrast:.3} worst={worst_case}({worst:.4})",
            rank = rank + 1,
            size = detail.key.font_size,
            subpixel = detail.key.subpixel,
            fg = detail.key.fg,
            bg = detail.key.bg,
            robust = detail.avg_robust_ink_error,
            ref_ink = detail.avg_reference_ink,
            tile_ink = detail.avg_tileink_ink,
            rmse = detail.fit.current_rmse,
            scale_rmse = detail.fit.scale_rmse,
            exp_rmse = detail.fit.exponent_rmse,
            scale = detail.fit.scale,
            exp = detail.fit.exponent,
            contrast = (detail.fg_luma - detail.bg_luma).abs(),
            worst_case = detail.worst_case,
            worst = detail.worst_case_robust_ink_error,
        );
    }
}

fn coverage_params_rust(params: TextCoverageParams) -> String {
    let values = coverage_params_to_values(params);
    let mut out = String::from("TextCoverageParams {\n");
    for (range, value) in COVERAGE_PARAM_RANGES.iter().zip(values) {
        out.push_str(&format!("    {}: {:.8},\n", range.name, value));
    }
    out.push_str("}\n");
    out
}

fn coverage_params_to_values(params: TextCoverageParams) -> [f32; COVERAGE_PARAM_COUNT] {
    [
        params.dark_on_light_coverage_strength,
        params.dark_on_light_luma_base,
        params.dark_on_light_luma_taper,
        params.dark_on_light_chroma_boost,
        params.source_chroma_coverage_boost,
        params.source_chroma_coverage_contrast_limit,
        params.light_on_dark_coverage_reduction,
        params.light_on_dark_black_luma_limit,
        params.light_on_dark_chroma_reduction,
        params.light_on_dark_high_luma_chroma_reduction,
        params.light_on_dark_high_luma_threshold,
        params.light_on_colored_dark_chroma_reduction,
        params.light_on_colored_dark_luma_limit,
        params.alpha_mask_chroma_scale,
        params.subpixel_mask_chroma_scale,
        params.alpha_mask_apparent_axis_strength,
        params.alpha_mask_apparent_axis_luma_limit,
        params.subpixel_mask_apparent_axis_strength,
        params.subpixel_mask_apparent_axis_luma_limit,
        params.alpha_mask_embolden,
        params.subpixel_mask_embolden,
        params.alpha_mask_low_luma_chroma_reduction,
        params.alpha_mask_low_luma_contrast_limit,
        params.subpixel_mask_low_luma_chroma_reduction,
        params.subpixel_mask_low_luma_contrast_limit,
    ]
}

fn coverage_params_from_values(values: [f32; COVERAGE_PARAM_COUNT]) -> TextCoverageParams {
    TextCoverageParams {
        dark_on_light_coverage_strength: values[0],
        dark_on_light_luma_base: values[1],
        dark_on_light_luma_taper: values[2],
        dark_on_light_chroma_boost: values[3],
        source_chroma_coverage_boost: values[4],
        source_chroma_coverage_contrast_limit: values[5],
        light_on_dark_coverage_reduction: values[6],
        light_on_dark_black_luma_limit: values[7],
        light_on_dark_chroma_reduction: values[8],
        light_on_dark_high_luma_chroma_reduction: values[9],
        light_on_dark_high_luma_threshold: values[10],
        light_on_colored_dark_chroma_reduction: values[11],
        light_on_colored_dark_luma_limit: values[12],
        alpha_mask_chroma_scale: values[13],
        subpixel_mask_chroma_scale: values[14],
        alpha_mask_apparent_axis_strength: values[15],
        alpha_mask_apparent_axis_luma_limit: values[16],
        subpixel_mask_apparent_axis_strength: values[17],
        subpixel_mask_apparent_axis_luma_limit: values[18],
        alpha_mask_embolden: values[19],
        subpixel_mask_embolden: values[20],
        alpha_mask_low_luma_chroma_reduction: values[21],
        alpha_mask_low_luma_contrast_limit: values[22],
        subpixel_mask_low_luma_chroma_reduction: values[23],
        subpixel_mask_low_luma_contrast_limit: values[24],
    }
}

#[derive(Clone)]
struct SmallRng {
    state: u64,
}

impl SmallRng {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn f32(&mut self) -> f32 {
        ((self.u64() >> 40) as f32) * (1.0 / ((1u64 << 24) as f32))
    }

    fn normal_f32(&mut self) -> f32 {
        let u1 = self.f32().max(1.0e-7);
        let u2 = self.f32();
        (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    }
}

fn render_tileink(case: &QualityCase, font_family: &str, backend: Backend) -> Image {
    let mut context = TextContext::new();
    render_tileink_with_context(
        case,
        font_family,
        backend,
        &mut context,
        TextCoverageParams::DEFAULT,
    )
}

fn render_tileink_with_context(
    case: &QualityCase,
    font_family: &str,
    backend: Backend,
    context: &mut TextContext,
    coverage_params: TextCoverageParams,
) -> Image {
    context.set_raster_options(
        TextRasterOptions::new()
            .with_subpixel_mode(case.subpixel)
            .with_composite_mode(TextCompositeMode::Linear)
            .with_coverage_params(coverage_params),
    );
    let layout = context.layout(
        TextLayoutOptions::new(case.text, case.font_size)
            .with_attrs(TextAttrs::new().family(TextFamily::Name(font_family)))
            .with_size(Some(case.layout_width), None),
    );
    let bounds = layout.bounds();
    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.push_text_layout(
        &layout,
        Point::new(
            f64::from(case.margin + case.origin_x) - f64::from(bounds.x0),
            f64::from(case.margin + case.origin_y) - f64::from(bounds.y0),
        ),
        case.foreground,
    );

    match backend {
        Backend::Cpu => {
            let mut renderer = CpuRenderer::new(WIDTH, HEIGHT, case.background);
            renderer.render_with_text(&scene, context);
            renderer.image().clone()
        }
        Backend::Cubecl => {
            let mut renderer = CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, case.background);
            renderer.render_with_text(&scene, context);
            renderer.image()
        }
        Backend::Both => unreachable!("render_tileink needs one concrete backend"),
    }
}

fn build_cases(full: bool, color_sweep: bool) -> Vec<QualityCase> {
    build_cases_with_color_group_target(full, color_sweep, None)
}

fn build_cases_with_color_group_target(
    full: bool,
    color_sweep: bool,
    color_group_target: Option<usize>,
) -> Vec<QualityCase> {
    let sizes: &[f32] = if full {
        &[9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 16.0, 20.0, 24.0]
    } else {
        &[12.0, 16.0, 24.0]
    };
    let origins: &[OriginCase] = if full {
        &[
            OriginCase {
                name: "x0_y0",
                x: 0.0,
                y: 0.0,
            },
            OriginCase {
                name: "x1of3_y0",
                x: 1.0 / 3.0,
                y: 0.0,
            },
            OriginCase {
                name: "x2of3_y0",
                x: 2.0 / 3.0,
                y: 0.0,
            },
            OriginCase {
                name: "x0_yhalf",
                x: 0.0,
                y: 0.5,
            },
        ]
    } else {
        &[
            OriginCase {
                name: "x0_y0",
                x: 0.0,
                y: 0.0,
            },
            OriginCase {
                name: "x1of3_yhalf",
                x: 1.0 / 3.0,
                y: 0.5,
            },
        ]
    };
    let modes = [
        ("rgb", TextSubpixelMode::Rgb),
        ("bgr", TextSubpixelMode::Bgr),
        ("gray", TextSubpixelMode::None),
    ];
    let mut color_cases = vec![
        ColorCase {
            name: "black_on_white".into(),
            foreground: Color::BLACK,
            background: Color::WHITE,
        },
        ColorCase {
            name: "white_on_black".into(),
            foreground: Color::WHITE,
            background: Color::BLACK,
        },
        ColorCase {
            name: "black_on_gray".into(),
            foreground: Color::BLACK,
            background: Color::from_rgb8(224, 224, 224),
        },
        ColorCase {
            name: "white_on_gray".into(),
            foreground: Color::WHITE,
            background: Color::from_rgb8(48, 48, 48),
        },
    ];
    if color_sweep {
        color_cases.extend([
            ColorCase {
                name: "red_on_white".into(),
                foreground: Color::from_rgb8(220, 32, 32),
                background: Color::WHITE,
            },
            ColorCase {
                name: "green_on_white".into(),
                foreground: Color::from_rgb8(32, 180, 72),
                background: Color::WHITE,
            },
            ColorCase {
                name: "blue_on_white".into(),
                foreground: Color::from_rgb8(48, 88, 220),
                background: Color::WHITE,
            },
            ColorCase {
                name: "red_on_black".into(),
                foreground: Color::from_rgb8(255, 80, 80),
                background: Color::BLACK,
            },
            ColorCase {
                name: "green_on_black".into(),
                foreground: Color::from_rgb8(80, 220, 120),
                background: Color::BLACK,
            },
            ColorCase {
                name: "blue_on_black".into(),
                foreground: Color::from_rgb8(120, 160, 255),
                background: Color::BLACK,
            },
            ColorCase {
                name: "black_on_yellow".into(),
                foreground: Color::BLACK,
                background: Color::from_rgb8(250, 220, 80),
            },
            ColorCase {
                name: "white_on_blue".into(),
                foreground: Color::WHITE,
                background: Color::from_rgb8(24, 68, 160),
            },
            ColorCase {
                name: "red_on_blue".into(),
                foreground: Color::from_rgb8(240, 72, 72),
                background: Color::from_rgb8(24, 68, 160),
            },
            ColorCase {
                name: "green_on_blue".into(),
                foreground: Color::from_rgb8(64, 220, 112),
                background: Color::from_rgb8(24, 68, 160),
            },
            ColorCase {
                name: "blue_on_yellow".into(),
                foreground: Color::from_rgb8(48, 88, 220),
                background: Color::from_rgb8(250, 220, 80),
            },
            ColorCase {
                name: "red_on_green_dark".into(),
                foreground: Color::from_rgb8(255, 92, 92),
                background: Color::from_rgb8(28, 96, 56),
            },
            ColorCase {
                name: "green_on_purple".into(),
                foreground: Color::from_rgb8(80, 220, 120),
                background: Color::from_rgb8(92, 48, 132),
            },
            ColorCase {
                name: "yellow_on_blue".into(),
                foreground: Color::from_rgb8(250, 220, 80),
                background: Color::from_rgb8(24, 68, 160),
            },
            ColorCase {
                name: "cyan_on_red_dark".into(),
                foreground: Color::from_rgb8(96, 220, 230),
                background: Color::from_rgb8(120, 36, 40),
            },
            ColorCase {
                name: "orange_on_teal".into(),
                foreground: Color::from_rgb8(245, 144, 56),
                background: Color::from_rgb8(28, 112, 116),
            },
            ColorCase {
                name: "magenta_on_teal".into(),
                foreground: Color::from_rgb8(230, 72, 210),
                background: Color::from_rgb8(28, 112, 116),
            },
            ColorCase {
                name: "cyan_on_purple".into(),
                foreground: Color::from_rgb8(96, 220, 230),
                background: Color::from_rgb8(92, 48, 132),
            },
            ColorCase {
                name: "orange_on_blue".into(),
                foreground: Color::from_rgb8(245, 144, 56),
                background: Color::from_rgb8(24, 68, 160),
            },
            ColorCase {
                name: "yellow_on_purple".into(),
                foreground: Color::from_rgb8(250, 220, 80),
                background: Color::from_rgb8(92, 48, 132),
            },
            ColorCase {
                name: "violet_on_yellow".into(),
                foreground: Color::from_rgb8(126, 92, 230),
                background: Color::from_rgb8(250, 220, 80),
            },
            ColorCase {
                name: "lime_on_red_dark".into(),
                foreground: Color::from_rgb8(170, 235, 72),
                background: Color::from_rgb8(120, 36, 40),
            },
            ColorCase {
                name: "pink_on_navy".into(),
                foreground: Color::from_rgb8(255, 112, 176),
                background: Color::from_rgb8(20, 44, 112),
            },
            ColorCase {
                name: "teal_on_maroon".into(),
                foreground: Color::from_rgb8(56, 196, 184),
                background: Color::from_rgb8(96, 32, 56),
            },
            ColorCase {
                name: "blue_on_orange_dark".into(),
                foreground: Color::from_rgb8(88, 132, 255),
                background: Color::from_rgb8(132, 68, 24),
            },
            ColorCase {
                name: "red_on_teal".into(),
                foreground: Color::from_rgb8(240, 72, 72),
                background: Color::from_rgb8(28, 112, 116),
            },
            ColorCase {
                name: "green_on_red_dark".into(),
                foreground: Color::from_rgb8(80, 220, 120),
                background: Color::from_rgb8(120, 36, 40),
            },
            ColorCase {
                name: "cyan_on_blue_dark".into(),
                foreground: Color::from_rgb8(96, 220, 230),
                background: Color::from_rgb8(20, 44, 112),
            },
            ColorCase {
                name: "purple_on_yellow".into(),
                foreground: Color::from_rgb8(126, 72, 190),
                background: Color::from_rgb8(250, 220, 80),
            },
            ColorCase {
                name: "yellow_on_green_dark".into(),
                foreground: Color::from_rgb8(250, 220, 80),
                background: Color::from_rgb8(28, 96, 56),
            },
            ColorCase {
                name: "orange_on_purple".into(),
                foreground: Color::from_rgb8(245, 144, 56),
                background: Color::from_rgb8(92, 48, 132),
            },
            ColorCase {
                name: "magenta_on_green_dark".into(),
                foreground: Color::from_rgb8(230, 72, 210),
                background: Color::from_rgb8(28, 96, 56),
            },
        ]);
    }
    if let Some(target) = color_group_target {
        extend_generated_mixed_color_cases(&mut color_cases, target);
    }

    let mut cases = Vec::new();
    for &size in sizes {
        for &(mode_name, subpixel) in &modes {
            for colors in &color_cases {
                for origin in origins {
                    cases.push(QualityCase {
                        name: format!(
                            "{}_{}_{}px_{}",
                            mode_name, colors.name, size as u32, origin.name
                        ),
                        text: TEXT,
                        font_size: size,
                        layout_width: WIDTH as f32 - 48.0,
                        margin: 24.0,
                        origin_x: origin.x,
                        origin_y: origin.y,
                        foreground: colors.foreground,
                        background: colors.background,
                        subpixel,
                    });
                }
            }
        }
    }
    cases
}

fn extend_generated_mixed_color_cases(color_cases: &mut Vec<ColorCase>, target: usize) {
    if color_cases.len() >= target {
        color_cases.truncate(target);
        return;
    }

    let mut ix = 0usize;
    while color_cases.len() < target {
        let hue = fract(ix as f32 * 0.618_034 + 0.071);
        let bg_hue = fract(hue + 0.29 + (ix % 11) as f32 * 0.037);
        let dark_bg = (ix / 3).is_multiple_of(2);
        let bg_sat = [0.55, 0.68, 0.82, 0.94][ix % 4];
        let fg_sat = [0.62, 0.76, 0.88, 0.98][(ix / 4) % 4];
        let bg_val = if dark_bg {
            [0.18, 0.24, 0.32, 0.42][(ix / 7) % 4]
        } else {
            [0.66, 0.76, 0.86, 0.96][(ix / 7) % 4]
        };
        let fg_val = if dark_bg {
            [0.70, 0.80, 0.90, 1.00][(ix / 5) % 4]
        } else {
            [0.18, 0.26, 0.34, 0.44][(ix / 5) % 4]
        };
        color_cases.push(ColorCase {
            name: Cow::Owned(format!("generated_mixed_{ix:03}")),
            foreground: hsv_color(hue, fg_sat, fg_val),
            background: hsv_color(bg_hue, bg_sat, bg_val),
        });
        ix += 1;
    }
}

fn fract(value: f32) -> f32 {
    value - value.floor()
}

fn hsv_color(h: f32, s: f32, v: f32) -> Color {
    let h = fract(h) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let [r, g, b] = match i.rem_euclid(6) {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    Color::from_rgb8(
        float_color_to_u8(r),
        float_color_to_u8(g),
        float_color_to_u8(b),
    )
}

fn float_color_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

struct DiffResult {
    image: Image,
    stats: DiffStats,
}

fn save_dump_image_set(
    dir: &Path,
    name: &str,
    image: &Image,
    bounds: Option<Bounds>,
) -> Result<(), Box<dyn std::error::Error>> {
    image.save(dir.join(format!("{name}.png")))?;
    let zoom = crop_zoom_image(image, expanded_bounds(bounds, image.width, image.height), 8);
    zoom.save(dir.join(format!("{name}_zoom8.png")))?;
    Ok(())
}

fn dump_bounds(stats: DiffStats) -> Option<Bounds> {
    union_bounds(stats.tileink_bbox, stats.reference_bbox)
}

fn union_bounds(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Bounds {
            x0: a.x0.min(b.x0),
            y0: a.y0.min(b.y0),
            x1: a.x1.max(b.x1),
            y1: a.y1.max(b.y1),
        }),
        (Some(bounds), None) | (None, Some(bounds)) => Some(bounds),
        (None, None) => None,
    }
}

fn expanded_bounds(bounds: Option<Bounds>, width: u32, height: u32) -> Bounds {
    let Some(bounds) = bounds else {
        return Bounds {
            x0: 0,
            y0: 0,
            x1: width,
            y1: height,
        };
    };
    Bounds {
        x0: bounds.x0.saturating_sub(4),
        y0: bounds.y0.saturating_sub(4),
        x1: (bounds.x1 + 4).min(width),
        y1: (bounds.y1 + 4).min(height),
    }
}

fn crop_zoom_image(image: &Image, bounds: Bounds, scale: u32) -> Image {
    let crop_width = (bounds.x1 - bounds.x0).max(1);
    let crop_height = (bounds.y1 - bounds.y0).max(1);
    let mut out = Image::new(crop_width * scale, crop_height * scale, Color::BLACK);
    for y in 0..crop_height {
        for x in 0..crop_width {
            let px = image.pixels[((bounds.y0 + y) * image.width + bounds.x0 + x) as usize];
            for sy in 0..scale {
                for sx in 0..scale {
                    let out_x = x * scale + sx;
                    let out_y = y * scale + sy;
                    out.pixels[(out_y * out.width + out_x) as usize] = px;
                }
            }
        }
    }
    out
}

fn apparent_coverage_image(image: &Image, foreground: Color, background: Color) -> Image {
    let fg = color_to_rgba8(foreground);
    let bg = color_to_rgba8(background);
    let direction = [
        f32::from(fg[0]) - f32::from(bg[0]),
        f32::from(fg[1]) - f32::from(bg[1]),
        f32::from(fg[2]) - f32::from(bg[2]),
    ];
    let denom = direction.iter().map(|value| value * value).sum::<f32>();
    let mut out = Image::new(image.width, image.height, Color::BLACK);
    if denom <= 1.0e-6 {
        return out;
    }
    for (ix, &px) in image.pixels.iter().enumerate() {
        let px = unpack_rgba8(px);
        let projected = (0..3)
            .map(|channel| (f32::from(px[channel]) - f32::from(bg[channel])) * direction[channel])
            .sum::<f32>();
        let alpha = (projected / denom).clamp(0.0, 1.0);
        let gray = (alpha * 255.0 + 0.5) as u8;
        out.pixels[ix] = pack_rgba8([gray, gray, gray, 255]);
    }
    out
}

fn coverage_delta_image(tileink: &Image, reference: &Image, stats: DiffStats) -> Image {
    let bg = [0, 0, 0, 255];
    let (dx, dy) = bbox_alignment_delta(stats);
    let mut out = Image::new(tileink.width, tileink.height, Color::BLACK);
    for y in 0..tileink.height {
        for x in 0..tileink.width {
            let tile = rgba8_at(tileink, x, y)[0];
            let reference = sample_shifted(reference, x as i32 + dx, y as i32 + dy, bg)[0];
            let delta = tile.abs_diff(reference).saturating_mul(4);
            out.pixels[(y * tileink.width + x) as usize] = if tile >= reference {
                pack_rgba8([delta, 0, 0, 255])
            } else {
                pack_rgba8([0, 64.min(delta), delta, 255])
            };
        }
    }
    out
}

fn aligned_reference_to_tileink(reference: &Image, stats: DiffStats, background: Color) -> Image {
    let fallback = color_to_rgba8(background);
    let (dx, dy) = bbox_alignment_delta(stats);
    let mut out = Image::new(reference.width, reference.height, background);
    for y in 0..reference.height {
        for x in 0..reference.width {
            out.pixels[(y * reference.width + x) as usize] = pack_rgba8(sample_shifted(
                reference,
                x as i32 + dx,
                y as i32 + dy,
                fallback,
            ));
        }
    }
    out
}

fn gray_aligned_diff_stats(
    tileink: &Image,
    reference: &Image,
    stats: DiffStats,
) -> GrayAlignedDiffStats {
    assert_eq!(
        (tileink.width, tileink.height),
        (reference.width, reference.height)
    );
    let (dx, dy) = bbox_alignment_delta(stats);
    let bounds = aligned_diff_bounds(stats, tileink.width, tileink.height);
    let pixels = u64::from(bounds.x1 - bounds.x0) * u64::from(bounds.y1 - bounds.y0);
    if pixels == 0 {
        return GrayAlignedDiffStats::default();
    }

    let mut out = GrayAlignedDiffStats {
        pixels,
        ..GrayAlignedDiffStats::default()
    };
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let tileink_gray = rgba8_at(tileink, x, y)[0];
            let reference_gray =
                sample_shifted(reference, x as i32 + dx, y as i32 + dy, [0, 0, 0, 255])[0];
            let delta = tileink_gray.abs_diff(reference_gray);
            let normalized_delta = f64::from(delta) / 255.0;
            out.tileink_sum += f64::from(tileink_gray) / 255.0;
            out.reference_sum += f64::from(reference_gray) / 255.0;
            out.mae += normalized_delta;
            out.rmse += normalized_delta * normalized_delta;
            out.max_delta = out.max_delta.max(delta);
        }
    }
    let pixels = pixels as f64;
    out.ratio = if out.reference_sum > 0.0 {
        out.tileink_sum / out.reference_sum
    } else {
        0.0
    };
    out.mae /= pixels;
    out.rmse = (out.rmse / pixels).sqrt();
    out
}

fn aligned_sum_error(stats: GrayAlignedDiffStats) -> f64 {
    (stats.tileink_sum - stats.reference_sum).abs()
        / stats.reference_sum.max(TUNING_INK_REFERENCE_FLOOR)
}

fn apparent_coverage_aligned_diff_stats(
    tileink: &Image,
    reference: &Image,
    foreground: Color,
    background: Color,
    stats: DiffStats,
) -> GrayAlignedDiffStats {
    assert_eq!(
        (tileink.width, tileink.height),
        (reference.width, reference.height)
    );
    let fg = color_to_rgba8(foreground);
    let bg = color_to_rgba8(background);
    let direction = [
        f64::from(fg[0]) - f64::from(bg[0]),
        f64::from(fg[1]) - f64::from(bg[1]),
        f64::from(fg[2]) - f64::from(bg[2]),
    ];
    let denom = direction.iter().map(|value| value * value).sum::<f64>();
    if denom <= 1.0e-6 {
        return GrayAlignedDiffStats::default();
    }

    let (dx, dy) = bbox_alignment_delta(stats);
    let bounds = aligned_diff_bounds(stats, tileink.width, tileink.height);
    let pixels = u64::from(bounds.x1 - bounds.x0) * u64::from(bounds.y1 - bounds.y0);
    if pixels == 0 {
        return GrayAlignedDiffStats::default();
    }

    let mut out = GrayAlignedDiffStats {
        pixels,
        ..GrayAlignedDiffStats::default()
    };
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let tileink_coverage =
                apparent_coverage_value(rgba8_at(tileink, x, y), bg, direction, denom);
            let reference_coverage = apparent_coverage_value(
                sample_shifted(reference, x as i32 + dx, y as i32 + dy, bg),
                bg,
                direction,
                denom,
            );
            let delta = (tileink_coverage - reference_coverage).abs();
            out.tileink_sum += tileink_coverage;
            out.reference_sum += reference_coverage;
            out.mae += delta;
            out.rmse += delta * delta;
            out.max_delta = out
                .max_delta
                .max((delta.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
    }
    let pixels = pixels as f64;
    out.ratio = if out.reference_sum > 0.0 {
        out.tileink_sum / out.reference_sum
    } else {
        0.0
    };
    out.mae /= pixels;
    out.rmse = (out.rmse / pixels).sqrt();
    out
}

fn accumulate_apparent_coverage_fit(
    tileink: &Image,
    reference: &Image,
    foreground: Color,
    background: Color,
    stats: DiffStats,
    accum: &mut CoverageFitAccum,
) {
    assert_eq!(
        (tileink.width, tileink.height),
        (reference.width, reference.height)
    );
    let fg = color_to_rgba8(foreground);
    let bg = color_to_rgba8(background);
    let direction = [
        f64::from(fg[0]) - f64::from(bg[0]),
        f64::from(fg[1]) - f64::from(bg[1]),
        f64::from(fg[2]) - f64::from(bg[2]),
    ];
    let denom = direction.iter().map(|value| value * value).sum::<f64>();
    if denom <= 1.0e-6 {
        return;
    }

    let (dx, dy) = bbox_alignment_delta(stats);
    let bounds = aligned_diff_bounds(stats, tileink.width, tileink.height);
    for y in bounds.y0..bounds.y1 {
        for x in bounds.x0..bounds.x1 {
            let tileink_coverage =
                apparent_coverage_value(rgba8_at(tileink, x, y), bg, direction, denom);
            let reference_coverage = apparent_coverage_value(
                sample_shifted(reference, x as i32 + dx, y as i32 + dy, bg),
                bg,
                direction,
                denom,
            );
            accum.add(tileink_coverage, reference_coverage);
        }
    }
}

fn apparent_coverage_value(px: [u8; 4], bg: [u8; 4], direction: [f64; 3], denom: f64) -> f64 {
    let projected = (0..3)
        .map(|channel| (f64::from(px[channel]) - f64::from(bg[channel])) * direction[channel])
        .sum::<f64>();
    (projected / denom).clamp(0.0, 1.0)
}

fn aligned_diff_bounds(stats: DiffStats, width: u32, height: u32) -> Bounds {
    let reference_bbox = stats
        .reference_bbox
        .and_then(|bounds| shifted_reference_bounds(bounds, stats, width, height));
    let bounds = union_bounds(stats.tileink_bbox, reference_bbox).unwrap_or(Bounds {
        x0: 0,
        y0: 0,
        x1: width,
        y1: height,
    });
    clip_bounds(bounds, width, height).unwrap_or_default()
}

fn shifted_reference_bounds(
    bounds: Bounds,
    stats: DiffStats,
    width: u32,
    height: u32,
) -> Option<Bounds> {
    let (dx, dy) = bbox_alignment_delta(stats);
    let shifted = Bounds {
        x0: clamp_i32_to_u32(bounds.x0 as i32 - dx, width),
        y0: clamp_i32_to_u32(bounds.y0 as i32 - dy, height),
        x1: clamp_i32_to_u32(bounds.x1 as i32 - dx, width),
        y1: clamp_i32_to_u32(bounds.y1 as i32 - dy, height),
    };
    (shifted.x0 < shifted.x1 && shifted.y0 < shifted.y1).then_some(shifted)
}

fn clip_bounds(bounds: Bounds, width: u32, height: u32) -> Option<Bounds> {
    let clipped = Bounds {
        x0: bounds.x0.min(width),
        y0: bounds.y0.min(height),
        x1: bounds.x1.min(width),
        y1: bounds.y1.min(height),
    };
    (clipped.x0 < clipped.x1 && clipped.y0 < clipped.y1).then_some(clipped)
}

fn clamp_i32_to_u32(value: i32, upper: u32) -> u32 {
    value.clamp(0, upper as i32) as u32
}

fn bbox_alignment_delta(stats: DiffStats) -> (i32, i32) {
    match (stats.tileink_bbox, stats.reference_bbox) {
        (Some(tileink), Some(reference)) => (
            reference.x0 as i32 - tileink.x0 as i32,
            reference.y0 as i32 - tileink.y0 as i32,
        ),
        _ => (0, 0),
    }
}

fn diff_images(tileink: &Image, reference: &Image, background: Color) -> DiffResult {
    assert_eq!(
        (tileink.width, tileink.height),
        (reference.width, reference.height)
    );
    let bg = color_to_rgba8(background);
    let tileink_stats = image_stats(tileink, background);
    let reference_stats = image_stats(reference, background);
    let (dx, dy) = bbox_alignment_delta(DiffStats {
        tileink_bbox: tileink_stats.bbox,
        reference_bbox: reference_stats.bbox,
        ..DiffStats::default()
    });

    let mut sum_luma_abs = 0.0;
    let mut sum_luma_sq = 0.0;
    let mut sum_rgb_abs = 0.0;
    let mut max_rgb = 0u8;
    let mut out = Image::new(tileink.width, tileink.height, Color::BLACK);

    for y in 0..tileink.height {
        for x in 0..tileink.width {
            let tileink_px = rgba8_at(tileink, x, y);
            let reference_px = sample_shifted(reference, x as i32 + dx, y as i32 + dy, bg);
            let dr = tileink_px[0].abs_diff(reference_px[0]);
            let dg = tileink_px[1].abs_diff(reference_px[1]);
            let db = tileink_px[2].abs_diff(reference_px[2]);
            max_rgb = max_rgb.max(dr).max(dg).max(db);

            let luma_delta = (luma(tileink_px) - luma(reference_px)).abs();
            sum_luma_abs += luma_delta;
            sum_luma_sq += luma_delta * luma_delta;
            sum_rgb_abs += (f64::from(dr) + f64::from(dg) + f64::from(db)) / (3.0 * 255.0);

            let d = dr.max(dg).max(db).saturating_mul(4);
            out.pixels[(y * tileink.width + x) as usize] = pack_rgba8([d, d, d, 255]);
        }
    }

    let pixels = f64::from(tileink.width * tileink.height);
    DiffResult {
        image: out,
        stats: DiffStats {
            mae_luma: sum_luma_abs / pixels,
            rmse_luma: (sum_luma_sq / pixels).sqrt(),
            mae_rgb: sum_rgb_abs / pixels,
            max_rgb,
            ink_ratio: if reference_stats.ink > 0.0 {
                tileink_stats.ink / reference_stats.ink
            } else {
                0.0
            },
            tileink_ink: tileink_stats.ink,
            reference_ink: reference_stats.ink,
            fringe_delta: average_fringe(tileink_stats) - average_fringe(reference_stats),
            tileink_bbox: tileink_stats.bbox,
            reference_bbox: reference_stats.bbox,
        },
    }
}

fn image_stats(image: &Image, background: Color) -> ImageStats {
    let bg = color_to_rgba8(background);
    let bg_luma = luma(bg);
    let mut stats = ImageStats::default();
    let mut x0 = image.width;
    let mut y0 = image.height;
    let mut x1 = 0;
    let mut y1 = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            let px = rgba8_at(image, x, y);
            if color_distance(px, bg) <= 1 {
                continue;
            }
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
            let px_luma = luma(px);
            stats.ink += (px_luma - bg_luma).abs();
            stats.fringe += f64::from(px[0].abs_diff(px[1]) + px[2].abs_diff(px[1])) / 255.0;
            stats.fringe_pixels += 1;
        }
    }
    if x1 > x0 && y1 > y0 {
        stats.bbox = Some(Bounds { x0, y0, x1, y1 });
    }
    stats
}

fn append_metrics_csv(csv: &mut String, case: &QualityCase, backend: &str, stats: &DiffStats) {
    csv.push_str(&format!(
        "{},{},{:.1},{},{},{},{},{:.6},{:.6},{:.6},{:.8},{:.8},{:.8},{},{:.8},{},{}\n",
        case.name,
        backend,
        case.font_size,
        mode_name(case.subpixel),
        color_name(case.foreground),
        color_name(case.background),
        origin_name(case.origin_x, case.origin_y),
        stats.tileink_ink,
        stats.reference_ink,
        stats.ink_ratio,
        stats.mae_luma,
        stats.rmse_luma,
        stats.mae_rgb,
        stats.max_rgb,
        stats.fringe_delta,
        bbox_csv(stats.tileink_bbox),
        bbox_csv(stats.reference_bbox),
    ));
}

struct ContactSheet {
    image: Image,
    row_ix: u32,
}

impl ContactSheet {
    fn new(rows: usize) -> Self {
        let gutter = 4;
        Self {
            image: Image::new(
                WIDTH * 3 + gutter * 2,
                (HEIGHT + gutter) * rows as u32,
                Color::from_rgb8(32, 32, 32),
            ),
            row_ix: 0,
        }
    }

    fn push(&mut self, tileink: &Image, reference: &Image, diff: &Image) {
        let gutter = 4;
        let y = self.row_ix * (HEIGHT + gutter);
        blit(&mut self.image, tileink, 0, y);
        blit(&mut self.image, reference, WIDTH + gutter, y);
        blit(&mut self.image, diff, WIDTH * 2 + gutter * 2, y);
        self.row_ix += 1;
    }

    fn save(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.image.save(path)?;
        Ok(())
    }
}

fn blit(dst: &mut Image, src: &Image, x0: u32, y0: u32) {
    for y in 0..src.height {
        for x in 0..src.width {
            let dst_ix = ((y0 + y) * dst.width + x0 + x) as usize;
            let src_ix = (y * src.width + x) as usize;
            dst.pixels[dst_ix] = src.pixels[src_ix];
        }
    }
}

impl Options {
    fn parse() -> Result<Self, Box<dyn std::error::Error>> {
        let mut backend = Backend::Cpu;
        let mut out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("text_quality")
            .join("out");
        let mut font_family = DEFAULT_FONT.to_string();
        let mut full = false;
        let mut color_sweep = false;
        let mut tune_coverage = false;
        let mut tune_iterations = 64;
        let mut tune_seed = 0x7469_6c65_696e_6b31;
        let mut tune_candidates = None;
        let mut tune_parallelism = std::thread::available_parallelism()
            .map(|threads| threads.get().min(4))
            .unwrap_or(1);
        let mut tune_color_groups = 512;
        let mut tune_target_score = None;
        let mut dump_case = None;
        let mut dump_coverage_results = None;
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--backend" => {
                    backend = match args.next().as_deref() {
                        Some("cpu") => Backend::Cpu,
                        Some("cubecl") => Backend::Cubecl,
                        Some("both") => Backend::Both,
                        Some(value) => return Err(format!("unknown backend {value:?}").into()),
                        None => return Err("--backend requires cpu, cubecl, or both".into()),
                    };
                }
                "--out" => {
                    out_dir = args.next().ok_or("--out requires a directory")?.into();
                }
                "--font" => {
                    font_family = args.next().ok_or("--font requires a family name")?;
                }
                "--full" => full = true,
                "--color-sweep" => color_sweep = true,
                "--tune-coverage" => tune_coverage = true,
                "--tune-iterations" => {
                    tune_iterations = args
                        .next()
                        .ok_or("--tune-iterations requires a positive integer")?
                        .parse()
                        .map_err(|_| "--tune-iterations requires a positive integer")?;
                    if tune_iterations == 0 {
                        return Err("--tune-iterations must be greater than zero".into());
                    }
                }
                "--tune-seed" => {
                    tune_seed = args
                        .next()
                        .ok_or("--tune-seed requires an integer")?
                        .parse()
                        .map_err(|_| "--tune-seed requires an integer")?;
                }
                "--tune-candidates" => {
                    tune_candidates = Some(
                        args.next()
                            .ok_or("--tune-candidates requires a CSV path")?
                            .into(),
                    );
                }
                "--tune-parallelism" => {
                    tune_parallelism = args
                        .next()
                        .ok_or("--tune-parallelism requires a positive integer")?
                        .parse()
                        .map_err(|_| "--tune-parallelism requires a positive integer")?;
                    if tune_parallelism == 0 {
                        return Err("--tune-parallelism must be greater than zero".into());
                    }
                }
                "--tune-color-groups" => {
                    tune_color_groups = args
                        .next()
                        .ok_or("--tune-color-groups requires a positive integer")?
                        .parse()
                        .map_err(|_| "--tune-color-groups requires a positive integer")?;
                    if tune_color_groups == 0 {
                        return Err("--tune-color-groups must be greater than zero".into());
                    }
                }
                "--tune-target-score" => {
                    tune_target_score = Some(
                        args.next()
                            .ok_or("--tune-target-score requires a float")?
                            .parse()
                            .map_err(|_| "--tune-target-score requires a float")?,
                    );
                }
                "--dump-case" => {
                    dump_case = Some(args.next().ok_or("--dump-case requires a case name")?);
                }
                "--dump-coverage-results" => {
                    dump_coverage_results = Some(
                        args.next()
                            .ok_or("--dump-coverage-results requires a coverage_tuning.csv path")?
                            .into(),
                    );
                }
                "--help" | "-h" => {
                    println!(
                        "Usage: cargo run --release --features directwrite-reference --example text_quality -- [--backend cpu|cubecl|both] [--out DIR] [--font FAMILY] [--full] [--color-sweep] [--tune-coverage] [--tune-iterations N] [--tune-seed N] [--tune-candidates CSV] [--tune-parallelism N] [--tune-color-groups N] [--tune-target-score SCORE] [--dump-case NAME] [--dump-coverage-results CSV]"
                    );
                    std::process::exit(0);
                }
                value => return Err(format!("unknown argument {value:?}").into()),
            }
        }
        Ok(Self {
            backend,
            out_dir,
            font_family,
            full,
            color_sweep,
            tune_coverage,
            tune_iterations,
            tune_seed,
            tune_candidates,
            tune_parallelism,
            tune_color_groups,
            tune_target_score,
            dump_case,
            dump_coverage_results,
        })
    }
}

fn sample_shifted(image: &Image, x: i32, y: i32, fallback: [u8; 4]) -> [u8; 4] {
    if x < 0 || y < 0 || x >= image.width as i32 || y >= image.height as i32 {
        return fallback;
    }
    rgba8_at(image, x as u32, y as u32)
}

fn rgba8_at(image: &Image, x: u32, y: u32) -> [u8; 4] {
    unpack_rgba8(image.pixels[(y * image.width + x) as usize])
}

fn pack_rgba8(rgba: [u8; 4]) -> u32 {
    u32::from(rgba[0])
        | (u32::from(rgba[1]) << 8)
        | (u32::from(rgba[2]) << 16)
        | (u32::from(rgba[3]) << 24)
}

fn unpack_rgba8(px: u32) -> [u8; 4] {
    [
        (px & 0xff) as u8,
        ((px >> 8) & 0xff) as u8,
        ((px >> 16) & 0xff) as u8,
        ((px >> 24) & 0xff) as u8,
    ]
}

fn color_to_rgba8(color: Color) -> [u8; 4] {
    let [r, g, b, a] = color.components;
    [
        (r * 255.0 + 0.5) as u8,
        (g * 255.0 + 0.5) as u8,
        (b * 255.0 + 0.5) as u8,
        (a * 255.0 + 0.5) as u8,
    ]
}

fn luma(rgba: [u8; 4]) -> f64 {
    let r = f64::from(rgba[0]) / 255.0;
    let g = f64::from(rgba[1]) / 255.0;
    let b = f64::from(rgba[2]) / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn rgb_chroma(rgba: [u8; 4]) -> f64 {
    let r = f64::from(rgba[0]) / 255.0;
    let g = f64::from(rgba[1]) / 255.0;
    let b = f64::from(rgba[2]) / 255.0;
    r.max(g).max(b) - r.min(g).min(b)
}

fn color_distance(a: [u8; 4], b: [u8; 4]) -> u16 {
    u16::from(a[0].abs_diff(b[0]))
        + u16::from(a[1].abs_diff(b[1]))
        + u16::from(a[2].abs_diff(b[2]))
        + u16::from(a[3].abs_diff(b[3]))
}

fn average_fringe(stats: ImageStats) -> f64 {
    if stats.fringe_pixels == 0 {
        0.0
    } else {
        stats.fringe / stats.fringe_pixels as f64
    }
}

fn bbox_csv(bbox: Option<Bounds>) -> String {
    match bbox {
        Some(b) => format!("{}:{}:{}:{}", b.x0, b.y0, b.x1, b.y1),
        None => "empty".to_string(),
    }
}

fn bbox_delta_csv(stats: DiffStats) -> String {
    let (dx, dy) = bbox_alignment_delta(stats);
    format!("{dx}:{dy}")
}

fn mode_name(mode: TextSubpixelMode) -> &'static str {
    match mode {
        TextSubpixelMode::None => "gray",
        TextSubpixelMode::Rgb => "rgb",
        TextSubpixelMode::Bgr => "bgr",
    }
}

fn origin_name(x: f32, y: f32) -> String {
    format!("{x:.3}:{y:.3}")
}

fn color_name(color: Color) -> String {
    let [r, g, b, a] = color_to_rgba8(color);
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric_row(bg: &str, ink_ratio: f64, mae_luma: f64, max_rgb: u8) -> MetricRow {
        metric_row_with_colors("#000000ff", bg, ink_ratio, mae_luma, max_rgb)
    }

    fn metric_row_with_colors(
        fg: &str,
        bg: &str,
        ink_ratio: f64,
        mae_luma: f64,
        max_rgb: u8,
    ) -> MetricRow {
        MetricRow {
            backend: "cpu".to_string(),
            font_size: "12.0".to_string(),
            subpixel: "rgb".to_string(),
            fg: fg.to_string(),
            bg: bg.to_string(),
            stats: DiffStats {
                ink_ratio,
                mae_luma,
                rmse_luma: mae_luma * 2.0,
                mae_rgb: mae_luma * 0.5,
                max_rgb,
                fringe_delta: -0.1,
                ..DiffStats::default()
            },
        }
    }

    #[test]
    fn summarize_metrics_groups_and_averages_rows() {
        let rows = [
            metric_row("#ffffffff", 1.2, 0.01, 100),
            metric_row("#ffffffff", 0.8, 0.03, 180),
            metric_row("#e0e0e0ff", 0.9, 0.02, 90),
        ];

        let summary = summarize_metrics(&rows);
        assert_eq!(summary.len(), 2);
        let white = summary
            .iter()
            .find(|row| row.key.bg == "#ffffffff")
            .expect("white summary row");
        assert_eq!(white.cases, 2);
        assert_eq!(white.avg_ink_ratio, 1.0);
        assert!((white.avg_abs_ink_error - 0.2).abs() < f64::EPSILON);
        assert!((white.avg_mae_luma - 0.02).abs() < f64::EPSILON);
        assert!((white.avg_rmse_luma - 0.04).abs() < f64::EPSILON);
        assert!((white.avg_mae_rgb - 0.01).abs() < f64::EPSILON);
        assert_eq!(white.max_rgb, 180);
    }

    #[test]
    fn summary_csv_uses_reference_neutral_metric_names() {
        let summary = summarize_metrics(&[metric_row("#ffffffff", 1.0, 0.01, 100)]);
        let csv = summary_csv(&summary);

        assert!(csv.starts_with("backend,font_size,subpixel,fg,bg,cases,avg_ink_ratio"));
        assert!(csv.contains("avg_abs_ink_error"));
        assert!(csv.contains("color_group,worst_mixed_rank"));
    }

    #[test]
    fn summary_csv_marks_and_ranks_worst_mixed_groups() {
        let rows = [
            metric_row_with_colors("#ff0000ff", "#0000ffff", 1.3, 0.04, 140),
            metric_row_with_colors("#00ff00ff", "#800080ff", 1.1, 0.02, 120),
            metric_row_with_colors("#000000ff", "#ffffffff", 1.4, 0.03, 160),
        ];
        let summary = summarize_metrics(&rows);
        let csv = summary_csv(&summary);

        let red_on_blue = csv
            .lines()
            .find(|line| line.contains("#ff0000ff,#0000ffff"))
            .expect("red on blue summary row");
        let green_on_purple = csv
            .lines()
            .find(|line| line.contains("#00ff00ff,#800080ff"))
            .expect("green on purple summary row");
        let black_on_white = csv
            .lines()
            .find(|line| line.contains("#000000ff,#ffffffff"))
            .expect("black on white summary row");

        assert!(red_on_blue.ends_with(",mixed,1"));
        assert!(green_on_purple.ends_with(",mixed,2"));
        assert!(black_on_white.ends_with(",other,"));
    }

    #[test]
    fn build_cases_keeps_default_corpus_focused() {
        let cases = build_cases(false, false);

        assert_eq!(cases.len(), 72);
        assert!(
            cases
                .iter()
                .any(|case| case.name == "rgb_black_on_white_12px_x0_y0")
        );
        assert!(!cases.iter().any(|case| case.name.contains("red_on_white")));
    }

    #[test]
    fn build_cases_color_sweep_adds_colored_foreground_and_background_pairs() {
        let cases = build_cases(false, true);

        assert_eq!(cases.len(), 648);
        assert!(
            cases
                .iter()
                .any(|case| case.name == "rgb_red_on_white_12px_x0_y0")
        );
        assert!(
            cases
                .iter()
                .any(|case| case.name == "bgr_white_on_blue_24px_x1of3_yhalf")
        );
        assert!(
            cases
                .iter()
                .any(|case| case.name == "rgb_red_on_blue_12px_x0_y0")
        );
        assert!(
            cases
                .iter()
                .any(|case| case.name == "gray_orange_on_teal_24px_x1of3_yhalf")
        );
        assert!(
            cases
                .iter()
                .any(|case| case.name == "rgb_magenta_on_green_dark_12px_x0_y0")
        );
    }

    #[test]
    fn build_cases_with_color_group_target_adds_generated_mixed_pairs() {
        let cases = build_cases_with_color_group_target(false, true, Some(512));

        assert_eq!(cases.len(), 512 * 3 * 3 * 2);
        assert!(
            cases
                .iter()
                .any(|case| case.name == "rgb_generated_mixed_000_12px_x0_y0")
        );
        assert!(
            cases
                .iter()
                .any(|case| case.name == "gray_generated_mixed_475_24px_x1of3_yhalf")
        );
    }

    #[test]
    fn parse_coverage_candidates_csv_keeps_defaults_for_missing_fields() {
        let candidates = parse_coverage_candidates_csv(
            "name,light_on_colored_dark_chroma_reduction,alpha_mask_chroma_scale\n\
             mixed_bias,1.15,2.25\n",
        )
        .expect("candidate csv");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "mixed_bias");
        assert_eq!(candidates[0].source, "input");
        assert_eq!(
            candidates[0].params.light_on_colored_dark_chroma_reduction,
            1.15
        );
        assert_eq!(candidates[0].params.alpha_mask_chroma_scale, 2.25);
        assert_eq!(
            candidates[0].params.dark_on_light_luma_base,
            TextCoverageParams::DEFAULT.dark_on_light_luma_base
        );
    }

    #[test]
    fn tuning_ink_error_uses_floor_for_low_contrast_reference_ink() {
        let stats = DiffStats {
            tileink_ink: 6.0,
            reference_ink: 1.0,
            ..DiffStats::default()
        };

        assert_eq!(tuning_ink_error(stats), 0.05);
    }

    #[test]
    fn apparent_coverage_image_projects_pixels_onto_foreground_background_axis() {
        let mut image = Image::new(3, 1, Color::BLACK);
        image.pixels = vec![
            pack_rgba8([0, 0, 0, 255]),
            pack_rgba8([128, 128, 128, 255]),
            pack_rgba8([255, 255, 255, 255]),
        ];

        let coverage = apparent_coverage_image(&image, Color::WHITE, Color::BLACK);

        assert_eq!(rgba8_at(&coverage, 0, 0), [0, 0, 0, 255]);
        assert_eq!(rgba8_at(&coverage, 1, 0), [128, 128, 128, 255]);
        assert_eq!(rgba8_at(&coverage, 2, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn aligned_reference_to_tileink_moves_reference_bbox_to_tileink_bbox() {
        let mut reference = Image::new(5, 3, Color::BLACK);
        reference.pixels[(5 + 3) as usize] = pack_rgba8([255, 255, 255, 255]);
        let stats = DiffStats {
            tileink_bbox: Some(Bounds {
                x0: 1,
                y0: 1,
                x1: 2,
                y1: 2,
            }),
            reference_bbox: Some(Bounds {
                x0: 3,
                y0: 1,
                x1: 4,
                y1: 2,
            }),
            ..DiffStats::default()
        };

        let aligned = aligned_reference_to_tileink(&reference, stats, Color::BLACK);

        assert_eq!(rgba8_at(&aligned, 1, 1), [255, 255, 255, 255]);
        assert_eq!(rgba8_at(&aligned, 3, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn gray_aligned_diff_stats_compares_shifted_reference_inside_text_bounds() {
        let mut tileink = Image::new(5, 3, Color::BLACK);
        let mut reference = Image::new(5, 3, Color::BLACK);
        tileink.pixels[(5 + 1) as usize] = pack_rgba8([128, 128, 128, 255]);
        reference.pixels[(5 + 3) as usize] = pack_rgba8([255, 255, 255, 255]);
        let stats = DiffStats {
            tileink_bbox: Some(Bounds {
                x0: 1,
                y0: 1,
                x1: 2,
                y1: 2,
            }),
            reference_bbox: Some(Bounds {
                x0: 3,
                y0: 1,
                x1: 4,
                y1: 2,
            }),
            ..DiffStats::default()
        };

        let stats = gray_aligned_diff_stats(&tileink, &reference, stats);

        assert_eq!(stats.pixels, 1);
        assert_eq!(stats.max_delta, 127);
        assert!((stats.tileink_sum - 128.0 / 255.0).abs() < 1.0e-12);
        assert!((stats.reference_sum - 1.0).abs() < 1.0e-12);
        assert!((stats.ratio - 128.0 / 255.0).abs() < 1.0e-12);
    }

    #[test]
    fn apparent_coverage_aligned_diff_stats_uses_foreground_background_axis() {
        let fg = Color::from_rgb8(0, 0, 128);
        let bg = Color::from_rgb8(128, 128, 0);
        let mut tileink = Image::new(5, 3, bg);
        let mut reference = Image::new(5, 3, bg);
        tileink.pixels[(5 + 1) as usize] = pack_rgba8([64, 64, 64, 255]);
        reference.pixels[(5 + 3) as usize] = pack_rgba8(color_to_rgba8(fg));
        let stats = DiffStats {
            tileink_bbox: Some(Bounds {
                x0: 1,
                y0: 1,
                x1: 2,
                y1: 2,
            }),
            reference_bbox: Some(Bounds {
                x0: 3,
                y0: 1,
                x1: 4,
                y1: 2,
            }),
            ..DiffStats::default()
        };

        let stats = apparent_coverage_aligned_diff_stats(&tileink, &reference, fg, bg, stats);

        assert_eq!(stats.pixels, 1);
        assert!((stats.tileink_sum - 0.5).abs() < 1.0e-12);
        assert!((stats.reference_sum - 1.0).abs() < 1.0e-12);
        assert!((stats.ratio - 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn tuning_case_coverage_error_uses_apparent_coverage_sum() {
        let fg = Color::from_rgb8(0, 0, 128);
        let bg = Color::from_rgb8(128, 128, 0);
        let case = QualityCase {
            name: "case".to_string(),
            text: TEXT,
            font_size: 12.0,
            layout_width: 100.0,
            margin: 0.0,
            origin_x: 0.0,
            origin_y: 0.0,
            foreground: fg,
            background: bg,
            subpixel: TextSubpixelMode::None,
        };
        let mut tileink = Image::new(1, 1, bg);
        let mut reference = Image::new(1, 1, bg);
        tileink.pixels[0] = pack_rgba8([64, 64, 64, 255]);
        reference.pixels[0] = pack_rgba8(color_to_rgba8(fg));
        let diff = DiffStats {
            tileink_bbox: Some(Bounds {
                x0: 0,
                y0: 0,
                x1: 1,
                y1: 1,
            }),
            reference_bbox: Some(Bounds {
                x0: 0,
                y0: 0,
                x1: 1,
                y1: 1,
            }),
            reference_ink: 1.0,
            tileink_ink: 1.0,
            ..DiffStats::default()
        };

        let error = tuning_case_coverage_error(&case, &tileink, &reference, diff);

        assert_eq!(error, 0.5 / TUNING_INK_REFERENCE_FLOOR);
    }

    #[test]
    fn apparent_coverage_reliability_keeps_neutral_and_high_luma_contrast_pairs() {
        assert!(
            apparent_coverage_reliability(Color::BLACK, Color::WHITE) > 0.999,
            "neutral black on white should keep full apparent coverage weight"
        );
        assert!(
            apparent_coverage_reliability(
                Color::from_rgb8(48, 88, 220),
                Color::from_rgb8(250, 220, 80),
            ) > 0.999,
            "high-luma-contrast chromatic pairs should keep full apparent coverage weight"
        );
    }

    #[test]
    fn apparent_coverage_reliability_downweights_low_luma_contrast_chromatic_pairs() {
        let reliability = apparent_coverage_reliability(
            Color::from_rgb8(0x32, 0x1f, 0xff),
            Color::from_rgb8(0x2e, 0x16, 0x15),
        );

        assert!(reliability < 0.40);
        assert!(reliability >= MIN_COVERAGE_RELIABILITY);
    }

    #[test]
    fn coverage_tuning_csv_roundtrips_for_resume() {
        let result = CoverageTuneResult {
            candidate: coverage_candidate("candidate_a", "test", TextCoverageParams::DEFAULT),
            score: 0.25,
            case_count: 10,
            mixed_group_count: 8,
            avg_mae_luma: 0.01,
            avg_mae_rgb: 0.02,
            avg_abs_ink_error: 0.03,
            mixed_avg_abs_ink_error: 0.04,
            worst_mixed_abs_ink_error: 0.05,
            worst_group_abs_ink_error: 0.06,
            avg_abs_fringe_delta: 0.07,
            max_rgb: 123,
        };

        let csv = coverage_tuning_csv(&[result]);
        let parsed = parse_coverage_tuning_results_csv(&csv).expect("resume csv");

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].candidate.name, "candidate_a");
        assert_eq!(parsed[0].candidate.source, "test");
        assert_eq!(parsed[0].case_count, 10);
        assert_eq!(parsed[0].mixed_group_count, 8);
        assert_eq!(parsed[0].max_rgb, 123);
        assert_eq!(parsed[0].candidate.params, TextCoverageParams::DEFAULT);
    }

    #[test]
    fn coverage_worst_mixed_groups_csv_includes_contrast_and_worst_case() {
        let result = CoverageTuneResult {
            candidate: coverage_candidate("candidate_a", "test", TextCoverageParams::DEFAULT),
            score: 0.25,
            case_count: 2,
            mixed_group_count: 1,
            avg_mae_luma: 0.0,
            avg_mae_rgb: 0.0,
            avg_abs_ink_error: 0.0,
            mixed_avg_abs_ink_error: 0.0,
            worst_mixed_abs_ink_error: 0.0,
            worst_group_abs_ink_error: 0.0,
            avg_abs_fringe_delta: 0.0,
            max_rgb: 0,
        };
        let detail = CoverageGroupDetail {
            key: SummaryKey {
                backend: "cpu".to_string(),
                font_size: "12.0".to_string(),
                subpixel: "gray".to_string(),
                fg: "#123456ff".to_string(),
                bg: "#203040ff".to_string(),
            },
            cases: 2,
            avg_robust_ink_error: 1.2,
            avg_mae_luma: 0.01,
            avg_mae_rgb: 0.02,
            avg_tileink_ink: 40.0,
            avg_reference_ink: 15.0,
            avg_abs_fringe_delta: 0.03,
            max_rgb: 99,
            reference_floor_hit_rate: 1.0,
            fg_luma: 0.2,
            bg_luma: 0.25,
            fg_chroma: 0.3,
            bg_chroma: 0.1,
            fit: CoverageFitStats {
                pixels: 10,
                current_sum_error: 0.4,
                current_rmse: 0.2,
                scale: 0.5,
                scale_sum_error: 0.0,
                scale_rmse: 0.1,
                scale_rmse_reduction: 0.5,
                axis_scale: 0.4,
                axis_bias: 0.1,
                axis_sum_error: 0.0,
                axis_rmse: 0.08,
                axis_rmse_reduction: 0.6,
                exponent: 1.25,
                exponent_scale: 0.7,
                exponent_sum_error: 0.02,
                exponent_rmse: 0.09,
                exponent_rmse_reduction: 0.55,
            },
            avg_abs_bbox_dx: 0.5,
            avg_abs_bbox_dy: 1.0,
            worst_case: "gray_generated_mixed_000_12px_x0_y0".to_string(),
            worst_case_robust_ink_error: 1.4,
            worst_case_tileink_ink: 45.0,
            worst_case_reference_ink: 10.0,
            worst_case_bbox_dx: -1,
            worst_case_bbox_dy: 2,
        };

        let csv = coverage_worst_mixed_groups_csv(&result, &[detail]);

        assert!(csv.contains("luma_contrast"));
        assert!(csv.contains("reference_floor_hit_rate"));
        assert!(csv.contains("scale_rmse_reduction"));
        assert!(csv.contains("gray_generated_mixed_000_12px_x0_y0"));
        assert!(csv.contains(",0.05000000,"));
    }

    #[test]
    fn coverage_fit_accum_reports_scale_residual_reduction() {
        let mut fit = CoverageFitAccum::default();
        fit.add(0.25, 0.5);
        fit.add(0.5, 1.0);

        let stats = fit.finish(0.0);

        assert!((stats.scale - 2.0).abs() < 1.0e-12);
        assert!(stats.current_rmse > 0.0);
        assert!(stats.scale_rmse < 1.0e-12);
        assert!(stats.scale_rmse_reduction > 0.999);
    }

    #[test]
    fn dedupe_coverage_results_keeps_best_score_for_same_params() {
        let mut worse = CoverageTuneResult {
            candidate: coverage_candidate("worse", "test", TextCoverageParams::DEFAULT),
            score: 2.0,
            case_count: 0,
            mixed_group_count: 0,
            avg_mae_luma: 0.0,
            avg_mae_rgb: 0.0,
            avg_abs_ink_error: 0.0,
            mixed_avg_abs_ink_error: 0.0,
            worst_mixed_abs_ink_error: 0.0,
            worst_group_abs_ink_error: 0.0,
            avg_abs_fringe_delta: 0.0,
            max_rgb: 0,
        };
        let mut better = worse.clone();
        better.candidate.name = "better".to_string();
        better.score = 1.0;
        worse.score = 3.0;

        let deduped = dedupe_coverage_results(vec![worse, better]);

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].candidate.name, "better");
        assert_eq!(deduped[0].score, 1.0);
    }

    #[test]
    fn coverage_target_reached_checks_best_score() {
        let result = CoverageTuneResult {
            candidate: coverage_candidate("candidate", "test", TextCoverageParams::DEFAULT),
            score: 0.42,
            case_count: 0,
            mixed_group_count: 0,
            avg_mae_luma: 0.0,
            avg_mae_rgb: 0.0,
            avg_abs_ink_error: 0.0,
            mixed_avg_abs_ink_error: 0.0,
            worst_mixed_abs_ink_error: 0.0,
            worst_group_abs_ink_error: 0.0,
            avg_abs_fringe_delta: 0.0,
            max_rgb: 0,
        };

        assert!(coverage_target_reached(
            std::slice::from_ref(&result),
            Some(0.5)
        ));
        assert!(!coverage_target_reached(&[result], Some(0.3)));
        assert!(!coverage_target_reached(&[], Some(0.5)));
        assert!(!coverage_target_reached(&[], None));
    }
}
