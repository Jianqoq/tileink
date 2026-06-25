use crate::{
    cpu::computes::coarse::rasterize_tile,
    shared::{
        bd_record::BackdropRecord, bounds::TileBbox, draw_record::DrawRecord, image::Image,
        line_seg::LineSegment, tile_seg_range::TileSegmentRange,
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

impl<'a> CoarseCpuPrepared<'a> {
    pub fn run(&mut self) {
        for draw in self.draw_records {
            let Some(path_id) = draw.path_id else {
                continue;
            };
            let backdrop_record = &self.backdrop_records[path_id as usize];
            let bbox = draw.tile_bbox(self.tiles_size.0, self.tiles_size.1);
            let stride = backdrop_record.tile_x1 - backdrop_record.tile_x0;
            if stride == 0 {
                return;
            }
            for tile_y in bbox.y0..bbox.y1 {
                for tile_x in bbox.x0..bbox.x1 {
                    let local_x = tile_x - backdrop_record.tile_x0;
                    let local_y = tile_y - backdrop_record.tile_y0;
                    let local_ix = (local_y * stride + local_x) as usize;
                    let backdrop_ix = backdrop_record.data_offset as usize + local_ix;
                    let segment_range = self.tile_segment_ranges[backdrop_ix];
                    let segments =
                        &self.segments[segment_range.start as usize..segment_range.end as usize];
                    let backdrop = self.backdrops[backdrop_ix];

                    rasterize_tile(
                        &mut self.target.pixels,
                        self.target.width,
                        self.target.height,
                        tile_x,
                        tile_y,
                        segments,
                        backdrop,
                        draw.fill_rule,
                        &draw.brush,
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
