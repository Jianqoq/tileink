//! Text layout and glyph preparation.
//!
//! Implementation modules follow the text pipeline stages so shaping,
//! rasterization, outline extraction, and atlas preparation can evolve
//! independently.

#[cfg(feature = "bench-internals")]
mod benchmark;
mod context;
mod layout;
mod options;
mod outline;
mod prepared;
mod raster;
mod scaler;

#[cfg(feature = "bench-internals")]
pub use benchmark::PreparedTextBenchmark;
pub use context::TextContext;
pub use layout::TextLayout;
pub use options::{TextCompositeMode, TextLayoutOptions, TextRasterOptions, TextSubpixelMode};

#[cfg(test)]
pub(crate) use layout::scene_glyphs_at_origin;
pub(crate) use layout::{
    CanvasGlyph, TextRun, layout_bounds_at_scaled_origin, scene_glyphs_at_scaled_origin,
};
#[cfg(test)]
pub(crate) use prepared::PreparedGlyphImage;
pub(crate) use prepared::{
    AtlasSignature, PreparedGlyphContent, PreparedTextChanges, PreparedTextData,
};

#[cfg(test)]
mod tests;
