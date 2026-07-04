use crate::shared::{
    bounds::{PixelBounds, TileBbox},
    fill::FillRule,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawTag {
    Brush,
    /// Vector glyph outlines: path geometry with text coverage compositing.
    PathGlyph,
    Clip,
    Isolate,
    Opacity,
    Blend,
}

/// One drawable path in document order (coarse iterates this list per tile).
#[derive(Clone, Debug)]
pub struct DrawRecord {
    /// Path index in [`Canvas`](crate::gpu::canvas::Canvas), or `None` for non-path draws.
    pub path_id: Option<u32>,
    /// Text glyph run index, or `None` for non-text draws.
    pub glyph_run_id: Option<u32>,
    /// Exact SDF geometry index in `Canvas::sdfs`, or `None` for non-SDF draws.
    pub sdf_id: Option<u32>,
    /// Soft SDF shadow geometry index in `Canvas::sdf_shadows`, or `None` for non-shadow draws.
    pub sdf_shadow_id: Option<u32>,
    /// Brush index in `Canvas::brushes`.
    pub brush_id: u32,
    pub tag: DrawTag,
    pub fill_rule: FillRule,
    pub pixel_bounds: PixelBounds,
    /// CPU `FillRect` fast path: coarse emits `Color` only (no flatten/scan).
    pub solid_rect: bool,
}

impl DrawRecord {
    pub fn tile_bbox(&self, width_in_tiles: u32, height_in_tiles: u32) -> TileBbox {
        self.pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles)
    }

    pub(crate) fn has_analytic_geometry(&self) -> bool {
        self.sdf_id.is_some() || self.sdf_shadow_id.is_some()
    }

    // pub fn covers_tile(&self, tile_x: u32, tile_y: u32, width: u32, height: u32) -> bool {
    //     let tile_bounds = Bounds::from_tile_coords(tile_x, tile_y, width, height);
    //     let pb = self.pixel_bounds.intersect(tile_bounds);
    //     pb.x0 < pb.x1 && pb.y0 < pb.y1
    // }
}
