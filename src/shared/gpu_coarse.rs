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

#[cfg(test)]
mod tests {
    use super::TileCoarseRecord;

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
}
