//! Criterion adapter for incremental glyph-capacity dependency updates.

use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem};

use super::*;
use crate::{
    TextContext,
    shared::{
        affine::GpuAffine,
        bounds::PixelBounds,
        draw_record::{DrawTag, FillRuleWord},
    },
    text::{CanvasGlyph, TextRun},
};

#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum GlyphCapacityBenchmarkCase {
    Stable,
    OneGlyph,
    FragmentedGlyphs,
    ChangedRuns,
    ChangedDraws,
    Mixed,
}

#[doc(hidden)]
pub struct GlyphCapacityBenchmark {
    cache: GlyphCapacityCache,
    canvas: Canvas,
    text: PreparedTextData,
    changes: SceneBufferChanges,
}

impl GlyphCapacityBenchmark {
    pub fn new(case: GlyphCapacityBenchmarkCase) -> Self {
        const RUNS: usize = 4_096;
        const GLYPHS_PER_RUN: usize = 8;
        const DRAWS: usize = 16_384;

        let mut canvas = Canvas::new(2048, 2048, 1.0);
        canvas.text_runs = (0..RUNS)
            .map(|run| TextRun {
                glyph_start: (run * GLYPHS_PER_RUN) as u32,
                glyph_count: GLYPHS_PER_RUN as u32,
            })
            .collect();
        let (cache_key, x, y) = CacheKey::new(
            cosmic_text::fontdb::ID::dummy(),
            0,
            16.0,
            (0.0, 0.0),
            cosmic_text::fontdb::Weight::NORMAL,
            CacheKeyFlags::empty(),
        );
        canvas.text_glyphs = vec![CanvasGlyph { cache_key, x, y }; RUNS * GLYPHS_PER_RUN];
        canvas.draw_records = (0..DRAWS)
            .map(|draw| glyph_draw((draw % RUNS) as u32))
            .collect();

        // Capacity arithmetic is intentionally empty so the benchmark isolates dependency
        // collection rather than font rasterization or glyph geometry.
        let mut font_system = FontSystem::new();
        let mut context = TextContext::new();
        let text = PreparedTextData::new(&[], &[], &mut font_system, &mut context);
        let mut cache = GlyphCapacityCache::default();
        cache.rebuild(&canvas, &text);

        let every = |len: usize, step: usize| {
            (0..len)
                .step_by(step)
                .map(|index| index..index + 1)
                .collect::<Vec<_>>()
        };
        let mut changes = SceneBufferChanges::default();
        match case {
            GlyphCapacityBenchmarkCase::Stable => {}
            GlyphCapacityBenchmarkCase::OneGlyph => changes
                .glyphs
                .push(RUNS * GLYPHS_PER_RUN / 2..RUNS * GLYPHS_PER_RUN / 2 + 1),
            GlyphCapacityBenchmarkCase::FragmentedGlyphs => {
                changes.glyphs = every(RUNS * GLYPHS_PER_RUN, 8)
            }
            GlyphCapacityBenchmarkCase::ChangedRuns => changes.text_runs = every(RUNS, 8),
            GlyphCapacityBenchmarkCase::ChangedDraws => changes.draws = every(DRAWS, 8),
            GlyphCapacityBenchmarkCase::Mixed => {
                changes.glyphs = every(RUNS * GLYPHS_PER_RUN, 16);
                changes.text_runs = every(RUNS, 16);
                changes.draws = every(DRAWS, 16);
            }
        }
        Self {
            cache,
            canvas,
            text,
            changes,
        }
    }

    pub fn update(&mut self) -> usize {
        self.cache
            .update_incremental(&self.canvas, &self.text, &self.changes);
        self.cache.total
    }
}

fn glyph_draw(run: u32) -> DrawRecord {
    DrawRecord {
        path_id: DrawRecord::NONE,
        glyph_run_id: run,
        sdf_offset: DrawRecord::NONE,
        sdf_len: 0,
        sdf_shadow_offset: DrawRecord::NONE,
        sdf_shadow_len: 0,
        brush_offset: DrawRecord::NONE,
        brush_len: 0,
        tag: DrawTag::Brush.into(),
        fill_rule: FillRuleWord::default(),
        pixel_bounds: PixelBounds {
            x0: 0,
            y0: 0,
            x1: 16,
            y1: 16,
        },
        local_pixel_bounds: PixelBounds {
            x0: 0,
            y0: 0,
            x1: 16,
            y1: 16,
        },
        solid_rect: 0,
        transform: GpuAffine::IDENTITY,
        inverse_transform: GpuAffine::IDENTITY,
    }
}
