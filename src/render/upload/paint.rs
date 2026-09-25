//! CPU paint packing shared by all GPU adapters.
//!
//! This owns prepared bytes and resource membership, not GPU upload completion.
//! Adapters preserve the immediate slice writes and retained dirty-range uploads.

use super::ranges::patch_u32_ranges;
use crate::{
    Canvas,
    shared::{gpu_brush::GpuBrushUpload, image_resource::GpuImageResourceUpload},
};
use std::ops::Range;

#[derive(Default)]
pub(crate) struct PaintUploadState {
    scene_brush_blob: Vec<u32>,
    resource_brush_draws: Vec<bool>,
    resource_brush_draw_count: usize,
    resource_brush_draws_initialized: bool,
    image_resource_generation: Option<u64>,
    paint_blob: Vec<u32>,
    paint_layout: (usize, usize, usize),
}

pub(crate) struct PreparedPaint<'a> {
    pub(crate) shadow_base: u32,
    pub(crate) brush_base: u32,
    pub(crate) data: PaintData<'a>,
}

pub(crate) enum PaintData<'a> {
    Immediate {
        sdfs: &'a [u32],
        shadows: &'a [u32],
        brushes: &'a [u32],
    },
    Retained {
        words: &'a [u32],
        ranges: Vec<Range<usize>>,
    },
}

impl PaintUploadState {
    pub(crate) fn prepare<'a>(
        &'a mut self,
        canvas: &'a Canvas,
        image_resources: Option<&GpuImageResourceUpload>,
    ) -> PreparedPaint<'a> {
        let has_image_resources = image_resources.is_some_and(|resources| !resources.is_empty());
        let patches_resources = has_image_resources && self.update_resource_brush_draws(canvas);
        if !has_image_resources {
            // Draw mutations while no image table exists are intentionally not tracked. The
            // first later resource insertion rebuilds membership once, then resumes journal
            // updates without charging image-free scenes for resource bookkeeping.
            self.resource_brush_draws_initialized = false;
        }
        let resource_generation = image_resources.map(GpuImageResourceUpload::generation);
        let repatch_all_resources =
            patches_resources && self.image_resource_generation != resource_generation;
        let scene_brush_blob = if patches_resources {
            let full_patch = repatch_all_resources
                || canvas.buffer_changes.is_none()
                || self.scene_brush_blob.len() != canvas.brush_blob.len();
            if full_patch {
                self.scene_brush_blob.clear();
                self.scene_brush_blob.extend_from_slice(&canvas.brush_blob);
                GpuBrushUpload::patch_scene_brush_blob(
                    &mut self.scene_brush_blob,
                    &canvas.draw_records,
                    image_resources,
                );
            } else {
                let changes = canvas.buffer_changes.as_ref().unwrap();
                patch_u32_ranges(
                    &mut self.scene_brush_blob,
                    0,
                    &canvas.brush_blob,
                    &changes.brushes,
                );
                GpuBrushUpload::patch_scene_brush_blob_draw_ranges(
                    &mut self.scene_brush_blob,
                    &canvas.draw_records,
                    &changes.draws,
                    image_resources,
                );
            }
            self.image_resource_generation = resource_generation;
            &self.scene_brush_blob
        } else {
            self.image_resource_generation = resource_generation;
            &canvas.brush_blob
        };

        if canvas.buffer_changes.is_none() {
            // Immediate scenes have no allocation journal. Preserve their direct slice
            // uploads rather than allocating or comparing a concatenated CPU buffer.
            self.paint_blob.clear();
            self.paint_layout = (0, 0, 0);
            return PreparedPaint {
                shadow_base: canvas.sdf_blob.len() as u32,
                brush_base: (canvas.sdf_blob.len() + canvas.sdf_shadow_blob.len()) as u32,
                data: PaintData::Immediate {
                    sdfs: &canvas.sdf_blob,
                    shadows: &canvas.sdf_shadow_blob,
                    brushes: scene_brush_blob,
                },
            };
        }
        // SDF, SDF-shadow, and scene brushes share one storage buffer so coarse, fine,
        // and filter bind the same paint data. Keeping one self vector
        // also lets retained uploads transmit only the changed range.
        let required = (
            canvas.sdf_blob.len(),
            canvas.sdf_shadow_blob.len(),
            scene_brush_blob.len(),
        );
        let layout = grow_paint_layout(self.paint_layout, required);
        let shadow_base = layout.0;
        let brush_base = layout.0 + layout.1;
        let relayout =
            self.paint_layout != layout || self.paint_blob.len() != layout.0 + layout.1 + layout.2;
        let ranges = if relayout {
            self.paint_blob.clear();
            self.paint_blob.resize(layout.0 + layout.1 + layout.2, 0);
            self.paint_blob[..required.0].copy_from_slice(&canvas.sdf_blob);
            self.paint_blob[shadow_base..shadow_base + required.1]
                .copy_from_slice(&canvas.sdf_shadow_blob);
            self.paint_blob[brush_base..brush_base + required.2].copy_from_slice(scene_brush_blob);
            self.paint_layout = layout;
            std::iter::once(0..self.paint_blob.len()).collect()
        } else {
            let changes = canvas.buffer_changes.as_ref().unwrap();
            patch_u32_ranges(&mut self.paint_blob, 0, &canvas.sdf_blob, &changes.sdfs);
            patch_u32_ranges(
                &mut self.paint_blob,
                shadow_base,
                &canvas.sdf_shadow_blob,
                &changes.shadows,
            );
            if repatch_all_resources {
                self.paint_blob[brush_base..brush_base + required.2]
                    .copy_from_slice(scene_brush_blob);
            } else {
                patch_u32_ranges(
                    &mut self.paint_blob,
                    brush_base,
                    scene_brush_blob,
                    &changes.brushes,
                );
            }
            let mut ranges = changes
                .sdfs
                .iter()
                .cloned()
                .chain(
                    changes
                        .shadows
                        .iter()
                        .map(|range| range.start + shadow_base..range.end + shadow_base),
                )
                .collect::<Vec<_>>();
            if repatch_all_resources {
                ranges.push(brush_base..brush_base + required.2);
            } else {
                ranges.extend(
                    changes
                        .brushes
                        .iter()
                        .map(|range| range.start + brush_base..range.end + brush_base),
                );
            }
            ranges
        };
        PreparedPaint {
            shadow_base: shadow_base as u32,
            brush_base: brush_base as u32,
            data: PaintData::Retained {
                words: &self.paint_blob,
                ranges,
            },
        }
    }

    fn update_resource_brush_draws(&mut self, canvas: &Canvas) -> bool {
        let full = !self.resource_brush_draws_initialized
            || canvas.buffer_changes.is_none()
            || canvas
                .buffer_changes
                .as_ref()
                .is_some_and(|changes| changes.full_scene_sync);
        if full {
            self.resource_brush_draws.clear();
            self.resource_brush_draws
                .resize(canvas.draw_records.len(), false);
            self.resource_brush_draw_count = 0;
            for (index, draw) in canvas.draw_records.iter().enumerate() {
                let resource = GpuBrushUpload::draw_uses_resource_brush(draw, &canvas.brush_blob);
                self.resource_brush_draws[index] = resource;
                self.resource_brush_draw_count += resource as usize;
            }
            self.resource_brush_draws_initialized = true;
        } else {
            if self.resource_brush_draws.len() > canvas.draw_records.len() {
                self.resource_brush_draw_count -= self.resource_brush_draws
                    [canvas.draw_records.len()..]
                    .iter()
                    .filter(|resource| **resource)
                    .count();
                self.resource_brush_draws
                    .truncate(canvas.draw_records.len());
            } else {
                self.resource_brush_draws
                    .resize(canvas.draw_records.len(), false);
            }
            for index in canvas
                .buffer_changes
                .as_ref()
                .unwrap()
                .draws
                .iter()
                .flat_map(|range| {
                    range.start.min(canvas.draw_records.len())
                        ..range.end.min(canvas.draw_records.len())
                })
            {
                let resource = GpuBrushUpload::draw_uses_resource_brush(
                    &canvas.draw_records[index],
                    &canvas.brush_blob,
                );
                if self.resource_brush_draws[index] != resource {
                    if resource {
                        self.resource_brush_draw_count += 1;
                    } else {
                        self.resource_brush_draw_count -= 1;
                    }
                    self.resource_brush_draws[index] = resource;
                }
            }
        }
        self.resource_brush_draw_count != 0
    }
}

fn grow_paint_layout(
    current: (usize, usize, usize),
    required: (usize, usize, usize),
) -> (usize, usize, usize) {
    let grow = |capacity: usize, live: usize| {
        if live == 0 {
            0
        } else if live > capacity || capacity.saturating_mul(10) > live.saturating_mul(18) {
            live.checked_div(256)
                .and_then(|pages| pages.checked_add(1))
                .and_then(|pages| pages.checked_mul(256))
                .unwrap_or(usize::MAX)
        } else {
            capacity
        }
    };
    (
        grow(current.0, required.0),
        grow(current.1, required.1),
        grow(current.2, required.2),
    )
}

#[cfg(test)]
mod tests;
