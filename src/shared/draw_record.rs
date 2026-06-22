use crate::{
    TILE_SIZE,
    shared::{bounds::Bounds, brush::Brush, fill::FillRule, sdf::Sdf},
};

/// One drawable path in document order (coarse iterates this list per tile).
#[derive(Clone, Debug)]
pub struct DrawRecord {
    /// Path index in [`Scene`](crate::gpu::scene::Scene), or `None` for non-path draws.
    pub path_id: Option<u32>,
    pub brush: Brush,
    pub fill_rule: FillRule,
    pub pixel_bounds: Bounds,
    /// CPU `FillRect` fast path: coarse emits `Color` only (no flatten/scan).
    pub solid_rect: bool,
    /// Native SDF primitive: coarse emits `Sdf` / solid `Color` (no flatten/scan).
    pub sdf: Option<Sdf>,
    pub opacity_depth: u8,
    pub blend_depth: u8,
    pub clip_depth: u8,
    pub allow_solid_override: bool,
}

impl DrawRecord {
    pub fn tile_bbox(&self, width_in_tiles: u32, height_in_tiles: u32) -> (u32, u32, u32, u32) {
        let tile_x0 = (self.pixel_bounds.x0.max(0) as u32 / TILE_SIZE).min(width_in_tiles);
        let tile_y0 = (self.pixel_bounds.y0.max(0) as u32 / TILE_SIZE).min(height_in_tiles);
        let tile_x1 = (self.pixel_bounds.x1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(width_in_tiles);
        let tile_y1 = (self.pixel_bounds.y1.max(0) as u32)
            .div_ceil(TILE_SIZE)
            .min(height_in_tiles);
        (tile_x0, tile_y0, tile_x1, tile_y1)
    }

    pub fn covers_tile(&self, tile_x: u32, tile_y: u32, width: u32, height: u32) -> bool {
        let tile_bounds = Bounds::from_tile_coords(tile_x, tile_y, width, height);
        let pb = self.pixel_bounds.intersect(tile_bounds);
        pb.x0 < pb.x1 && pb.y0 < pb.y1
    }
}
