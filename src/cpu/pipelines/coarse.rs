use crate::shared::{
    bd_record::BackdropRecord,
    draw_record::{DrawRecord, DrawTag},
    execution::FusedLayerEntry,
    tile_ptcl::{TileColorPtcl, TileFillPtcl, TilePtcl, TilePtclRange},
    tile_seg_range::TileSegmentRange,
};

pub struct CoarseCpuPrepared<'a> {
    draw_records: &'a [DrawRecord],
    draw_range: std::ops::Range<usize>,
    fused_layers: &'a [FusedLayerEntry],
    // This range selects the batch's active fused-layer stack snapshot from
    // the execution-plan arena. It is nesting state, not a screen-space bound.
    fused_range: std::ops::Range<usize>,
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
        let mut per_tile = vec![Vec::new(); tile_count];
        let mut per_tile_wrappers = vec![Vec::new(); tile_count];
        let fused_layers = &self.fused_layers[self.fused_range.clone()];

        for draw in &self.draw_records[self.draw_range.clone()] {
            let Some(path_id) = draw.path_id else {
                continue;
            };
            let backdrop_record = &self.backdrop_records[path_id as usize];
            let bbox = draw.tile_bbox(self.tiles_size.0, self.tiles_size.1);
            let stride = backdrop_record.tile_x1 - backdrop_record.tile_x0;
            if stride == 0 {
                continue;
            }

            for tile_y in bbox.y0..bbox.y1 {
                for tile_x in bbox.x0..bbox.x1 {
                    let local_x = tile_x - backdrop_record.tile_x0;
                    let local_y = tile_y - backdrop_record.tile_y0;
                    let local_ix = (local_y * stride + local_x) as usize;
                    let backdrop_ix = backdrop_record.data_offset as usize + local_ix;
                    let segment_range = self.tile_segment_ranges[backdrop_ix];
                    let backdrop = self.backdrops[backdrop_ix];
                    let tile_ix = (tile_y * self.tiles_size.0 + tile_x) as usize;

                    if segment_range.start == segment_range.end && backdrop == 0 {
                        continue;
                    }

                    Self::ensure_batch_wrappers(
                        &mut per_tile[tile_ix],
                        &mut per_tile_wrappers[tile_ix],
                        tile_x,
                        tile_y,
                        fused_layers,
                        self.draw_records,
                        self.backdrop_records,
                        self.backdrops,
                        self.tile_segment_ranges,
                        self.tiles_size,
                    );
                    match draw.tag {
                        DrawTag::Brush => {
                            if let Some(color) = draw.brush.solid_color() {
                                if draw.solid_rect && segment_range.start == segment_range.end {
                                    per_tile[tile_ix].push(TilePtcl::Color(TileColorPtcl {
                                        color: crate::shared::pixel::premul_f32_to_u32(
                                            color.premultiply().components,
                                        ),
                                    }));
                                    continue;
                                }
                            }
                            per_tile[tile_ix].push(TilePtcl::Fill(TileFillPtcl {
                                backdrop,
                                fill_rule: draw.fill_rule,
                                segment_range: segment_range.start..segment_range.end,
                                brush: draw.brush.clone(),
                            }));
                        }
                        DrawTag::Clip => {
                            per_tile[tile_ix].push(TilePtcl::BeginClip(TileFillPtcl {
                                backdrop,
                                fill_rule: draw.fill_rule,
                                segment_range: segment_range.start..segment_range.end,
                                brush: draw.brush.clone(),
                            }));
                        }
                        DrawTag::Opacity | DrawTag::Blend => {}
                    }
                }
            }
        }

        self.tile_ptcl_ranges.clear();
        self.tile_ptcl_ranges
            .resize(tile_count, TilePtclRange::default());
        self.tile_ptcls.clear();

        for (tile_ix, ptcls) in per_tile.into_iter().enumerate() {
            let start = self.tile_ptcls.len() as u32;
            if !ptcls.is_empty() {
                self.tile_ptcls.extend(ptcls);
                for wrapper in per_tile_wrappers[tile_ix].iter().rev() {
                    self.tile_ptcls.push(match wrapper {
                        FusedLayerEntry::Clip { .. } => TilePtcl::EndClip,
                        FusedLayerEntry::Opacity { .. } => TilePtcl::EndOpacity,
                        FusedLayerEntry::Blend { .. } => TilePtcl::EndBlend,
                    });
                }
                self.tile_ptcls.push(TilePtcl::End);
            }
            let end = self.tile_ptcls.len() as u32;
            self.tile_ptcl_ranges[tile_ix] = TilePtclRange { start, end };
        }
    }

    /// Emits the once-per-tile wrapper particles for the current batch.
    ///
    /// This is called lazily: the wrappers are only inserted when the batch
    /// first contributes something to a tile. If the tile is untouched by the
    /// batch, no wrapper particles are emitted for that tile.
    ///
    /// Current behavior:
    /// - replays the active fused-layer stack in user nesting order;
    /// - emits a `Begin*` particle only when that layer's own draw covers the
    ///   tile.
    ///
    /// The matching `End*` particles are appended later when the tile particle
    /// list is finalized.
    fn ensure_batch_wrappers(
        ptcls: &mut Vec<TilePtcl>,
        emitted_wrappers: &mut Vec<FusedLayerEntry>,
        tile_x: u32,
        tile_y: u32,
        fused_layers: &[FusedLayerEntry],
        draw_records: &[DrawRecord],
        backdrop_records: &[BackdropRecord],
        backdrops: &[i32],
        tile_segment_ranges: &[TileSegmentRange],
        tiles_size: (u32, u32),
    ) {
        if !ptcls.is_empty() {
            return;
        }

        for &layer in fused_layers {
            let Some((draw, backdrop, segment_range)) = Self::layer_tile_coverage(
                layer.draw_ix() as usize,
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

            match layer {
                FusedLayerEntry::Clip { .. } => {
                    ptcls.push(TilePtcl::BeginClip(TileFillPtcl {
                        backdrop,
                        fill_rule: draw.fill_rule,
                        segment_range,
                        brush: draw.brush.clone(),
                    }));
                }
                FusedLayerEntry::Opacity { opacity, .. } => {
                    let opacity = (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    ptcls.push(TilePtcl::BeginOpacity {
                        opacity,
                        fill: TileFillPtcl {
                            backdrop,
                            fill_rule: draw.fill_rule,
                            segment_range,
                            brush: draw.brush.clone(),
                        },
                    });
                }
                FusedLayerEntry::Blend { mode, .. } => {
                    ptcls.push(TilePtcl::BeginBlend {
                        mode,
                        fill: TileFillPtcl {
                            backdrop,
                            fill_rule: draw.fill_rule,
                            segment_range,
                            brush: draw.brush.clone(),
                        },
                    });
                }
            }
            emitted_wrappers.push(layer);
        }
    }

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
}

pub struct CoarseCpuPipeline;

impl CoarseCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    /// Prepares the CPU coarse stage for one draw batch and the batch's fused
    /// layer stack snapshot.
    ///
    /// `fused_range` selects a batch-local ordered stack slice from the plan.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare<'a>(
        &self,
        draw_records: &'a [DrawRecord],
        draw_range: std::ops::Range<usize>,
        fused_layers: &'a [FusedLayerEntry],
        fused_range: std::ops::Range<usize>,
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
            fused_layers,
            fused_range,
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
    use peniko::{BlendMode, Color, Compose, Mix};

    use super::CoarseCpuPipeline;
    use crate::shared::{
        bd_record::BackdropRecord,
        bounds::PixelBounds,
        brush::Brush,
        draw_record::{DrawRecord, DrawTag},
        execution::FusedLayerEntry,
        fill::FillRule,
        tile_ptcl::TilePtcl,
        tile_seg_range::TileSegmentRange,
    };

    #[test]
    fn run_replays_fused_layers_in_user_nesting_order() {
        let draw_records = [
            DrawRecord {
                path_id: Some(0),
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
                opacity_depth: 0,
                blend_depth: 0,
                clip_depth: 0,
                allow_solid_override: false,
            },
            DrawRecord {
                path_id: Some(1),
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
                opacity_depth: 0,
                blend_depth: 0,
                clip_depth: 0,
                allow_solid_override: false,
            },
            DrawRecord {
                path_id: Some(2),
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
                opacity_depth: 1,
                blend_depth: 0,
                clip_depth: 2,
                allow_solid_override: true,
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
        let fused_layers = [
            FusedLayerEntry::Clip { draw_ix: 0 },
            FusedLayerEntry::Opacity {
                draw_ix: 0,
                opacity: 0.5,
            },
            FusedLayerEntry::Clip { draw_ix: 1 },
            FusedLayerEntry::Blend {
                draw_ix: 1,
                mode: BlendMode::new(Mix::Normal, Compose::SrcOver),
            },
        ];

        CoarseCpuPipeline::new()
            .prepare(
                &draw_records,
                2..3,
                &fused_layers,
                0..fused_layers.len(),
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
        assert_eq!(tile_ptcl_ranges[0].end, 10);
        assert!(matches!(tile_ptcls[0], TilePtcl::BeginClip(_)));
        assert!(matches!(
            tile_ptcls[1],
            TilePtcl::BeginOpacity { opacity: 128, .. }
        ));
        assert!(matches!(tile_ptcls[2], TilePtcl::BeginClip(_)));
        assert!(matches!(tile_ptcls[3], TilePtcl::BeginBlend { .. }));
        assert!(matches!(tile_ptcls[4], TilePtcl::Fill(_)));
        assert!(matches!(tile_ptcls[5], TilePtcl::EndBlend));
        assert!(matches!(tile_ptcls[6], TilePtcl::EndClip));
        assert!(matches!(tile_ptcls[7], TilePtcl::EndOpacity));
        assert!(matches!(tile_ptcls[8], TilePtcl::EndClip));
        assert!(matches!(tile_ptcls[9], TilePtcl::End));
    }
}
