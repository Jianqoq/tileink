use cosmic_text::{Align, Attrs};

use crate::shared::pixel::TextCoverageParams;

#[derive(Clone, Debug)]
pub struct TextLayoutOptions<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub line_height: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub attrs: Attrs<'a>,
    pub alignment: Option<Align>,
}

impl<'a> TextLayoutOptions<'a> {
    pub fn new(text: &'a str, font_size: f32) -> Self {
        Self {
            text,
            font_size,
            line_height: font_size * 1.2,
            width: None,
            height: None,
            attrs: Attrs::new(),
            alignment: None,
        }
    }

    pub fn with_size(mut self, width: Option<f32>, height: Option<f32>) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub fn with_attrs(mut self, attrs: Attrs<'a>) -> Self {
        self.attrs = attrs;
        self
    }

    pub fn with_alignment(mut self, alignment: Option<Align>) -> Self {
        self.alignment = alignment;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TextRasterOptions {
    pub subpixel_mode: TextSubpixelMode,
    pub composite_mode: TextCompositeMode,
    /// Contrast-dependent text coverage parameters.
    ///
    /// The CPU and wgpu renderers consume this at runtime, which lets the
    /// quality harness search candidates without recompiling.
    pub coverage_params: TextCoverageParams,
}

impl TextRasterOptions {
    pub const fn new() -> Self {
        Self {
            subpixel_mode: TextSubpixelMode::Rgb,
            composite_mode: TextCompositeMode::Linear,
            coverage_params: TextCoverageParams::DEFAULT,
        }
    }

    pub const fn with_subpixel_mode(mut self, mode: TextSubpixelMode) -> Self {
        self.subpixel_mode = mode;
        self
    }

    pub const fn with_composite_mode(mut self, mode: TextCompositeMode) -> Self {
        self.composite_mode = mode;
        self
    }

    /// Overrides CPU text coverage compensation parameters for quality tuning.
    pub const fn with_coverage_params(mut self, params: TextCoverageParams) -> Self {
        self.coverage_params = params;
        self
    }

    pub(crate) const fn mask_embolden(self) -> f32 {
        match self.subpixel_mode {
            TextSubpixelMode::None => self.coverage_params.alpha_mask_embolden,
            TextSubpixelMode::Rgb | TextSubpixelMode::Bgr => {
                self.coverage_params.subpixel_mask_embolden
            }
        }
    }
}

impl Default for TextRasterOptions {
    fn default() -> Self {
        Self::new()
    }
}
