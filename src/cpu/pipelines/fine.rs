use crate::{
    cpu::computes::fine::{
        build_tile_alpha, combine_alpha, composite_blend_group_tile,
        composite_color_tile_buffer_into, composite_opacity_group_tile, rasterize_tile_buffer_into,
    },
    shared::{
        bounds::{Bounds, PixelBounds},
        image::Image,
        line_seg::LineSegment,
        pixel::TileBuffer,
        tile_ptcl::TilePtcl,
        tile_ptcl::TilePtclRange,
    },
};

pub struct FineCpuPrepared<'a> {
    tile_ptcl_ranges: &'a [TilePtclRange],
    tile_ptcls: &'a [TilePtcl],
    segments: &'a [LineSegment],
    target: &'a mut Image,
    target_bounds: Bounds,
    tiles_size: (u32, u32),
}

impl<'a> FineCpuPrepared<'a> {
    pub fn run(&mut self) {
        let bounds = self.target_bounds.intersect(Bounds::new(
            0,
            0,
            (self.tiles_size.0 * crate::TILE_SIZE) as i32,
            (self.tiles_size.1 * crate::TILE_SIZE) as i32,
        ));
        let tile_bbox = PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
        .tile_bbox(self.tiles_size.0, self.tiles_size.1);
        for tile_y in tile_bbox.y0..tile_bbox.y1 {
            for tile_x in tile_bbox.x0..tile_bbox.x1 {
                let tile_ix = (tile_y * self.tiles_size.0 + tile_x) as usize;
                let range = self.tile_ptcl_ranges[tile_ix];
                if range.start == range.end {
                    continue;
                }

                let mut tile = self.load_tile(tile_x, tile_y);
                let mut clip_mask = [255u8; 256];
                let mut clip_stack = Vec::new();
                let mut group_stack = Vec::new();
                for ptcl in &self.tile_ptcls[range.start as usize..range.end as usize] {
                    match ptcl {
                        TilePtcl::End => break,
                        TilePtcl::Color(color) => {
                            composite_color_tile_buffer_into(&mut tile, color.color, &clip_mask);
                        }
                        TilePtcl::Fill(fill) => {
                            let segments = &self.segments[fill.segment_range.start as usize
                                ..fill.segment_range.end as usize];
                            rasterize_tile_buffer_into(
                                &mut tile,
                                tile_x,
                                tile_y,
                                segments,
                                fill.backdrop,
                                fill.fill_rule,
                                &fill.brush,
                                &clip_mask,
                            );
                        }
                        TilePtcl::BeginClip(fill) => {
                            let segments = &self.segments[fill.segment_range.start as usize
                                ..fill.segment_range.end as usize];
                            let alpha = build_tile_alpha(segments, fill.backdrop, fill.fill_rule);
                            clip_stack.push(clip_mask);
                            for (dst, src) in clip_mask.iter_mut().zip(alpha) {
                                *dst = combine_alpha(*dst, src);
                            }
                        }
                        TilePtcl::EndClip => {
                            if let Some(previous) = clip_stack.pop() {
                                clip_mask = previous;
                            }
                        }
                        TilePtcl::BeginOpacity { opacity, fill } => {
                            let segments = &self.segments[fill.segment_range.start as usize
                                ..fill.segment_range.end as usize];
                            group_stack.push(GroupFrame::Opacity {
                                parent: tile,
                                parent_clip_mask: clip_mask,
                                layer_alpha: build_tile_alpha(
                                    segments,
                                    fill.backdrop,
                                    fill.fill_rule,
                                ),
                                opacity: *opacity,
                            });
                            tile = [0; 256];
                        }
                        TilePtcl::EndOpacity => {
                            if let Some(GroupFrame::Opacity {
                                mut parent,
                                parent_clip_mask,
                                layer_alpha,
                                opacity,
                            }) = group_stack.pop()
                            {
                                composite_opacity_group_tile(
                                    &mut parent,
                                    &tile,
                                    &layer_alpha,
                                    &parent_clip_mask,
                                    opacity,
                                );
                                tile = parent;
                            }
                        }
                        TilePtcl::BeginBlend { mode, fill } => {
                            let segments = &self.segments[fill.segment_range.start as usize
                                ..fill.segment_range.end as usize];
                            group_stack.push(GroupFrame::Blend {
                                parent: tile,
                                parent_clip_mask: clip_mask,
                                layer_alpha: build_tile_alpha(
                                    segments,
                                    fill.backdrop,
                                    fill.fill_rule,
                                ),
                                mode: *mode,
                            });
                            tile = [0; 256];
                        }
                        TilePtcl::EndBlend => {
                            if let Some(GroupFrame::Blend {
                                mut parent,
                                parent_clip_mask,
                                layer_alpha,
                                mode,
                            }) = group_stack.pop()
                            {
                                composite_blend_group_tile(
                                    &mut parent,
                                    &tile,
                                    &layer_alpha,
                                    &parent_clip_mask,
                                    mode,
                                );
                                tile = parent;
                            }
                        }
                    }
                }
                self.store_tile(tile_x, tile_y, &tile);
            }
        }
    }

    fn load_tile(&self, tile_x: u32, tile_y: u32) -> TileBuffer {
        let mut tile = [0; 256];
        let Some((global, base_x, base_y)) = self.tile_target_bounds(tile_x, tile_y) else {
            return tile;
        };
        for global_y in global.y0..global.y1 {
            let local_y = (global_y - base_y) as usize;
            for global_x in global.x0..global.x1 {
                let local_x = (global_x - base_x) as usize;
                let image_x = (global_x - self.target_bounds.x0) as u32;
                let image_y = (global_y - self.target_bounds.y0) as u32;
                tile[local_y * 16 + local_x] =
                    self.target.pixels[(image_y * self.target.width + image_x) as usize];
            }
        }
        tile
    }

    fn store_tile(&mut self, tile_x: u32, tile_y: u32, tile: &TileBuffer) {
        let Some((global, base_x, base_y)) = self.tile_target_bounds(tile_x, tile_y) else {
            return;
        };
        for global_y in global.y0..global.y1 {
            let local_y = (global_y - base_y) as usize;
            for global_x in global.x0..global.x1 {
                let local_x = (global_x - base_x) as usize;
                let image_x = (global_x - self.target_bounds.x0) as u32;
                let image_y = (global_y - self.target_bounds.y0) as u32;
                self.target.pixels[(image_y * self.target.width + image_x) as usize] =
                    tile[local_y * 16 + local_x];
            }
        }
    }

    fn tile_target_bounds(&self, tile_x: u32, tile_y: u32) -> Option<(Bounds, i32, i32)> {
        let base_x = (tile_x * crate::TILE_SIZE) as i32;
        let base_y = (tile_y * crate::TILE_SIZE) as i32;
        let tile_bounds = Bounds::new(
            base_x,
            base_y,
            base_x + crate::TILE_SIZE as i32,
            base_y + crate::TILE_SIZE as i32,
        );
        let bounds = tile_bounds.intersect(self.target_bounds);
        (!bounds.is_empty()).then_some((bounds, base_x, base_y))
    }
}

enum GroupFrame {
    Opacity {
        parent: TileBuffer,
        parent_clip_mask: [u8; 256],
        layer_alpha: [u8; 256],
        opacity: u8,
    },
    Blend {
        parent: TileBuffer,
        parent_clip_mask: [u8; 256],
        layer_alpha: [u8; 256],
        mode: peniko::BlendMode,
    },
}

pub struct FineCpuPipeline;

impl FineCpuPipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn prepare<'a>(
        &self,
        tile_ptcl_ranges: &'a [TilePtclRange],
        tile_ptcls: &'a [TilePtcl],
        segments: &'a [LineSegment],
        target: &'a mut Image,
        target_bounds: Bounds,
        tiles_size: (u32, u32),
    ) -> FineCpuPrepared<'a> {
        FineCpuPrepared {
            tile_ptcl_ranges,
            tile_ptcls,
            segments,
            target,
            target_bounds,
            tiles_size,
        }
    }
}
