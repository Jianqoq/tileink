use std::{collections::HashMap, ptr};

use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{
    Canvas, Image, TextAttrs, TextCompositeMode, TextContext, TextCoverageParams, TextFamily,
    TextFontSystem, TextLayout, TextLayoutOptions, TextRasterOptions, TextSubpixelMode,
    WgpuRenderer,
};
use windows::{
    Win32::{
        Graphics::{
            Direct2D::{
                Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT},
                D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
                D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
                D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
                D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, D2D1CreateFactory, ID2D1Factory,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_PIXEL_GEOMETRY_BGR,
                DWRITE_PIXEL_GEOMETRY_RGB, DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
                DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
                IDWriteFactory,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGR, IWICImagingFactory,
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

use super::matrix::{FONT_FAMILY, HEIGHT, MARGIN, MatrixWeight, QualityCase, TEXT, WIDTH};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct LayoutKey {
    font_size: u8,
    weight: MatrixWeight,
}

pub(crate) struct DirectWriteReference {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
}

pub(crate) struct TileinkReference {
    font_system: TextFontSystem,
    text_context: TextContext,
    layouts: HashMap<LayoutKey, TextLayout>,
    renderer: WgpuRenderer,
    coverage: TextCoverageParams,
}

impl TileinkReference {
    pub(crate) fn new(embolden: Option<f32>) -> Self {
        let font_system = TextFontSystem::new();
        let text_context = TextContext::new();
        let mut coverage = TextCoverageParams::DEFAULT;
        if let Some(value) = embolden {
            coverage.subpixel_mask_embolden = value;
        }
        Self {
            font_system,
            text_context,
            layouts: HashMap::new(),
            renderer: WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT),
            coverage,
        }
    }

    pub(crate) fn render(&mut self, case: &QualityCase) -> Image {
        self.text_context.set_raster_options(
            TextRasterOptions::new()
                .with_subpixel_mode(case.subpixel)
                .with_composite_mode(TextCompositeMode::Linear)
                .with_coverage_params(self.coverage),
        );
        let key = LayoutKey {
            font_size: case.font_size,
            weight: case.weight,
        };
        if !self.layouts.contains_key(&key) {
            let layout = self.text_context.layout(
                &mut self.font_system,
                TextLayoutOptions::new(TEXT, f32::from(case.font_size)).with_attrs(
                    TextAttrs::new()
                        .family(TextFamily::Name(FONT_FAMILY))
                        .weight(case.weight.tileink()),
                ),
            );
            assert!(
                !layout.is_empty(),
                "Segoe UI should be available on Windows"
            );
            self.layouts.insert(key, layout);
        }
        let layout = self.layouts.get(&key).expect("layout was inserted");
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
        canvas.push_rect(
            Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
            tileink::Radius::ZERO,
            case.background_color(),
        );
        canvas.push_text_layout(
            layout,
            Point::new(
                f64::from(MARGIN + case.phase()),
                f64::from(MARGIN) - f64::from(layout.bounds().y0),
            ),
            case.foreground_color(),
        );
        self.renderer
            .render_with_text(&canvas, &mut self.font_system, &mut self.text_context);
        self.renderer.image()
    }
}

impl DirectWriteReference {
    pub(crate) fn new() -> windows::core::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            Ok(Self {
                d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?,
            })
        }
    }

    pub(crate) fn render(&self, case: &QualityCase) -> windows::core::Result<Image> {
        unsafe {
            let bitmap = self.wic.CreateBitmap(
                WIDTH,
                HEIGHT,
                &GUID_WICPixelFormat32bppBGR,
                WICBitmapCacheOnLoad,
            )?;
            let target = self.d2d.CreateWicBitmapRenderTarget(
                &bitmap,
                &D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        // ClearType requires an opaque render target. Premultiplied-alpha WIC
                        // targets silently fall back to grayscale antialiasing.
                        alphaMode: D2D1_ALPHA_MODE_IGNORE,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    usage: D2D1_RENDER_TARGET_USAGE_NONE,
                    minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                },
            )?;

            let foreground = color_to_d2d(case.foreground_color());
            let background = color_to_d2d(case.background_color());
            let brush = target.CreateSolidColorBrush(&foreground, None)?;
            let family = wide_null(FONT_FAMILY);
            let locale = wide_null("en-us");
            let text = wide(TEXT);
            let format = self.dwrite.CreateTextFormat(
                PCWSTR(family.as_ptr()),
                None,
                match case.weight {
                    MatrixWeight::Regular => DWRITE_FONT_WEIGHT_NORMAL,
                    MatrixWeight::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
                    MatrixWeight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
                },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                f32::from(case.font_size),
                PCWSTR(locale.as_ptr()),
            )?;
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let layout = self.dwrite.CreateTextLayout(
                &text,
                &format,
                WIDTH as f32 - MARGIN * 2.0,
                HEIGHT as f32,
            )?;
            let rendering = self.dwrite.CreateCustomRenderingParams(
                2.2,
                1.0,
                1.0,
                match case.subpixel {
                    TextSubpixelMode::Rgb => DWRITE_PIXEL_GEOMETRY_RGB,
                    TextSubpixelMode::Bgr => DWRITE_PIXEL_GEOMETRY_BGR,
                    TextSubpixelMode::None => unreachable!("matrix only exercises LCD geometry"),
                },
                DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
            )?;

            target.BeginDraw();
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
            target.SetTextRenderingParams(&rendering);
            target.Clear(Some(&background));
            target.DrawTextLayout(
                Vector2 {
                    X: MARGIN + case.phase(),
                    Y: MARGIN,
                },
                &layout,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            target.EndDraw(None, None)?;

            let stride = WIDTH * 4;
            let mut bgra = vec![0; (stride * HEIGHT) as usize];
            bitmap.CopyPixels(ptr::null(), stride, &mut bgra)?;
            let rgba = bgra
                .chunks_exact(4)
                .flat_map(|pixel| [pixel[2], pixel[1], pixel[0], 255])
                .collect::<Vec<_>>();
            Ok(Image::from_rgba8(WIDTH, HEIGHT, rgba))
        }
    }
}

fn color_to_d2d(color: Color) -> D2D1_COLOR_F {
    let [r, g, b, a] = color.components;
    D2D1_COLOR_F { r, g, b, a }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
