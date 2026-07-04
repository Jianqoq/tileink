use rayon::prelude::*;

use crate::{
    shared::{
        bounds::Bounds,
        brush::decode_encoded_brush,
        draw_record::{DrawRecord, DrawTag},
        execution::LayerStackEntry,
        gpu_plan::TileDrawBins,
        gpu_sdf::{decode_sdf, decode_sdf_shadow},
        path::PathRecord,
        pixel::opacity_f32_to_u8,
        tile_ptcl::{
            TileColorPtcl, TileFillPtcl, TileGlyphPtcl, TilePtcl, TilePtclRange, TileSdfPtcl,
            TileSdfShadowPtcl,
        },
        tile_seg_range::TileSegmentRange,
    },
    text::PreparedTextData,
};

pub struct CoarseCpuPrepared<'a> {
    draw_records: &'a [DrawRecord],
    brush_blob: &'a [u32],
    sdf_blob: &'a [u32],
    sdf_shadow_blob: &'a [u32],
    draw_range: std::ops::Range<usize>,
    layer_stack_data: &'a [LayerStackEntry],
    tile_draw_bins: &'a TileDrawBins,
    // This range selects the batch's active fused layer stack snapshot from
    // the execution-plan arena. It is nesting state, not a screen-space bound.
    layer_stack_range: std::ops::Range<usize>,
    backdrop_records: &'a [PathRecord],
    backdrops: &'a [i32],
    tile_segment_ranges: &'a [TileSegmentRange],
    tile_ptcl_ranges: &'a mut Vec<TilePtclRange>,
    tile_ptcls: &'a mut Vec<TilePtcl>,
    tile_glyphs: &'a mut Vec<u32>,
    tiles_size: (u32, u32),
    text: Option<&'a PreparedTextData>,
}

impl<'a> CoarseCpuPrepared<'a> {
    /// Builds per-tile particle streams for one draw batch.
    ///
    /// Current limitation: fused clip / opacity / blend state is replayed only
    /// when a draw touches a tile. This is good enough for the current batch
    /// plumbing, but it is not the final semantic model. Each fused layer
    /// should eventually contribute its own coverage range instead of behaving
    /// like unbounded state attached to every touched draw tile.
    pub fn run(&mut self) {
        let tile_count = (self.tiles_size.0 * self.tiles_size.1) as usize;
        let layer_stack = &self.layer_stack_data[self.layer_stack_range.clone()];

        // Coarse output is ordered per tile, so the safe parallel boundary is
        // one worker-owned particle stream per tile. Each worker still scans
        // draws in document order, preserving rendering semantics without locks.
        let per_tile = (0..tile_count)
            .into_par_iter()
            .map(|tile_ix| {
                Self::build_tile_ptcls(
                    tile_ix,
                    self.tiles_size,
                    self.draw_range.clone(),
                    self.tile_draw_bins,
                    layer_stack,
                    self.draw_records,
                    self.brush_blob,
                    self.sdf_blob,
                    self.sdf_shadow_blob,
                    self.backdrop_records,
                    self.backdrops,
                    self.tile_segment_ranges,
                    self.text,
                )
            })
            .collect::<Vec<_>>();

        self.tile_ptcl_ranges.clear();
        self.tile_ptcl_ranges
            .resize(tile_count, TilePtclRange::default());
        self.tile_ptcls.clear();
        self.tile_glyphs.clear();

        for (tile_ix, output) in per_tile.into_iter().enumerate() {
            let start = self.tile_ptcls.len() as u32;
            if !output.ptcls.is_empty() {
                // Glyph runs can span many tiles; store only this tile's glyph ids
                // so fine does not rescan the whole run for every covered pixel.
                let glyph_start = self.tile_glyphs.len() as u32;
                self.tile_glyphs.extend(output.glyphs);
                self.tile_ptcls
                    .extend(output.ptcls.into_iter().map(|mut ptcl| {
                        if let TilePtcl::Glyph(glyph) = &mut ptcl {
                            glyph.glyph_range = glyph_start + glyph.glyph_range.start
                                ..glyph_start + glyph.glyph_range.end;
                        }
                        ptcl
                    }));
                self.tile_ptcls.push(TilePtcl::End);
            }
            let end = self.tile_ptcls.len() as u32;
            self.tile_ptcl_ranges[tile_ix] = TilePtclRange { start, end };
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn build_tile_ptcls(
        tile_ix: usize,
        tiles_size: (u32, u32),
        draw_range: std::ops::Range<usize>,
        tile_draw_bins: &TileDrawBins,
        layer_stack: &[LayerStackEntry],
        draw_records: &[DrawRecord],
        brush_blob: &[u32],
        sdf_blob: &[u32],
        sdf_shadow_blob: &[u32],
        backdrop_records: &[PathRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
        text: Option<&PreparedTextData>,
    ) -> TileCoarseOutput {
        let tile_x = tile_ix as u32 % tiles_size.0;
        let tile_y = tile_ix as u32 / tiles_size.0;
        let mut output = TileCoarseOutput::default();
        let mut emitted_wrappers = Vec::new();
        let tile_draw_range = tile_draw_bins.records[tile_ix];
        let draw_start = tile_draw_range.start as usize;
        let draw_end = tile_draw_range.end as usize;

        for &draw_ix in &tile_draw_bins.draw_indices[draw_start..draw_end] {
            let draw_ix = draw_ix as usize;
            if draw_ix < draw_range.start || draw_ix >= draw_range.end {
                continue;
            }
            let Some(coverage) = Self::draw_tile_coverage(
                draw_ix,
                tile_x,
                tile_y,
                draw_records,
                sdf_blob,
                sdf_shadow_blob,
                backdrop_records,
                backdrops,
                tile_segment_ranges,
                tiles_size,
                text,
            ) else {
                continue;
            };
            let draw = coverage.draw();
            let Some(brush) = decode_encoded_brush(brush_blob, draw.brush_offset, draw.brush_len)
            else {
                continue;
            };

            if !Self::ensure_batch_wrappers(
                &mut output.ptcls,
                &mut emitted_wrappers,
                tile_x,
                tile_y,
                layer_stack,
                draw_records,
                brush_blob,
                sdf_blob,
                sdf_shadow_blob,
                backdrop_records,
                backdrops,
                tile_segment_ranges,
                tiles_size,
            ) {
                continue;
            }

            match draw.tag() {
                DrawTag::Brush => match coverage {
                    DrawTileCoverage::Path {
                        draw,
                        backdrop,
                        segment_range,
                    } => {
                        if let Some(color) = brush.solid_color()
                            && draw.solid_rect()
                            && segment_range.start == segment_range.end
                        {
                            output.ptcls.push(TilePtcl::Color(TileColorPtcl {
                                color: crate::shared::pixel::premul_f32_to_u32(
                                    color.premultiply().components,
                                ),
                            }));
                            continue;
                        }
                        output.ptcls.push(TilePtcl::Fill(TileFillPtcl {
                            backdrop,
                            fill_rule: draw.fill_rule(),
                            segment_range,
                            brush: brush.clone(),
                        }));
                    }
                    DrawTileCoverage::Sdf { sdf, .. } => {
                        if let Some(color) = brush.solid_color()
                            && sdf.tile_is_solid(tile_bounds(tile_x, tile_y))
                        {
                            output.ptcls.push(TilePtcl::Color(TileColorPtcl {
                                color: crate::shared::pixel::premul_f32_to_u32(
                                    color.premultiply().components,
                                ),
                            }));
                            continue;
                        }
                        output.ptcls.push(TilePtcl::Sdf(TileSdfPtcl {
                            sdf,
                            brush: brush.clone(),
                        }));
                    }
                    DrawTileCoverage::SdfShadow { sdf_shadow, .. } => {
                        output.ptcls.push(TilePtcl::SdfShadow(TileSdfShadowPtcl {
                            sdf_shadow,
                            brush: brush.clone(),
                        }));
                    }
                    DrawTileCoverage::Glyph { glyphs, .. } => {
                        let start = output.glyphs.len() as u32;
                        output.glyphs.extend(glyphs);
                        let end = output.glyphs.len() as u32;
                        output.ptcls.push(TilePtcl::Glyph(TileGlyphPtcl {
                            glyph_range: start..end,
                            brush: brush.clone(),
                        }));
                    }
                },
                DrawTag::PathGlyph => {
                    if let DrawTileCoverage::Path {
                        draw,
                        backdrop,
                        segment_range,
                    } = coverage
                    {
                        output.ptcls.push(TilePtcl::PathGlyph(TileFillPtcl {
                            backdrop,
                            fill_rule: draw.fill_rule(),
                            segment_range,
                            brush: brush.clone(),
                        }));
                    }
                }
                DrawTag::Clip => {
                    if let DrawTileCoverage::Path {
                        draw,
                        backdrop,
                        segment_range,
                    } = coverage
                    {
                        output.ptcls.push(TilePtcl::BeginClip(TileFillPtcl {
                            backdrop,
                            fill_rule: draw.fill_rule(),
                            segment_range,
                            brush: brush.clone(),
                        }));
                    }
                }
                DrawTag::Isolate | DrawTag::Opacity | DrawTag::Blend => {}
            }
        }

        for wrapper in emitted_wrappers.iter().rev() {
            output.ptcls.push(match wrapper {
                LayerStackEntry::Clip { .. } => TilePtcl::EndClip,
                LayerStackEntry::Opacity { .. } => TilePtcl::EndOpacity,
                LayerStackEntry::Blend { .. } => TilePtcl::EndBlend,
            });
        }
        output
    }

    /// Emits the once-per-tile wrapper particles for the current batch.
    ///
    /// This is called lazily: the wrappers are only inserted when the batch
    /// first contributes something to a tile. If the tile is untouched by the
    /// batch, no wrapper particles are emitted for that tile.
    ///
    /// Current behavior:
    /// - replays the active fused layer stack in user nesting order;
    /// - emits a `Begin*` particle only when that layer's own draw covers the
    ///   tile.
    ///
    /// The matching `End*` particles are appended later when the tile particle
    /// list is finalized.
    #[allow(clippy::too_many_arguments)]
    fn ensure_batch_wrappers(
        ptcls: &mut Vec<TilePtcl>,
        emitted_wrappers: &mut Vec<LayerStackEntry>,
        tile_x: u32,
        tile_y: u32,
        layer_stack: &[LayerStackEntry],
        draw_records: &[DrawRecord],
        brush_blob: &[u32],
        sdf_blob: &[u32],
        sdf_shadow_blob: &[u32],
        backdrop_records: &[PathRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
        tiles_size: (u32, u32),
    ) -> bool {
        if !ptcls.is_empty() {
            return true;
        }

        let mut pending = Vec::with_capacity(layer_stack.len());
        for entry in layer_stack {
            let draw_ix = match *entry {
                LayerStackEntry::Clip { draw }
                | LayerStackEntry::Opacity { draw, .. }
                | LayerStackEntry::Blend { draw, .. } => draw,
            };
            let Some(coverage) = Self::layer_tile_coverage(
                draw_ix as usize,
                tile_x,
                tile_y,
                draw_records,
                sdf_blob,
                sdf_shadow_blob,
                backdrop_records,
                backdrops,
                tile_segment_ranges,
                tiles_size,
            ) else {
                return false;
            };

            let ptcl = match (*entry, coverage) {
                (
                    LayerStackEntry::Clip { .. },
                    LayerTileCoverage::Path {
                        draw,
                        backdrop,
                        segment_range,
                    },
                )
                | (
                    LayerStackEntry::Opacity { .. } | LayerStackEntry::Blend { .. },
                    LayerTileCoverage::Path {
                        draw,
                        backdrop,
                        segment_range,
                    },
                ) => {
                    let Some(brush) =
                        decode_encoded_brush(brush_blob, draw.brush_offset, draw.brush_len)
                    else {
                        return false;
                    };
                    LayerCoveragePtcl::Path(TileFillPtcl {
                        backdrop,
                        fill_rule: draw.fill_rule(),
                        segment_range,
                        brush: brush.clone(),
                    })
                }
                (LayerStackEntry::Clip { .. }, LayerTileCoverage::Sdf { draw, sdf }) => {
                    let Some(brush) =
                        decode_encoded_brush(brush_blob, draw.brush_offset, draw.brush_len)
                    else {
                        return false;
                    };
                    LayerCoveragePtcl::Sdf(TileSdfPtcl {
                        sdf,
                        brush: brush.clone(),
                    })
                }
                (
                    LayerStackEntry::Opacity { .. } | LayerStackEntry::Blend { .. },
                    LayerTileCoverage::Sdf { .. },
                ) => return false,
            };
            pending.push((*entry, ptcl));
        }

        for (entry, ptcl) in pending {
            ptcls.push(match (entry, ptcl) {
                (LayerStackEntry::Clip { .. }, LayerCoveragePtcl::Path(fill)) => {
                    TilePtcl::BeginClip(fill)
                }
                (LayerStackEntry::Clip { .. }, LayerCoveragePtcl::Sdf(sdf)) => {
                    TilePtcl::BeginSdfClip(sdf)
                }
                (LayerStackEntry::Opacity { opacity, .. }, LayerCoveragePtcl::Path(fill)) => {
                    TilePtcl::BeginOpacity {
                        opacity: opacity_f32_to_u8(opacity),
                        fill,
                    }
                }
                (LayerStackEntry::Blend { mode, .. }, LayerCoveragePtcl::Path(fill)) => {
                    TilePtcl::BeginBlend { mode, fill }
                }
                (
                    LayerStackEntry::Opacity { .. } | LayerStackEntry::Blend { .. },
                    LayerCoveragePtcl::Sdf(_),
                ) => unreachable!("opacity and blend layer masks are path-backed"),
            });
            emitted_wrappers.push(entry);
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn layer_tile_coverage<'b>(
        draw_ix: usize,
        tile_x: u32,
        tile_y: u32,
        draw_records: &'b [DrawRecord],
        sdf_blob: &[u32],
        _sdf_shadow_blob: &[u32],
        backdrop_records: &[PathRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
        tiles_size: (u32, u32),
    ) -> Option<LayerTileCoverage<'b>> {
        let draw = &draw_records[draw_ix];
        if let Some(sdf) = decode_sdf(sdf_blob, draw.sdf_offset, draw.sdf_len) {
            let bbox = draw.tile_bbox(tiles_size.0, tiles_size.1);
            return (tile_x >= bbox.x0
                && tile_x < bbox.x1
                && tile_y >= bbox.y0
                && tile_y < bbox.y1)
                .then_some(LayerTileCoverage::Sdf { draw, sdf });
        }
        if draw.sdf_shadow_range().is_some() {
            return None;
        }

        let path_id = draw.path_id()?;
        let bbox = draw.tile_bbox(tiles_size.0, tiles_size.1);
        if tile_x < bbox.x0 || tile_x >= bbox.x1 || tile_y < bbox.y0 || tile_y >= bbox.y1 {
            return None;
        }
        let backdrop_record = &backdrop_records[path_id as usize];
        if tile_x < backdrop_record.tile_x0
            || tile_x >= backdrop_record.tile_x1
            || tile_y < backdrop_record.tile_y0
            || tile_y >= backdrop_record.tile_y1
        {
            return None;
        }
        let stride = backdrop_record.tile_x1 - backdrop_record.tile_x0;
        if stride == 0 {
            return None;
        }
        let local_x = tile_x - backdrop_record.tile_x0;
        let local_y = tile_y - backdrop_record.tile_y0;
        let local_ix = (local_y * stride + local_x) as usize;
        let backdrop_ix = backdrop_record.data_offset as usize + local_ix;
        let segment_range = tile_segment_ranges[backdrop_ix];
        let backdrop = backdrops[backdrop_ix];
        if segment_range.start == segment_range.end && backdrop == 0 {
            return None;
        }
        Some(LayerTileCoverage::Path {
            draw,
            backdrop,
            segment_range: segment_range.start..segment_range.end,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tile_coverage<'b>(
        draw_ix: usize,
        tile_x: u32,
        tile_y: u32,
        draw_records: &'b [DrawRecord],
        sdf_blob: &[u32],
        sdf_shadow_blob: &[u32],
        backdrop_records: &[PathRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
        tiles_size: (u32, u32),
        text: Option<&PreparedTextData>,
    ) -> Option<DrawTileCoverage<'b>> {
        let draw = &draw_records[draw_ix];
        if let Some(sdf) = decode_sdf(sdf_blob, draw.sdf_offset, draw.sdf_len) {
            let bbox = draw.tile_bbox(tiles_size.0, tiles_size.1);
            if tile_x >= bbox.x0 && tile_x < bbox.x1 && tile_y >= bbox.y0 && tile_y < bbox.y1 {
                return Some(DrawTileCoverage::Sdf { draw, sdf });
            }
            return None;
        }
        if let Some(sdf_shadow) =
            decode_sdf_shadow(sdf_shadow_blob, draw.sdf_shadow_offset, draw.sdf_shadow_len)
        {
            let bbox = draw.tile_bbox(tiles_size.0, tiles_size.1);
            if tile_x >= bbox.x0 && tile_x < bbox.x1 && tile_y >= bbox.y0 && tile_y < bbox.y1 {
                return Some(DrawTileCoverage::SdfShadow { draw, sdf_shadow });
            }
            return None;
        }

        if let Some(glyph_run_id) = draw.glyph_run_id() {
            let bbox = draw.tile_bbox(tiles_size.0, tiles_size.1);
            if tile_x >= bbox.x0 && tile_x < bbox.x1 && tile_y >= bbox.y0 && tile_y < bbox.y1 {
                let text = text?;
                let glyphs = glyphs_for_tile(text, glyph_run_id, tile_x, tile_y);
                if !glyphs.is_empty() {
                    return Some(DrawTileCoverage::Glyph { draw, glyphs });
                }
            }
            return None;
        }

        let LayerTileCoverage::Path {
            draw,
            backdrop,
            segment_range,
        } = Self::layer_tile_coverage(
            draw_ix,
            tile_x,
            tile_y,
            draw_records,
            sdf_blob,
            sdf_shadow_blob,
            backdrop_records,
            backdrops,
            tile_segment_ranges,
            tiles_size,
        )?
        else {
            return None;
        };
        Some(DrawTileCoverage::Path {
            draw,
            backdrop,
            segment_range,
        })
    }
}

#[derive(Default)]
struct TileCoarseOutput {
    ptcls: Vec<TilePtcl>,
    glyphs: Vec<u32>,
}

enum LayerCoveragePtcl {
    Path(TileFillPtcl),
    Sdf(TileSdfPtcl),
}

enum LayerTileCoverage<'a> {
    Path {
        draw: &'a DrawRecord,
        backdrop: i32,
        segment_range: std::ops::Range<u32>,
    },
    Sdf {
        draw: &'a DrawRecord,
        sdf: crate::shared::sdf::Sdf,
    },
}

enum DrawTileCoverage<'a> {
    Path {
        draw: &'a DrawRecord,
        backdrop: i32,
        segment_range: std::ops::Range<u32>,
    },
    Sdf {
        draw: &'a DrawRecord,
        sdf: crate::shared::sdf::Sdf,
    },
    SdfShadow {
        draw: &'a DrawRecord,
        sdf_shadow: crate::shared::sdf::SdfShadow,
    },
    Glyph {
        draw: &'a DrawRecord,
        glyphs: Vec<u32>,
    },
}

impl<'a> DrawTileCoverage<'a> {
    fn draw(&self) -> &'a DrawRecord {
        match self {
            Self::Path { draw, .. }
            | Self::Sdf { draw, .. }
            | Self::SdfShadow { draw, .. }
            | Self::Glyph { draw, .. } => draw,
        }
    }
}

fn tile_bounds(tile_x: u32, tile_y: u32) -> Bounds {
    let x0 = (tile_x * crate::TILE_SIZE) as i32;
    let y0 = (tile_y * crate::TILE_SIZE) as i32;
    Bounds::new(
        x0,
        y0,
        x0 + crate::TILE_SIZE as i32,
        y0 + crate::TILE_SIZE as i32,
    )
}

fn glyphs_for_tile(
    text: &PreparedTextData,
    glyph_run_id: u32,
    tile_x: u32,
    tile_y: u32,
) -> Vec<u32> {
    let tile_bounds = tile_bounds(tile_x, tile_y);
    text.run_glyph_indices(glyph_run_id)
        .filter(|&glyph_id| {
            text.glyph_bounds(glyph_id)
                .is_some_and(|glyph_bounds| bounds_intersect(glyph_bounds, tile_bounds))
        })
        .collect()
}

fn bounds_intersect(a: Bounds, b: Bounds) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

pub struct CoarseCpuPipeline;

impl CoarseCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    /// Prepares the CPU coarse stage for one draw batch and the batch's fused
    /// layer stack snapshot.
    ///
    /// `layer_stack_range` selects a batch-local ordered stack slice from the plan.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare<'a>(
        &self,
        draw_records: &'a [DrawRecord],
        brush_blob: &'a [u32],
        sdf_blob: &'a [u32],
        sdf_shadow_blob: &'a [u32],
        draw_range: std::ops::Range<usize>,
        layer_stack_data: &'a [LayerStackEntry],
        layer_stack_range: std::ops::Range<usize>,
        tile_draw_bins: &'a TileDrawBins,
        backdrop_records: &'a [PathRecord],
        backdrops: &'a [i32],
        tile_segment_ranges: &'a [TileSegmentRange],
        tile_ptcl_ranges: &'a mut Vec<TilePtclRange>,
        tile_ptcls: &'a mut Vec<TilePtcl>,
        tile_glyphs: &'a mut Vec<u32>,
        tiles_size: (u32, u32),
        text: Option<&'a PreparedTextData>,
    ) -> CoarseCpuPrepared<'a> {
        CoarseCpuPrepared {
            draw_records,
            brush_blob,
            sdf_blob,
            sdf_shadow_blob,
            draw_range,
            layer_stack_data,
            layer_stack_range,
            tile_draw_bins,
            backdrop_records,
            backdrops,
            tile_segment_ranges,
            tile_ptcl_ranges,
            tile_ptcls,
            tile_glyphs,
            tiles_size,
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use peniko::Color;
    use peniko::kurbo::Point;

    use super::CoarseCpuPipeline;
    use crate::{
        Canvas, TextFontSystem,
        shared::{
            bounds::{Bounds, PixelBounds},
            brush::{Brush, push_encoded_brush},
            draw_record::{DrawRecord, DrawTag},
            execution::LayerStackEntry,
            fill::FillRule,
            gpu_plan::{TileDrawBins, build_tile_draw_bins_for_draws_into},
            path::PathRecord,
            tile_ptcl::TilePtcl,
            tile_seg_range::TileSegmentRange,
        },
        text::{PreparedTextData, TextContext, TextLayoutOptions},
    };

    fn solid_color_u32(color: Color) -> u32 {
        crate::shared::pixel::premul_f32_to_u32(color.premultiply().components)
    }

    fn tile_draw_bins(draw_records: &[DrawRecord], tiles_size: (u32, u32)) -> TileDrawBins {
        let mut bins = TileDrawBins::default();
        let mut cursors = Vec::new();
        build_tile_draw_bins_for_draws_into(draw_records, tiles_size, &mut bins, &mut cursors);
        bins
    }

    fn assign_brushes(draw_records: &mut [DrawRecord], brushes: &[Brush]) -> Vec<u32> {
        let mut blob = Vec::new();
        for (draw, brush) in draw_records.iter_mut().zip(brushes) {
            (draw.brush_offset, draw.brush_len) = push_encoded_brush(&mut blob, brush);
        }
        blob
    }

    #[test]
    fn run_replays_clip_layers_in_user_nesting_order() {
        let mut draw_records = [
            DrawRecord {
                path_id: 0,
                glyph_run_id: DrawRecord::NONE,
                sdf_offset: DrawRecord::NONE,
                sdf_len: 0,
                sdf_shadow_offset: DrawRecord::NONE,
                sdf_shadow_len: 0,
                brush_offset: DrawRecord::NONE,
                brush_len: 0,
                tag: DrawTag::Clip.into(),
                fill_rule: FillRule::NonZero.into(),
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 16,
                    y1: 16,
                },
                solid_rect: 0,
            },
            DrawRecord {
                path_id: 1,
                glyph_run_id: DrawRecord::NONE,
                sdf_offset: DrawRecord::NONE,
                sdf_len: 0,
                sdf_shadow_offset: DrawRecord::NONE,
                sdf_shadow_len: 0,
                brush_offset: DrawRecord::NONE,
                brush_len: 0,
                tag: DrawTag::Clip.into(),
                fill_rule: FillRule::NonZero.into(),
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 16,
                    y1: 16,
                },
                solid_rect: 0,
            },
            DrawRecord {
                path_id: 2,
                glyph_run_id: DrawRecord::NONE,
                sdf_offset: DrawRecord::NONE,
                sdf_len: 0,
                sdf_shadow_offset: DrawRecord::NONE,
                sdf_shadow_len: 0,
                brush_offset: DrawRecord::NONE,
                brush_len: 0,
                tag: DrawTag::Brush.into(),
                fill_rule: FillRule::NonZero.into(),
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 16,
                    y1: 16,
                },
                solid_rect: 0,
            },
        ];
        let brushes = [
            Brush::Solid(Color::TRANSPARENT),
            Brush::Solid(Color::TRANSPARENT),
            Brush::Solid(Color::BLACK),
        ];
        let brush_blob = assign_brushes(&mut draw_records, &brushes);
        let backdrop_records = [
            PathRecord {
                path_id: 0,
                data_offset: 0,
                data_len: 1,
                tile_x0: 0,
                tile_y0: 0,
                tile_x1: 1,
                tile_y1: 1,
                segment_start: 0,
                segment_capacity: 1,
                segment_count: 0,
                ..PathRecord::default()
            },
            PathRecord {
                path_id: 1,
                data_offset: 1,
                data_len: 1,
                tile_x0: 0,
                tile_y0: 0,
                tile_x1: 1,
                tile_y1: 1,
                segment_start: 1,
                segment_capacity: 1,
                segment_count: 0,
                ..PathRecord::default()
            },
            PathRecord {
                path_id: 2,
                data_offset: 2,
                data_len: 1,
                tile_x0: 0,
                tile_y0: 0,
                tile_x1: 1,
                tile_y1: 1,
                segment_start: 2,
                segment_capacity: 1,
                segment_count: 0,
                ..PathRecord::default()
            },
        ];
        let backdrops = [0, 0, 0];
        let tile_segment_ranges = [
            TileSegmentRange { start: 0, end: 1 },
            TileSegmentRange { start: 1, end: 2 },
            TileSegmentRange { start: 2, end: 3 },
        ];
        let mut tile_ptcl_ranges = Vec::new();
        let mut tile_ptcls = Vec::new();
        let mut tile_glyphs = Vec::new();
        let layer_stack_data = [
            LayerStackEntry::Clip { draw: 0 },
            LayerStackEntry::Clip { draw: 1 },
        ];
        let bins = tile_draw_bins(&draw_records, (1, 1));

        CoarseCpuPipeline::new()
            .prepare(
                &draw_records,
                &brush_blob,
                &[],
                &[],
                2..3,
                &layer_stack_data,
                0..layer_stack_data.len(),
                &bins,
                &backdrop_records,
                &backdrops,
                &tile_segment_ranges,
                &mut tile_ptcl_ranges,
                &mut tile_ptcls,
                &mut tile_glyphs,
                (1, 1),
                None,
            )
            .run();

        assert_eq!(tile_ptcl_ranges.len(), 1);
        assert_eq!(tile_ptcl_ranges[0].start, 0);
        assert_eq!(tile_ptcl_ranges[0].end, 6);
        assert!(matches!(tile_ptcls[0], TilePtcl::BeginClip(_)));
        assert!(matches!(tile_ptcls[1], TilePtcl::BeginClip(_)));
        assert!(matches!(tile_ptcls[2], TilePtcl::Fill(_)));
        assert!(matches!(tile_ptcls[3], TilePtcl::EndClip));
        assert!(matches!(tile_ptcls[4], TilePtcl::EndClip));
        assert!(matches!(tile_ptcls[5], TilePtcl::End));
    }

    #[test]
    fn run_builds_per_tile_glyph_lists() {
        let mut font_system = TextFontSystem::new();
        let mut context = TextContext::new();
        let layout = context.layout(&mut font_system, TextLayoutOptions::new("MMMMMMMM", 24.0));
        if layout.is_empty() {
            return;
        }

        let mut canvas = Canvas::new(160, 64);
        canvas.push_text_layout(&layout, Point::new(2.0, 32.0), Color::WHITE);
        let text = PreparedTextData::new(
            &canvas.text_glyphs,
            &canvas.text_runs,
            &mut font_system,
            &mut context,
        );
        if canvas.draw_records.is_empty() {
            return;
        }

        let mut tile_ptcl_ranges = Vec::new();
        let mut tile_ptcls = Vec::new();
        let mut tile_glyphs = Vec::new();
        let tiles_size = (canvas.width_in_tiles(), canvas.height_in_tiles());
        let bins = tile_draw_bins(&canvas.draw_records, tiles_size);

        CoarseCpuPipeline::new()
            .prepare(
                &canvas.draw_records,
                &canvas.brush_blob,
                &canvas.sdf_blob,
                &canvas.sdf_shadow_blob,
                0..canvas.draw_records.len(),
                &[],
                0..0,
                &bins,
                &[],
                &[],
                &[],
                &mut tile_ptcl_ranges,
                &mut tile_ptcls,
                &mut tile_glyphs,
                tiles_size,
                Some(&text),
            )
            .run();

        for tile_y in 0..tiles_size.1 {
            for tile_x in 0..tiles_size.0 {
                let tile_ix = (tile_y * tiles_size.0 + tile_x) as usize;
                let tile_bounds = Bounds::new(
                    (tile_x * crate::TILE_SIZE) as i32,
                    (tile_y * crate::TILE_SIZE) as i32,
                    ((tile_x + 1) * crate::TILE_SIZE) as i32,
                    ((tile_y + 1) * crate::TILE_SIZE) as i32,
                );
                let expected = text
                    .run_glyph_indices(0)
                    .filter(|&glyph_id| {
                        text.glyph_bounds(glyph_id).is_some_and(|glyph_bounds| {
                            glyph_bounds.x0 < tile_bounds.x1
                                && glyph_bounds.x1 > tile_bounds.x0
                                && glyph_bounds.y0 < tile_bounds.y1
                                && glyph_bounds.y1 > tile_bounds.y0
                        })
                    })
                    .collect::<Vec<_>>();

                let range = tile_ptcl_ranges[tile_ix];
                let actual = tile_ptcls[range.start as usize..range.end as usize]
                    .iter()
                    .find_map(|ptcl| match ptcl {
                        TilePtcl::Glyph(glyph) => Some(
                            tile_glyphs
                                [glyph.glyph_range.start as usize..glyph.glyph_range.end as usize]
                                .to_vec(),
                        ),
                        _ => None,
                    })
                    .unwrap_or_default();

                assert_eq!(actual, expected, "tile {tile_x},{tile_y}");
            }
        }
    }

    #[test]
    fn run_builds_each_tile_stream_independently_in_draw_order() {
        let mut draw_records = [
            DrawRecord {
                path_id: 0,
                glyph_run_id: DrawRecord::NONE,
                sdf_offset: DrawRecord::NONE,
                sdf_len: 0,
                sdf_shadow_offset: DrawRecord::NONE,
                sdf_shadow_len: 0,
                brush_offset: DrawRecord::NONE,
                brush_len: 0,
                tag: DrawTag::Brush.into(),
                fill_rule: FillRule::NonZero.into(),
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 32,
                    y1: 16,
                },
                solid_rect: 1,
            },
            DrawRecord {
                path_id: 1,
                glyph_run_id: DrawRecord::NONE,
                sdf_offset: DrawRecord::NONE,
                sdf_len: 0,
                sdf_shadow_offset: DrawRecord::NONE,
                sdf_shadow_len: 0,
                brush_offset: DrawRecord::NONE,
                brush_len: 0,
                tag: DrawTag::Brush.into(),
                fill_rule: FillRule::NonZero.into(),
                pixel_bounds: PixelBounds {
                    x0: 16,
                    y0: 0,
                    x1: 32,
                    y1: 16,
                },
                solid_rect: 1,
            },
        ];
        let brushes = [
            Brush::Solid(Color::from_rgb8(255, 0, 0)),
            Brush::Solid(Color::from_rgb8(0, 0, 255)),
        ];
        let brush_blob = assign_brushes(&mut draw_records, &brushes);
        let backdrop_records = [
            PathRecord {
                path_id: 0,
                data_offset: 0,
                data_len: 2,
                tile_x0: 0,
                tile_y0: 0,
                tile_x1: 2,
                tile_y1: 1,
                segment_start: 0,
                segment_capacity: 0,
                segment_count: 0,
                ..PathRecord::default()
            },
            PathRecord {
                path_id: 1,
                data_offset: 2,
                data_len: 1,
                tile_x0: 1,
                tile_y0: 0,
                tile_x1: 2,
                tile_y1: 1,
                segment_start: 0,
                segment_capacity: 0,
                segment_count: 0,
                ..PathRecord::default()
            },
        ];
        let backdrops = [1, 1, 1];
        let tile_segment_ranges = [TileSegmentRange::default(); 3];
        let mut tile_ptcl_ranges = Vec::new();
        let mut tile_ptcls = Vec::new();
        let mut tile_glyphs = Vec::new();
        let bins = tile_draw_bins(&draw_records, (2, 1));

        CoarseCpuPipeline::new()
            .prepare(
                &draw_records,
                &brush_blob,
                &[],
                &[],
                0..draw_records.len(),
                &[],
                0..0,
                &bins,
                &backdrop_records,
                &backdrops,
                &tile_segment_ranges,
                &mut tile_ptcl_ranges,
                &mut tile_ptcls,
                &mut tile_glyphs,
                (2, 1),
                None,
            )
            .run();

        assert_eq!(tile_ptcl_ranges.len(), 2);
        assert_eq!(tile_ptcl_ranges[0].start, 0);
        assert_eq!(tile_ptcl_ranges[0].end, 2);
        assert_eq!(tile_ptcl_ranges[1].start, 2);
        assert_eq!(tile_ptcl_ranges[1].end, 5);
        assert!(matches!(
            &tile_ptcls[0],
            TilePtcl::Color(color) if color.color == solid_color_u32(Color::from_rgb8(255, 0, 0))
        ));
        assert!(matches!(tile_ptcls[1], TilePtcl::End));
        assert!(matches!(
            &tile_ptcls[2],
            TilePtcl::Color(color) if color.color == solid_color_u32(Color::from_rgb8(255, 0, 0))
        ));
        assert!(matches!(
            &tile_ptcls[3],
            TilePtcl::Color(color) if color.color == solid_color_u32(Color::from_rgb8(0, 0, 255))
        ));
        assert!(matches!(tile_ptcls[4], TilePtcl::End));
    }
}
