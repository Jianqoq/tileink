//! Criterion adapter for flat immediate-scene text preparation.

use peniko::{Color, kurbo::Point};

use crate::{Canvas, TextFontSystem};

use super::{
    PreparedTextData, TextCompositeMode, TextContext, TextLayout, TextLayoutOptions,
    TextRasterOptions,
};

#[doc(hidden)]
pub struct PreparedTextBenchmark {
    glyphs: Vec<super::CanvasGlyph>,
    runs: Vec<super::TextRun>,
    base_glyphs: Vec<super::CanvasGlyph>,
    alternate_glyphs: Vec<super::CanvasGlyph>,
    full_runs: Vec<super::TextRun>,
    short_runs: Vec<super::TextRun>,
    font_system: TextFontSystem,
    context: TextContext,
    prepared: PreparedTextData,
    shifted: bool,
    alternate: bool,
}

impl PreparedTextBenchmark {
    pub fn new(label_count: usize) -> Self {
        let mut font_system = TextFontSystem::new();
        let mut context = TextContext::new();
        let layout = context.layout(
            &mut font_system,
            TextLayoutOptions::new(
                "Market Watch AAPL 228.51 +0.63% Strategy Running Risk 34.2%",
                13.0,
            )
            .with_line_height(18.0)
            .with_size(Some(460.0), Some(22.0)),
        );
        let alternate_layout = context.layout(
            &mut font_system,
            TextLayoutOptions::new(
                "Market Watch MSFT 439.01 +0.60% Strategy Running Risk 34.2%",
                13.0,
            )
            .with_line_height(18.0)
            .with_size(Some(460.0), Some(22.0)),
        );
        let (glyphs, runs) = record_labels(&layout, label_count);
        let (alternate_glyphs, alternate_runs) = record_labels(&alternate_layout, label_count);
        assert_eq!(
            (glyphs.len(), runs.len()),
            (alternate_glyphs.len(), alternate_runs.len()),
            "benchmark replacement fixture must preserve glyph/run topology"
        );
        let short_runs = runs[..runs.len().saturating_sub(1)].to_vec();
        let base_glyphs = glyphs.clone();
        let full_runs = runs.clone();
        let prepared = PreparedTextData::new(&glyphs, &runs, &mut font_system, &mut context);
        Self {
            glyphs,
            runs,
            base_glyphs,
            alternate_glyphs,
            full_runs,
            short_runs,
            font_system,
            context,
            prepared,
            shifted: false,
            alternate: false,
        }
    }

    pub fn rebuild(&mut self) -> usize {
        self.prepared = PreparedTextData::new(
            &self.glyphs,
            &self.runs,
            &mut self.font_system,
            &mut self.context,
        );
        self.prepared.images().len()
    }

    pub fn prepare_position_shift(&mut self) -> usize {
        let delta = if self.shifted { -1.0 } else { 1.0 };
        self.shifted = !self.shifted;
        for glyph in &mut self.glyphs {
            *glyph = glyph.translated(delta, 0.0);
        }
        let changes = self.prepared.reconcile(
            &self.glyphs,
            &self.runs,
            &mut self.font_system,
            &mut self.context,
        );
        change_count(changes.as_ref()) + self.prepared.images().len()
    }

    pub fn prepare_glyph_replacement(&mut self) -> usize {
        self.alternate = !self.alternate;
        self.glyphs.clone_from(if self.alternate {
            &self.alternate_glyphs
        } else {
            &self.base_glyphs
        });
        let changes = self.prepared.reconcile(
            &self.glyphs,
            &self.runs,
            &mut self.font_system,
            &mut self.context,
        );
        change_count(changes.as_ref()) + self.prepared.images().len()
    }

    pub fn prepare_run_topology(&mut self) -> usize {
        self.alternate = !self.alternate;
        self.runs.clone_from(if self.alternate {
            &self.short_runs
        } else {
            &self.full_runs
        });
        let changes = self.prepared.reconcile(
            &self.glyphs,
            &self.runs,
            &mut self.font_system,
            &mut self.context,
        );
        change_count(changes.as_ref()) + self.prepared.images().len()
    }

    pub fn prepare_raster_option_change(&mut self) -> usize {
        self.alternate = !self.alternate;
        let composite = if self.alternate {
            TextCompositeMode::Srgb
        } else {
            TextCompositeMode::Linear
        };
        self.context
            .set_raster_options(TextRasterOptions::new().with_composite_mode(composite));
        let changes = self.prepared.reconcile(
            &self.glyphs,
            &self.runs,
            &mut self.font_system,
            &mut self.context,
        );
        change_count(changes.as_ref()) + self.prepared.images().len()
    }

    pub fn prepare_font_cache_clear(&mut self) -> usize {
        self.context.clear_glyph_caches();
        let changes = self.prepared.reconcile(
            &self.glyphs,
            &self.runs,
            &mut self.font_system,
            &mut self.context,
        );
        change_count(changes.as_ref()) + self.prepared.images().len()
    }
}

fn record_labels(
    layout: &TextLayout,
    label_count: usize,
) -> (Vec<super::CanvasGlyph>, Vec<super::TextRun>) {
    let mut canvas = Canvas::new(1920, 1200, 1.0);
    for index in 0..label_count {
        canvas.push_text_layout(
            layout,
            Point::new(8.0, 18.0 + (index % 60) as f64 * 19.0),
            Color::WHITE,
        );
    }
    (canvas.text_glyphs, canvas.text_runs)
}

fn change_count(changes: Option<&super::PreparedTextChanges>) -> usize {
    changes.map_or(0, |changes| {
        changes
            .glyphs()
            .iter()
            .chain(changes.runs())
            .map(std::ops::Range::len)
            .sum()
    })
}
