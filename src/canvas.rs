mod append;
mod compile;
mod draw;
mod layers;
mod paths;
mod primitives;
mod retained;
mod storage;
mod surface;

use std::{ops::Range, rc::Rc as SharedRc};

use peniko::{
    Color, Compose, Extend, Mix,
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
        blend::Blend,
        filter::{Filter, FilterPrimitive, FilterPrimitiveKind, LightSource},
        mask::Mask,
        opacity::Opacity,
        region::Region,
    },
    line::Line,
    path::PathRecord,
    path_flatten::PathFlatten,
    scan_line::line_scanned_tile_count,
    sdf::{
        Sdf, SdfShadow,
        arc::{ArcShadow as SdfArcShadow, Rc as SdfArc},
        candlestick::CandleStick as SdfCandleStick,
        checkerboard::Checkerboard as SdfCheckerboard,
        circle::{
            Circle as SdfCircle, CircleShadow as SdfCircleShadow, CircleStroke as SdfCircleStroke,
        },
        line::{DashLine as SdfDashLine, Line as SdfLine, LineShadow as SdfLineShadow},
        rect::{
            Radius, Rect as SdfRect, RectShadow as SdfRectShadow, RectShadowOptions,
            RectStroke as SdfRectStroke, StrokeWidths,
        },
        triangle::Triangle as SdfTriangle,
    },
};
use crate::text::{TextRun, layout_bounds_at_scaled_origin, scene_glyphs_at_scaled_origin};
use crate::{TextContext, TextFontSystem, TextLayout};

pub use retained::RetainedNodeId;
pub(crate) use retained::{
    NodeGeneration, PersistentLayerKey, RetainedDamage, RetainedFrame, RetainedFrameDelta,
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
    pub(crate) root_commands: CommandListId,
    pub(crate) command_stack: Vec<CommandListId>,
    pub(crate) layer_stack: Vec<LayerKind>,
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
    pub(crate) compiled_plan: Option<SharedRc<ExecPlan>>,
    pub(crate) persistent_frame: Option<RetainedFrame>,
    pub(crate) painter_keys: Option<Vec<PainterKey>>,
    pub(crate) stable_batch_ids: Option<Vec<u32>>,
    /// Live physical draw count per stable batch. Persistent plans may keep an empty placeholder
    /// or a stale shared draw list while membership moves through the stable ID table.
    pub(crate) stable_batch_counts: Option<Vec<u32>>,
    /// Local-to-device transform installed by retained materialization. Immediate canvases keep
    /// identity; retained transform edits use the previous value to patch command metadata by a
    /// delta without ever walking path lines.
    retained_transform: GpuAffine,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SceneBufferChanges {
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
    /// Offscreen filter descriptors changed and their auxiliary GPU tables must be refreshed.
    pub(crate) filter_resources_changed: bool,
    pub(crate) arena_live_bytes: u64,
    pub(crate) arena_capacity_bytes: u64,
    pub(crate) arena_fragmentation: f32,
    pub(crate) arena_compactions: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PainterKey {
    pub(crate) path: SharedRc<[u128]>,
    pub(crate) local: u32,
}

impl PainterKey {
    pub(crate) fn inactive() -> Self {
        Self {
            path: SharedRc::from([u128::MAX]),
            local: u32::MAX,
        }
    }
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
    pub fn index(self) -> usize {
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
    ((value as f64) * f64::from(scale))
        .ceil()
        .clamp(0.0, u32::MAX as f64) as u32
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

impl SceneOffset {
    fn new(pos: Point) -> Self {
        assert!(
            pos.x.is_finite() && pos.y.is_finite(),
            "canvas append position must be finite"
        );
        Self {
            dx: pos.x,
            dy: pos.y,
        }
    }

    fn is_zero(self) -> bool {
        self.dx == 0.0 && self.dy == 0.0
    }

    fn line(self, line: &mut Line) {
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        line.p0[0] += dx;
        line.p0[1] += dy;
        line.p1[0] += dx;
        line.p1[1] += dy;
    }

    fn pixel_bounds(self, bounds: PixelBounds) -> PixelBounds {
        PixelBounds {
            x0: (bounds.x0 as f64 + self.dx).floor() as i32,
            y0: (bounds.y0 as f64 + self.dy).floor() as i32,
            x1: (bounds.x1 as f64 + self.dx).ceil() as i32,
            y1: (bounds.y1 as f64 + self.dy).ceil() as i32,
        }
    }

    fn bounds(self, bounds: Bounds) -> Bounds {
        Bounds::new(
            (bounds.x0 as f64 + self.dx).floor() as i32,
            (bounds.y0 as f64 + self.dy).floor() as i32,
            (bounds.x1 as f64 + self.dx).ceil() as i32,
            (bounds.y1 as f64 + self.dy).ceil() as i32,
        )
    }

    fn rect(self, rect: Rect) -> Rect {
        Rect::new(
            rect.x0 + self.dx,
            rect.y0 + self.dy,
            rect.x1 + self.dx,
            rect.y1 + self.dy,
        )
    }

    fn transform(self, transform: Affine) -> Affine {
        Affine::translate((self.dx, self.dy)) * transform
    }

    fn sdf(self, sdf: Sdf) -> Sdf {
        // SDF::translated subtracts its arguments for offscreen local-space
        // conversion. Appending moves local child geometry into parent space.
        sdf.translated(-(self.dx as f32), -(self.dy as f32))
    }

    fn sdf_shadow(self, sdf_shadow: SdfShadow) -> SdfShadow {
        sdf_shadow.translated(-(self.dx as f32), -(self.dy as f32))
    }

    fn brush(self, brush: Brush) -> Brush {
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        match brush {
            Brush::Solid(_) => brush,
            Brush::Linear(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Linear(gradient)
            }
            Brush::Radial(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Radial(gradient)
            }
            Brush::Sweep(mut gradient) => {
                gradient.center[0] += dx;
                gradient.center[1] += dy;
                Brush::Sweep(gradient)
            }
            Brush::FourCorner(mut gradient) => {
                gradient.bounds[0] += dx;
                gradient.bounds[1] += dy;
                gradient.bounds[2] += dx;
                gradient.bounds[3] += dy;
                Brush::FourCorner(gradient)
            }
            Brush::Pattern(mut pattern) => {
                pattern.transform = self.brush_transform(pattern.transform);
                Brush::Pattern(pattern)
            }
        }
    }

    fn brush_transform(self, transform: [f32; 6]) -> [f32; 6] {
        let [a, b, c, d, e, f] = transform;
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        [a, b, c, d, e - a * dx - c * dy, f - b * dx - d * dy]
    }

    fn layer(self, layer: Layer) -> Layer {
        match layer {
            Layer::Clip | Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_) => layer,
            Layer::ClipSdf { sdf, bounds } => Layer::ClipSdf {
                sdf: self.sdf(sdf),
                bounds: self.bounds(bounds),
            },
            Layer::Filter {
                filter,
                sample_region,
            } => Layer::Filter {
                filter: self.filter(filter),
                sample_region: self.region(sample_region),
            },
            Layer::Backdrop {
                filter,
                sample_region,
            } => Layer::Backdrop {
                filter: self.filter(filter),
                sample_region: self.region(sample_region),
            },
        }
    }

    fn mask(self, mask: Mask) -> Mask {
        Mask {
            region: self.region(mask.region),
            kind: mask.kind,
        }
    }

    fn filter(self, filter: Filter) -> Filter {
        match filter {
            Filter::Chain {
                filters,
                fixed_region,
            } => Filter::Chain {
                filters: filters
                    .into_iter()
                    .map(|filter| self.filter(filter))
                    .collect(),
                fixed_region,
            },
            Filter::Graph {
                primitives,
                fixed_region,
            } => Filter::Graph {
                primitives: primitives
                    .into_iter()
                    .map(|primitive| FilterPrimitive {
                        input: primitive.input,
                        input2: primitive.input2,
                        region: self.bounds(primitive.region),
                        kind: self.primitive_kind(primitive.kind),
                    })
                    .collect(),
                fixed_region,
            },
            Filter::Flood { brush } => Filter::Flood {
                brush: self.brush(brush),
            },
            Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                brush,
            } => Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                brush: self.brush(brush),
            },
            Filter::DiffuseLighting(mut lighting) => {
                lighting.light_source = self.light_source(lighting.light_source);
                Filter::DiffuseLighting(lighting)
            }
            Filter::SpecularLighting(mut lighting) => {
                lighting.light_source = self.light_source(lighting.light_source);
                Filter::SpecularLighting(lighting)
            }
            _ => filter,
        }
    }

    fn light_source(self, source: LightSource) -> LightSource {
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        match source {
            LightSource::Distant { .. } => source,
            LightSource::Point { x, y, z } => LightSource::Point {
                x: x + dx,
                y: y + dy,
                z,
            },
            LightSource::Spot {
                x,
                y,
                z,
                points_at_x,
                points_at_y,
                points_at_z,
                specular_exponent,
                limiting_cone_angle,
            } => LightSource::Spot {
                x: x + dx,
                y: y + dy,
                z,
                points_at_x: points_at_x + dx,
                points_at_y: points_at_y + dy,
                points_at_z,
                specular_exponent,
                limiting_cone_angle,
            },
        }
    }

    fn primitive_kind(self, kind: FilterPrimitiveKind) -> FilterPrimitiveKind {
        match kind {
            FilterPrimitiveKind::Filter(filter) => {
                FilterPrimitiveKind::Filter(Box::new(self.filter(*filter)))
            }
            FilterPrimitiveKind::Image { brush } => FilterPrimitiveKind::Image {
                brush: self.brush(brush),
            },
            FilterPrimitiveKind::Tile { source_region } => FilterPrimitiveKind::Tile {
                source_region: self.bounds(source_region),
            },
            FilterPrimitiveKind::Turbulence(mut turbulence) => {
                turbulence.transform_x += self.dx as f32;
                turbulence.transform_y += self.dy as f32;
                turbulence.tile_x += self.dx as f32;
                turbulence.tile_y += self.dy as f32;
                FilterPrimitiveKind::Turbulence(turbulence)
            }
            _ => kind,
        }
    }

    fn region(self, region: Region) -> Region {
        match region {
            Region::Rect { rect, radius } => Region::rect(self.rect(rect), radius),
            Region::Path {
                path,
                transform,
                tolerance,
            } => Region::path(path, self.transform(transform), tolerance),
        }
    }

    fn command(self, command: &mut Command) {
        match command {
            Command::Draw(_) => {}
            Command::MaterializedRetainedScene { .. } => {}
            Command::Layer { layer, .. } => {
                *layer = self.layer(layer.clone());
            }
            Command::MaskLayer { layer, .. } => {
                *layer = self.mask(layer.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests;
