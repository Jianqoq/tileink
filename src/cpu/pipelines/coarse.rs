use crate::shared::{
    bd_record::BackdropRecord,
    draw_record::DrawRecord,
    tile_ptcl::{TileColorPtcl, TileFillPtcl, TilePtcl, TilePtclRange},
    tile_seg_range::TileSegmentRange,
};

pub struct CoarseCpuPrepared<'a> {
    draw_records: &'a [DrawRecord],
    backdrop_records: &'a [BackdropRecord],
    backdrops: &'a [i32],
    tile_segment_ranges: &'a [TileSegmentRange],
    tile_ptcl_ranges: &'a mut Vec<TilePtclRange>,
    tile_ptcls: &'a mut Vec<TilePtcl>,
    tiles_size: (u32, u32),
}

impl<'a> CoarseCpuPrepared<'a> {
    pub fn run(&mut self) {
        let tile_count = (self.tiles_size.0 * self.tiles_size.1) as usize;
        let mut per_tile = vec![Vec::new(); tile_count];

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
                self.tile_ptcls.push(TilePtcl::End);
            }
            let end = self.tile_ptcls.len() as u32;
            self.tile_ptcl_ranges[tile_ix] = TilePtclRange { start, end };
        }
    }
}

pub struct CoarseCpuPipeline;

impl CoarseCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare<'a>(
        &self,
        draw_records: &'a [DrawRecord],
        backdrop_records: &'a [BackdropRecord],
        backdrops: &'a [i32],
        tile_segment_ranges: &'a [TileSegmentRange],
        tile_ptcl_ranges: &'a mut Vec<TilePtclRange>,
        tile_ptcls: &'a mut Vec<TilePtcl>,
        tiles_size: (u32, u32),
    ) -> CoarseCpuPrepared<'a> {
        CoarseCpuPrepared {
            draw_records,
            backdrop_records,
            backdrops,
            tile_segment_ranges,
            tile_ptcl_ranges,
            tile_ptcls,
            tiles_size,
        }
    }
}
