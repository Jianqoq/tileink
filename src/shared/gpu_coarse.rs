#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TileCoarseRecord {
    pub(crate) ptcl_count: u32,
    pub(crate) ptcl_start: u32,
    pub(crate) ptcl_end: u32,
    pub(crate) glyph_count: u32,
    pub(crate) glyph_start: u32,
    pub(crate) glyph_end: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CoarseChunkRecord {
    pub(crate) ptcl_total: u32,
    pub(crate) ptcl_offset: u32,
    pub(crate) glyph_total: u32,
    pub(crate) glyph_offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TileEmitChunkRecord {
    pub(crate) count: u32,
    pub(crate) offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EmitChunkRecord {
    pub(crate) tile: u32,
    pub(crate) local_chunk: u32,
    pub(crate) ptcl_count: u32,
    pub(crate) ptcl_offset: u32,
    pub(crate) glyph_count: u32,
    pub(crate) glyph_offset: u32,
    pub(crate) class_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TileDrawRecord {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[cfg(test)]
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FineTileKind {
    FullInterpreter = 0,
    EmptyOrClear = 1,
    ColorOnlyNoStack = 2,
    PureSdfSolidNoStack = 3,
    MixedAnalyticSolidNoStack = 4,
    AnalyticWithStack = 5,
}

#[cfg(test)]
impl FineTileKind {
    pub(crate) fn from_word(word: u32) -> Self {
        match word {
            1 => Self::EmptyOrClear,
            2 => Self::ColorOnlyNoStack,
            3 => Self::PureSdfSolidNoStack,
            4 => Self::MixedAnalyticSolidNoStack,
            5 => Self::AnalyticWithStack,
            _ => Self::FullInterpreter,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LayerStackRecord {
    pub(crate) tag: u32,
    pub(crate) draw: u32,
    pub(crate) payload: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PtclRecord {
    pub(crate) tag: u32,
    pub(crate) backdrop: i32,
    pub(crate) fill_rule: u32,
    pub(crate) segment_start: u32,
    pub(crate) segment_end: u32,
    pub(crate) color: u32,
}

pub(crate) const TILE_COARSE_RECORD_WORDS: usize = std::mem::size_of::<TileCoarseRecord>() / 4;
pub(crate) const PTCL_RECORD_WORDS: usize = std::mem::size_of::<PtclRecord>() / 4;
pub(crate) const TILE_DRAW_RECORD_WORDS: usize = std::mem::size_of::<TileDrawRecord>() / 4;
pub(crate) const TILE_EMIT_CHUNK_RECORD_WORDS: usize =
    std::mem::size_of::<TileEmitChunkRecord>() / 4;
pub(crate) const EMIT_CHUNK_RECORD_WORDS: usize = std::mem::size_of::<EmitChunkRecord>() / 4;
pub(crate) const FINE_TILE_KIND_WORDS: usize = 1;

pub(crate) fn coarse_work_ptcl_word_offset(tile_count: usize) -> usize {
    tile_count * TILE_COARSE_RECORD_WORDS
}

pub(crate) fn coarse_work_glyph_word_offset(tile_count: usize, ptcl_capacity: usize) -> usize {
    coarse_work_ptcl_word_offset(tile_count) + ptcl_capacity * PTCL_RECORD_WORDS
}

pub(crate) fn coarse_work_tile_draw_record_word_offset(
    tile_count: usize,
    ptcl_capacity: usize,
    glyph_capacity: usize,
) -> usize {
    coarse_work_glyph_word_offset(tile_count, ptcl_capacity) + glyph_capacity
}

pub(crate) fn coarse_work_tile_draw_index_word_offset(
    tile_count: usize,
    ptcl_capacity: usize,
    glyph_capacity: usize,
) -> usize {
    coarse_work_tile_draw_record_word_offset(tile_count, ptcl_capacity, glyph_capacity)
        + tile_count * TILE_DRAW_RECORD_WORDS
}

pub(crate) fn coarse_work_tile_emit_chunk_record_word_offset(
    tile_count: usize,
    ptcl_capacity: usize,
    glyph_capacity: usize,
    tile_draw_index_count: usize,
) -> usize {
    coarse_work_tile_draw_index_word_offset(tile_count, ptcl_capacity, glyph_capacity)
        + tile_draw_index_count
}

pub(crate) fn coarse_work_emit_chunk_record_word_offset(
    tile_count: usize,
    ptcl_capacity: usize,
    glyph_capacity: usize,
    tile_draw_index_count: usize,
) -> usize {
    coarse_work_tile_emit_chunk_record_word_offset(
        tile_count,
        ptcl_capacity,
        glyph_capacity,
        tile_draw_index_count,
    ) + tile_count * TILE_EMIT_CHUNK_RECORD_WORDS
}

pub(crate) fn coarse_work_word_len(
    tile_count: usize,
    ptcl_capacity: usize,
    glyph_capacity: usize,
    tile_draw_index_count: usize,
    tile_draw_chunk_count: usize,
) -> usize {
    coarse_work_active_tile_list_word_offset(
        tile_count,
        ptcl_capacity,
        glyph_capacity,
        tile_draw_index_count,
        tile_draw_chunk_count,
    ) + tile_count
}

pub(crate) fn coarse_work_fine_tile_kind_word_offset(
    tile_count: usize,
    ptcl_capacity: usize,
    glyph_capacity: usize,
    tile_draw_index_count: usize,
    tile_draw_chunk_count: usize,
) -> usize {
    coarse_work_emit_chunk_record_word_offset(
        tile_count,
        ptcl_capacity,
        glyph_capacity,
        tile_draw_index_count,
    ) + tile_draw_chunk_count * EMIT_CHUNK_RECORD_WORDS
}

pub(crate) fn coarse_work_active_tile_list_word_offset(
    tile_count: usize,
    ptcl_capacity: usize,
    glyph_capacity: usize,
    tile_draw_index_count: usize,
    tile_draw_chunk_count: usize,
) -> usize {
    coarse_work_fine_tile_kind_word_offset(
        tile_count,
        ptcl_capacity,
        glyph_capacity,
        tile_draw_index_count,
        tile_draw_chunk_count,
    ) + tile_count * FINE_TILE_KIND_WORDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_coarse_record_is_gpu_word_layout() {
        assert_eq!(std::mem::size_of::<TileCoarseRecord>(), 24);
        let record = TileCoarseRecord {
            ptcl_count: 1,
            ptcl_start: 2,
            ptcl_end: 3,
            glyph_count: 4,
            glyph_start: 5,
            glyph_end: 6,
        };
        assert_eq!(
            bytemuck::cast_slice::<_, u32>(&[record]),
            &[1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn coarse_record_structs_are_gpu_word_layouts() {
        assert_eq!(std::mem::size_of::<CoarseChunkRecord>(), 16);
        assert_eq!(std::mem::size_of::<TileEmitChunkRecord>(), 8);
        assert_eq!(std::mem::size_of::<EmitChunkRecord>(), 28);
        assert_eq!(std::mem::size_of::<TileDrawRecord>(), 8);
        assert_eq!(std::mem::size_of::<LayerStackRecord>(), 12);
        assert_eq!(std::mem::size_of::<PtclRecord>(), 24);

        let chunk = CoarseChunkRecord {
            ptcl_total: 1,
            ptcl_offset: 2,
            glyph_total: 3,
            glyph_offset: 4,
        };
        assert_eq!(bytemuck::cast_slice::<_, u32>(&[chunk]), &[1, 2, 3, 4]);

        let tile = TileDrawRecord { start: 1, end: 2 };
        assert_eq!(bytemuck::cast_slice::<_, u32>(&[tile]), &[1, 2]);

        let tile_emit_chunk = TileEmitChunkRecord {
            count: 5,
            offset: 6,
        };
        assert_eq!(bytemuck::cast_slice::<_, u32>(&[tile_emit_chunk]), &[5, 6]);

        let emit_chunk = EmitChunkRecord {
            tile: 7,
            local_chunk: 8,
            ptcl_count: 9,
            ptcl_offset: 10,
            glyph_count: 11,
            glyph_offset: 12,
            class_flags: 13,
        };
        assert_eq!(
            bytemuck::cast_slice::<_, u32>(&[emit_chunk]),
            &[7, 8, 9, 10, 11, 12, 13]
        );

        let layer = LayerStackRecord {
            tag: 3,
            draw: 4,
            payload: 5,
        };
        assert_eq!(bytemuck::cast_slice::<_, u32>(&[layer]), &[3, 4, 5]);

        let ptcl = PtclRecord {
            tag: 6,
            backdrop: -7,
            fill_rule: 8,
            segment_start: 9,
            segment_end: 10,
            color: 11,
        };
        assert_eq!(
            bytemuck::cast_slice::<_, u32>(&[ptcl]),
            &[6, (-7i32) as u32, 8, 9, 10, 11]
        );
    }

    #[test]
    fn coarse_work_sections_are_tightly_packed_words() {
        assert_eq!(TILE_COARSE_RECORD_WORDS, 6);
        assert_eq!(PTCL_RECORD_WORDS, 6);
        assert_eq!(TILE_DRAW_RECORD_WORDS, 2);
        assert_eq!(TILE_EMIT_CHUNK_RECORD_WORDS, 2);
        assert_eq!(EMIT_CHUNK_RECORD_WORDS, 7);
        assert_eq!(FINE_TILE_KIND_WORDS, 1);
        assert_eq!(coarse_work_ptcl_word_offset(3), 18);
        assert_eq!(coarse_work_glyph_word_offset(3, 5), 48);
        assert_eq!(coarse_work_tile_draw_record_word_offset(3, 5, 7), 55);
        assert_eq!(coarse_work_tile_draw_index_word_offset(3, 5, 7), 61);
        assert_eq!(
            coarse_work_tile_emit_chunk_record_word_offset(3, 5, 7, 11),
            72
        );
        assert_eq!(coarse_work_emit_chunk_record_word_offset(3, 5, 7, 11), 78);
        assert_eq!(coarse_work_fine_tile_kind_word_offset(3, 5, 7, 11, 13), 169);
        assert_eq!(
            coarse_work_active_tile_list_word_offset(3, 5, 7, 11, 13),
            172
        );
        assert_eq!(coarse_work_word_len(3, 5, 7, 11, 13), 175);
    }
}
