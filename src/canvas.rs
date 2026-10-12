mod append;
mod compile;
mod draw;
mod journal;
mod layers;
mod offset;
mod painter;
mod paths;
mod primitives;
mod retained;
mod storage;
mod surface;

use std::{ops::Range, rc::Rc as SharedRc};

use peniko::{
    BlendMode, Color, Compose, Extend, Mix,
    kurbo::{
        Affine, Arc, BezPath, Circle, PathEl, Point, Rect, Shape, Stroke, StrokeOpts,
        stroke as kurbo_stroke,
    },
};

use crate::shared::{
    affine::GpuAffine,
    bounds::{Bounds, PixelBounds},
    brush::{Brush, PatternSampling, decode_encoded_brush, push_encoded_brush},
    draw_record::{DrawRecord, DrawTag},
    execution::{
        Command, CommandList, CommandListId, ExecOp, ExecPlan, LayerStackEntry,
        ROOT_COMMAND_LIST_ID, RetainedBatchBranch, RetainedBatchOwner,
    },
    fill::FillRule,
    gpu_sdf::{decode_sdf, decode_sdf_shadow, push_encoded_sdf, push_encoded_sdf_shadow},
    image::Image,
    image_resource::{ImageKey, ImageResourceStore},
    layer::{
        Layer, LayerKind,
        filter::{Filter, FilterPrimitive, FilterPrimitiveKind, LightSource},
        mask::Mask,
        region::Region,
    },
    line::Line,
    path::PathRecord,
    path_flatten::PathFlatten,
    scan_line::line_scanned_tile_count,
    sdf::{
        Sdf, SdfShadow,
        arc::{ArcShadow as SdfArcShadow, Rc as SdfArc},
        callout::{
            Callout as SdfCallout, CalloutShadow as SdfCalloutShadow,
            CalloutStroke as SdfCalloutStroke,
        },
        candlestick::CandleStick as SdfCandleStick,
        checkerboard::Checkerboard as SdfCheckerboard,
        circle::{
            Circle as SdfCircle, CircleShadow as SdfCircleShadow, CircleStroke as SdfCircleStroke,
        },
        line::{DashLine as SdfDashLine, Line as SdfLine, LineShadow as SdfLineShadow},
        rect::{
            Radius, Rect as SdfRect, RectShadow as SdfRectShadow, RectStroke as SdfRectStroke,
            StrokeWidths,
        },
        shadow::ShadowOptions,
        star::{Star as SdfStar, StarStroke as SdfStarStroke},
        triangle::Triangle as SdfTriangle,
    },
};
use crate::text::{TextRun, layout_bounds_at_scaled_origin, scene_glyphs_at_scaled_origin};
use crate::{TextContext, TextFontSystem, TextLayout};

pub(crate) use journal::MaterializationStamp;
pub(crate) use painter::{PainterKey, PainterMetadata};
pub use retained::RetainedNodeId;
pub(crate) mod damage_history;

pub(crate) use retained::{
    NodeRevision, PersistentLayerKey, RetainedDamage, RetainedFrame, RetainedFrameDelta,
    RetainedNodeKind, RetainedNodePatch, RetainedNodeState, RetainedSurfaceId,
};

const SDF_RECORD_FILL_RULE: FillRule = FillRule::NonZero;

#[derive(Clone)]
pub struct Canvas {
    pub(crate) lines: Vec<Line>,
    pub(crate) path_records: Vec<PathRecord>,
    pub(crate) draw_records: Vec<DrawRecord>,
    pub(crate) brush_blob: Vec<u32>,
    pub(crate) sdf_blob: Vec<u32>,
    pub(crate) sdf_shadow_blob: Vec<u32>,
    pub(crate) text_glyphs: Vec<crate::text::CanvasGlyph>,
    pub(crate) text_runs: Vec<TextRun>,
    pub(crate) scene_images: ImageResourceStore,
    pub(crate) command_lists: Vec<CommandList>,
    pub(crate) current_commands: CommandListId,
    pub(crate) layer_stack: Vec<OpenLayer>,
    pub(crate) path_cnt: u32,
    pub(crate) backdrop_pool_capacity: u32,
    pub(crate) tile_cnt: u32,
    pub(crate) logical_width: u32,
    pub(crate) logical_height: u32,
    pub(crate) scale_factor: f32,
    draw_generation: u32,
    pub(crate) persistent_root: Option<RetainedNodeId>,
    pub(crate) invalidated_bounds: Vec<Bounds>,
    pub(crate) invalidate_all: bool,
    pub(crate) buffer_changes: Option<SceneBufferChanges>,
    /// Stable identity for a precompiled persistent command topology.
    ///
    /// Ordinary canvases derive this by hashing commands. Persistent scenes assign an opaque key
    /// only when topology or physical draw slots change, so buffer-only edits do not scan the
    /// whole command graph during prepare.
    pub(crate) plan_cache_key: Option<u64>,
    /// Identity of one materialized scene's physical arenas. Descriptor/command edits
    /// can change the plan key without replacing geometry; a full rebuild gets a new identity.
    pub(crate) materialization_key: Option<u64>,
    pub(crate) compiled_plan: Option<SharedRc<ExecPlan>>,
    pub(crate) persistent_frame: Option<RetainedFrame>,
    pub(crate) painter: PainterMetadata,
    /// Local-to-device transform installed by retained materialization. Immediate canvases keep
    /// identity; retained transform edits use the previous value to patch command metadata by a
    /// delta without ever walking path lines.
    retained_transform: GpuAffine,
}

/// An open layer owns its return address and kind together. Root recording always
/// uses command list zero; no second stack or missing-root recovery is needed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OpenLayer {
    parent: CommandListId,
    kind: LayerKind,
}

#[derive(Clone, Debug, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Flags describe independent changes or statistics that may be true together"
)]
pub(crate) struct SceneBufferChanges {
    /// Materialized CPU version these ranges replace. Geometry reuse must follow
    /// this journal, independently of damage-frame compaction or plan patching.
    pub(crate) from_version: Option<u64>,
    pub(crate) lines: Vec<Range<usize>>,
    pub(crate) paths: Vec<Range<usize>>,
    pub(crate) draws: Vec<Range<usize>>,
    pub(crate) brushes: Vec<Range<usize>>,
    pub(crate) sdfs: Vec<Range<usize>>,
    pub(crate) shadows: Vec<Range<usize>>,
    pub(crate) glyphs: Vec<Range<usize>>,
    pub(crate) text_runs: Vec<Range<usize>>,
    pub(crate) chunks_rebuilt: u32,
    pub(crate) plan_fragments_rebuilt: u32,
    pub(crate) full_scene_sync: bool,
    /// The viewport changed and this frame necessarily redraws the full target.
    ///
    /// GPU preparation may use a dense one-frame spatial representation instead of rebuilding
    /// mutation indexes that cannot survive another resize. The next ordinary retained mutation
    /// restores those indexes before applying its dirty ranges.
    pub(crate) surface_changed: bool,
    pub(crate) cpu_copied_bytes: u64,
    pub(crate) painter: Vec<Range<usize>>,
    /// The persistent plan object was patched without changing execution structure. Renderers
    /// may reuse cached depth/scratch metadata while consuming the new precompiled plan.
    pub(crate) plan_structure_reused: bool,
    /// The plan has new descriptor values but unchanged buffer lengths and stack/scratch depth.
    /// Renderers consume the new precompiled plan while reusing only its size metadata.
    pub(crate) plan_values_patched: bool,
    /// Changed fused layer-stack records in the precompiled plan.
    pub(crate) plan_layer_stack: Vec<Range<usize>>,
    pub(crate) arena_live_bytes: u64,
    pub(crate) arena_capacity_bytes: u64,
    pub(crate) arena_fragmentation: f32,
    pub(crate) arena_compactions: u64,
}

/// Opaque handle to a draw stored inside a [`Canvas`].
///
/// `DrawId` is an O(1) index into the canvas's draw table plus a generation
/// check so handles from before [`Canvas::reset`] cannot accidentally mutate a
/// later draw with the same numeric index.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DrawId {
    index: u32,
    generation: u32,
}

impl DrawId {
    #[must_use]
    pub const fn index(self) -> usize {
        self.index as usize
    }
}

struct PathPushOptions {
    bounds_override: Option<Bounds>,
    brush: Brush,
    emit_draw_command: bool,
    tag: DrawTag,
}

fn valid_scale_factor(scale_factor: f32) -> f32 {
    assert!(
        scale_factor.is_finite() && scale_factor > 0.0,
        "canvas scale factor must be finite and positive"
    );
    scale_factor
}

fn scaled_canvas_extent(value: u32, scale: f32) -> u32 {
    (f64::from(value) * f64::from(scale))
        .ceil()
        .clamp(0.0, f64::from(u32::MAX)) as u32
}

fn scale_brush_transform(transform: [f32; 6], scale: f32) -> [f32; 6] {
    [
        transform[0] * scale,
        transform[1] * scale,
        transform[2] * scale,
        transform[3] * scale,
        transform[4],
        transform[5],
    ]
}

fn scaled_positive_u32(value: u32, scale: f32) -> u32 {
    ((value as f32) * scale)
        .round()
        .max(1.0)
        .min(u32::MAX as f32) as u32
}

fn image_rect_is_valid(rect: Rect) -> bool {
    rect.x0.is_finite()
        && rect.y0.is_finite()
        && rect.x1.is_finite()
        && rect.y1.is_finite()
        && rect.width() > 0.0
        && rect.height() > 0.0
}

#[derive(Clone, Copy)]
enum SceneAppendMode {
    MergeCurrent,
    AppendAsCommandList,
}

#[derive(Clone, Copy)]
struct SceneOffset {
    dx: f64,
    dy: f64,
}

#[cfg(test)]
mod tests;
