use crate::shared::{
    bd_record::BackdropRecord, bounds::TileBbox, draw_record::DrawRecord, image::Image,
    line_seg::LineSegment, tile_seg_range::TileSegmentRange,
};

pub struct CoarseCpuPrepared<'a> {
    draw_records: &'a [DrawRecord],
}

impl<'a> CoarseCpuPrepared<'a> {
    pub fn run(&mut self) {}
}

pub struct CoarseCpuPipeline;

impl CoarseCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn prepare<'a>(&self, draw_records: &'a [DrawRecord]) -> CoarseCpuPrepared<'a> {
        CoarseCpuPrepared { draw_records }
    }
}
