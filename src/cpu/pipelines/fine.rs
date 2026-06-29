use crate::{
    cpu::computes::fine::{
        build_tile_alpha, combine_alpha, composite_blend_group_tile,
        composite_color_tile_buffer_into, composite_opacity_group_tile,
        rasterize_glyphs_tile_buffer_into, rasterize_sdf_tile_buffer_into,
        rasterize_tile_buffer_into,
    },
    shared::{
        bounds::{Bounds, PixelBounds},
        image::Image,
        line_seg::LineSegment,
        pixel::TileBuffer,
        tile_ptcl::TilePtcl,
        tile_ptcl::TilePtclRange,
    },
    text::PreparedTextData,
};
use rayon::prelude::*;

pub struct FineCpuPrepared<'a> {
    tile_ptcl_ranges: &'a [TilePtclRange],
    tile_ptcls: &'a [TilePtcl],
    tile_glyphs: &'a [u32],
    segments: &'a [LineSegment],
    target: &'a mut Image,
    target_bounds: Bounds,
    tiles_size: (u32, u32),
    text: Option<&'a PreparedTextData>,
}

impl<'a> FineCpuPrepared<'a> {
    pub fn run(&mut self) {
        let bounds = self.target_bounds.intersect(Bounds::new(
            0,
            0,
            (self.tiles_size.0 * crate::TILE_SIZE) as i32,
            (self.tiles_size.1 * crate::TILE_SIZE) as i32,
        ));
        if bounds.is_empty() {
            return;
        }

        let tile_bbox = PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
        .tile_bbox(self.tiles_size.0, self.tiles_size.1);

        let pixels = self.target.pixels.as_mut_ptr() as usize;
        let image_width = self.target.width;
        let image_len = self.target.pixels.len();
        let target_bounds = self.target_bounds;
        let tile_count = tile_bbox.tile_count() as usize;
        let resources = FineTileResources {
            tile_ptcls: self.tile_ptcls,
            tile_glyphs: self.tile_glyphs,
            segments: self.segments,
            text: self.text,
        };

        (0..tile_count).into_par_iter().for_each(|tile_offset| {
            let tile_x = tile_bbox.x0 + tile_offset as u32 % tile_bbox.tile_stride();
            let tile_y = tile_bbox.y0 + tile_offset as u32 / tile_bbox.tile_stride();
            let tile_ix = (tile_y * self.tiles_size.0 + tile_x) as usize;
            let range = self.tile_ptcl_ranges[tile_ix];
            if range.start == range.end {
                return;
            }

            let Some((global, base_x, base_y)) = tile_target_bounds(tile_x, tile_y, target_bounds)
            else {
                return;
            };

            let pixels = pixels as *mut u32;
            let mut tile = unsafe {
                load_tile(
                    pixels,
                    image_len,
                    image_width,
                    target_bounds,
                    global,
                    base_x,
                    base_y,
                )
            };
            render_tile(&mut tile, tile_x, tile_y, range, resources);
            unsafe {
                store_tile(
                    pixels,
                    image_len,
                    image_width,
                    target_bounds,
                    global,
                    base_x,
                    base_y,
                    &tile,
                );
            }
        });
    }
}

#[derive(Clone, Copy)]
struct FineTileResources<'a> {
    tile_ptcls: &'a [TilePtcl],
    tile_glyphs: &'a [u32],
    segments: &'a [LineSegment],
    text: Option<&'a PreparedTextData>,
}

fn render_tile(
    tile: &mut TileBuffer,
    tile_x: u32,
    tile_y: u32,
    range: TilePtclRange,
    resources: FineTileResources<'_>,
) {
    let mut clip_mask = [255u8; 256];
    let mut clip_stack = Vec::new();
    let mut group_stack = Vec::new();
    for ptcl in &resources.tile_ptcls[range.start as usize..range.end as usize] {
        match ptcl {
            TilePtcl::End => break,
            TilePtcl::Color(color) => {
                composite_color_tile_buffer_into(tile, color.color, &clip_mask);
            }
            TilePtcl::Fill(fill) => {
                let segments = &resources.segments
                    [fill.segment_range.start as usize..fill.segment_range.end as usize];
                rasterize_tile_buffer_into(
                    tile,
                    tile_x,
                    tile_y,
                    segments,
                    fill.backdrop,
                    fill.fill_rule,
                    &fill.brush,
                    &clip_mask,
                );
            }
            TilePtcl::Sdf(sdf) => {
                rasterize_sdf_tile_buffer_into(
                    tile, tile_x, tile_y, &sdf.sdf, &sdf.brush, &clip_mask,
                );
            }
            TilePtcl::Glyph(glyph) => {
                if let Some(text) = resources.text {
                    let glyph_ids = &resources.tile_glyphs
                        [glyph.glyph_range.start as usize..glyph.glyph_range.end as usize];
                    rasterize_glyphs_tile_buffer_into(
                        tile,
                        tile_x,
                        tile_y,
                        glyph_ids,
                        &glyph.brush,
                        text,
                        &clip_mask,
                    );
                }
            }
            TilePtcl::BeginClip(fill) => {
                let segments = &resources.segments
                    [fill.segment_range.start as usize..fill.segment_range.end as usize];
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
                let segments = &resources.segments
                    [fill.segment_range.start as usize..fill.segment_range.end as usize];
                group_stack.push(GroupFrame::Opacity {
                    parent: *tile,
                    parent_clip_mask: clip_mask,
                    layer_alpha: build_tile_alpha(segments, fill.backdrop, fill.fill_rule),
                    opacity: *opacity,
                });
                *tile = [0; 256];
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
                        tile,
                        &layer_alpha,
                        &parent_clip_mask,
                        opacity,
                    );
                    *tile = parent;
                }
            }
            TilePtcl::BeginBlend { mode, fill } => {
                let segments = &resources.segments
                    [fill.segment_range.start as usize..fill.segment_range.end as usize];
                group_stack.push(GroupFrame::Blend {
                    parent: *tile,
                    parent_clip_mask: clip_mask,
                    layer_alpha: build_tile_alpha(segments, fill.backdrop, fill.fill_rule),
                    mode: *mode,
                });
                *tile = [0; 256];
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
                        tile,
                        &layer_alpha,
                        &parent_clip_mask,
                        mode,
                    );
                    *tile = parent;
                }
            }
        }
    }
}

fn tile_target_bounds(
    tile_x: u32,
    tile_y: u32,
    target_bounds: Bounds,
) -> Option<(Bounds, i32, i32)> {
    let base_x = (tile_x * crate::TILE_SIZE) as i32;
    let base_y = (tile_y * crate::TILE_SIZE) as i32;
    let tile_bounds = Bounds::new(
        base_x,
        base_y,
        base_x + crate::TILE_SIZE as i32,
        base_y + crate::TILE_SIZE as i32,
    );
    let bounds = tile_bounds.intersect(target_bounds);
    (!bounds.is_empty()).then_some((bounds, base_x, base_y))
}

/// Loads one tile from a disjoint framebuffer region owned by a single fine worker.
///
/// # Safety
/// `pixels` must point to `image_len` valid `u32` pixels. Parallel callers must pass
/// non-overlapping `global` bounds when any caller writes through the same pointer.
unsafe fn load_tile(
    pixels: *const u32,
    image_len: usize,
    image_width: u32,
    target_bounds: Bounds,
    global: Bounds,
    base_x: i32,
    base_y: i32,
) -> TileBuffer {
    let mut tile = [0; 256];
    for global_y in global.y0..global.y1 {
        let local_y = (global_y - base_y) as usize;
        for global_x in global.x0..global.x1 {
            let local_x = (global_x - base_x) as usize;
            let image_x = (global_x - target_bounds.x0) as u32;
            let image_y = (global_y - target_bounds.y0) as u32;
            let image_ix = (image_y * image_width + image_x) as usize;
            debug_assert!(image_ix < image_len);
            tile[local_y * 16 + local_x] = unsafe { *pixels.add(image_ix) };
        }
    }
    tile
}

/// Stores one tile into a disjoint framebuffer region owned by a single fine worker.
///
/// # Safety
/// `pixels` must point to `image_len` valid `u32` pixels. Parallel callers must pass
/// non-overlapping `global` bounds when writing through the same pointer.
#[allow(clippy::too_many_arguments)]
unsafe fn store_tile(
    pixels: *mut u32,
    image_len: usize,
    image_width: u32,
    target_bounds: Bounds,
    global: Bounds,
    base_x: i32,
    base_y: i32,
    tile: &TileBuffer,
) {
    for global_y in global.y0..global.y1 {
        let local_y = (global_y - base_y) as usize;
        for global_x in global.x0..global.x1 {
            let local_x = (global_x - base_x) as usize;
            let image_x = (global_x - target_bounds.x0) as u32;
            let image_y = (global_y - target_bounds.y0) as u32;
            let image_ix = (image_y * image_width + image_x) as usize;
            debug_assert!(image_ix < image_len);
            unsafe {
                *pixels.add(image_ix) = tile[local_y * 16 + local_x];
            }
        }
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

    #[allow(clippy::too_many_arguments)]
    pub fn prepare<'a>(
        &self,
        tile_ptcl_ranges: &'a [TilePtclRange],
        tile_ptcls: &'a [TilePtcl],
        tile_glyphs: &'a [u32],
        segments: &'a [LineSegment],
        target: &'a mut Image,
        target_bounds: Bounds,
        tiles_size: (u32, u32),
        text: Option<&'a PreparedTextData>,
    ) -> FineCpuPrepared<'a> {
        FineCpuPrepared {
            tile_ptcl_ranges,
            tile_ptcls,
            tile_glyphs,
            segments,
            target,
            target_bounds,
            tiles_size,
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use peniko::Color;

    use super::FineCpuPipeline;
    use crate::shared::{
        bounds::Bounds,
        image::{Image, rgba8_pack},
        tile_ptcl::{TileColorPtcl, TilePtcl, TilePtclRange},
    };

    #[test]
    fn run_parallelizes_one_worker_item_per_tile() {
        let tiles_size = (3, 2);
        let colors = [
            rgba8_pack([255, 0, 0, 255]),
            rgba8_pack([0, 255, 0, 255]),
            rgba8_pack([0, 0, 255, 255]),
            rgba8_pack([255, 255, 0, 255]),
            rgba8_pack([0, 255, 255, 255]),
            rgba8_pack([255, 0, 255, 255]),
        ];
        let mut tile_ptcl_ranges =
            vec![TilePtclRange::default(); (tiles_size.0 * tiles_size.1) as usize];
        let mut tile_ptcls = Vec::new();

        for (tile_ix, color) in colors.into_iter().enumerate() {
            let start = tile_ptcls.len() as u32;
            tile_ptcls.push(TilePtcl::Color(TileColorPtcl { color }));
            tile_ptcls.push(TilePtcl::End);
            tile_ptcl_ranges[tile_ix] = TilePtclRange {
                start,
                end: tile_ptcls.len() as u32,
            };
        }

        let target_bounds = Bounds::new(3, 5, 35, 29);
        let mut image = Image::new(target_bounds.width(), target_bounds.height(), Color::BLACK);
        FineCpuPipeline::new()
            .prepare(
                &tile_ptcl_ranges,
                &tile_ptcls,
                &[],
                &[],
                &mut image,
                target_bounds,
                tiles_size,
                None,
            )
            .run();

        assert_eq!(image.rgba8_at(0, 0), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(16, 0), [0, 255, 0, 255]);
        assert_eq!(image.rgba8_at(31, 0), [0, 0, 255, 255]);
        assert_eq!(image.rgba8_at(0, 12), [255, 255, 0, 255]);
        assert_eq!(image.rgba8_at(16, 12), [0, 255, 255, 255]);
        assert_eq!(image.rgba8_at(31, 23), [255, 0, 255, 255]);
    }
}
