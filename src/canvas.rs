mod retained;

use std::{ops::Range, sync::Arc as SharedArc};

use peniko::{
    Color, Compose, Extend, Mix,
    kurbo::{
        Affine, Arc, BezPath, Circle, PathEl, Point, Rect, Shape, Stroke, StrokeOpts,
        stroke as kurbo_stroke,
    },
};

use crate::shared::{
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
        arc::{Arc as SdfArc, ArcShadow as SdfArcShadow},
        candlestick::CandleStick as SdfCandleStick,
        circle::{
            Circle as SdfCircle, CircleShadow as SdfCircleShadow, CircleStroke as SdfCircleStroke,
        },
        line::{DashLine as SdfDashLine, Line as SdfLine, LineShadow as SdfLineShadow},
        rect::{
            Radius, Rect as SdfRect, RectShadow as SdfRectShadow, RectShadowOptions,
            RectStroke as SdfRectStroke, StrokeWidths,
        },
    },
};
use crate::text::{TextRun, layout_bounds_at_scaled_origin, scene_glyphs_at_scaled_origin};
use crate::{TextContext, TextFontSystem, TextLayout};

pub(crate) use retained::{
    RetainedDamage, RetainedFrame, RetainedFrameDelta, RetainedNodeKind, RetainedNodePatch,
    RetainedNodeState, RetainedSceneCache, RetainedSurfaceId,
};
pub use retained::{RetainedLayerKey, RetainedNodeId, SceneRevision};

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
    pub(crate) retained_root: Option<RetainedNodeId>,
    pub(crate) invalidated_bounds: Vec<Bounds>,
    pub(crate) invalidate_all: bool,
    pub(crate) buffer_changes: Option<SceneBufferChanges>,
    /// Stable identity for a precompiled persistent command topology.
    ///
    /// Ordinary canvases derive this by hashing commands. Persistent scenes assign an opaque key
    /// only when topology or physical draw slots change, so buffer-only edits do not scan the
    /// whole command graph during prepare.
    pub(crate) plan_cache_key: Option<u64>,
    pub(crate) compiled_plan: Option<SharedArc<ExecPlan>>,
    pub(crate) retained_frame_override: Option<RetainedFrame>,
    pub(crate) painter_keys: Option<Vec<PainterKey>>,
    pub(crate) stable_batch_ids: Option<Vec<u32>>,
    /// Live physical draw count per stable batch. Persistent plans may keep an empty placeholder
    /// or a stale shared draw list while membership moves through the stable ID table.
    pub(crate) stable_batch_counts: Option<Vec<u32>>,
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
    pub(crate) cpu_copied_bytes: u64,
    pub(crate) painter: Vec<Range<usize>>,
    /// The persistent plan object was patched without changing execution structure. Renderers
    /// may reuse cached depth/scratch metadata while consuming the new precompiled plan.
    pub(crate) plan_structure_reused: bool,
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
    pub(crate) path: SharedArc<[u128]>,
    pub(crate) local: u32,
}

impl PainterKey {
    pub(crate) fn inactive() -> Self {
        Self {
            path: SharedArc::from([u128::MAX]),
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
            Command::RetainedScene { offset, .. } => {
                offset.0 += self.dx;
                offset.1 += self.dy;
            }
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

impl Canvas {
    pub fn new(logical_width: u32, logical_height: u32, scale_factor: f32) -> Self {
        let scale = valid_scale_factor(scale_factor);
        Self {
            lines: Vec::new(),
            path_records: Vec::new(),
            draw_records: Vec::new(),
            brush_blob: Vec::new(),
            sdf_blob: Vec::new(),
            sdf_shadow_blob: Vec::new(),
            text_glyphs: Vec::new(),
            text_runs: Vec::new(),
            scene_images: ImageResourceStore::default(),
            command_lists: vec![CommandList::default()],
            root_commands: ROOT_COMMAND_LIST_ID,
            command_stack: vec![ROOT_COMMAND_LIST_ID],
            layer_stack: Vec::new(),
            path_cnt: 0,
            backdrop_pool_capacity: 0,
            tile_cnt: 0,
            logical_width,
            logical_height,
            scale_factor: scale,
            draw_generation: 0,
            retained_root: None,
            invalidated_bounds: Vec::new(),
            invalidate_all: false,
            buffer_changes: None,
            plan_cache_key: None,
            compiled_plan: None,
            retained_frame_override: None,
            painter_keys: None,
            stable_batch_ids: None,
            stable_batch_counts: None,
        }
    }

    /// Creates a frame whose retained identity is stable across Canvas values.
    ///
    /// The renderer uses `root_id` to select the previous frame texture and to
    /// diff retained descendants. Callers must use a different id for unrelated
    /// render targets.
    pub fn new_retained(
        logical_width: u32,
        logical_height: u32,
        scale_factor: f32,
        root_id: RetainedNodeId,
    ) -> Self {
        let mut canvas = Self::new(logical_width, logical_height, scale_factor);
        canvas.retained_root = Some(root_id);
        canvas
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (self.physical_width(), self.physical_height())
    }

    pub fn logical_size(&self) -> (u32, u32) {
        (self.logical_width, self.logical_height)
    }

    /// Returns whether this canvas records stable retained scene history.
    pub fn is_retained(&self) -> bool {
        self.retained_root.is_some()
    }

    pub(crate) fn is_closed_for_append(&self) -> bool {
        self.command_stack.len() == 1 && self.layer_stack.is_empty()
    }

    pub fn physical_width(&self) -> u32 {
        scaled_canvas_extent(self.logical_width, self.scale_factor)
    }

    pub fn physical_height(&self) -> u32 {
        scaled_canvas_extent(self.logical_height, self.scale_factor)
    }

    fn scale_f64(&self) -> f64 {
        f64::from(self.scale_factor)
    }

    fn scale_f32(&self) -> f32 {
        self.scale_factor
    }

    fn device_transform(&self) -> Affine {
        Affine::scale(self.scale_f64())
    }

    fn device_tolerance(&self, tolerance: f64) -> f64 {
        tolerance * self.scale_f64()
    }

    fn physical_point(&self, point: Point) -> Point {
        Point::new(point.x * self.scale_f64(), point.y * self.scale_f64())
    }

    fn physical_rect(&self, rect: Rect) -> Rect {
        let scale = self.scale_f64();
        Rect::new(
            rect.x0 * scale,
            rect.y0 * scale,
            rect.x1 * scale,
            rect.y1 * scale,
        )
    }

    fn physical_bounds(&self, bounds: Bounds) -> Bounds {
        let scale = self.scale_f32();
        Bounds::new(
            (bounds.x0 as f32 * scale).floor() as i32,
            (bounds.y0 as f32 * scale).floor() as i32,
            (bounds.x1 as f32 * scale).ceil() as i32,
            (bounds.y1 as f32 * scale).ceil() as i32,
        )
    }

    fn physical_radius(&self, radius: Radius) -> Radius {
        let scale = self.scale_f32();
        Radius {
            top_left: radius.top_left * scale,
            top_right: radius.top_right * scale,
            bottom_left: radius.bottom_left * scale,
            bottom_right: radius.bottom_right * scale,
        }
    }

    fn physical_stroke_widths(&self, widths: StrokeWidths) -> StrokeWidths {
        let scale = self.scale_f32();
        StrokeWidths {
            top: widths.top * scale,
            right: widths.right * scale,
            bottom: widths.bottom * scale,
            left: widths.left * scale,
        }
    }

    fn physical_shadow_options(&self, options: RectShadowOptions) -> RectShadowOptions {
        let scale = self.scale_f32();
        RectShadowOptions {
            offset_x: options.offset_x * scale,
            offset_y: options.offset_y * scale,
            expand: options.expand * scale,
            intensity: options.intensity,
        }
    }

    fn physical_filter(&self, filter: Filter) -> Filter {
        let scale = self.scale_f32();
        match filter {
            Filter::Chain {
                filters,
                fixed_region,
            } => Filter::Chain {
                filters: filters
                    .into_iter()
                    .map(|filter| self.physical_filter(filter))
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
                        region: self.physical_bounds(primitive.region),
                        kind: self.physical_filter_primitive_kind(primitive.kind),
                    })
                    .collect(),
                fixed_region,
            },
            Filter::RectLiquidGlass(mut glass) => {
                glass.blur_radius = ((glass.blur_radius as f32) * scale).round().max(1.0) as u32;
                glass.refraction_thickness *= scale;
                glass.fresnel_range *= scale;
                glass.glare_range *= scale;
                Filter::RectLiquidGlass(glass)
            }
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => Filter::Blur {
                std_dev_x: std_dev_x * scale,
                std_dev_y: std_dev_y * scale,
                sampling,
            },
            Filter::Flood { brush } => Filter::Flood {
                brush: self.physical_brush(brush),
            },
            Filter::Offset { dx, dy } => Filter::Offset {
                dx: dx * scale,
                dy: dy * scale,
            },
            Filter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => Filter::Morphology {
                radius_x: radius_x * scale,
                radius_y: radius_y * scale,
                operator,
            },
            Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                brush,
            } => Filter::DropShadow {
                offset_x: offset_x * scale,
                offset_y: offset_y * scale,
                std_dev: std_dev * scale,
                brush: self.physical_brush(brush),
            },
            filter => filter,
        }
    }

    fn physical_filter_primitive_kind(&self, kind: FilterPrimitiveKind) -> FilterPrimitiveKind {
        let scale = self.scale_f32();
        match kind {
            FilterPrimitiveKind::Filter(filter) => {
                FilterPrimitiveKind::Filter(Box::new(self.physical_filter(*filter)))
            }
            FilterPrimitiveKind::Image { brush } => FilterPrimitiveKind::Image {
                brush: self.physical_brush(brush),
            },
            FilterPrimitiveKind::DisplacementMap(mut map) => {
                map.scale_x *= scale;
                map.scale_y *= scale;
                FilterPrimitiveKind::DisplacementMap(map)
            }
            FilterPrimitiveKind::Tile { source_region } => FilterPrimitiveKind::Tile {
                source_region: self.physical_bounds(source_region),
            },
            FilterPrimitiveKind::Turbulence(mut turbulence) => {
                turbulence.transform_x *= scale;
                turbulence.transform_y *= scale;
                turbulence.scale_x *= scale;
                turbulence.scale_y *= scale;
                turbulence.tile_x *= scale;
                turbulence.tile_y *= scale;
                turbulence.tile_width *= scale;
                turbulence.tile_height *= scale;
                FilterPrimitiveKind::Turbulence(turbulence)
            }
            kind => kind,
        }
    }

    fn physical_region(&self, region: Region) -> Region {
        match region {
            Region::Rect { rect, radius } => Region::Rect {
                rect: self.physical_rect(rect),
                radius: self.physical_radius(radius),
            },
            Region::Path {
                path,
                transform,
                tolerance,
            } => Region::Path {
                path,
                transform: self.device_transform() * transform,
                tolerance: self.device_tolerance(tolerance),
            },
        }
    }

    fn physical_mask(&self, mask: Mask) -> Mask {
        Mask {
            region: self.physical_region(mask.region),
            kind: mask.kind,
        }
    }

    fn physical_brush(&self, brush: Brush) -> Brush {
        let inv_scale = 1.0 / self.scale_f32();
        match brush {
            Brush::Solid(_) => brush,
            Brush::Linear(mut gradient) => {
                gradient.transform = scale_brush_transform(gradient.transform, inv_scale);
                Brush::Linear(gradient)
            }
            Brush::Radial(mut gradient) => {
                gradient.transform = scale_brush_transform(gradient.transform, inv_scale);
                Brush::Radial(gradient)
            }
            Brush::Sweep(mut gradient) => {
                gradient.center[0] *= self.scale_f32();
                gradient.center[1] *= self.scale_f32();
                Brush::Sweep(gradient)
            }
            Brush::FourCorner(mut gradient) => {
                let scale = self.scale_f32();
                gradient.bounds[0] *= scale;
                gradient.bounds[1] *= scale;
                gradient.bounds[2] *= scale;
                gradient.bounds[3] *= scale;
                Brush::FourCorner(gradient)
            }
            Brush::Pattern(mut pattern) => {
                pattern.transform = scale_brush_transform(pattern.transform, inv_scale);
                Brush::Pattern(pattern)
            }
        }
    }

    fn physical_sdf(&self, sdf: Sdf) -> Sdf {
        match sdf {
            Sdf::Rect(rect) => Sdf::Rect(self.physical_sdf_rect(rect)),
            Sdf::RectStroke(stroke) => Sdf::RectStroke(SdfRectStroke {
                rect: self.physical_sdf_rect(stroke.rect),
                widths: self.physical_stroke_widths(stroke.widths),
            }),
            Sdf::Circle(circle) => Sdf::Circle(SdfCircle {
                center: self.physical_point(circle.center),
                radius: circle.radius * self.scale_f32(),
            }),
            Sdf::CircleStroke(stroke) => Sdf::CircleStroke(SdfCircleStroke {
                circle: SdfCircle {
                    center: self.physical_point(stroke.circle.center),
                    radius: stroke.circle.radius * self.scale_f32(),
                },
                half_width: stroke.half_width * self.scale_f32(),
            }),
            Sdf::Arc(mut arc) => {
                arc.center = self.physical_point(arc.center);
                arc.radius *= self.scale_f32();
                arc.width *= self.scale_f32();
                Sdf::Arc(arc)
            }
            Sdf::CandleStick(mut candle) => {
                let scale = self.scale_f32();
                candle.center_x *= scale;
                candle.high_y *= scale;
                candle.low_y *= scale;
                candle.body_top_y *= scale;
                candle.body_bottom_y *= scale;
                candle.body_width = scaled_positive_u32(candle.body_width, scale);
                candle.wick_width = scaled_positive_u32(candle.wick_width, scale);
                Sdf::CandleStick(candle)
            }
            Sdf::Line(line) => Sdf::Line(self.physical_sdf_line(line)),
            Sdf::DashLine(mut line) => {
                line.line = self.physical_sdf_line(line.line);
                line.dash_length *= self.scale_f32();
                line.gap_length *= self.scale_f32();
                line.dash_offset *= self.scale_f32();
                Sdf::DashLine(line)
            }
        }
    }

    fn physical_sdf_shadow(&self, shadow: SdfShadow) -> SdfShadow {
        match shadow {
            SdfShadow::Rect(shadow) => SdfShadow::Rect(SdfRectShadow {
                rect: self.physical_sdf_rect(shadow.rect),
                options: self.physical_shadow_options(shadow.options),
            }),
            SdfShadow::Circle(shadow) => SdfShadow::Circle(SdfCircleShadow {
                circle: SdfCircle {
                    center: self.physical_point(shadow.circle.center),
                    radius: shadow.circle.radius * self.scale_f32(),
                },
                options: self.physical_shadow_options(shadow.options),
            }),
            SdfShadow::Arc(mut shadow) => {
                shadow.arc.center = self.physical_point(shadow.arc.center);
                shadow.arc.radius *= self.scale_f32();
                shadow.arc.width *= self.scale_f32();
                shadow.options = self.physical_shadow_options(shadow.options);
                SdfShadow::Arc(shadow)
            }
            SdfShadow::Line(shadow) => SdfShadow::Line(SdfLineShadow {
                line: self.physical_sdf_line(shadow.line),
                options: self.physical_shadow_options(shadow.options),
            }),
        }
    }

    fn physical_sdf_rect(&self, rect: SdfRect) -> SdfRect {
        SdfRect {
            start: self.physical_point(rect.start),
            end: self.physical_point(rect.end),
            radius: self.physical_radius(rect.radius),
        }
    }

    fn physical_sdf_line(&self, line: SdfLine) -> SdfLine {
        SdfLine {
            start: self.physical_point(line.start),
            end: self.physical_point(line.end),
            width: line.width * self.scale_f32(),
            cap: line.cap,
        }
    }

    pub fn draw_count(&self) -> usize {
        self.draw_records.len()
    }

    pub fn draw_id_at(&self, index: usize) -> Option<DrawId> {
        (index < self.draw_records.len()).then(|| self.draw_id_from_index(index))
    }

    pub fn draw_brush(&self, draw: DrawId) -> Option<Brush> {
        self.draw_index(draw)
            .and_then(|index| self.draw_records.get(index))
            .and_then(|draw| self.draw_brush_for_record(draw))
    }

    /// Replaces a draw's brush in the semantic draw record.
    ///
    /// GPU upload data is derived from draw records during upload, so this
    /// mutation only updates the scene source of truth.
    pub fn set_draw_brush(&mut self, draw: DrawId, brush: impl Into<Brush>) -> bool {
        let Some(index) = self.draw_index(draw) else {
            return false;
        };
        let (brush_offset, brush_len) = self.push_brush(brush.into());
        self.draw_records[index].brush_offset = brush_offset;
        self.draw_records[index].brush_len = brush_len;
        true
    }

    pub fn set_draw_color(&mut self, draw: DrawId, color: Color) -> bool {
        self.set_draw_brush(draw, Brush::Solid(color))
    }

    pub fn draw_solid_color(&self, draw: DrawId) -> Option<Color> {
        self.draw_brush(draw).and_then(|brush| brush.solid_color())
    }

    fn draw_id_from_index(&self, index: usize) -> DrawId {
        debug_assert!(index < self.draw_records.len());
        DrawId {
            index: index as u32,
            generation: self.draw_generation,
        }
    }

    fn draw_index(&self, draw: DrawId) -> Option<usize> {
        if draw.generation != self.draw_generation {
            return None;
        }
        let index = draw.index as usize;
        (index < self.draw_records.len()).then_some(index)
    }

    fn ensure_command_root(&mut self) {
        if self.command_lists.is_empty() {
            self.command_lists.push(CommandList::default());
        }
        self.root_commands = ROOT_COMMAND_LIST_ID;
        if self.command_stack.is_empty() {
            self.command_stack.push(self.root_commands);
        }
    }

    fn current_command_list_id(&self) -> CommandListId {
        self.command_stack
            .last()
            .copied()
            .unwrap_or(self.root_commands)
    }

    fn current_command_list_mut(&mut self) -> &mut CommandList {
        let id = self.current_command_list_id();
        &mut self.command_lists[id]
    }

    fn push_child_command_list(&mut self) -> CommandListId {
        let children = self.command_lists.len();
        self.command_lists.push(CommandList::default());
        children
    }

    fn push_layer_command(
        &mut self,
        retained: Option<RetainedLayerKey>,
        draw: usize,
        layer: Layer,
        kind: LayerKind,
    ) {
        let retained = if self.is_retained() { retained } else { None };
        let children = self.push_child_command_list();
        self.current_command_list_mut()
            .commands
            .push(Command::Layer {
                retained,
                draw,
                layer,
                children,
            });
        self.command_stack.push(children);
        self.layer_stack.push(kind);
    }

    fn push_mask_command(
        &mut self,
        retained: Option<RetainedLayerKey>,
        layer: Mask,
        mask_commands: CommandListId,
    ) {
        let retained = if self.is_retained() { retained } else { None };
        let content = self.push_child_command_list();
        self.current_command_list_mut()
            .commands
            .push(Command::MaskLayer {
                retained,
                layer,
                content,
                mask: mask_commands,
            });
        self.command_stack.push(content);
        self.layer_stack.push(LayerKind::Mask);
    }

    /// Appends `other` with its local canvas origin placed at `pos`.
    ///
    /// Append translates the child canvas's geometry, brushes, and layer/filter
    /// regions into parent coordinates, then inserts its root commands into the
    /// current command list. It deliberately does not add a child-canvas clip;
    /// callers that need clipping can open a clip layer around the append.
    /// The borrowed child canvas is not mutated and remains reusable.
    pub fn append(&mut self, other: &Canvas, pos: impl Into<Point>) {
        self.ensure_command_root();
        assert!(
            other.command_stack.len() == 1 && other.layer_stack.is_empty(),
            "cannot append a canvas with unclosed layers"
        );
        assert!(
            (self.scale_factor - other.scale_factor).abs() <= f32::EPSILON,
            "cannot append canvases with different scale factors"
        );

        let offset = SceneOffset::new(self.physical_point(pos.into()));
        self.append_scene_ref_unchecked(other, SceneAppendMode::MergeCurrent, offset);
    }

    fn append_scene_ref_unchecked(
        &mut self,
        other: &Canvas,
        mode: SceneAppendMode,
        offset: SceneOffset,
    ) -> Option<CommandListId> {
        self.append_scene_ref_to_list_unchecked(other, self.current_command_list_id(), mode, offset)
    }

    fn append_scene_ref_to_list_unchecked(
        &mut self,
        other: &Canvas,
        target_commands: CommandListId,
        mode: SceneAppendMode,
        offset: SceneOffset,
    ) -> Option<CommandListId> {
        let draw_offset = self.append_scene_data(other, offset);
        let command_list_offset = self.command_lists.len();
        let root_commands = other.root_commands;
        match mode {
            SceneAppendMode::MergeCurrent => {
                let child_list_offset = command_list_offset.saturating_sub(1);
                let remapped_root_commands = other.command_lists[root_commands]
                    .commands
                    .iter()
                    .map(|command| {
                        Self::translated_remapped_command(
                            command,
                            draw_offset,
                            child_list_offset,
                            offset,
                        )
                    })
                    .collect::<Vec<_>>();

                for (list_ix, list) in other.command_lists.iter().enumerate() {
                    if list_ix == root_commands {
                        continue;
                    }
                    self.command_lists
                        .push(Self::translated_remapped_command_list(
                            list,
                            draw_offset,
                            child_list_offset,
                            offset,
                        ));
                }

                self.command_lists[target_commands]
                    .commands
                    .extend(remapped_root_commands);
                None
            }
            SceneAppendMode::AppendAsCommandList => {
                for list in &other.command_lists {
                    self.command_lists
                        .push(Self::translated_remapped_command_list(
                            list,
                            draw_offset,
                            command_list_offset,
                            offset,
                        ));
                }
                Some(command_list_offset + root_commands)
            }
        }
    }

    fn translate_draw_for_append(draw: &mut DrawRecord, offset: SceneOffset) {
        if !draw.has_analytic_geometry() {
            draw.pixel_bounds = offset.pixel_bounds(draw.pixel_bounds);
        }
    }

    fn pixel_bounds_from_bounds(bounds: Bounds) -> PixelBounds {
        PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
    }

    fn path_pixel_bounds(&self, path_ix: usize) -> PixelBounds {
        let Some(record) = self.path_records.get(path_ix) else {
            return PixelBounds {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            };
        };
        let lines = &self.lines
            [record.line_start as usize..(record.line_start + record.line_count) as usize];
        if lines.is_empty() {
            return PixelBounds {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            };
        }

        let mut x0 = f32::INFINITY;
        let mut y0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        for line in lines {
            x0 = x0.min(line.p0[0]).min(line.p1[0]);
            y0 = y0.min(line.p0[1]).min(line.p1[1]);
            x1 = x1.max(line.p0[0]).max(line.p1[0]);
            y1 = y1.max(line.p0[1]).max(line.p1[1]);
        }
        PixelBounds {
            x0: x0.floor() as i32,
            y0: y0.floor() as i32,
            x1: x1.ceil() as i32,
            y1: y1.ceil() as i32,
        }
    }

    fn segment_capacity_for_path_record(
        &self,
        record: &PathRecord,
        tile_bbox: crate::shared::bounds::TileBbox,
        width_in_tiles: u32,
        height_in_tiles: u32,
    ) -> u32 {
        self.lines[record.line_start as usize..(record.line_start + record.line_count) as usize]
            .iter()
            .fold(0u32, |capacity, &line| {
                capacity.saturating_add(line_scanned_tile_count(
                    line,
                    tile_bbox,
                    (width_in_tiles, height_in_tiles),
                ))
            })
    }

    fn append_scene_data(&mut self, other: &Canvas, offset: SceneOffset) -> usize {
        let line_offset = self.lines.len() as u32;
        let path_offset = self.path_cnt;
        let draw_offset = self.draw_records.len();
        let glyph_offset = self.text_glyphs.len() as u32;
        let text_run_offset = self.text_runs.len() as u32;
        let path_record_start = self.path_records.len();
        let draw_start = self.draw_records.len();

        self.scene_images.extend_from(&other.scene_images);

        self.lines.reserve(other.lines.len());
        for &line in &other.lines {
            let mut line = line;
            line.path_id = line.path_id.saturating_add(path_offset);
            if !offset.is_zero() {
                offset.line(&mut line);
            }
            self.lines.push(line);
        }

        self.path_records.reserve(other.path_records.len());
        for &record in &other.path_records {
            let mut record = record;
            record.path_id = record.path_id.saturating_add(path_offset);
            record.line_start = record.line_start.saturating_add(line_offset);
            record.data_offset = 0;
            record.data_len = 0;
            record.tile_x0 = 0;
            record.tile_y0 = 0;
            record.tile_x1 = 0;
            record.tile_y1 = 0;
            record.segment_start = 0;
            record.segment_capacity = 0;
            record.segment_count = 0;
            self.path_records.push(record);
        }

        self.draw_records.reserve(other.draw_records.len());
        for draw in &other.draw_records {
            let mut draw = *draw;
            if draw.path_id != DrawRecord::NONE {
                draw.path_id = draw.path_id.saturating_add(path_offset);
            }
            if draw.glyph_run_id != DrawRecord::NONE {
                draw.glyph_run_id = draw.glyph_run_id.saturating_add(text_run_offset);
            }
            if let Some(brush) = other.draw_brush_for_record(&draw) {
                let brush = if offset.is_zero() {
                    brush
                } else {
                    offset.brush(brush)
                };
                (draw.brush_offset, draw.brush_len) = self.push_physical_brush(brush);
            } else {
                draw.brush_offset = DrawRecord::NONE;
                draw.brush_len = 0;
            }
            if let Some(sdf) = other.draw_sdf(&draw) {
                let sdf = if offset.is_zero() {
                    sdf
                } else {
                    offset.sdf(sdf)
                };
                (draw.sdf_offset, draw.sdf_len) = self.push_sdf(sdf);
                draw.sdf_shadow_offset = DrawRecord::NONE;
                draw.sdf_shadow_len = 0;
                draw.pixel_bounds = Self::pixel_bounds_from_bounds(sdf.bounds());
            } else if let Some(sdf_shadow) = other.draw_sdf_shadow(&draw) {
                let sdf_shadow = if offset.is_zero() {
                    sdf_shadow
                } else {
                    offset.sdf_shadow(sdf_shadow)
                };
                (draw.sdf_shadow_offset, draw.sdf_shadow_len) = self.push_sdf_shadow(sdf_shadow);
                draw.sdf_offset = DrawRecord::NONE;
                draw.sdf_len = 0;
                draw.pixel_bounds = Self::pixel_bounds_from_bounds(sdf_shadow.bounds());
            } else {
                draw.sdf_offset = DrawRecord::NONE;
                draw.sdf_len = 0;
                draw.sdf_shadow_offset = DrawRecord::NONE;
                draw.sdf_shadow_len = 0;
            }
            if !offset.is_zero() {
                Self::translate_draw_for_append(&mut draw, offset);
            }
            self.draw_records.push(draw);
        }

        self.text_glyphs.reserve(other.text_glyphs.len());
        for &glyph in &other.text_glyphs {
            let glyph = if offset.is_zero() {
                glyph
            } else {
                glyph.translated(offset.dx, offset.dy)
            };
            self.text_glyphs.push(glyph);
        }

        self.text_runs.reserve(other.text_runs.len());
        for &run in &other.text_runs {
            let run = TextRun {
                glyph_start: run.glyph_start.saturating_add(glyph_offset),
                glyph_count: run.glyph_count,
            };
            self.text_runs.push(run);
        }

        self.path_cnt = self.path_cnt.saturating_add(other.path_cnt);
        self.append_path_backdrops_for_paths(
            path_record_start,
            other.path_records.len(),
            draw_start,
            other.draw_records.len(),
        );

        draw_offset
    }

    fn append_path_backdrops_for_paths(
        &mut self,
        path_record_start: usize,
        path_record_count: usize,
        draw_start: usize,
        draw_count: usize,
    ) {
        if path_record_count == 0 {
            return;
        }

        let width_in_tiles = self.width_in_tiles();
        let height_in_tiles = self.height_in_tiles();
        let mut data_offset = self.backdrop_pool_capacity;
        let mut segment_start = self.tile_cnt;
        let path_record_end = path_record_start + path_record_count;
        let draw_end = draw_start + draw_count;

        for path_ix in path_record_start..path_record_end {
            let record = self.path_records[path_ix];
            let pixel_bounds = self.draw_records[draw_start..draw_end]
                .iter()
                .filter(|draw| draw.path_id == record.path_id)
                .map(|draw| draw.pixel_bounds)
                .reduce(PixelBounds::union)
                .unwrap_or_else(|| self.path_pixel_bounds(path_ix));
            let tile_bbox = pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles);
            let data_len = tile_bbox.tile_count();
            let segment_capacity = self.segment_capacity_for_path_record(
                &record,
                tile_bbox,
                width_in_tiles,
                height_in_tiles,
            );
            let record = &mut self.path_records[path_ix];
            record.data_offset = data_offset;
            record.data_len = data_len;
            record.tile_x0 = tile_bbox.x0;
            record.tile_y0 = tile_bbox.y0;
            record.tile_x1 = tile_bbox.x1;
            record.tile_y1 = tile_bbox.y1;
            record.segment_start = segment_start;
            record.segment_capacity = segment_capacity;
            record.segment_count = 0;
            data_offset = data_offset.saturating_add(data_len);
            segment_start = segment_start.saturating_add(segment_capacity);
        }

        self.backdrop_pool_capacity = data_offset;
        self.tile_cnt = segment_start;
    }

    pub(crate) fn remap_command(
        command: Command,
        draw_offset: usize,
        child_list_offset: usize,
    ) -> Command {
        match command {
            Command::Draw(draw_ix) => Command::Draw(draw_ix + draw_offset),
            Command::RetainedScene {
                id,
                revision,
                canvas,
                offset,
            } => Command::RetainedScene {
                id,
                revision,
                canvas,
                offset,
            },
            Command::MaterializedRetainedScene {
                id,
                revision,
                children,
            } => Command::MaterializedRetainedScene {
                id,
                revision,
                children: children + child_list_offset,
            },
            Command::Layer {
                retained,
                draw,
                layer,
                children,
            } => Command::Layer {
                retained,
                draw: draw + draw_offset,
                layer,
                children: children + child_list_offset,
            },
            Command::MaskLayer {
                retained,
                layer,
                content,
                mask,
            } => Command::MaskLayer {
                retained,
                layer,
                content: content + child_list_offset,
                mask: mask + child_list_offset,
            },
        }
    }

    fn translated_remapped_command_list(
        list: &CommandList,
        draw_offset: usize,
        child_list_offset: usize,
        offset: SceneOffset,
    ) -> CommandList {
        CommandList {
            commands: list
                .commands
                .iter()
                .map(|command| {
                    Self::translated_remapped_command(
                        command,
                        draw_offset,
                        child_list_offset,
                        offset,
                    )
                })
                .collect(),
        }
    }

    fn translated_remapped_command(
        command: &Command,
        draw_offset: usize,
        child_list_offset: usize,
        offset: SceneOffset,
    ) -> Command {
        let mut command = command.clone();
        if !offset.is_zero() {
            offset.command(&mut command);
        }
        Self::remap_command(command, draw_offset, child_list_offset)
    }

    fn append_scene_as_command_list(&mut self, other: &Canvas) -> CommandListId {
        self.append_scene_ref_unchecked(
            other,
            SceneAppendMode::AppendAsCommandList,
            SceneOffset::new(Point::new(0.0, 0.0)),
        )
        .expect("append mode returns a command list id")
    }

    pub fn push_clip_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) {
        self.push_clip_layer_with_retention(None, path, transform, rule, tolerance);
    }

    pub fn push_retained_clip_layer(
        &mut self,
        retained: RetainedLayerKey,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) {
        self.push_clip_layer_with_retention(Some(retained), path, transform, rule, tolerance);
    }

    fn push_clip_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) {
        // Rectangular clips are common in UI trees. Keeping them as generic
        // paths creates scan segments on tile boundaries and prevents coarse
        // from proving that a fully covered tile needs no clip wrappers. An
        // exact axis-aligned rectangle with pixel-aligned physical edges has
        // identical SDF coverage semantics, avoids path storage entirely, and
        // works for translated/reflected input paths as well.
        if let Some(rect) = axis_aligned_rect_path(&path, transform)
            && self.rect_has_pixel_aligned_edges(rect)
        {
            self.push_clip_sdf_rect_layer_with_retention(retained, rect, Radius::ZERO);
            return;
        }
        self.ensure_command_root();
        let draw = self.push_layer_path(DrawTag::Clip, path, transform, rule, tolerance);
        self.push_layer_command(retained, draw, Layer::Clip, LayerKind::Clip);
    }

    /// Adds a rounded/sharp rectangle clip that is rasterized directly from an SDF.
    ///
    /// This avoids flattening simple rounded clips into path segments while keeping
    /// the SDF geometry as the source of truth until render time.
    pub fn push_clip_sdf_rect_layer(&mut self, rect: Rect, radius: Radius) {
        self.push_clip_sdf_rect_layer_with_retention(None, rect, radius);
    }

    /// Opens an SDF rectangle clip whose command subtree has stable retained identity.
    pub fn push_retained_clip_sdf_rect_layer(
        &mut self,
        retained: RetainedLayerKey,
        rect: Rect,
        radius: Radius,
    ) {
        self.push_clip_sdf_rect_layer_with_retention(Some(retained), rect, radius);
    }

    fn push_clip_sdf_rect_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        rect: Rect,
        radius: Radius,
    ) {
        self.push_clip_sdf_layer_with_retention(
            retained,
            Sdf::Rect(SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
            }),
        );
    }

    fn rect_has_pixel_aligned_edges(&self, rect: Rect) -> bool {
        let rect = self.physical_rect(rect);
        [rect.x0, rect.y0, rect.x1, rect.y1]
            .into_iter()
            .all(|value| (value - value.round()).abs() <= 1.0e-9)
    }

    pub fn push_clip_sdf_circle_layer(&mut self, circle: Circle) {
        self.push_clip_sdf_layer_with_retention(
            None,
            Sdf::Circle(SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            }),
        );
    }

    pub fn push_retained_clip_sdf_circle_layer(
        &mut self,
        retained: RetainedLayerKey,
        circle: Circle,
    ) {
        self.push_clip_sdf_layer_with_retention(
            Some(retained),
            Sdf::Circle(SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            }),
        );
    }

    pub fn push_clip_sdf_arc_layer(&mut self, arc: SdfArc) {
        self.push_clip_sdf_layer(Sdf::Arc(arc));
    }

    pub fn push_retained_clip_sdf_arc_layer(&mut self, retained: RetainedLayerKey, arc: SdfArc) {
        self.push_clip_sdf_layer_with_retention(Some(retained), Sdf::Arc(arc));
    }

    pub fn push_clip_sdf_line_layer(&mut self, line: SdfLine) {
        self.push_clip_sdf_layer(Sdf::Line(line));
    }

    pub fn push_retained_clip_sdf_line_layer(&mut self, retained: RetainedLayerKey, line: SdfLine) {
        self.push_clip_sdf_layer_with_retention(Some(retained), Sdf::Line(line));
    }

    /// Adds a clip layer backed by exact SDF geometry.
    ///
    /// Unlike path clips, SDF clips do not allocate path records, scan backdrops,
    /// or per-tile segments. The renderer rasterizes the mask directly from the
    /// SDF bounds, so future SDF primitives automatically work as clip layers.
    pub fn push_clip_sdf_layer(&mut self, sdf: Sdf) {
        self.push_clip_sdf_layer_with_retention(None, sdf);
    }

    pub fn push_retained_clip_sdf_layer(&mut self, retained: RetainedLayerKey, sdf: Sdf) {
        self.push_clip_sdf_layer_with_retention(Some(retained), sdf);
    }

    fn push_clip_sdf_layer_with_retention(&mut self, retained: Option<RetainedLayerKey>, sdf: Sdf) {
        self.ensure_command_root();
        let sdf = self.physical_sdf(sdf);
        let bounds = sdf.bounds();
        let draw = self.push_physical_sdf_record(
            sdf,
            Brush::Solid(Color::TRANSPARENT),
            DrawTag::Clip,
            false,
        );
        let layer = Layer::ClipSdf { bounds, sdf };
        self.push_layer_command(retained, draw, layer, LayerKind::ClipSdf);
    }

    /// Starts an isolated source-over group.
    ///
    /// This is the renderer primitive for SVG/CSS `isolation:isolate` without
    /// opacity, blending, or filtering. The children are composited into a
    /// transparent offscreen buffer first, then the group is composited back
    /// through the supplied layer path and any outer clips.
    pub fn push_isolate_layer(&mut self, path: BezPath, transform: Affine, tolerance: f64) {
        self.push_isolate_layer_with_retention(None, path, transform, tolerance);
    }

    pub fn push_retained_isolate_layer(
        &mut self,
        retained: RetainedLayerKey,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
    ) {
        self.push_isolate_layer_with_retention(Some(retained), path, transform, tolerance);
    }

    fn push_isolate_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
    ) {
        self.ensure_command_root();
        let draw = self.push_layer_path(
            DrawTag::Isolate,
            path,
            transform,
            FillRule::NonZero,
            tolerance,
        );
        self.push_layer_command(retained, draw, Layer::Isolate, LayerKind::Isolate);
    }

    pub fn push_opacity_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    ) {
        self.push_opacity_layer_with_retention(None, path, transform, tolerance, opacity);
    }

    pub fn push_retained_opacity_layer(
        &mut self,
        retained: RetainedLayerKey,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    ) {
        self.push_opacity_layer_with_retention(Some(retained), path, transform, tolerance, opacity);
    }

    fn push_opacity_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    ) {
        self.ensure_command_root();
        let draw = self.push_layer_path(
            DrawTag::Opacity,
            path,
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let layer = Layer::Opacity(Opacity { opacity });
        self.push_layer_command(retained, draw, layer, LayerKind::Opacity);
    }

    pub(crate) fn push_blend_layer_inner(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        blend: Blend,
    ) {
        self.push_blend_layer_with_retention(None, path, transform, tolerance, blend);
    }

    fn push_blend_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        blend: Blend,
    ) {
        self.ensure_command_root();
        let draw = self.push_layer_path(
            DrawTag::Blend,
            path,
            transform,
            FillRule::NonZero,
            tolerance,
        );
        let layer = Layer::Blend(Blend { mode: blend.mode });
        self.push_layer_command(retained, draw, layer, LayerKind::Blend);
    }

    pub fn push_blend_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        mix: Mix,
        compose: Compose,
    ) {
        self.push_blend_layer_inner(path, transform, tolerance, Blend::new(mix, compose));
    }

    pub fn push_retained_blend_layer(
        &mut self,
        retained: RetainedLayerKey,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        mix: Mix,
        compose: Compose,
    ) {
        self.push_blend_layer_with_retention(
            Some(retained),
            path,
            transform,
            tolerance,
            Blend::new(mix, compose),
        );
    }

    /// Starts a masked group using `mask_scene` as the mask source.
    ///
    /// The mask source is rendered isolated, converted to either alpha or
    /// luminance coverage, clipped to `mask.region`, then applied to this
    /// layer's content before compositing through any outer clips.
    pub fn push_mask_layer(&mut self, mask_scene: Canvas, mask: Mask) {
        self.push_mask_layer_with_retention(None, mask_scene, mask);
    }

    /// Opens a retained mask boundary and uses its key for both damage and surface history.
    pub fn push_retained_mask_layer(
        &mut self,
        retained: RetainedLayerKey,
        mask_scene: Canvas,
        mask: Mask,
    ) {
        self.push_mask_layer_with_retention(Some(retained), mask_scene, mask);
    }

    fn push_mask_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        mask_scene: Canvas,
        mask: Mask,
    ) {
        self.ensure_command_root();
        assert!(
            (self.scale_factor - mask_scene.scale_factor).abs() <= f32::EPSILON,
            "cannot use a mask canvas with a different scale factor"
        );
        let mask_commands = self.append_scene_as_command_list(&mask_scene);
        self.push_mask_command(retained, self.physical_mask(mask), mask_commands);
    }

    /// Adds an offscreen filter group sampled from `sample_region`.
    ///
    /// Filters derive their final output bounds from this region. Blur and
    /// drop-shadow expand it internally so their output is not clipped back to
    /// the original geometry.
    pub fn push_filter_layer(&mut self, filter: Filter, sample_region: Region) {
        self.push_filter_layer_with_retention(None, filter, sample_region);
    }

    /// Opens a filter layer whose offscreen output can be reused across frames.
    pub fn push_retained_filter_layer(
        &mut self,
        retained: RetainedLayerKey,
        filter: Filter,
        sample_region: Region,
    ) {
        self.push_filter_layer_with_retention(Some(retained), filter, sample_region);
    }

    fn push_filter_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        filter: Filter,
        sample_region: Region,
    ) {
        self.ensure_command_root();
        let filter = self.physical_filter(filter);
        let sample_region = self.physical_region(sample_region);
        assert!(
            !filter.contains_rect_liquid_glass(),
            "RectLiquidGlass is a rounded-rectangle backdrop effect; use push_backdrop_layer with Region::Rect"
        );
        self.push_layer_command(
            retained,
            0,
            Layer::Filter {
                filter,
                sample_region,
            },
            LayerKind::Filter,
        );
    }

    /// Adds a backdrop filter group sampled from the already-rendered target.
    ///
    /// The filter samples pixels behind this layer from `sample_region`, clips
    /// the filtered backdrop back to that region, then renders this layer's
    /// children normally on top.
    pub fn push_backdrop_layer(&mut self, filter: Filter, sample_region: Region) {
        self.push_backdrop_layer_with_retention(None, filter, sample_region);
    }

    /// Opens a retained backdrop layer with stable history ownership.
    pub fn push_retained_backdrop_layer(
        &mut self,
        retained: RetainedLayerKey,
        filter: Filter,
        sample_region: Region,
    ) {
        self.push_backdrop_layer_with_retention(Some(retained), filter, sample_region);
    }

    fn push_backdrop_layer_with_retention(
        &mut self,
        retained: Option<RetainedLayerKey>,
        filter: Filter,
        sample_region: Region,
    ) {
        self.ensure_command_root();
        let filter = self.physical_filter(filter);
        let sample_region = self.physical_region(sample_region);
        if filter.contains_rect_liquid_glass() {
            assert!(
                matches!(sample_region, Region::Rect { .. }),
                "RectLiquidGlass requires Region::Rect because it uses rounded-rectangle SDF normals"
            );
        }
        self.push_layer_command(
            retained,
            0,
            Layer::Backdrop {
                filter,
                sample_region,
            },
            LayerKind::Backdrop,
        );
    }

    pub fn pop_layer(&mut self) -> Option<LayerKind> {
        self.ensure_command_root();
        let layer_kind = self.layer_stack.pop()?;
        if self.command_stack.len() > 1 {
            self.command_stack.pop();
        }
        Some(layer_kind)
    }

    /// Adds a filled rectangle as SDF geometry with independent corner radii.
    ///
    /// This keeps rounded rectangles on the SDF path instead of flattening them
    /// to path segments, matching the SDF shadow/stroke APIs and preserving
    /// subpixel edge ownership in the wgpu renderer. SDF primitives
    /// have inherent coverage; use path APIs when fill-rule semantics matter.
    pub fn push_rect(&mut self, rect: Rect, radius: Radius, brush: impl Into<Brush>) -> DrawId {
        let draw = self.push_sdf_draw(
            Sdf::Rect(SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
            }),
            brush,
        );
        self.draw_id_from_index(draw)
    }

    /// Adds an external image scaled into `rect` with explicit extend and sampling.
    ///
    /// The image is stored as a pattern brush, so it uses the same upload/sampling
    /// path as SVG raster images. Empty images, empty rectangles, and non-finite
    /// rectangles are ignored.
    pub fn push_image(
        &mut self,
        rect: Rect,
        image: impl Into<SharedArc<Image>>,
        extend: Extend,
        sampling: PatternSampling,
    ) -> Option<DrawId> {
        if !image_rect_is_valid(rect) {
            return None;
        }
        let image = image.into();
        if image.width == 0 || image.height == 0 {
            return None;
        }
        let key = self.register_scene_image(image)?;
        let brush = Brush::from_scene_image_key_with_options(key, rect, extend, sampling, 255)?;
        Some(self.push_rect(rect, Radius::ZERO, brush))
    }

    /// Adds a renderer-owned image resource scaled into `rect` with explicit extend and sampling.
    ///
    /// The scene only stores `key`; the wgpu renderer resolves the image through
    /// its resource table at render time.
    pub fn push_image_key(
        &mut self,
        rect: Rect,
        key: ImageKey,
        extend: Extend,
        sampling: PatternSampling,
    ) -> Option<DrawId> {
        let brush = Brush::from_image_key_with_options(key, rect, extend, sampling, 255)?;
        Some(self.push_rect(rect, Radius::ZERO, brush))
    }

    pub(crate) fn register_scene_image(
        &mut self,
        image: impl Into<SharedArc<Image>>,
    ) -> Option<ImageKey> {
        let image = image.into();
        let key = ImageKey::new(SharedArc::as_ptr(&image) as usize as u64);
        if self.scene_images.get(key).is_some() {
            return Some(key);
        }
        self.scene_images.insert(key, image).then_some(key)
    }

    pub(crate) fn scene_image_resources(&self) -> &ImageResourceStore {
        &self.scene_images
    }

    pub fn push_rect_stroke(
        &mut self,
        rect: Rect,
        radius: Radius,
        stroke: Stroke,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if stroke.width <= 0.0 {
            return None;
        }
        if !stroke.dash_pattern.is_empty() {
            let path = Self::rounded_rect_path(rect, radius, 0.1);
            let outline = kurbo_stroke(path, &stroke, &StrokeOpts::default(), 0.1);
            let draw = self.push_path_inner_with_tag(
                outline,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
                PathPushOptions {
                    bounds_override: None,
                    brush: brush.into(),
                    emit_draw_command: true,
                    tag: DrawTag::Brush,
                },
            );
            return Some(self.draw_id_from_index(draw));
        }

        self.push_rect_stroke_widths(rect, radius, StrokeWidths::all(stroke.width as f32), brush)
    }

    /// Adds a rectangle stroke with independent per-side widths as SDF geometry.
    ///
    /// `widths` are full centered stroke widths. This path is meant for dense
    /// rectangle borders; dashed or arbitrary stroked shapes should use
    /// `push_stroke`, which expands through the path stroker.
    pub fn push_rect_stroke_widths(
        &mut self,
        rect: Rect,
        radius: Radius,
        widths: StrokeWidths,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        let widths = widths.clamped();
        if widths.is_empty() {
            return None;
        }

        let draw = self.push_sdf_draw(
            Sdf::RectStroke(SdfRectStroke {
                rect: SdfRect {
                    start: Point::new(rect.x0, rect.y0),
                    end: Point::new(rect.x1, rect.y1),
                    radius,
                },
                widths,
            }),
            brush,
        );
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a soft SDF shadow for a rounded rectangle.
    ///
    /// This is intentionally a separate draw instead of a hidden side effect of
    /// `push_rect`: shadow order matters under clips, blend layers, filters, and
    /// overlapping content. Push the shadow before the rectangle when it should
    /// sit behind the rectangle.
    pub fn push_rect_shadow(
        &mut self,
        rect: Rect,
        radius: Radius,
        options: RectShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        let options = options.normalized()?;
        let shadow = SdfRectShadow {
            rect: SdfRect {
                start: Point::new(rect.x0, rect.y0),
                end: Point::new(rect.x1, rect.y1),
                radius,
            },
            options,
        };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Rect(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a filled circle as exact SDF geometry instead of flattening it to path segments.
    pub fn push_circle(&mut self, circle: Circle, brush: impl Into<Brush>) -> DrawId {
        let draw = self.push_sdf_draw(
            Sdf::Circle(SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            }),
            brush,
        );
        self.draw_id_from_index(draw)
    }

    pub fn push_circle_stroke(
        &mut self,
        circle: Circle,
        stroke: Stroke,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if stroke.width <= 0.0 {
            return None;
        }
        if !stroke.dash_pattern.is_empty() {
            let draw = self.push_stroke(
                circle,
                stroke,
                brush,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
            return Some(draw);
        }

        let half_width = (stroke.width * 0.5) as f32;
        let draw = self.push_sdf_draw(
            Sdf::CircleStroke(SdfCircleStroke {
                circle: SdfCircle {
                    center: circle.center,
                    radius: circle.radius as f32,
                },
                half_width,
            }),
            brush,
        );
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_circle_shadow(
        &mut self,
        circle: Circle,
        options: RectShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        let options = options.normalized()?;
        let shadow = SdfCircleShadow {
            circle: SdfCircle {
                center: circle.center,
                radius: circle.radius as f32,
            },
            options,
        };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Circle(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    /// Adds a circular stroked arc as SDF geometry.
    ///
    /// This is separate from [`push_arc`](Self::push_arc), which preserves the
    /// existing path-backed kurbo arc semantics. Use this method when the arc is
    /// a stroke-like primitive and should avoid path flattening.
    pub fn push_sdf_arc(&mut self, arc: SdfArc, brush: impl Into<Brush>) -> Option<DrawId> {
        if arc.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::Arc(arc), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_arc_shadow(
        &mut self,
        arc: SdfArc,
        options: RectShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if arc.is_empty() {
            return None;
        }
        let options = options.normalized()?;
        let shadow = SdfArcShadow { arc, options };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Arc(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_candlestick(&mut self, candle: SdfCandleStick, brush: impl Into<Brush>) -> DrawId {
        assert!(
            SdfCandleStick::valid_body_width(candle.body_width),
            "candlestick body width must be positive"
        );
        assert!(
            SdfCandleStick::valid_wick_width(candle.wick_width),
            "candlestick wick width must be positive"
        );
        let draw = self.push_sdf_draw(Sdf::CandleStick(candle), brush);
        self.draw_id_from_index(draw)
    }

    pub fn push_line(&mut self, line: SdfLine, brush: impl Into<Brush>) -> Option<DrawId> {
        if line.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::Line(line), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_dash_line(&mut self, line: SdfDashLine, brush: impl Into<Brush>) -> Option<DrawId> {
        if line.is_empty() {
            return None;
        }
        let draw = self.push_sdf_draw(Sdf::DashLine(line), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_line_shadow(
        &mut self,
        line: SdfLine,
        options: RectShadowOptions,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if line.is_empty() {
            return None;
        }
        let options = options.normalized()?;
        let shadow = SdfLineShadow { line, options };
        let draw = self.push_sdf_shadow_draw(SdfShadow::Line(shadow), brush);
        Some(self.draw_id_from_index(draw))
    }

    pub fn push_arc(
        &mut self,
        arc: Arc,
        brush: impl Into<Brush>,
        rule: FillRule,
        tolerance: f64,
    ) -> DrawId {
        self.push_path(
            arc.to_path(tolerance),
            brush,
            Affine::IDENTITY,
            rule,
            tolerance,
        )
    }

    pub fn push_stroke(
        &mut self,
        shape: impl Shape,
        stroke: Stroke,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) -> DrawId {
        let path = shape.to_path(tolerance);
        let outline = kurbo_stroke(path, &stroke, &StrokeOpts::default(), tolerance);
        let draw = self.push_path_inner_with_tag(
            outline,
            transform,
            rule,
            tolerance,
            PathPushOptions {
                bounds_override: None,
                brush: brush.into(),
                emit_draw_command: true,
                tag: DrawTag::Brush,
            },
        );
        self.draw_id_from_index(draw)
    }

    pub fn push_path(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) -> DrawId {
        let draw = self.push_path_inner(path, brush, transform, rule, tolerance, None);
        self.draw_id_from_index(draw)
    }

    /// Adds a laid-out text run at `origin`.
    ///
    /// Text layout and glyph rasterization stay in [`TextContext`](crate::TextContext);
    /// the canvas stores only positioned glyph cache keys. This keeps cached UI text
    /// reusable across renderers while leaving transform-heavy glyph quads for a
    /// future atlas path instead of pretending bitmap glyphs support arbitrary affine
    /// transforms here.
    pub fn push_text_layout(
        &mut self,
        layout: &TextLayout,
        origin: Point,
        brush: impl Into<Brush>,
    ) -> Option<DrawId> {
        if layout.is_empty() {
            return None;
        }

        self.ensure_command_root();
        let glyph_start = self.text_glyphs.len() as u32;
        self.text_glyphs.extend(scene_glyphs_at_scaled_origin(
            layout,
            origin,
            self.scale_factor,
        ));
        let glyph_count = self.text_glyphs.len() as u32 - glyph_start;
        if glyph_count == 0 {
            return None;
        }
        let run_id = self.text_runs.len() as u32;
        let run = TextRun {
            glyph_start,
            glyph_count,
        };
        self.text_runs.push(run);
        let bounds = layout_bounds_at_scaled_origin(layout, origin, self.scale_factor);
        let (brush_offset, brush_len) = self.push_brush(brush.into());
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: run_id,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset,
            brush_len,
            tag: DrawTag::Brush.into(),
            fill_rule: FillRule::NonZero.into(),
            pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            solid_rect: 0,
        });
        self.current_command_list_mut()
            .commands
            .push(Command::Draw(draw_ix));
        Some(self.draw_id_from_index(draw_ix))
    }

    /// Adds a laid-out text run as vector outlines.
    ///
    /// Cosmic-text still owns shaping, fallback, and ligatures; this method asks
    /// `text_context` for each scalable swash outline and appends the outlines as
    /// path geometry tagged with text compositing semantics. Glyphs backed only
    /// by bitmap strikes do not have a vector outline, so keep
    /// [`push_text_layout`](Self::push_text_layout) for small hinted text and
    /// bitmap/color emoji.
    #[allow(clippy::too_many_arguments)]
    pub fn push_text_layout_as_path(
        &mut self,
        text_context: &mut TextContext,
        font_system: &mut TextFontSystem,
        layout: &TextLayout,
        origin: Point,
        brush: impl Into<Brush>,
        transform: Affine,
        tolerance: f64,
    ) -> Option<DrawId> {
        if layout.glyphs().is_empty() {
            return None;
        }

        let path = text_context.layout_outline_path(font_system, layout, origin);
        if path.is_empty() {
            return None;
        }
        let draw = self.push_path_inner_with_tag(
            path,
            transform,
            FillRule::NonZero,
            tolerance,
            PathPushOptions {
                bounds_override: None,
                brush: brush.into(),
                emit_draw_command: true,
                tag: DrawTag::PathGlyph,
            },
        );
        Some(self.draw_id_from_index(draw))
    }

    fn rounded_rect_path(rect: Rect, radius: Radius, tolerance: f64) -> BezPath {
        if radius.is_zero() {
            rect.to_path(tolerance)
        } else {
            peniko::kurbo::RoundedRect::new(
                rect.x0,
                rect.y0,
                rect.x1,
                rect.y1,
                (
                    radius.top_left as f64,
                    radius.top_right as f64,
                    radius.bottom_right as f64,
                    radius.bottom_left as f64,
                ),
            )
            .to_path(tolerance)
        }
    }

    fn transform_path(path: BezPath, transform: Affine) -> BezPath {
        if transform == Affine::IDENTITY {
            path
        } else {
            transform * path
        }
    }

    fn pixel_bounds_for_transformed_path(path: &BezPath) -> PixelBounds {
        let rect = path.bounding_box();
        PixelBounds {
            x0: rect.x0.floor() as i32,
            y0: rect.y0.floor() as i32,
            x1: rect.x1.ceil() as i32,
            y1: rect.y1.ceil() as i32,
        }
    }

    fn push_path_inner(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        bounds_override: Option<Bounds>,
    ) -> usize {
        self.push_path_inner_with_tag(
            path,
            transform,
            rule,
            tolerance,
            PathPushOptions {
                bounds_override,
                brush: brush.into(),
                emit_draw_command: true,
                tag: DrawTag::Brush,
            },
        )
    }

    fn push_path_inner_with_tag(
        &mut self,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        options: PathPushOptions,
    ) -> usize {
        self.ensure_command_root();
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let path = Self::transform_path(path, self.device_transform() * transform);
        PathFlatten::new(&path, self.device_tolerance(tolerance) as f32, path_id)
            .flatten(&mut self.lines);
        let line_count = self.lines.len() as u32 - line_start;
        let pixel_bounds = match options.bounds_override {
            Some(bounds) => {
                let bounds = self.physical_bounds(bounds);
                PixelBounds {
                    x0: bounds.x0,
                    y0: bounds.y0,
                    x1: bounds.x1,
                    y1: bounds.y1,
                }
            }
            None => Self::pixel_bounds_for_transformed_path(&path),
        };
        let path_record = PathRecord {
            path_id,
            line_count,
            line_start,
            flags: 0,
            data_offset: 0,
            data_len: 0,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 0,
            tile_y1: 0,
            segment_start: 0,
            segment_capacity: 0,
            segment_count: 0,
        };
        let tile_bbox = pixel_bounds.tile_bbox(self.width_in_tiles(), self.height_in_tiles());
        let tile_stride = tile_bbox.tile_stride();
        let tile_height = tile_bbox.tile_height();
        let backdrop_len = tile_stride * tile_height;
        let local_tile_cnt =
            self.segment_capacity_for_path_lines(line_start, line_count, tile_bbox);

        let backdrop_offset = self.backdrop_pool_capacity;
        self.backdrop_pool_capacity += backdrop_len;
        let segment_start = self.tile_cnt;
        self.tile_cnt += local_tile_cnt;
        self.path_records.push(PathRecord {
            data_offset: backdrop_offset,
            data_len: backdrop_len,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity: local_tile_cnt,
            segment_count: 0,
            ..path_record
        });

        let (brush_offset, brush_len) = self.push_brush(options.brush);
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id,
            glyph_run_id: DrawRecord::NONE,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset,
            brush_len,
            tag: options.tag.into(),
            fill_rule: rule.into(),
            pixel_bounds,
            solid_rect: 0,
        });
        if options.emit_draw_command {
            self.current_command_list_mut()
                .commands
                .push(Command::Draw(draw_ix));
        }
        draw_ix
    }

    fn push_layer_path(
        &mut self,
        tag: DrawTag,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) -> usize {
        self.push_path_inner_with_tag(
            path,
            transform,
            rule,
            tolerance,
            PathPushOptions {
                bounds_override: None,
                brush: Brush::Solid(Color::TRANSPARENT),
                emit_draw_command: false,
                tag,
            },
        )
    }

    fn push_sdf_draw(&mut self, sdf: Sdf, brush: impl Into<Brush>) -> usize {
        self.push_sdf_record(sdf, brush.into(), DrawTag::Brush, true)
    }

    fn push_sdf_shadow_draw(&mut self, sdf_shadow: SdfShadow, brush: impl Into<Brush>) -> usize {
        self.push_sdf_shadow_record(sdf_shadow, brush.into(), DrawTag::Brush, true)
    }

    fn push_sdf_record(
        &mut self,
        sdf: Sdf,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.push_physical_sdf_record(
            self.physical_sdf(sdf),
            self.physical_brush(brush),
            tag,
            emit_draw_command,
        )
    }

    fn push_physical_sdf_record(
        &mut self,
        sdf: Sdf,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.ensure_command_root();
        let bounds = sdf.bounds();
        let (sdf_offset, sdf_len) = self.push_sdf(sdf);
        let (brush_offset, brush_len) = self.push_physical_brush(brush);
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: DrawRecord::NONE,
            sdf_offset,
            sdf_len,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset,
            brush_len,
            tag: tag.into(),
            fill_rule: SDF_RECORD_FILL_RULE.into(),
            pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            solid_rect: 0,
        });
        if emit_draw_command {
            self.current_command_list_mut()
                .commands
                .push(Command::Draw(draw_ix));
        }
        draw_ix
    }

    fn push_sdf_shadow_record(
        &mut self,
        sdf_shadow: SdfShadow,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.push_physical_sdf_shadow_record(
            self.physical_sdf_shadow(sdf_shadow),
            self.physical_brush(brush),
            tag,
            emit_draw_command,
        )
    }

    fn push_physical_sdf_shadow_record(
        &mut self,
        sdf_shadow: SdfShadow,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.ensure_command_root();
        let bounds = sdf_shadow.bounds();
        let (sdf_shadow_offset, sdf_shadow_len) = self.push_sdf_shadow(sdf_shadow);
        let (brush_offset, brush_len) = self.push_physical_brush(brush);
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: DrawRecord::NONE,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset,
            sdf_shadow_len,
            brush_offset,
            brush_len,
            tag: tag.into(),
            fill_rule: SDF_RECORD_FILL_RULE.into(),
            pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            solid_rect: 0,
        });
        if emit_draw_command {
            self.current_command_list_mut()
                .commands
                .push(Command::Draw(draw_ix));
        }
        draw_ix
    }

    pub fn reset(&mut self) {
        self.lines.clear();
        self.path_records.clear();
        self.draw_records.clear();
        self.brush_blob.clear();
        self.sdf_blob.clear();
        self.sdf_shadow_blob.clear();
        self.text_glyphs.clear();
        self.text_runs.clear();
        self.scene_images.clear();
        self.command_lists.clear();
        self.command_lists.push(CommandList::default());
        self.root_commands = ROOT_COMMAND_LIST_ID;
        self.command_stack.clear();
        self.command_stack.push(self.root_commands);
        self.layer_stack.clear();
        self.invalidated_bounds.clear();
        self.invalidate_all = false;
        self.buffer_changes = None;
        self.plan_cache_key = None;
        self.compiled_plan = None;
        self.retained_frame_override = None;
        self.painter_keys = None;
        self.stable_batch_ids = None;
        self.stable_batch_counts = None;
        self.path_cnt = 0;
        self.backdrop_pool_capacity = 0;
        self.tile_cnt = 0;
        self.draw_generation = self.draw_generation.wrapping_add(1);
    }

    fn push_draw_record(&mut self, draw: DrawRecord) -> usize {
        let draw_ix = self.draw_records.len();
        self.draw_records.push(draw);
        draw_ix
    }

    fn push_brush(&mut self, brush: Brush) -> (u32, u32) {
        self.push_physical_brush(self.physical_brush(brush))
    }

    fn push_physical_brush(&mut self, brush: Brush) -> (u32, u32) {
        push_encoded_brush(&mut self.brush_blob, &brush)
    }

    fn push_sdf(&mut self, sdf: Sdf) -> (u32, u32) {
        push_encoded_sdf(&mut self.sdf_blob, sdf)
    }

    fn push_sdf_shadow(&mut self, sdf_shadow: SdfShadow) -> (u32, u32) {
        push_encoded_sdf_shadow(&mut self.sdf_shadow_blob, sdf_shadow)
    }

    pub(crate) fn draw_brush_for_record(&self, draw: &DrawRecord) -> Option<Brush> {
        decode_encoded_brush(&self.brush_blob, draw.brush_offset, draw.brush_len)
    }

    pub(crate) fn draw_sdf(&self, draw: &DrawRecord) -> Option<Sdf> {
        decode_sdf(&self.sdf_blob, draw.sdf_offset, draw.sdf_len)
    }

    pub(crate) fn draw_sdf_shadow(&self, draw: &DrawRecord) -> Option<SdfShadow> {
        decode_sdf_shadow(
            &self.sdf_shadow_blob,
            draw.sdf_shadow_offset,
            draw.sdf_shadow_len,
        )
    }

    pub(crate) fn width_in_tiles(&self) -> u32 {
        self.physical_width().div_ceil(crate::TILE_SIZE)
    }

    pub(crate) fn height_in_tiles(&self) -> u32 {
        self.physical_height().div_ceil(crate::TILE_SIZE)
    }

    fn segment_capacity_for_path_lines(
        &self,
        line_start: u32,
        line_count: u32,
        tile_bbox: crate::shared::bounds::TileBbox,
    ) -> u32 {
        let tiles_size = (self.width_in_tiles(), self.height_in_tiles());
        self.lines[line_start as usize..(line_start + line_count) as usize]
            .iter()
            .fold(0u32, |capacity, &line| {
                capacity.saturating_add(line_scanned_tile_count(line, tile_bbox, tiles_size))
            })
    }

    pub(crate) fn compile(&self, list_id: CommandListId) -> ExecPlan {
        if list_id == ROOT_COMMAND_LIST_ID
            && let Some(plan) = &self.compiled_plan
        {
            return (**plan).clone();
        }
        self.compile_uncached(list_id)
    }

    pub(crate) fn compile_shared(&self, list_id: CommandListId) -> SharedArc<ExecPlan> {
        if list_id == ROOT_COMMAND_LIST_ID
            && let Some(plan) = &self.compiled_plan
        {
            return plan.clone();
        }
        SharedArc::new(self.compile_uncached(list_id))
    }

    fn compile_uncached(&self, list_id: CommandListId) -> ExecPlan {
        let mut ops = Vec::new();
        let mut plan = ExecPlan {
            ops: Vec::new(),
            layer_stack_data: Vec::new(),
            draw_order: SharedArc::new(Vec::new()),
            draw_batch_ids: SharedArc::new(Vec::new()),
            retained_batch_ids: std::collections::HashMap::new(),
            layer_stack_locations: std::collections::HashMap::new(),
            direct_root_batch_ops: None,
        };
        let mut layer_stack = Vec::new();
        let mut surface_slots = std::collections::HashMap::new();
        self.compile_into(
            list_id,
            &mut ops,
            &mut plan,
            &mut layer_stack,
            CompileOwners {
                retained: self.retained_root,
                batch: None,
            },
            &mut surface_slots,
        );
        plan.ops = ops;
        plan.coalesce_draw_batches();
        plan.finalize_draw_batches(self.draw_records.len());
        plan
    }

    /// Hashes the command topology and layer parameters that determine an
    /// [`ExecPlan`]. Geometry, brushes, and text data are intentionally absent:
    /// they live in scene buffers and can change without rebuilding execution
    /// control flow.
    pub(crate) fn execution_plan_fingerprint(&self) -> u64 {
        use std::{fmt::Write as _, hash::Hasher as _};

        if let Some(key) = self.plan_cache_key {
            return key;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for (list_ix, list) in self.command_lists.iter().enumerate() {
            hasher.write_usize(list_ix);
            hasher.write_usize(list.commands.len());
            for command in &list.commands {
                match command {
                    Command::Draw(draw) => {
                        hasher.write_u8(0);
                        hasher.write_usize(*draw);
                    }
                    Command::RetainedScene {
                        id,
                        revision,
                        offset,
                        ..
                    } => {
                        hasher.write_u8(1);
                        hasher.write_u64(id.owner);
                        hasher.write_u32(id.slot);
                        hasher.write_u64(revision.get());
                        hasher.write_u64(offset.0.to_bits());
                        hasher.write_u64(offset.1.to_bits());
                    }
                    Command::MaterializedRetainedScene { id, children, .. } => {
                        hasher.write_u8(2);
                        hasher.write_u64(id.owner);
                        hasher.write_u32(id.slot);
                        hasher.write_usize(*children);
                    }
                    Command::Layer {
                        retained,
                        draw,
                        layer,
                        children,
                    } => {
                        hasher.write_u8(3);
                        hasher.write_usize(*draw);
                        hasher.write_usize(*children);
                        if let Some(retained) = retained {
                            hasher.write_u64(retained.id.owner);
                            hasher.write_u32(retained.id.slot);
                        }
                        let _ = write!(HasherWriter(&mut hasher), "{layer:?}");
                    }
                    Command::MaskLayer {
                        retained,
                        layer,
                        content,
                        mask,
                    } => {
                        hasher.write_u8(4);
                        hasher.write_usize(*content);
                        hasher.write_usize(*mask);
                        if let Some(retained) = retained {
                            hasher.write_u64(retained.id.owner);
                            hasher.write_u32(retained.id.slot);
                        }
                        let _ = write!(HasherWriter(&mut hasher), "{layer:?}");
                    }
                }
            }
        }
        // Persistent keys use the high bit, keeping the two identity domains disjoint.
        hasher.finish() & !(1 << 63)
    }

    fn compile_into(
        &self,
        list_id: CommandListId,
        ops: &mut Vec<ExecOp>,
        plan: &mut ExecPlan,
        layer_stack: &mut Vec<LayerStackEntry>,
        owners: CompileOwners,
        surface_slots: &mut std::collections::HashMap<RetainedNodeId, u32>,
    ) {
        let CompileOwners {
            retained: retained_owner,
            batch: batch_owner,
        } = owners;
        let op_start = ops.len();
        let mut pending_batch = Vec::<usize>::new();

        let flush_batch = |pending_batch: &mut Vec<usize>,
                           ops: &mut Vec<ExecOp>,
                           plan: &mut ExecPlan,
                           layer_stack: &[LayerStackEntry],
                           batch_owner: Option<RetainedBatchOwner>| {
            if pending_batch.is_empty() {
                return;
            }
            let layer_start = plan.layer_stack_data.len();
            plan.layer_stack_data.extend_from_slice(layer_stack);
            let layer_end = plan.layer_stack_data.len();

            ops.push(ExecOp::DrawBatch {
                draws: SharedArc::new(std::mem::take(pending_batch)),
                batch_id: u32::MAX,
                owners: SharedArc::new(batch_owner.into_iter().collect()),
                layer_stack: layer_start..layer_end,
            });
        };

        for command in &self.command_lists[list_id].commands {
            match command {
                Command::Draw(draw_ix) => pending_batch.push(*draw_ix),
                Command::RetainedScene { .. } => {
                    panic!("retained scenes must be materialized before compile")
                }
                Command::MaterializedRetainedScene {
                    id,
                    revision: _,
                    children,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
                    self.compile_into(
                        *children,
                        ops,
                        plan,
                        layer_stack,
                        CompileOwners {
                            retained: Some(*id),
                            batch: batch_owner,
                        },
                        surface_slots,
                    );
                }
                Command::Layer {
                    retained,
                    draw,
                    layer,
                    children,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
                    let retained_owner = retained.map(|key| key.id).or(retained_owner);
                    let child_batch_owner = retained
                        .map(|key| RetainedBatchOwner {
                            node: key.id,
                            branch: RetainedBatchBranch::Content,
                        })
                        .or(batch_owner);
                    if self.can_fuse(layer, *children) {
                        match layer {
                            Layer::Clip | Layer::ClipSdf { .. } => {
                                // SDF clips stay analytic by using their hidden
                                // draw record as the same layer-stack entry as
                                // path clips, instead of materializing a mask.
                                ops.push(ExecOp::BeginClip);
                                layer_stack.push(LayerStackEntry::Clip { draw: *draw as u32 });
                                self.compile_into(
                                    *children,
                                    ops,
                                    plan,
                                    layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                layer_stack.pop();
                                ops.push(ExecOp::EndClip);
                            }
                            Layer::Opacity(opacity) => {
                                ops.push(ExecOp::BeginOpacity);
                                layer_stack.push(LayerStackEntry::Opacity {
                                    draw: *draw as u32,
                                    opacity: opacity.opacity,
                                });
                                self.compile_into(
                                    *children,
                                    ops,
                                    plan,
                                    layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                layer_stack.pop();
                                ops.push(ExecOp::EndOpacity);
                            }
                            Layer::Blend(blend) => {
                                ops.push(ExecOp::BeginBlend);
                                layer_stack.push(LayerStackEntry::Blend {
                                    draw: *draw as u32,
                                    mode: blend.mode,
                                });
                                self.compile_into(
                                    *children,
                                    ops,
                                    plan,
                                    layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                layer_stack.pop();
                                ops.push(ExecOp::EndBlend);
                            }
                            _ => unreachable!(),
                        }
                    } else {
                        let stack_start = plan.layer_stack_data.len();
                        plan.layer_stack_data.extend_from_slice(layer_stack);
                        let stack_end = plan.layer_stack_data.len();
                        ops.push(ExecOp::OffscreenLayer {
                            retained_id: retained_owner.map(|owner| {
                                let slot = surface_slots.entry(owner).or_default();
                                let id = RetainedSurfaceId::new(owner, *slot);
                                *slot += 1;
                                id
                            }),
                            draw: *draw,
                            layer: layer.clone(),
                            outer_stack: stack_start..stack_end,
                            children: {
                                let mut child_ops = Vec::new();
                                let mut child_layer_stack =
                                    if matches!(layer, Layer::Backdrop { .. }) {
                                        layer_stack.clone()
                                    } else {
                                        Vec::new()
                                    };
                                self.compile_into(
                                    *children,
                                    &mut child_ops,
                                    plan,
                                    &mut child_layer_stack,
                                    CompileOwners {
                                        retained: retained_owner,
                                        batch: child_batch_owner,
                                    },
                                    surface_slots,
                                );
                                child_ops
                            },
                        });
                    }
                }
                Command::MaskLayer {
                    retained,
                    layer,
                    content,
                    mask,
                } => {
                    flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
                    let retained_owner = retained.map(|key| key.id).or(retained_owner);
                    let content_batch_owner = retained
                        .map(|key| RetainedBatchOwner {
                            node: key.id,
                            branch: RetainedBatchBranch::Content,
                        })
                        .or(batch_owner);
                    let mask_batch_owner = retained
                        .map(|key| RetainedBatchOwner {
                            node: key.id,
                            branch: RetainedBatchBranch::Mask,
                        })
                        .or(batch_owner);
                    let stack_start = plan.layer_stack_data.len();
                    plan.layer_stack_data.extend_from_slice(layer_stack);
                    let stack_end = plan.layer_stack_data.len();
                    ops.push(ExecOp::OffscreenMaskLayer {
                        retained_id: retained_owner.map(|owner| {
                            let slot = surface_slots.entry(owner).or_default();
                            let id = RetainedSurfaceId::new(owner, *slot);
                            *slot += 1;
                            id
                        }),
                        layer: layer.clone(),
                        outer_stack: stack_start..stack_end,
                        content: {
                            let mut child_ops = Vec::new();
                            let mut child_layer_stack = Vec::new();
                            self.compile_into(
                                *content,
                                &mut child_ops,
                                plan,
                                &mut child_layer_stack,
                                CompileOwners {
                                    retained: retained_owner,
                                    batch: content_batch_owner,
                                },
                                surface_slots,
                            );
                            child_ops
                        },
                        mask: {
                            let mut mask_ops = Vec::new();
                            let mut mask_layer_stack = Vec::new();
                            self.compile_into(
                                *mask,
                                &mut mask_ops,
                                plan,
                                &mut mask_layer_stack,
                                CompileOwners {
                                    retained: retained_owner,
                                    batch: mask_batch_owner,
                                },
                                surface_slots,
                            );
                            mask_ops
                        },
                    });
                }
            }
        }

        flush_batch(&mut pending_batch, ops, plan, layer_stack, batch_owner);
        if let Some(owner) = batch_owner
            && !exec_ops_contain_owner(&ops[op_start..], owner)
        {
            let layer_start = plan.layer_stack_data.len();
            plan.layer_stack_data.extend_from_slice(layer_stack);
            let layer_end = plan.layer_stack_data.len();
            ops.push(ExecOp::DrawBatch {
                draws: SharedArc::new(Vec::new()),
                batch_id: u32::MAX,
                owners: SharedArc::new(vec![owner]),
                layer_stack: layer_start..layer_end,
            });
        }
    }

    fn can_fuse(&self, layer: &Layer, children: CommandListId) -> bool {
        match layer {
            Layer::Clip => true,
            Layer::ClipSdf { .. } => true,
            // Group opacity and blend must wrap the composited child subtree.
            // If a child opens its own offscreen layer, keeping the group fused
            // would apply it to separate fragments before they are combined.
            Layer::Opacity(_) | Layer::Blend(_) => !self.command_list_contains_offscreen(children),
            _ => false,
        }
    }

    fn command_list_contains_offscreen(&self, list_id: CommandListId) -> bool {
        self.command_lists[list_id]
            .commands
            .iter()
            .any(|command| match command {
                Command::Draw(_) => false,
                Command::RetainedScene { .. } => true,
                Command::MaterializedRetainedScene { children, .. } => {
                    self.command_list_contains_offscreen(*children)
                }
                Command::Layer {
                    layer, children, ..
                } => {
                    !matches!(
                        layer,
                        Layer::Clip | Layer::ClipSdf { .. } | Layer::Opacity(_) | Layer::Blend(_)
                    ) || self.command_list_contains_offscreen(*children)
                }
                Command::MaskLayer { .. } => true,
            })
    }
}

#[derive(Clone, Copy)]
struct CompileOwners {
    retained: Option<RetainedNodeId>,
    batch: Option<RetainedBatchOwner>,
}

fn exec_ops_contain_owner(ops: &[ExecOp], owner: RetainedBatchOwner) -> bool {
    ops.iter().any(|op| match op {
        ExecOp::DrawBatch { owners, .. } => owners.contains(&owner),
        ExecOp::OffscreenLayer { children, .. } => exec_ops_contain_owner(children, owner),
        ExecOp::OffscreenMaskLayer { content, mask, .. } => {
            exec_ops_contain_owner(content, owner) || exec_ops_contain_owner(mask, owner)
        }
        _ => false,
    })
}

fn axis_aligned_rect_path(path: &BezPath, transform: Affine) -> Option<Rect> {
    let mut points = Vec::with_capacity(5);
    let mut closed = false;
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(point) if points.is_empty() => points.push(transform * point),
            PathEl::LineTo(point) if !closed => points.push(transform * point),
            PathEl::ClosePath if !closed => closed = true,
            _ => return None,
        }
    }
    if !closed || points.len() < 4 || points.len() > 5 {
        return None;
    }

    if points
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite())
    {
        return None;
    }
    let min_x = points
        .iter()
        .map(|point| point.x)
        .fold(f64::INFINITY, f64::min);
    let min_y = points
        .iter()
        .map(|point| point.y)
        .fold(f64::INFINITY, f64::min);
    let max_x = points
        .iter()
        .map(|point| point.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_y = points
        .iter()
        .map(|point| point.y)
        .fold(f64::NEG_INFINITY, f64::max);
    // Base tolerance on shape extent, not world-space translation. A large
    // translation must not make a slightly rotated quadrilateral look axial.
    let epsilon = (max_x - min_x).max(max_y - min_y).max(1.0) * 1.0e-12;
    let same = |a: Point, b: Point| (a.x - b.x).abs() <= epsilon && (a.y - b.y).abs() <= epsilon;
    if points.len() == 5 && same(points[0], points[4]) {
        points.pop();
    }
    if points.len() != 4 {
        return None;
    }

    let x0 = points
        .iter()
        .map(|point| point.x)
        .fold(f64::INFINITY, f64::min);
    let y0 = points
        .iter()
        .map(|point| point.y)
        .fold(f64::INFINITY, f64::min);
    let x1 = points
        .iter()
        .map(|point| point.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let y1 = points
        .iter()
        .map(|point| point.y)
        .fold(f64::NEG_INFINITY, f64::max);
    if x1 - x0 <= epsilon || y1 - y0 <= epsilon {
        return None;
    }

    let mut corners = 0u8;
    for (index, point) in points.iter().enumerate() {
        let x_side = if (point.x - x0).abs() <= epsilon {
            0
        } else if (point.x - x1).abs() <= epsilon {
            1
        } else {
            return None;
        };
        let y_side = if (point.y - y0).abs() <= epsilon {
            0
        } else if (point.y - y1).abs() <= epsilon {
            1
        } else {
            return None;
        };
        let corner = 1 << (y_side * 2 + x_side);
        if corners & corner != 0 {
            return None;
        }
        corners |= corner;

        let next = points[(index + 1) % points.len()];
        let horizontal = (point.y - next.y).abs() <= epsilon;
        let vertical = (point.x - next.x).abs() <= epsilon;
        if horizontal == vertical {
            return None;
        }
    }
    (corners == 0b1111).then(|| Rect::new(x0, y0, x1, y1))
}

struct HasherWriter<'a, H>(&'a mut H);

impl<H: std::hash::Hasher> std::fmt::Write for HasherWriter<'_, H> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.0.write(value.as_bytes());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
