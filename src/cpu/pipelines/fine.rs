use crate::{
    cpu::computes::fine::{composite_color_tile, rasterize_tile},
    shared::{
        image::Image, line_seg::LineSegment, tile_ptcl::TilePtcl, tile_ptcl::TilePtclRange,
    },
};

pub struct FineCpuPrepared<'a> {
    tile_ptcl_ranges: &'a [TilePtclRange],
    tile_ptcls: &'a [TilePtcl],
    segments: &'a [LineSegment],
    target: &'a mut Image,
    tiles_size: (u32, u32),
}

impl<'a> FineCpuPrepared<'a> {
    pub fn run(&mut self) {
        for tile_y in 0..self.tiles_size.1 {
            for tile_x in 0..self.tiles_size.0 {
                let tile_ix = (tile_y * self.tiles_size.0 + tile_x) as usize;
                let range = self.tile_ptcl_ranges[tile_ix];
                if range.start == range.end {
                    continue;
                }

                for ptcl in &self.tile_ptcls[range.start as usize..range.end as usize] {
                    match ptcl {
                        TilePtcl::End => break,
                        TilePtcl::Color(color) => {
                            composite_color_tile(
                                &mut self.target.pixels,
                                self.target.width,
                                self.target.height,
                                tile_x,
                                tile_y,
                                color.color,
                            );
                        }
                        TilePtcl::Fill(fill) | TilePtcl::BeginClip(fill) => {
                            let segments = &self.segments
                                [fill.segment_range.start as usize..fill.segment_range.end as usize];
                            rasterize_tile(
                                &mut self.target.pixels,
                                self.target.width,
                                self.target.height,
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
        tiles_size: (u32, u32),
    ) -> FineCpuPrepared<'a> {
        FineCpuPrepared {
            tile_ptcl_ranges,
            tile_ptcls,
            segments,
            target,
            tiles_size,
        }
    }
}
