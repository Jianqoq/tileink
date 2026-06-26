use crate::{
    cpu::computes::fine::{composite_color_tile_into, rasterize_tile_into},
    shared::{
        bounds::{Bounds, PixelBounds}, image::Image, line_seg::LineSegment, tile_ptcl::TilePtcl,
        tile_ptcl::TilePtclRange,
    },
};

pub struct FineCpuPrepared<'a> {
    tile_ptcl_ranges: &'a [TilePtclRange],
    tile_ptcls: &'a [TilePtcl],
    segments: &'a [LineSegment],
    target: &'a mut Image,
    target_bounds: Bounds,
    tiles_size: (u32, u32),
}

impl<'a> FineCpuPrepared<'a> {
    pub fn run(&mut self) {
        let bounds = self.target_bounds.intersect(Bounds::new(
            0,
            0,
            (self.tiles_size.0 * crate::TILE_SIZE) as i32,
            (self.tiles_size.1 * crate::TILE_SIZE) as i32,
        ));
        let tile_bbox = PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
        .tile_bbox(self.tiles_size.0, self.tiles_size.1);
        for tile_y in tile_bbox.y0..tile_bbox.y1 {
            for tile_x in tile_bbox.x0..tile_bbox.x1 {
                let tile_ix = (tile_y * self.tiles_size.0 + tile_x) as usize;
                let range = self.tile_ptcl_ranges[tile_ix];
                if range.start == range.end {
                    continue;
                }

                for ptcl in &self.tile_ptcls[range.start as usize..range.end as usize] {
                    match ptcl {
                        TilePtcl::End => break,
                        TilePtcl::Color(color) => {
                            composite_color_tile_into(
                                &mut self.target.pixels,
                                self.target.width,
                                self.target.height,
                                self.target_bounds.x0,
                                self.target_bounds.y0,
                                tile_x,
                                tile_y,
                                color.color,
                            );
                        }
                        TilePtcl::Fill(fill) | TilePtcl::BeginClip(fill) => {
                            let segments = &self.segments
                                [fill.segment_range.start as usize..fill.segment_range.end as usize];
                            rasterize_tile_into(
                                &mut self.target.pixels,
                                self.target.width,
                                self.target.height,
                                self.target_bounds.x0,
                                self.target_bounds.y0,
                                tile_x,
                                tile_y,
                                segments,
                                fill.backdrop,
                                fill.fill_rule,
                                &fill.brush,
                            );
                        }
                        TilePtcl::EndClip
                        | TilePtcl::BeginOpacity { .. }
                        | TilePtcl::EndOpacity
                        | TilePtcl::BeginBlend { .. }
                        | TilePtcl::EndBlend => {}
                    }
                }
            }
        }
    }
}

pub struct FineCpuPipeline;

impl FineCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn prepare<'a>(
        &self,
        tile_ptcl_ranges: &'a [TilePtclRange],
        tile_ptcls: &'a [TilePtcl],
        segments: &'a [LineSegment],
        target: &'a mut Image,
        target_bounds: Bounds,
        tiles_size: (u32, u32),
    ) -> FineCpuPrepared<'a> {
        FineCpuPrepared {
            tile_ptcl_ranges,
            tile_ptcls,
            segments,
            target,
            target_bounds,
            tiles_size,
        }
    }
}
