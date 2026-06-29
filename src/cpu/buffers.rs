use std::sync::atomic::{AtomicU32, Ordering};

use crate::shared::{
    bd_record::BackdropRecord,
    line_seg::LineSegment,
    tile_ptcl::{TilePtcl, TilePtclRange},
    tile_seg_range::TileSegmentRange,
};

#[derive(Default)]
pub(in crate::cpu) struct RasterBuffers {
    pub(in crate::cpu) backdrops: Vec<i32>,
    pub(in crate::cpu) tile_segment_ranges: Vec<TileSegmentRange>,
    pub(in crate::cpu) segments: Vec<LineSegment>,
    pub(in crate::cpu) tile_glyphs: Vec<u32>,
    pub(in crate::cpu) segments_bump: Vec<AtomicU32>,
    pub(in crate::cpu) segment_tile_counts: Vec<u32>,
    pub(in crate::cpu) segment_tile_cursors: Vec<AtomicU32>,
    pub(in crate::cpu) tile_ptcl_ranges: Vec<TilePtclRange>,
    pub(in crate::cpu) tile_ptcls: Vec<TilePtcl>,
}

impl RasterBuffers {
    pub(in crate::cpu) fn clear_scan_outputs(&mut self) {
        self.backdrops.clear();
        self.tile_segment_ranges.clear();
        self.segments.clear();
        self.tile_glyphs.clear();
        self.segment_tile_counts.clear();
        self.segment_tile_cursors.clear();
        self.segments_bump.clear();
        self.tile_ptcl_ranges.clear();
        self.tile_ptcls.clear();
    }

    pub(in crate::cpu) fn resize_scan_outputs(
        &mut self,
        scene: &crate::scene::Scene,
        last_bd_record: BackdropRecord,
    ) {
        let backdrop_len = last_bd_record.data_offset as usize + last_bd_record.data_len as usize;
        let segment_len =
            last_bd_record.segment_start as usize + last_bd_record.segment_capacity as usize;
        self.backdrops.resize(backdrop_len, 0);
        self.tile_segment_ranges
            .resize(backdrop_len, TileSegmentRange::default());
        self.segments.resize(segment_len, LineSegment::default());
        self.segment_tile_counts.resize(backdrop_len, 0);
        self.segment_tile_cursors
            .resize_with(backdrop_len, || AtomicU32::new(0));
        self.segments_bump
            .resize_with(scene.bd_records.len(), || AtomicU32::new(0));
        for bump in &self.segments_bump {
            bump.store(0, Ordering::Relaxed);
        }
    }
}
