pub(crate) const CUBE_DRAW_BRUSH: u32 = 0;
pub(crate) const CUBE_DRAW_CLIP: u32 = 1;
pub(crate) const CUBE_DRAW_OPACITY: u32 = 2;
pub(crate) const CUBE_DRAW_BLEND: u32 = 3;
pub(crate) const CUBE_DRAW_ISOLATE: u32 = 4;
pub(crate) const CUBE_DRAW_PATH_GLYPH: u32 = 5;

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
pub(crate) const CUBE_PTCL_PATH_GLYPH: u32 = 11;
pub(crate) const CUBE_PTCL_BEGIN_SDF_CLIP: u32 = 12;

pub(crate) const CUBE_GLYPH_MASK: u32 = 0;
pub(crate) const CUBE_GLYPH_COLOR: u32 = 1;
pub(crate) const CUBE_GLYPH_SUBPIXEL_MASK: u32 = 2;
pub(crate) const CUBE_GLYPH_LINEAR_MASK: u32 = 3;
pub(crate) const CUBE_GLYPH_LINEAR_COLOR: u32 = 4;
pub(crate) const CUBE_GLYPH_LINEAR_SUBPIXEL_MASK: u32 = 5;

pub(crate) const CUBE_SDF_NONE: u32 = 0;
pub(crate) const CUBE_SDF_RECT: u32 = 1;
pub(crate) const CUBE_SDF_CIRCLE: u32 = 2;
pub(crate) const CUBE_SDF_RECT_STROKE: u32 = 3;
pub(crate) const CUBE_SDF_CIRCLE_STROKE: u32 = 4;
pub(crate) const CUBE_SDF_CANDLESTICK: u32 = 5;
pub(crate) const CUBE_SDF_LINE: u32 = 6;
pub(crate) const CUBE_SDF_RECT_SHADOW: u32 = 7;
pub(crate) const CUBE_SDF_ARC: u32 = 8;
pub(crate) const CUBE_SDF_ARC_SHADOW: u32 = 9;
pub(crate) const CUBE_SDF_CIRCLE_SHADOW: u32 = 10;
pub(crate) const CUBE_SDF_LINE_SHADOW: u32 = 11;
pub(crate) const CUBE_SDF_DASH_LINE: u32 = 12;

pub(crate) const DRAW_FLAG_TAG_MASK: u32 = 0b0000_0111;
pub(crate) const DRAW_FLAG_FILL_RULE_EVEN_ODD: u32 = 1 << 3;
pub(crate) const DRAW_FLAG_SOLID_RECT: u32 = 1 << 4;
pub(crate) const DRAW_FLAG_SOLID_COLOR_FAST_PATH: u32 = 1 << 5;
pub(crate) const DRAW_FLAG_HAS_SDF: u32 = 1 << 6;
pub(crate) const DRAW_FLAG_HAS_GLYPH: u32 = 1 << 7;
