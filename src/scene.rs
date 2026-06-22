use peniko::kurbo::{Affine, BezPath, Shape};

use crate::shared::{
    bd_record::BackdropRecord,
    bounds::{Bounds, PixelBounds},
    brush::Brush,
    draw_record::DrawRecord,
    execution::DisplayItem,
    fill::FillRule,
    layer::{Layer, LayerKind, clip::Clip},
    line::Line,
    path::PathRecord,
    path_flatten::PathFlatten,
};

pub struct Scene {
    lines: Vec<Line>,
    path_records: Vec<PathRecord>,
    pub(crate) draw_records: Vec<DrawRecord>,
    bd_records: Vec<BackdropRecord>,
    pub(crate) items: Vec<DisplayItem>,
    layer_stack: Vec<LayerKind>,
    path_cnt: u32,
    backdrop_pool_capacity: u32,
    tile_cnt: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl Scene {
    pub fn push_clip_layer(&mut self, path: BezPath, transform: Affine, tolerance: f64) {
        let bounds = transform.transform_rect_bbox(path.bounding_box());
        let layer = Layer::Clip(Clip {
            path,
            bounds: Bounds {
                x0: bounds.x0.floor() as i32,
                y0: bounds.y0.floor() as i32,
                x1: bounds.x1.ceil() as i32,
                y1: bounds.y1.ceil() as i32,
            },
            transform,
            tolerance,
        });
        self.items.push(DisplayItem::BeginLayer(layer));
        self.layer_stack.push(LayerKind::Clip);
    }

    pub fn pop_layer(&mut self) -> Option<LayerKind> {
        let layer_kind = self.layer_stack.pop()?;
        self.items.push(DisplayItem::EndLayer);
        Some(layer_kind)
    }

    fn push_path(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        bounds_override: Option<Bounds>,
    ) {
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let mut local_tile_cnt = 0;
        PathFlatten::new(&path, tolerance as f32, path_id, &mut local_tile_cnt)
            .flatten(&mut self.lines);
        let line_count = self.lines.len() as u32 - line_start;
        self.path_records.push(PathRecord {
            path_id,
            line_count,
            line_start,
            _pad: 0,
        });
        let pixel_bounds = match bounds_override {
            Some(bounds) => PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            None => PixelBounds::from_path(&path, transform),
        };
        let tile_bbox = pixel_bounds.tile_bbox(self.width_in_tiles(), self.height_in_tiles());
        let tile_stride = tile_bbox.tile_stride();
        let tile_height = tile_bbox.tile_height();
        let backdrop_len = tile_stride * tile_height;

        let backdrop_offset = self.backdrop_pool_capacity;
        self.backdrop_pool_capacity += backdrop_len;
        let segment_start = self.tile_cnt;
        self.tile_cnt += local_tile_cnt;

        let draw_ix = self.draw_records.len();
        self.draw_records.push(DrawRecord {
            path_id: Some(path_id),
            brush: brush.into(),
            fill_rule: rule,
            pixel_bounds: Bounds::new(pixel_bounds.x0, pixel_bounds.y0, pixel_bounds.x1, pixel_bounds.y1),
            solid_rect: false,
            sdf: None,
            opacity_depth: 0,
            blend_depth: 0,
            clip_depth: self
                .layer_stack
                .iter()
                .filter(|&&kind| kind == LayerKind::Clip)
                .count() as u8,
            allow_solid_override: true,
        });
        self.items.push(DisplayItem::Draw(draw_ix));
        self.bd_records.push(BackdropRecord {
            path_id,
            data_offset: backdrop_offset,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity: local_tile_cnt,
            segment_count: 0,
        });
    }

    pub fn reset(&mut self) {
        self.lines.clear();
        self.path_records.clear();
        self.draw_records.clear();
        self.bd_records.clear();
        self.items.clear();
        self.layer_stack.clear();
        self.path_cnt = 0;
        self.backdrop_pool_capacity = 0;
        self.tile_cnt = 0;
    }

    fn width_in_tiles(&self) -> u32 {
        self.width.div_ceil(crate::TILE_SIZE)
    }

    fn height_in_tiles(&self) -> u32 {
        self.height.div_ceil(crate::TILE_SIZE)
    }
}
