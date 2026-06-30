mod cpu;
mod cubecl;
mod debug;
mod render;
mod scene;
mod shared;
mod svg;
mod text;

pub const TILE_SIZE: u32 = 16;
pub const TILE_SCALE: f32 = 1.0 / TILE_SIZE as f32;
pub const BLOCK_SIZE: u32 = 16 * 16;

pub use cosmic_text::{
    Align as TextAlign, Attrs as TextAttrs, CacheKeyFlags as TextCacheKeyFlags,
    Family as TextFamily, Stretch as TextStretch, Style as TextStyle, Weight as TextWeight,
};
pub use cpu::Renderer as CpuRenderer;
#[cfg(feature = "bench-api")]
pub use cubecl::CubePreparedStage;
#[cfg(feature = "cuda")]
pub use cubecl::CudaRenderer as CubeCudaRenderer;
#[cfg(feature = "profile")]
pub use cubecl::{
    RenderProfile, RenderProfileEntry, RenderProfileEventSummary, RenderProfileMemoryEntry,
    RenderProfileMemorySpace, RenderProfileReport,
};
pub use cubecl::{Renderer as CubeRenderer, WgpuRenderer as CubeWgpuRenderer};
pub use debug::{
    DebugLineSegment, DebugTileDump, DebugTilePath, DebugTilePathSummary, DebugTileSummary,
    RenderDebugCapture, RenderDebugImage, RenderDebugOptions, RenderDebugText, RenderOptions,
    debug_capture_json,
};
pub use scene::Scene;
pub use shared::{
    bounds::Bounds,
    brush::Brush,
    fill::FillRule,
    image::{Image, ImageSaveError},
    layer::{
        filter::{
            CompositeOperator, ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting, Filter,
            FilterInput, FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator,
            RectLiquidGlass, SpecularLighting,
        },
        mask::{Mask, MaskKind},
        region::Region,
    },
    sdf::{
        Sdf, SdfShadow,
        arc::{Arc as SdfArc, ArcShadow as SdfArcShadow},
        candlestick::CandleStick,
        circle::{
            Circle as SdfCircle, CircleShadow as SdfCircleShadow, CircleStroke as SdfCircleStroke,
        },
        line::{Line as SdfLine, LineCap as SdfLineCap, LineShadow as SdfLineShadow},
        rect::{
            Radius, Rect as SdfRect, RectShadow as SdfRectShadow, RectShadowOptions,
            RectStroke as SdfRectStroke, StrokeWidths,
        },
        shadow::ShadowOptions,
    },
};
pub use svg::{SvgError, SvgOptions};
pub use text::{
    TextCompositeMode, TextContext, TextLayout, TextLayoutOptions, TextRasterOptions,
    TextSubpixelMode,
};
