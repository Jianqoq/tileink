use bytemuck::{Pod, Zeroable};

use crate::scene::Scene;

pub(crate) const SCAN_CHUNK_SIZE: u32 = 256;

/// Scene-derived fixed capacities for CubeCL buffers.
///
/// The CubeCL backend keeps the CPU renderer's stage boundaries but makes
/// buffer capacity explicit before kernels launch. This fixes the real issue:
/// compute stages must write into known memory ranges instead of growing
/// per-dispatch vectors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CubeBufferLengths {
    pub line_count: usize,
    pub path_count: usize,
    pub draw_count: usize,
    pub backdrop_record_count: usize,
    pub backdrop_len: usize,
    pub segment_capacity: usize,
    pub scan_chunk_count: usize,
    pub tile_count: usize,
    pub image_pixels: usize,
}

impl CubeBufferLengths {
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
            scan_chunk_count: scene
                .bd_records
                .iter()
                .map(|record| record.data_len.div_ceil(SCAN_CHUNK_SIZE) as usize)
                .sum(),
            tile_count: tiles_width * tiles_height,
            image_pixels: scene.width as usize * scene.height as usize,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(crate) struct CubeSceneConfig {
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
    pub scan_chunk_count: u32,
    pub clear_color: u32,
}

impl CubeSceneConfig {
    pub(crate) fn new(scene: &Scene, lengths: CubeBufferLengths, clear_color: u32) -> Self {
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
            scan_chunk_count: lengths.scan_chunk_count as u32,
            clear_color,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub(crate) struct CubeScanChunk {
    pub path_id: u32,
    pub backdrop_offset: u32,
    pub segment_start: u32,
    pub len: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub(crate) struct CubeScanChunkRange {
    pub start: u32,
    pub end: u32,
}

pub(crate) fn build_scan_chunks(scene: &Scene) -> (Vec<CubeScanChunk>, Vec<CubeScanChunkRange>) {
    let mut chunks = Vec::with_capacity(CubeBufferLengths::from_scene(scene).scan_chunk_count);
    let mut ranges = vec![CubeScanChunkRange::default(); scene.path_records.len()];

    for record in &scene.bd_records {
        let range_start = chunks.len() as u32;
        let mut local = 0;
        while local < record.data_len {
            let len = (record.data_len - local).min(SCAN_CHUNK_SIZE);
            chunks.push(CubeScanChunk {
                path_id: record.path_id,
                backdrop_offset: record.data_offset + local,
                segment_start: record.segment_start,
                len,
            });
            local += len;
        }
        if let Some(range) = ranges.get_mut(record.path_id as usize) {
            *range = CubeScanChunkRange {
                start: range_start,
                end: chunks.len() as u32,
            };
        }
    }

    (chunks, ranges)
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::{CubeBufferLengths, SCAN_CHUNK_SIZE, build_scan_chunks};
    use crate::{FillRule, Scene};

    #[test]
    fn scan_chunks_cover_each_backdrop_record_in_fixed_size_tiles() {
        let mut scene = Scene::new((SCAN_CHUNK_SIZE + 17) * crate::TILE_SIZE, crate::TILE_SIZE);
        scene.push_path(
            Rect::new(
                0.0,
                0.0,
                f64::from((SCAN_CHUNK_SIZE + 17) * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE),
            )
            .to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let lengths = CubeBufferLengths::from_scene(&scene);
        let (chunks, ranges) = build_scan_chunks(&scene);

        assert_eq!(lengths.scan_chunk_count, 2);
        assert_eq!(chunks.len(), 2);
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges[0].end, 2);
        assert_eq!(chunks[0].backdrop_offset, 0);
        assert_eq!(chunks[0].len, SCAN_CHUNK_SIZE);
        assert_eq!(chunks[1].backdrop_offset, SCAN_CHUNK_SIZE);
        assert_eq!(chunks[1].len, 17);
    }
}
