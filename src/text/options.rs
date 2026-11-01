use cosmic_text::{Align, Attrs, Wrap};

use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct TextLayoutOptions<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub line_height: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub attrs: Attrs<'a>,
    pub alignment: Option<Align>,
    pub wrap: Wrap,
}

impl<'a> TextLayoutOptions<'a> {
    #[must_use]
    pub fn new(text: &'a str, font_size: f32) -> Self {
        Self {
            text,
            font_size,
            line_height: font_size * 1.2,
            width: None,
            height: None,
            attrs: Attrs::new(),
            alignment: None,
            // Match `cosmic_text::Buffer::new`; adding explicit wrap control must not change
            // existing callers that omit this option.
            wrap: Wrap::WordOrGlyph,
        }
    }

    #[must_use]
    pub const fn with_size(mut self, width: Option<f32>, height: Option<f32>) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    #[must_use]
    pub const fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    #[must_use]
    pub fn with_attrs(mut self, attrs: Attrs<'a>) -> Self {
        self.attrs = attrs;
        self
    }

    #[must_use]
    pub const fn with_alignment(mut self, alignment: Option<Align>) -> Self {
        self.alignment = alignment;
        self
    }

    #[must_use]
    pub const fn with_wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = wrap;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TextSubpixelMode {
    None,
    Rgb,
    Bgr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TextCompositeMode {
    Srgb,
    Linear,
}

#[derive(Clone, Copy, Debug)]
pub struct TextRasterOptions {
    pub subpixel_mode: TextSubpixelMode,
    pub composite_mode: TextCompositeMode,
    /// Extra outline weight in raster pixels for grayscale masks.
    pub alpha_mask_embolden: f32,
    /// Extra outline weight in raster pixels for RGB/BGR masks.
    pub subpixel_mask_embolden: f32,
}

impl TextRasterOptions {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            subpixel_mode: TextSubpixelMode::Rgb,
            composite_mode: TextCompositeMode::Linear,
            alpha_mask_embolden: if cfg!(target_os = "windows") {
                0.0
            } else {
                0.119_441_95
            },
            subpixel_mask_embolden: if cfg!(target_os = "windows") {
                0.0
            } else {
                0.032_732_67
            },
        }
    }

    #[must_use]
    pub const fn with_subpixel_mode(mut self, mode: TextSubpixelMode) -> Self {
        self.subpixel_mode = mode;
        self
    }

    #[must_use]
    pub const fn with_composite_mode(mut self, mode: TextCompositeMode) -> Self {
        self.composite_mode = mode;
        self
    }

    #[must_use]
    pub const fn with_alpha_mask_embolden(mut self, pixels: f32) -> Self {
        self.alpha_mask_embolden = pixels;
        self
    }

    #[must_use]
    pub const fn with_subpixel_mask_embolden(mut self, pixels: f32) -> Self {
        self.subpixel_mask_embolden = pixels;
        self
    }

    pub(crate) const fn mask_embolden(self) -> f32 {
        match self.subpixel_mode {
            TextSubpixelMode::None => self.alpha_mask_embolden,
            TextSubpixelMode::Rgb | TextSubpixelMode::Bgr => self.subpixel_mask_embolden,
        }
    }
}

impl Default for TextRasterOptions {
    fn default() -> Self {
        Self::new()
    }
}

// Bitwise float identity keeps glyph preparation cache keys reflexive, including
// NaN payloads and signed zero, without retaining the removed coverage settings.
impl PartialEq for TextRasterOptions {
    fn eq(&self, other: &Self) -> bool {
        self.subpixel_mode == other.subpixel_mode
            && self.composite_mode == other.composite_mode
            && self.alpha_mask_embolden.to_bits() == other.alpha_mask_embolden.to_bits()
            && self.subpixel_mask_embolden.to_bits() == other.subpixel_mask_embolden.to_bits()
    }
}
impl Eq for TextRasterOptions {}
impl Hash for TextRasterOptions {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.subpixel_mode.hash(state);
        self.composite_mode.hash(state);
        state.write_u32(self.alpha_mask_embolden.to_bits());
        state.write_u32(self.subpixel_mask_embolden.to_bits());
    }
}

#[cfg(test)]
#[path = "tests/options.rs"]
mod tests;
