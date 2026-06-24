use crate::{
    cpu::computes::coarse::rasterize_tile,
    shared::{
        bd_record::BackdropRecord,
        draw_record::DrawRecord,
        image::Image,
        line_seg::LineSegment,
        tile_seg_range::TileSegmentRange,
    },
};

pub struct CoarseCpuPrepared<'a> {
    draw_records: &'a [DrawRecord],
    backdrop_records: &'a [BackdropRecord],
    backdrops: &'a [i32],
    tile_segment_ranges: &'a [TileSegmentRange],
    segments: &'a [LineSegment],
    target: &'a mut Image,
    tiles_size: (u32, u32),
}

impl CoarseCpuPrepared<'_> {
    pub fn run(&mut self) {
        for draw_record in self.draw_records {
            let Some(path_id) = draw_record.path_id else {
                continue;
            };
            let backdrop_record = &self.backdrop_records[path_id as usize];
            let bbox = draw_record.tile_bbox(self.tiles_size.0, self.tiles_size.1);
            for tile_y in bbox.y0..bbox.y1 {
                for tile_x in bbox.x0..bbox.x1 {
                    let local_ix = ((tile_y - backdrop_record.tile_y0)
                        * (backdrop_record.tile_x1 - backdrop_record.tile_x0)
                        + (tile_x - backdrop_record.tile_x0)) as usize;
                    let tile_ix = backdrop_record.data_offset as usize + local_ix;
                    let backdrop = self.backdrops[tile_ix];
                    let range = self.tile_segment_ranges[tile_ix];
                    let segments = &self.segments[range.start as usize..range.end as usize];
                    rasterize_tile(
                        &mut self.target.pixels,
                        self.target.width,
                        self.target.height,
                        tile_x,
                        tile_y,
                        segments,
                        backdrop,
                        draw_record.fill_rule,
                        &draw_record.brush,
                    );
                }
            }
        }
    }
}

pub struct CoarseCpuPipeline;

impl CoarseCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare<'a>(
        &self,
        draw_records: &'a [DrawRecord],
        backdrop_records: &'a [BackdropRecord],
        backdrops: &'a [i32],
        tile_segment_ranges: &'a [TileSegmentRange],
        segments: &'a [LineSegment],
        target: &'a mut Image,
        tiles_size: (u32, u32),
    ) -> CoarseCpuPrepared<'a> {
        CoarseCpuPrepared {
            draw_records,
            backdrop_records,
            backdrops,
            tile_segment_ranges,
            segments,
            target,
            tiles_size,
        }
    }
}
