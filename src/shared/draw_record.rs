use crate::shared::{
    bounds::{PixelBounds, TileBbox},
    fill::FillRule,
};

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrawTag {
    Brush = 0,
    /// Vector glyph outlines: path geometry with text coverage compositing.
    PathGlyph = 5,
    Clip = 1,
    Isolate = 4,
    Opacity = 2,
    Blend = 3,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawTagWord(pub u32);

impl From<DrawTag> for DrawTagWord {
    fn from(tag: DrawTag) -> Self {
        Self(tag as u32)
    }
}

impl PartialEq<DrawTag> for DrawTagWord {
    fn eq(&self, other: &DrawTag) -> bool {
        self.0 == *other as u32
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillRuleWord(pub u32);

impl From<FillRule> for FillRuleWord {
    fn from(rule: FillRule) -> Self {
        Self(rule as u32)
    }
}

impl PartialEq<FillRule> for FillRuleWord {
    fn eq(&self, other: &FillRule) -> bool {
        self.0 == *other as u32
    }
}

/// One drawable path in document order (coarse iterates this list per tile).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawRecord {
    /// Path index in [`Canvas`](crate::gpu::canvas::Canvas), or `NONE` for non-path draws.
    pub path_id: u32,
    /// Text glyph run index, or `NONE` for non-text draws.
    pub glyph_run_id: u32,
    /// Start word in `Canvas::sdf_blob`, or `NONE` for non-SDF draws.
    pub sdf_offset: u32,
    /// Exact SDF record length in 32-bit words.
    pub sdf_len: u32,
    /// Start word in `Canvas::sdf_shadow_blob`, or `NONE` for non-shadow draws.
    pub sdf_shadow_offset: u32,
    /// Soft SDF shadow record length in 32-bit words.
    pub sdf_shadow_len: u32,
    /// Start word in `Canvas::brush_blob`, or `NONE` when the draw has no brush.
    pub brush_offset: u32,
    /// Brush record length in 32-bit words.
    pub brush_len: u32,
    pub tag: DrawTagWord,
    pub fill_rule: FillRuleWord,
    pub pixel_bounds: PixelBounds,
    /// CPU `FillRect` fast path: coarse emits `Color` only (no flatten/scan).
    pub solid_rect: u32,
}

impl DrawRecord {
    pub const NONE: u32 = u32::MAX;

    pub fn tile_bbox(&self, width_in_tiles: u32, height_in_tiles: u32) -> TileBbox {
        self.pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles)
    }

    pub(crate) fn path_id(self) -> Option<u32> {
        (self.path_id != Self::NONE).then_some(self.path_id)
    }

    pub(crate) fn glyph_run_id(self) -> Option<u32> {
        (self.glyph_run_id != Self::NONE).then_some(self.glyph_run_id)
    }

    pub(crate) fn sdf_range(self) -> Option<std::ops::Range<usize>> {
        let end = self.sdf_offset.checked_add(self.sdf_len)?;
        (self.sdf_offset != Self::NONE).then_some(self.sdf_offset as usize..end as usize)
    }

    pub(crate) fn sdf_shadow_range(self) -> Option<std::ops::Range<usize>> {
        let end = self.sdf_shadow_offset.checked_add(self.sdf_shadow_len)?;
        (self.sdf_shadow_offset != Self::NONE)
            .then_some(self.sdf_shadow_offset as usize..end as usize)
    }

    pub(crate) fn has_analytic_geometry(&self) -> bool {
        self.sdf_offset != Self::NONE || self.sdf_shadow_offset != Self::NONE
    }

    pub(crate) fn has_path(self) -> bool {
        self.path_id != Self::NONE
    }

    pub(crate) fn solid_rect(self) -> bool {
        self.solid_rect != 0
    }

    pub(crate) fn tag(self) -> DrawTag {
        match self.tag.0 {
            value if value == DrawTag::Brush as u32 => DrawTag::Brush,
            value if value == DrawTag::PathGlyph as u32 => DrawTag::PathGlyph,
            value if value == DrawTag::Clip as u32 => DrawTag::Clip,
            value if value == DrawTag::Isolate as u32 => DrawTag::Isolate,
            value if value == DrawTag::Opacity as u32 => DrawTag::Opacity,
            value if value == DrawTag::Blend as u32 => DrawTag::Blend,
            _ => DrawTag::Brush,
        }
    }

    pub(crate) fn fill_rule(self) -> FillRule {
        match self.fill_rule.0 {
            value if value == FillRule::EvenOdd as u32 => FillRule::EvenOdd,
            _ => FillRule::NonZero,
        }
    }

    // pub fn covers_tile(&self, tile_x: u32, tile_y: u32, width: u32, height: u32) -> bool {
    //     let tile_bounds = Bounds::from_tile_coords(tile_x, tile_y, width, height);
    //     let pb = self.pixel_bounds.intersect(tile_bounds);
    //     pb.x0 < pb.x1 && pb.y0 < pb.y1
    // }
}

#[cfg(test)]
mod tests {
    use std::mem::{align_of, size_of};

    use super::*;

    #[test]
    fn draw_record_is_gpu_buffer_layout() {
        fn assert_pod<T: bytemuck::Pod>() {}

        assert_pod::<DrawRecord>();
        assert_eq!(DrawRecord::NONE, u32::MAX);
        assert_eq!(size_of::<DrawTagWord>(), size_of::<u32>());
        assert_eq!(size_of::<FillRuleWord>(), size_of::<u32>());
        assert_eq!(size_of::<DrawRecord>(), 60);
        assert_eq!(align_of::<DrawRecord>(), align_of::<u32>());
    }
}
