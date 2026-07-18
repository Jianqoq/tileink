mod canvas;
mod debug;
mod retained_scene;
mod shared;
mod svg;
mod text;
mod wgpu;

pub const TILE_SIZE: u32 = 16;
pub const TILE_SCALE: f32 = 1.0 / TILE_SIZE as f32;
pub const BLOCK_SIZE: u32 = 16 * 16;

pub use crate::wgpu::{
    CoarseBinningMode, ExternalTextureHistoryId, FullRedrawReason, IncrementalOutputMode,
    IncrementalRenderConfig, IncrementalRenderMode, IncrementalRenderStats, Renderer,
    Renderer as WgpuRenderer, RendererOptions, RendererOptions as WgpuRendererOptions,
    WgpuRenderProfile, WgpuRenderProfileEntry, WgpuRenderProfileEventSummary,
    WgpuRenderProfileReport, WgpuTextureRenderError,
};
pub use canvas::{Canvas, DrawId, RetainedNodeId};
pub(crate) use canvas::{NodeGeneration, PersistentLayerKey};
pub use cosmic_text::{
    Align as TextAlign, Attrs as TextAttrs, CacheKeyFlags as TextCacheKeyFlags,
    Family as TextFamily, FontSystem as TextFontSystem, Stretch as TextStretch, Style as TextStyle,
    Weight as TextWeight, Wrap as TextWrap,
};
pub use debug::{
    DebugLineSegment, DebugTileDump, DebugTilePath, DebugTilePathSummary, DebugTileSummary,
    RenderDebugCapture, RenderDebugImage, RenderDebugOptions, RenderDebugText, RenderOptions,
    TileOverlayOptions, debug_capture_json,
};
#[cfg(feature = "bench-internals")]
pub use retained_scene::RetainedMaterializerBenchmark;
pub use retained_scene::{
    RetainedChildBranch, RetainedLayerDescriptor, RetainedParent, RetainedScene,
    RetainedSceneError, RetainedSceneTransaction, SceneVersion,
};
#[cfg(feature = "bench-internals")]
pub use shared::gpu_plan::{GpuDirtyRangesBenchmark, TileDrawBinsBenchmark};
#[cfg(feature = "bench-internals")]
pub use shared::scene_arena::{SceneArenaDirtyBenchmark, SceneArenaFillBenchmark};
pub use shared::{
    bounds::Bounds,
    brush::{Brush, PatternBrush, PatternSampling},
    fill::FillRule,
    image::{Image, ImageSaveError},
    image_resource::ImageKey,
    layer::{
        filter::{
            BlurDownsampleFilter, BlurSampling, BlurUpsampleFilter, CompositeOperator,
            ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting, Filter, FilterInput,
            FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator, RectLiquidGlass,
            SpecularLighting,
        },
        mask::{Mask, MaskKind},
        region::Region,
    },
    pixel::TextCoverageParams,
    sdf::{
        Sdf, SdfShadow,
        arc::{ArcShadow as SdfArcShadow, Rc as SdfArc},
        candlestick::CandleStick,
        circle::{
            Circle as SdfCircle, CircleShadow as SdfCircleShadow, CircleStroke as SdfCircleStroke,
        },
        line::{
            DashLine as SdfDashLine, Line as SdfLine, LineCap as SdfLineCap,
            LineShadow as SdfLineShadow,
        },
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
#[cfg(feature = "bench-internals")]
pub use wgpu::{DamageTilesBenchmark, GlyphCapacityBenchmark, GlyphCapacityBenchmarkCase};
#[cfg(feature = "bench-internals")]
pub use wgpu::{FrameDiffBenchmark, FrameDiffBenchmarkCase};
