use std::ops::Range;

use crate::shared::{
    brush::Brush,
    fill::FillRule,
    layer::blend::Blend,
};

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
pub enum TilePtcl {
    End,
    Fill(TileFillPtcl),
    Color(TileColorPtcl),
    BeginClip(TileFillPtcl),
    EndClip,
    BeginOpacity { opacity: u8 },
    EndOpacity,
    BeginBlend { blend: Blend },
    EndBlend,
}
