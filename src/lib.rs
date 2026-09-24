mod canvas;
mod debug;
mod render;
mod retained_scene;
mod shared;
mod svg;
mod text;

pub use shared::gpu_constants::TILE_SIZE;
pub const TILE_SCALE: f32 = 1.0 / TILE_SIZE as f32;
pub const BLOCK_SIZE: u32 = 16 * 16;

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
pub use render::damage_tiles::DamageTilesBenchmark;
pub use render::incremental::{
    CoarseBinningMode, FullRedrawReason, IncrementalOutputMode, IncrementalRenderConfig,
    IncrementalRenderMode, IncrementalRenderStats,
};
#[cfg(feature = "bench-internals")]
pub use render::incremental::{FrameDiffBenchmark, FrameDiffBenchmarkCase};
pub use render::output::ExternalTextureHistoryId;
#[cfg(feature = "bench-internals")]
pub use render::upload::glyph_capacity::{GlyphCapacityBenchmark, GlyphCapacityBenchmarkCase};
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
            FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator, ProgressiveBlur,
            ProgressiveBlurQuality, RectLiquidGlass, SpecularLighting,
        },
        mask::{Mask, MaskKind},
        region::Region,
    },
    pixel::TextCoverageParams,
    sdf::{
        Sdf, SdfShadow,
        arc::{ArcShadow as SdfArcShadow, Rc as SdfArc},
        callout::{
            Callout as SdfCallout, CalloutShadow as SdfCalloutShadow,
            CalloutSide as SdfCalloutSide, CalloutStroke as SdfCalloutStroke,
            CalloutTail as SdfCalloutTail,
        },
        candlestick::CandleStick,
        checkerboard::Checkerboard as SdfCheckerboard,
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
        star::{Star as SdfStar, StarStroke as SdfStarStroke},
        triangle::Triangle as SdfTriangle,
    },
};
pub use svg::{SvgError, SvgOptions};
#[cfg(feature = "bench-internals")]
pub use text::PreparedTextBenchmark;
pub use text::{
    TextCompositeMode, TextContext, TextLayout, TextLayoutOptions, TextRasterOptions,
    TextSubpixelMode,
};

#[cfg(feature = "bench-internals")]
#[doc(hidden)]
pub use render::upload::uniforms::benchmark::UniformWriteBenchmark;

#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
mod native;
#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
pub use native::{
    BackendUnavailable, BackendUnavailableReason, NativeBackend, NativeContext,
    NativeContextOptions, NativeError, NativeImageSubmission, NativeRenderTarget, NativeRenderer,
    NativeShaderArtifact, NativeSubmission, NativeTexture,
    SHADER_ARTIFACTS as NATIVE_SHADER_ARTIFACTS,
};

#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
pub use native::interop as native_interop;

#[cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
pub use native::{NativeTargetState, NativeTargetSubmission, NativeTargetUse};

mod backend_features;
