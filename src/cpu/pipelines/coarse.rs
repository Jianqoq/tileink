use rayon::prelude::*;

use crate::shared::{
    bd_record::BackdropRecord,
    bounds::Bounds,
    draw_record::{DrawRecord, DrawTag},
    execution::LayerStackEntry,
    pixel::opacity_f32_to_u8,
    tile_ptcl::{TileColorPtcl, TileFillPtcl, TileGlyphPtcl, TilePtcl, TilePtclRange, TileSdfPtcl},
    tile_seg_range::TileSegmentRange,
};

pub struct CoarseCpuPrepared<'a> {
    draw_records: &'a [DrawRecord],
    draw_range: std::ops::Range<usize>,
    layer_stack_data: &'a [LayerStackEntry],
    // This range selects the batch's active fused layer stack snapshot from
    // the execution-plan arena. It is nesting state, not a screen-space bound.
    layer_stack_range: std::ops::Range<usize>,
    backdrop_records: &'a [BackdropRecord],
    backdrops: &'a [i32],
    tile_segment_ranges: &'a [TileSegmentRange],
    tile_ptcl_ranges: &'a mut Vec<TilePtclRange>,
    tile_ptcls: &'a mut Vec<TilePtcl>,
    tiles_size: (u32, u32),
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
                    layer_stack,
                    self.draw_records,
                    self.backdrop_records,
                    self.backdrops,
                    self.tile_segment_ranges,
                )
            })
            .collect::<Vec<_>>();

        self.tile_ptcl_ranges.clear();
        self.tile_ptcl_ranges
            .resize(tile_count, TilePtclRange::default());
        self.tile_ptcls.clear();

        for (tile_ix, ptcls) in per_tile.into_iter().enumerate() {
            let start = self.tile_ptcls.len() as u32;
            if !ptcls.is_empty() {
                self.tile_ptcls.extend(ptcls);
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
        layer_stack: &[LayerStackEntry],
        draw_records: &[DrawRecord],
        backdrop_records: &[BackdropRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
    ) -> Vec<TilePtcl> {
        let tile_x = tile_ix as u32 % tiles_size.0;
        let tile_y = tile_ix as u32 / tiles_size.0;
        let mut ptcls = Vec::new();
        let mut emitted_wrappers = Vec::new();

        for draw_ix in draw_range {
            let Some(coverage) = Self::draw_tile_coverage(
                draw_ix,
                tile_x,
                tile_y,
                draw_records,
                backdrop_records,
                backdrops,
                tile_segment_ranges,
                tiles_size,
            ) else {
                continue;
            };
            let draw = coverage.draw();

            if !Self::ensure_batch_wrappers(
                &mut ptcls,
                &mut emitted_wrappers,
                tile_x,
                tile_y,
                layer_stack,
                draw_records,
                backdrop_records,
                backdrops,
                tile_segment_ranges,
                tiles_size,
            ) {
                continue;
            }

            match draw.tag {
                DrawTag::Brush => match coverage {
                    DrawTileCoverage::Path {
                        draw,
                        backdrop,
                        segment_range,
                    } => {
                        if let Some(color) = draw.brush.solid_color()
                            && draw.solid_rect
                            && segment_range.start == segment_range.end
                        {
                            ptcls.push(TilePtcl::Color(TileColorPtcl {
                                color: crate::shared::pixel::premul_f32_to_u32(
                                    color.premultiply().components,
                                ),
                            }));
                            continue;
                        }
                        ptcls.push(TilePtcl::Fill(TileFillPtcl {
                            backdrop,
                            fill_rule: draw.fill_rule,
                            segment_range,
                            brush: draw.brush.clone(),
                        }));
                    }
                    DrawTileCoverage::Sdf { draw, sdf } => {
                        if let Some(color) = draw.brush.solid_color()
                            && sdf.tile_is_solid(tile_bounds(tile_x, tile_y))
                        {
                            ptcls.push(TilePtcl::Color(TileColorPtcl {
                                color: crate::shared::pixel::premul_f32_to_u32(
                                    color.premultiply().components,
                                ),
                            }));
                            continue;
                        }
                        ptcls.push(TilePtcl::Sdf(TileSdfPtcl {
                            sdf: *sdf,
                            brush: draw.brush.clone(),
                        }));
                    }
                    DrawTileCoverage::Glyph { draw, glyph_run_id } => {
                        ptcls.push(TilePtcl::Glyph(TileGlyphPtcl {
                            glyph_run_id,
                            brush: draw.brush.clone(),
                        }));
                    }
                },
                DrawTag::Clip => {
                    if let DrawTileCoverage::Path {
                        draw,
                        backdrop,
                        segment_range,
                    } = coverage
                    {
                        ptcls.push(TilePtcl::BeginClip(TileFillPtcl {
                            backdrop,
                            fill_rule: draw.fill_rule,
                            segment_range,
                            brush: draw.brush.clone(),
                        }));
                    }
                }
                DrawTag::Isolate | DrawTag::Opacity | DrawTag::Blend => {}
            }
        }

        for wrapper in emitted_wrappers.iter().rev() {
            ptcls.push(match wrapper {
                LayerStackEntry::Clip { .. } => TilePtcl::EndClip,
                LayerStackEntry::Opacity { .. } => TilePtcl::EndOpacity,
                LayerStackEntry::Blend { .. } => TilePtcl::EndBlend,
            });
        }
        ptcls
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
        backdrop_records: &[BackdropRecord],
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
            let Some((draw, backdrop, segment_range)) = Self::layer_tile_coverage(
                draw_ix as usize,
                tile_x,
                tile_y,
                draw_records,
                backdrop_records,
                backdrops,
                tile_segment_ranges,
                tiles_size,
            ) else {
                return false;
            };

            pending.push((
                *entry,
                TileFillPtcl {
                    backdrop,
                    fill_rule: draw.fill_rule,
                    segment_range,
                    brush: draw.brush.clone(),
                },
            ));
        }

        for (entry, fill) in pending {
            ptcls.push(match entry {
                LayerStackEntry::Clip { .. } => TilePtcl::BeginClip(fill),
                LayerStackEntry::Opacity { opacity, .. } => TilePtcl::BeginOpacity {
                    opacity: opacity_f32_to_u8(opacity),
                    fill,
                },
                LayerStackEntry::Blend { mode, .. } => TilePtcl::BeginBlend { mode, fill },
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
        backdrop_records: &[BackdropRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
        tiles_size: (u32, u32),
    ) -> Option<(&'b DrawRecord, i32, std::ops::Range<u32>)> {
        let draw = &draw_records[draw_ix];
        let path_id = draw.path_id?;
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
        Some((draw, backdrop, segment_range.start..segment_range.end))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tile_coverage<'b>(
        draw_ix: usize,
        tile_x: u32,
        tile_y: u32,
        draw_records: &'b [DrawRecord],
        backdrop_records: &[BackdropRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
        tiles_size: (u32, u32),
    ) -> Option<DrawTileCoverage<'b>> {
        let draw = &draw_records[draw_ix];
        if let Some(sdf) = &draw.sdf {
            let bbox = draw.tile_bbox(tiles_size.0, tiles_size.1);
            if tile_x >= bbox.x0 && tile_x < bbox.x1 && tile_y >= bbox.y0 && tile_y < bbox.y1 {
                return Some(DrawTileCoverage::Sdf { draw, sdf });
            }
            return None;
        }

        if let Some(glyph_run_id) = draw.glyph_run_id {
            let bbox = draw.tile_bbox(tiles_size.0, tiles_size.1);
            if tile_x >= bbox.x0 && tile_x < bbox.x1 && tile_y >= bbox.y0 && tile_y < bbox.y1 {
                return Some(DrawTileCoverage::Glyph { draw, glyph_run_id });
            }
            return None;
        }

        let (draw, backdrop, segment_range) = Self::layer_tile_coverage(
            draw_ix,
            tile_x,
            tile_y,
            draw_records,
            backdrop_records,
            backdrops,
            tile_segment_ranges,
            tiles_size,
        )?;
        Some(DrawTileCoverage::Path {
            draw,
            backdrop,
            segment_range,
        })
    }
}

enum DrawTileCoverage<'a> {
    Path {
        draw: &'a DrawRecord,
        backdrop: i32,
        segment_range: std::ops::Range<u32>,
    },
    Sdf {
        draw: &'a DrawRecord,
        sdf: &'a crate::shared::sdf::Sdf,
    },
    Glyph {
        draw: &'a DrawRecord,
        glyph_run_id: u32,
    },
}

impl<'a> DrawTileCoverage<'a> {
    fn draw(&self) -> &'a DrawRecord {
        match self {
            Self::Path { draw, .. } | Self::Sdf { draw, .. } | Self::Glyph { draw, .. } => draw,
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
        draw_range: std::ops::Range<usize>,
        layer_stack_data: &'a [LayerStackEntry],
        layer_stack_range: std::ops::Range<usize>,
        backdrop_records: &'a [BackdropRecord],
        backdrops: &'a [i32],
        tile_segment_ranges: &'a [TileSegmentRange],
        tile_ptcl_ranges: &'a mut Vec<TilePtclRange>,
        tile_ptcls: &'a mut Vec<TilePtcl>,
        tiles_size: (u32, u32),
    ) -> CoarseCpuPrepared<'a> {
        CoarseCpuPrepared {
            draw_records,
            draw_range,
            layer_stack_data,
            layer_stack_range,
            backdrop_records,
            backdrops,
            tile_segment_ranges,
            tile_ptcl_ranges,
            tile_ptcls,
            tiles_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use peniko::Color;

    use super::CoarseCpuPipeline;
    use crate::shared::{
        bd_record::BackdropRecord,
        bounds::PixelBounds,
        brush::Brush,
        draw_record::{DrawRecord, DrawTag},
        execution::LayerStackEntry,
        fill::FillRule,
        tile_ptcl::TilePtcl,
        tile_seg_range::TileSegmentRange,
    };

    fn solid_color_u32(color: Color) -> u32 {
        crate::shared::pixel::premul_f32_to_u32(color.premultiply().components)
    }

    #[test]
    fn run_replays_clip_layers_in_user_nesting_order() {
        let draw_records = [
            DrawRecord {
                path_id: Some(0),
                glyph_run_id: None,
                sdf: None,
                tag: DrawTag::Clip,
                brush: Brush::Solid(Color::TRANSPARENT),
                fill_rule: FillRule::NonZero,
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 16,
                    y1: 16,
                },
                solid_rect: false,
            },
            DrawRecord {
                path_id: Some(1),
                glyph_run_id: None,
                sdf: None,
                tag: DrawTag::Clip,
                brush: Brush::Solid(Color::TRANSPARENT),
                fill_rule: FillRule::NonZero,
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 16,
                    y1: 16,
                },
                solid_rect: false,
            },
            DrawRecord {
                path_id: Some(2),
                glyph_run_id: None,
                sdf: None,
                tag: DrawTag::Brush,
                brush: Brush::Solid(Color::BLACK),
                fill_rule: FillRule::NonZero,
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 16,
                    y1: 16,
                },
                solid_rect: false,
            },
        ];
        let backdrop_records = [
            BackdropRecord {
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
            },
            BackdropRecord {
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
            },
            BackdropRecord {
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
        let layer_stack_data = [
            LayerStackEntry::Clip { draw: 0 },
            LayerStackEntry::Clip { draw: 1 },
        ];

        CoarseCpuPipeline::new()
            .prepare(
                &draw_records,
                2..3,
                &layer_stack_data,
                0..layer_stack_data.len(),
                &backdrop_records,
                &backdrops,
                &tile_segment_ranges,
                &mut tile_ptcl_ranges,
                &mut tile_ptcls,
                (1, 1),
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
    fn run_builds_each_tile_stream_independently_in_draw_order() {
        let draw_records = [
            DrawRecord {
                path_id: Some(0),
                glyph_run_id: None,
                sdf: None,
                tag: DrawTag::Brush,
                brush: Brush::Solid(Color::from_rgb8(255, 0, 0)),
                fill_rule: FillRule::NonZero,
                pixel_bounds: PixelBounds {
                    x0: 0,
                    y0: 0,
                    x1: 32,
                    y1: 16,
                },
                solid_rect: true,
            },
            DrawRecord {
                path_id: Some(1),
                glyph_run_id: None,
                sdf: None,
                tag: DrawTag::Brush,
                brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                fill_rule: FillRule::NonZero,
                pixel_bounds: PixelBounds {
                    x0: 16,
                    y0: 0,
                    x1: 32,
                    y1: 16,
                },
                solid_rect: true,
            },
        ];
        let backdrop_records = [
            BackdropRecord {
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
            },
            BackdropRecord {
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
            },
        ];
        let backdrops = [1, 1, 1];
        let tile_segment_ranges = [TileSegmentRange::default(); 3];
        let mut tile_ptcl_ranges = Vec::new();
        let mut tile_ptcls = Vec::new();

        CoarseCpuPipeline::new()
            .prepare(
                &draw_records,
                0..draw_records.len(),
                &[],
                0..0,
                &backdrop_records,
                &backdrops,
                &tile_segment_ranges,
                &mut tile_ptcl_ranges,
                &mut tile_ptcls,
                (2, 1),
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
