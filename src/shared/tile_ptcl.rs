use std::ops::Range;

use peniko::BlendMode;

use crate::shared::{brush::Brush, fill::FillRule, sdf::Sdf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TilePtclRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug)]
pub struct TileFillPtcl {
    pub backdrop: i32,
    pub fill_rule: FillRule,
    pub segment_range: Range<u32>,
    pub brush: Brush,
}

#[derive(Clone, Debug)]
pub struct TileColorPtcl {
    pub color: u32,
}

#[derive(Clone, Debug)]
pub struct TileSdfPtcl {
    pub sdf: Sdf,
    pub brush: Brush,
}

#[derive(Clone, Debug)]
pub struct TileGlyphPtcl {
    pub glyph_run_id: u32,
    pub brush: Brush,
}

#[derive(Clone, Debug)]
pub enum TilePtcl {
    End,
    Fill(TileFillPtcl),
    Sdf(TileSdfPtcl),
    Glyph(TileGlyphPtcl),
    Color(TileColorPtcl),
    BeginClip(TileFillPtcl),
    EndClip,
    BeginOpacity { opacity: u8, fill: TileFillPtcl },
    EndOpacity,
    BeginBlend { mode: BlendMode, fill: TileFillPtcl },
    EndBlend,
}
