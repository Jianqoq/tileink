use bytemuck::{Pod, Zeroable};

use crate::{scene::Scene, shared::draw_record::DrawTag};

pub(crate) const SCAN_CHUNK_SIZE: u32 = 256;
pub(crate) const CUMSUM_CHUNK_SIZE: u32 = 256;
pub(crate) const COARSE_CHUNK_SIZE: u32 = 256;

pub(crate) const CUBE_DRAW_BRUSH: u32 = 0;
pub(crate) const CUBE_DRAW_CLIP: u32 = 1;
pub(crate) const CUBE_DRAW_OPACITY: u32 = 2;
pub(crate) const CUBE_DRAW_BLEND: u32 = 3;
pub(crate) const CUBE_DRAW_ISOLATE: u32 = 4;

pub(crate) const CUBE_LAYER_CLIP: u32 = 0;
pub(crate) const CUBE_LAYER_OPACITY: u32 = 1;
pub(crate) const CUBE_LAYER_BLEND: u32 = 2;

pub(crate) const CUBE_PTCL_END: u32 = 0;
pub(crate) const CUBE_PTCL_FILL: u32 = 1;
pub(crate) const CUBE_PTCL_COLOR: u32 = 2;
pub(crate) const CUBE_PTCL_BEGIN_CLIP: u32 = 3;
pub(crate) const CUBE_PTCL_END_CLIP: u32 = 4;
pub(crate) const CUBE_PTCL_BEGIN_OPACITY: u32 = 5;
pub(crate) const CUBE_PTCL_END_OPACITY: u32 = 6;
pub(crate) const CUBE_PTCL_BEGIN_BLEND: u32 = 7;
pub(crate) const CUBE_PTCL_END_BLEND: u32 = 8;
pub(crate) const CUBE_PTCL_SDF: u32 = 9;
pub(crate) const CUBE_PTCL_GLYPH: u32 = 10;

pub(crate) const CUBE_GLYPH_MASK: u32 = 0;
pub(crate) const CUBE_GLYPH_COLOR: u32 = 1;

pub(crate) const CUBE_SDF_NONE: u32 = 0;
pub(crate) const CUBE_SDF_RECT: u32 = 1;
pub(crate) const CUBE_SDF_CIRCLE: u32 = 2;
pub(crate) const CUBE_SDF_RECT_STROKE: u32 = 3;
pub(crate) const CUBE_SDF_CIRCLE_STROKE: u32 = 4;

/// Scene-derived fixed capacities for CubeCL buffers.
///
/// The CubeCL backend keeps the CPU renderer's stage boundaries but makes
/// buffer capacity explicit before kernels launch. This fixes the real issue:
/// compute stages must write into known memory ranges instead of growing
/// per-dispatch vectors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CubeBufferLengths {
    pub line_count: usize,
    pub path_count: usize,
    pub draw_count: usize,
    pub backdrop_record_count: usize,
    pub backdrop_len: usize,
    pub segment_capacity: usize,
    pub scan_chunk_count: usize,
    pub cumsum_chunk_count: usize,
    pub cumsum_row_count: usize,
    pub coarse_chunk_count: usize,
    pub coarse_ptcl_capacity: usize,
    pub tiles_width: usize,
    pub tiles_height: usize,
    pub tile_count: usize,
    pub image_pixels: usize,
}

impl CubeBufferLengths {
    pub(crate) fn from_scene(scene: &Scene) -> Self {
        let tiles_width = scene.width_in_tiles() as usize;
        let tiles_height = scene.height_in_tiles() as usize;
        let tile_count = tiles_width * tiles_height;
        let coarse_ptcl_capacity =
            coarse_ptcl_capacity(scene, tiles_width as u32, tiles_height as u32);
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
            cumsum_chunk_count: scene
                .bd_records
                .iter()
                .map(|record| {
                    let stride = record.tile_x1.saturating_sub(record.tile_x0);
                    let height = record.tile_y1.saturating_sub(record.tile_y0);
                    if stride == 0 {
                        0
                    } else {
                        (height * stride.div_ceil(CUMSUM_CHUNK_SIZE)) as usize
                    }
                })
                .sum(),
            cumsum_row_count: scene
                .bd_records
                .iter()
                .map(|record| {
                    let stride = record.tile_x1.saturating_sub(record.tile_x0);
                    let height = record.tile_y1.saturating_sub(record.tile_y0);
                    if stride == 0 { 0 } else { height as usize }
                })
                .sum(),
            coarse_chunk_count: tile_count.div_ceil(COARSE_CHUNK_SIZE as usize),
            coarse_ptcl_capacity,
            tiles_width,
            tiles_height,
            tile_count,
            image_pixels: scene.width as usize * scene.height as usize,
        }
    }
}

fn coarse_ptcl_capacity(scene: &Scene, width_in_tiles: u32, height_in_tiles: u32) -> usize {
    let draw_particles = scene
        .draw_records
        .iter()
        .filter(|draw| {
            (draw.path_id.is_some() || draw.sdf.is_some() || draw.glyph_run_id.is_some())
                && matches!(draw.tag, DrawTag::Brush | DrawTag::Clip)
        })
        .map(|draw| draw.tile_bbox(width_in_tiles, height_in_tiles).tile_count() as usize)
        .sum::<usize>();
    let group_begin_particles = scene
        .draw_records
        .iter()
        .filter(|draw| {
            draw.path_id.is_some()
                && matches!(
                    draw.tag,
                    DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
                )
        })
        .map(|draw| draw.tile_bbox(width_in_tiles, height_in_tiles).tile_count() as usize)
        .sum::<usize>();
    let layer_end_particles = scene
        .draw_records
        .iter()
        .filter(|draw| {
            draw.path_id.is_some()
                && matches!(
                    draw.tag,
                    DrawTag::Clip | DrawTag::Opacity | DrawTag::Blend | DrawTag::Isolate
                )
        })
        .map(|draw| draw.tile_bbox(width_in_tiles, height_in_tiles).tile_count() as usize)
        .sum::<usize>();
    width_in_tiles as usize * height_in_tiles as usize
        + draw_particles
        + group_begin_particles
        + layer_end_particles
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
    pub cumsum_chunk_count: u32,
    pub cumsum_row_count: u32,
    pub coarse_chunk_count: u32,
    pub coarse_ptcl_capacity: u32,
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
            cumsum_chunk_count: lengths.cumsum_chunk_count as u32,
            cumsum_row_count: lengths.cumsum_row_count as u32,
            coarse_chunk_count: lengths.coarse_chunk_count as u32,
            coarse_ptcl_capacity: lengths.coarse_ptcl_capacity as u32,
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

#[cfg(test)]
pub(crate) fn build_scan_chunks(scene: &Scene) -> (Vec<CubeScanChunk>, Vec<CubeScanChunkRange>) {
    let mut chunks = Vec::with_capacity(CubeBufferLengths::from_scene(scene).scan_chunk_count);
    let mut ranges = Vec::with_capacity(scene.path_records.len());
    build_scan_chunks_into(scene, &mut chunks, &mut ranges);
    (chunks, ranges)
}

pub(crate) fn build_scan_chunks_into(
    scene: &Scene,
    chunks: &mut Vec<CubeScanChunk>,
    ranges: &mut Vec<CubeScanChunkRange>,
) {
    chunks.clear();
    chunks.reserve(CubeBufferLengths::from_scene(scene).scan_chunk_count);
    ranges.clear();
    ranges.resize(scene.path_records.len(), CubeScanChunkRange::default());

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
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CubeCumsumPlan {
    pub chunk_backdrop_offsets: Vec<u32>,
    pub chunk_lens: Vec<u32>,
    pub row_chunk_starts: Vec<u32>,
    pub row_chunk_ends: Vec<u32>,
}

#[cfg(test)]
pub(crate) fn build_cumsum_plan(scene: &Scene) -> CubeCumsumPlan {
    let mut plan = CubeCumsumPlan::default();
    build_cumsum_plan_into(scene, &mut plan);
    plan
}

pub(crate) fn build_cumsum_plan_into(scene: &Scene, plan: &mut CubeCumsumPlan) {
    let lengths = CubeBufferLengths::from_scene(scene);
    plan.chunk_backdrop_offsets.clear();
    plan.chunk_lens.clear();
    plan.row_chunk_starts.clear();
    plan.row_chunk_ends.clear();
    plan.chunk_backdrop_offsets
        .reserve(lengths.cumsum_chunk_count);
    plan.chunk_lens.reserve(lengths.cumsum_chunk_count);
    plan.row_chunk_starts.reserve(lengths.cumsum_row_count);
    plan.row_chunk_ends.reserve(lengths.cumsum_row_count);

    for record in &scene.bd_records {
        let stride = record.tile_x1.saturating_sub(record.tile_x0);
        let height = record.tile_y1.saturating_sub(record.tile_y0);
        if stride == 0 || height == 0 {
            continue;
        }

        for row in 0..height {
            let row_start = plan.chunk_backdrop_offsets.len() as u32;
            let row_offset = record.data_offset + row * stride;
            let mut local_x = 0;
            while local_x < stride {
                let len = (stride - local_x).min(CUMSUM_CHUNK_SIZE);
                plan.chunk_backdrop_offsets.push(row_offset + local_x);
                plan.chunk_lens.push(len);
                local_x += len;
            }
            plan.row_chunk_starts.push(row_start);
            plan.row_chunk_ends
                .push(plan.chunk_backdrop_offsets.len() as u32);
        }
    }
}

#[cfg(test)]
mod tests {
    use peniko::{
        Color,
        kurbo::{Affine, Rect, Shape},
    };

    use super::{
        CUMSUM_CHUNK_SIZE, CubeBufferLengths, SCAN_CHUNK_SIZE, build_cumsum_plan, build_scan_chunks,
    };
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

    #[test]
    fn cumsum_plan_splits_each_backdrop_row_into_fixed_size_chunks() {
        let row_tiles = CUMSUM_CHUNK_SIZE + 17;
        let mut scene = Scene::new(row_tiles * crate::TILE_SIZE, crate::TILE_SIZE * 2);
        scene.push_path(
            Rect::new(
                0.0,
                0.0,
                f64::from(row_tiles * crate::TILE_SIZE),
                f64::from(crate::TILE_SIZE * 2),
            )
            .to_path(0.0),
            Color::BLACK,
            Affine::IDENTITY,
            FillRule::NonZero,
            0.0,
        );

        let lengths = CubeBufferLengths::from_scene(&scene);
        let plan = build_cumsum_plan(&scene);

        assert_eq!(lengths.cumsum_row_count, 2);
        assert_eq!(lengths.cumsum_chunk_count, 4);
        assert_eq!(plan.row_chunk_starts, vec![0, 2]);
        assert_eq!(plan.row_chunk_ends, vec![2, 4]);
        assert_eq!(
            plan.chunk_backdrop_offsets,
            vec![
                0,
                CUMSUM_CHUNK_SIZE,
                row_tiles,
                row_tiles + CUMSUM_CHUNK_SIZE
            ]
        );
        assert_eq!(
            plan.chunk_lens,
            vec![CUMSUM_CHUNK_SIZE, 17, CUMSUM_CHUNK_SIZE, 17]
        );
    }
}
