use crate::shared::{
    bounds::{PixelBounds, TileBbox},
    brush::Brush,
    fill::FillRule,
    sdf::Sdf,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawTag {
    Brush,
    Clip,
    Isolate,
    Opacity,
    Blend,
}

/// One drawable path in document order (coarse iterates this list per tile).
#[derive(Clone, Debug)]
pub struct DrawRecord {
    /// Path index in [`Scene`](crate::gpu::scene::Scene), or `None` for non-path draws.
    pub path_id: Option<u32>,
    /// Text glyph run index, or `None` for non-text draws.
    pub glyph_run_id: Option<u32>,
    /// Exact SDF geometry for simple primitives that do not need path scan/cumsum.
    pub sdf: Option<Sdf>,
    pub tag: DrawTag,
    pub brush: Brush,
    pub fill_rule: FillRule,
    pub pixel_bounds: PixelBounds,
    /// CPU `FillRect` fast path: coarse emits `Color` only (no flatten/scan).
    pub solid_rect: bool,
}

impl DrawRecord {
    pub fn tile_bbox(&self, width_in_tiles: u32, height_in_tiles: u32) -> TileBbox {
        self.pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles)
    }

    // pub fn covers_tile(&self, tile_x: u32, tile_y: u32, width: u32, height: u32) -> bool {
    //     let tile_bounds = Bounds::from_tile_coords(tile_x, tile_y, width, height);
    //     let pb = self.pixel_bounds.intersect(tile_bounds);
    //     pb.x0 < pb.x1 && pb.y0 < pb.y1
    // }
}
