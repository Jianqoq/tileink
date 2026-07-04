#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlyphRunRecord {
    pub(crate) glyph_start: u32,
    pub(crate) glyph_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlyphRecord {
    pub(crate) image_id: u32,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlyphImageRecord {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) content: u32,
    pub(crate) data_offset: u32,
}

#[cfg(test)]
mod tests {
    use super::{GlyphImageRecord, GlyphRecord, GlyphRunRecord};

    #[test]
    fn glyph_records_are_gpu_word_layouts() {
        assert_eq!(std::mem::size_of::<GlyphRunRecord>(), 8);
        assert_eq!(std::mem::size_of::<GlyphRecord>(), 12);
        assert_eq!(std::mem::size_of::<GlyphImageRecord>(), 24);

        let run = GlyphRunRecord {
            glyph_start: 1,
            glyph_count: 2,
        };
        assert_eq!(bytemuck::cast_slice::<_, u32>(&[run]), &[1, 2]);

        let glyph = GlyphRecord {
            image_id: 3,
            x: -4,
            y: 5,
        };
        assert_eq!(
            bytemuck::cast_slice::<_, u32>(&[glyph]),
            &[3, (-4i32) as u32, 5]
        );

        let image = GlyphImageRecord {
            left: -6,
            top: 7,
            width: 8,
            height: 9,
            content: 10,
            data_offset: 11,
        };
        assert_eq!(
            bytemuck::cast_slice::<_, u32>(&[image]),
            &[(-6i32) as u32, 7, 8, 9, 10, 11]
        );
    }
}
