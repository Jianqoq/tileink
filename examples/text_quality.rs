//! Generates a text raster quality corpus against Windows DirectWrite.
//!
//! The harness keeps Tileink and DirectWrite on the same explicit font family,
//! then compares rendered pixels after visual-bbox alignment. That separates
//! raster quality issues from normal layout overhang and baseline differences.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use peniko::{Color, kurbo::Point};
use tileink::{
    CpuRenderer, CubeWgpuRenderer, Image, Scene, TextAttrs, TextCompositeMode, TextContext,
    TextFamily, TextLayoutOptions, TextRasterOptions, TextSubpixelMode,
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

#[derive(Clone, Copy, Debug)]
struct ColorCase {
    name: &'static str,
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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse()?;
    let cases = build_cases(options.full);
    fs::create_dir_all(&options.out_dir)?;

    let directwrite = directwrite::DirectWriteReference::new()?;
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
    let mut csv = String::from(
        "backend,font_size,subpixel,fg,bg,cases,avg_ink_ratio,avg_abs_ink_error,avg_mae_luma,avg_rmse_luma,avg_mae_rgb,max_rgb,avg_fringe_delta\n",
    );
    for row in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{:.6},{:.6},{:.8},{:.8},{:.8},{},{:.8}\n",
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
        ));
    }
    csv
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
}

fn render_tileink(case: &QualityCase, font_family: &str, backend: Backend) -> Image {
    let mut context = TextContext::new();
    context.set_raster_options(
        TextRasterOptions::new()
            .with_subpixel_mode(case.subpixel)
            .with_composite_mode(TextCompositeMode::Linear),
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
            renderer.render_with_text(&scene, &mut context);
            renderer.image().clone()
        }
        Backend::Cubecl => {
            let mut renderer = CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, case.background);
            renderer.render_with_text(&scene, &mut context);
            renderer.image()
        }
        Backend::Both => unreachable!("render_tileink needs one concrete backend"),
    }
}

fn build_cases(full: bool) -> Vec<QualityCase> {
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
    let color_cases = [
        ColorCase {
            name: "black_on_white",
            foreground: Color::BLACK,
            background: Color::WHITE,
        },
        ColorCase {
            name: "white_on_black",
            foreground: Color::WHITE,
            background: Color::BLACK,
        },
        ColorCase {
            name: "black_on_gray",
            foreground: Color::BLACK,
            background: Color::from_rgb8(224, 224, 224),
        },
        ColorCase {
            name: "white_on_gray",
            foreground: Color::WHITE,
            background: Color::from_rgb8(48, 48, 48),
        },
    ];

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

struct DiffResult {
    image: Image,
    stats: DiffStats,
}

fn diff_images(tileink: &Image, reference: &Image, background: Color) -> DiffResult {
    assert_eq!(
        (tileink.width, tileink.height),
        (reference.width, reference.height)
    );
    let bg = color_to_rgba8(background);
    let tileink_stats = image_stats(tileink, background);
    let reference_stats = image_stats(reference, background);
    let tileink_bbox = tileink_stats.bbox.unwrap_or_default();
    let reference_bbox = reference_stats.bbox.unwrap_or_default();
    let dx = reference_bbox.x0 as i32 - tileink_bbox.x0 as i32;
    let dy = reference_bbox.y0 as i32 - tileink_bbox.y0 as i32;

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
                "--help" | "-h" => {
                    println!(
                        "Usage: cargo run --release --features directwrite-reference --example text_quality -- [--backend cpu|cubecl|both] [--out DIR] [--font FAMILY] [--full]"
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
        MetricRow {
            backend: "cpu".to_string(),
            font_size: "12.0".to_string(),
            subpixel: "rgb".to_string(),
            fg: "#000000ff".to_string(),
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
    }
}
