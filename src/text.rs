//! Text layout and glyph preparation.
//!
//! Implementation modules follow the text pipeline stages so shaping,
//! rasterization, outline extraction, and atlas preparation can evolve
//! independently.

mod context;
mod hit;
mod layout;
mod line_metrics;
mod options;
mod outline;
mod prepared;
mod raster;
mod scaler;

pub use context::TextContext;
pub use hit::text_hit;
pub use layout::TextLayout;
pub use line_metrics::text_layout_runs;
pub use options::{TextCompositeMode, TextLayoutOptions, TextRasterOptions, TextSubpixelMode};

#[cfg(test)]
pub(crate) use layout::scene_glyphs_at_origin;
pub(crate) use layout::{
    CanvasGlyph, TextRun, layout_bounds_at_scaled_origin, scene_glyphs_at_scaled_origin,
};
#[cfg(test)]
pub(crate) use prepared::PreparedGlyphImage;
pub(crate) use prepared::{AtlasSignature, PreparedTextChanges, PreparedTextData};

pub(crate) use raster::GlyphContent;

#[cfg(test)]
mod tests;
