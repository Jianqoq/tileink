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

pub struct CoarseCpuPrepared {
}

impl CoarseCpuPrepared {
    pub fn run(&mut self) {
        
    }
}

pub struct CoarseCpuPipeline;

impl CoarseCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn prepare(
        &self,
    ) -> CoarseCpuPrepared {
        CoarseCpuPrepared {
        }
    }
}
