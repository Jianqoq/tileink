use peniko::BlendMode;

use crate::shared::{
    bd_record::BackdropRecord,
    draw_record::DrawRecord,
    layer::Layer,
    tile_ptcl::{TileColorPtcl, TileFillPtcl, TilePtcl, TilePtclRange},
    tile_seg_range::TileSegmentRange,
};

pub struct CoarseCpuPrepared<'a> {
    draw_records: &'a [DrawRecord],
    clip_layers: &'a [Layer],
    clip_stack_data: &'a [u32],
    opacity_stack_data: &'a [f32],
    blend_layers: &'a [BlendMode],
    blend_stack_data: &'a [u32],
    // These ranges select the batch's active fused-layer stacks from the
    // execution plan arenas. They are not screen-space bounds.
    //
    // The correct long-term model is for clip / opacity / blend to carry their
    // own tile or pixel coverage ranges in addition to stack membership.
    clip_stack: std::ops::Range<usize>,
    opacity_stack: std::ops::Range<usize>,
    blend_stack: std::ops::Range<usize>,
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
        let clip_stack = &self.clip_stack_data[self.clip_stack.clone()];
        let opacity_stack = &self.opacity_stack_data[self.opacity_stack.clone()];
        let blend_stack = &self.blend_stack_data[self.blend_stack.clone()];

        for draw in self.draw_records {
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

                    if let Some(color) = draw.brush.solid_color() {
                        if draw.solid_rect && segment_range.start == segment_range.end {
                            Self::ensure_batch_wrappers(
                                &mut per_tile[tile_ix],
                                clip_stack,
                                opacity_stack,
                                blend_stack,
                                self.clip_layers,
                                self.blend_layers,
                            );
                            per_tile[tile_ix].push(TilePtcl::Color(TileColorPtcl {
                                color: crate::shared::pixel::premul_f32_to_u32(
                                    color.premultiply().components,
                                ),
                            }));
                            continue;
                        }
                    }

                    if segment_range.start == segment_range.end && backdrop == 0 {
                        continue;
                    }

                    Self::ensure_batch_wrappers(
                        &mut per_tile[tile_ix],
                        clip_stack,
                        opacity_stack,
                        blend_stack,
                        self.clip_layers,
                        self.blend_layers,
                    );
                    per_tile[tile_ix].push(TilePtcl::Fill(TileFillPtcl {
                        backdrop,
                        fill_rule: draw.fill_rule,
                        segment_range: segment_range.start..segment_range.end,
                        brush: draw.brush.clone(),
                    }));
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
                for _ in 0..blend_stack.len() {
                    self.tile_ptcls.push(TilePtcl::EndBlend);
                }
                for _ in 0..opacity_stack.len() {
                    self.tile_ptcls.push(TilePtcl::EndOpacity);
                }
                self.tile_ptcls.push(TilePtcl::End);
            }
            let end = self.tile_ptcls.len() as u32;
            self.tile_ptcl_ranges[tile_ix] = TilePtclRange { start, end };
        }
    }

    fn ensure_batch_wrappers(
        ptcls: &mut Vec<TilePtcl>,
        clip_stack: &[u32],
        opacity_stack: &[f32],
        blend_stack: &[u32],
        clip_layers: &[Layer],
        blend_layers: &[BlendMode],
    ) {
        if !ptcls.is_empty() {
            return;
        }

        // This currently inserts fused-layer wrappers at first tile touch.
        // That makes the stack visible to fine, but it does not yet express
        // independent clip/opacity/blend coverage. Once layer-local ranges
        // exist, wrapper emission should be driven by those ranges instead of
        // by draw coverage alone.
        let _ = clip_stack;
        let _ = clip_layers;
        for &opacity in opacity_stack {
            let opacity = (opacity.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            ptcls.push(TilePtcl::BeginOpacity { opacity });
        }
        for &blend_ix in blend_stack {
            ptcls.push(TilePtcl::BeginBlend {
                mode: blend_layers[blend_ix as usize],
            });
        }
    }
}

pub struct CoarseCpuPipeline;

impl CoarseCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    /// Prepares the CPU coarse stage for one draw batch and the batch's fused
    /// layer stack snapshots.
    ///
    /// `clip_stack`, `opacity_stack`, and `blend_stack` are execution-plan
    /// arena slices, not layer coverage bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare<'a>(
        &self,
        draw_records: &'a [DrawRecord],
        clip_layers: &'a [Layer],
        clip_stack_data: &'a [u32],
        opacity_stack_data: &'a [f32],
        blend_layers: &'a [BlendMode],
        blend_stack_data: &'a [u32],
        clip_stack: std::ops::Range<usize>,
        opacity_stack: std::ops::Range<usize>,
        blend_stack: std::ops::Range<usize>,
        backdrop_records: &'a [BackdropRecord],
        backdrops: &'a [i32],
        tile_segment_ranges: &'a [TileSegmentRange],
        tile_ptcl_ranges: &'a mut Vec<TilePtclRange>,
        tile_ptcls: &'a mut Vec<TilePtcl>,
        tiles_size: (u32, u32),
    ) -> CoarseCpuPrepared<'a> {
        CoarseCpuPrepared {
            draw_records,
            clip_layers,
            clip_stack_data,
            opacity_stack_data,
            blend_layers,
            blend_stack_data,
            clip_stack,
            opacity_stack,
            blend_stack,
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
        draw_record::DrawRecord,
        fill::FillRule,
        tile_ptcl::TilePtcl,
        tile_seg_range::TileSegmentRange,
    };

    #[test]
    fn run_wraps_tile_draws_with_opacity_and_blend_ptcls() {
        let draw_records = [DrawRecord {
            path_id: Some(0),
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
            blend_depth: 1,
            clip_depth: 0,
            allow_solid_override: true,
        }];
        let backdrop_records = [BackdropRecord {
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
        }];
        let backdrops = [0];
        let tile_segment_ranges = [TileSegmentRange { start: 0, end: 1 }];
        let mut tile_ptcl_ranges = Vec::new();
        let mut tile_ptcls = Vec::new();
        let blend_layers = vec![BlendMode::new(Mix::Normal, Compose::SrcOver)];

        CoarseCpuPipeline::new()
            .prepare(
                &draw_records,
                &[],
                &[],
                &[0.5],
                &blend_layers,
                &[0],
                0..0,
                0..1,
                0..1,
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
        assert!(matches!(tile_ptcls[0], TilePtcl::BeginOpacity { opacity: 128 }));
        assert!(matches!(tile_ptcls[1], TilePtcl::BeginBlend { .. }));
        assert!(matches!(tile_ptcls[2], TilePtcl::Fill(_)));
        assert!(matches!(tile_ptcls[3], TilePtcl::EndBlend));
        assert!(matches!(tile_ptcls[4], TilePtcl::EndOpacity));
        assert!(matches!(tile_ptcls[5], TilePtcl::End));
    }
}
