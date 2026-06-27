use bytemuck::{Pod, Zeroable};

use crate::{scene::Scene, shared::tile_seg_range::TileSegmentRange};

/// CPU scene metadata converted into fixed GPU buffer lengths.
///
/// WGPU compute cannot append arbitrary memory at runtime, so every stage gets
/// deterministic capacity from scene compilation. This is a real resource
/// contract for the GPU backend, not a temporary testing shim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WgpuBufferLengths {
    pub line_count: usize,
    pub path_count: usize,
    pub draw_count: usize,
    pub backdrop_record_count: usize,
    pub backdrop_len: usize,
    pub segment_capacity: usize,
    pub tile_count: usize,
    pub image_pixels: usize,
}

impl WgpuBufferLengths {
    pub(crate) fn from_scene(scene: &Scene) -> Self {
        let tiles_width = scene.width_in_tiles() as usize;
        let tiles_height = scene.height_in_tiles() as usize;
        Self {
            line_count: scene.lines.len(),
            path_count: scene.path_records.len(),
            draw_count: scene.draw_records.len(),
            backdrop_record_count: scene.bd_records.len(),
            backdrop_len: scene.backdrop_pool_capacity as usize,
            segment_capacity: scene.tile_cnt as usize,
            tile_count: tiles_width * tiles_height,
            image_pixels: scene.width as usize * scene.height as usize,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct GpuSceneConfig {
    pub width: u32,
    pub height: u32,
    pub tiles_width: u32,
    pub tiles_height: u32,
    pub line_count: u32,
    pub path_count: u32,
    pub draw_count: u32,
    pub backdrop_record_count: u32,
    pub backdrop_len: u32,
    pub segment_capacity: u32,
    pub clear_color: u32,
    pub _pad0: u32,
}

impl GpuSceneConfig {
    pub(crate) fn new(scene: &Scene, lengths: WgpuBufferLengths, clear_color: u32) -> Self {
        Self {
            width: scene.width,
            height: scene.height,
            tiles_width: scene.width_in_tiles(),
            tiles_height: scene.height_in_tiles(),
            line_count: lengths.line_count as u32,
            path_count: lengths.path_count as u32,
            draw_count: lengths.draw_count as u32,
            backdrop_record_count: lengths.backdrop_record_count as u32,
            backdrop_len: lengths.backdrop_len as u32,
            segment_capacity: lengths.segment_capacity as u32,
            clear_color,
            _pad0: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct GpuTileSegmentRange {
    pub start: u32,
    pub end: u32,
}

impl From<TileSegmentRange> for GpuTileSegmentRange {
    fn from(range: TileSegmentRange) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct GpuLineSegment {
    pub point0: [f32; 2],
    pub point1: [f32; 2],
    pub y_edge: f32,
    pub _pad0: f32,
}
