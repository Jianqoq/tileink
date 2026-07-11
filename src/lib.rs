#![allow(dead_code)]

mod canvas;
mod debug;
mod render;
mod retained_scene;
mod shared;
mod svg;
mod text;
mod wgpu;

pub const TILE_SIZE: u32 = 16;
pub const TILE_SCALE: f32 = 1.0 / TILE_SIZE as f32;
pub const BLOCK_SIZE: u32 = 16 * 16;

pub use crate::wgpu::{
    ExternalTextureHistoryId, FullRedrawReason, IncrementalOutputMode, IncrementalRenderConfig,
    IncrementalRenderMode, IncrementalRenderStats, Renderer, Renderer as WgpuRenderer,
    RendererOptions, RendererOptions as WgpuRendererOptions, WgpuRenderProfile,
    WgpuRenderProfileEntry, WgpuRenderProfileEventSummary, WgpuRenderProfileReport,
    WgpuTextureRenderError,
};
pub use canvas::{Canvas, DrawId, RetainedLayerKey, RetainedNodeId, SceneRevision};
pub use cosmic_text::{
    Align as TextAlign, Attrs as TextAttrs, CacheKeyFlags as TextCacheKeyFlags,
    Family as TextFamily, FontSystem as TextFontSystem, Stretch as TextStretch, Style as TextStyle,
    Weight as TextWeight,
};
pub use debug::{
    DebugLineSegment, DebugTileDump, DebugTilePath, DebugTilePathSummary, DebugTileSummary,
    RenderDebugCapture, RenderDebugImage, RenderDebugOptions, RenderDebugText, RenderOptions,
    TileOverlayOptions, debug_capture_json,
};
pub use retained_scene::{
    RetainedChildBranch, RetainedLayerDescriptor, RetainedParent, RetainedScene,
    RetainedSceneError, RetainedSceneTransaction, SceneVersion,
};
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
        arc::{Arc as SdfArc, ArcShadow as SdfArcShadow},
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
