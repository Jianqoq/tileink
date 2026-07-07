//! Text layout and glyph preparation.
//!
//! Implementation modules follow the text pipeline stages so shaping,
//! rasterization, outline extraction, and atlas preparation can evolve
//! independently.

mod context;
mod layout;
mod options;
mod outline;
mod prepared;
mod raster;
mod scaler;

pub use context::TextContext;
pub use layout::TextLayout;
pub use options::{TextCompositeMode, TextLayoutOptions, TextRasterOptions, TextSubpixelMode};

#[cfg(test)]
pub(crate) use layout::scene_glyphs_at_origin;
pub(crate) use layout::{
    CanvasGlyph, TextRun, layout_bounds_at_scaled_origin, scene_glyphs_at_scaled_origin,
};
pub(crate) use prepared::{AtlasSignature, PreparedGlyphContent, PreparedTextData};
#[cfg(test)]
pub(crate) use prepared::{PreparedGlyph, PreparedGlyphImage};

#[cfg(test)]
mod tests;
