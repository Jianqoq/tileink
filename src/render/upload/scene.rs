//! Shared CPU staging for scene uploads. Both adapters must derive lengths,
//! tile bins and retained capacity from this same preparation state.
use super::{glyph_capacity::GlyphCapacityCache, paint::PaintUploadState, text::TextUpload};
use crate::shared::execution::{ExecPlan, LayerStackEntry};
use crate::{
    Canvas,
    shared::{
        gpu_coarse::LayerStackRecord,
        gpu_plan::{GpuBufferLengths, GpuLengthOverrides, PersistentPathPlans, TileDrawBins},
    },
    text::{PreparedTextChanges, PreparedTextData},
};
use std::{ops::Range, rc::Rc};
#[derive(Default)]
pub(crate) struct SceneUploadStaging {
    pub(crate) text: TextUpload,
    pub(crate) path_plans: PersistentPathPlans,
    glyph_capacity: GlyphCapacityCache,
    coarse_ptcl_capacity: usize,
    coarse_ptcl_underused_frames: u16,
    coarse_glyph_capacity: usize,
    coarse_glyph_underused_frames: u16,
    pub(crate) tile_draw_bins: Rc<TileDrawBins>,
    tile_draw_cursors: Vec<u32>,
    pub(crate) layer_stack: Vec<LayerStackRecord>,
    pub(crate) paint: PaintUploadState,
}

impl SceneUploadStaging {
    /// Returns true when every entry was refreshed and the GPU upload must be full.
    pub(crate) fn refresh_layer_stack(
        &mut self,
        entries: &[LayerStackEntry],
        ranges: Option<&[Range<usize>]>,
    ) -> bool {
        if self.layer_stack.len() != entries.len() || ranges.is_none() {
            self.layer_stack.clear();
            self.layer_stack
                .extend(entries.iter().copied().map(LayerStackRecord::from));
            return true;
        }
        for range in ranges.unwrap() {
            for index in range.clone() {
                self.layer_stack[index] = LayerStackRecord::from(entries[index]);
            }
        }
        false
    }

    pub(crate) fn active_batch_ids(&mut self, tiles: &[u32], draw_batch_ids: &[u32]) -> Vec<u32> {
        Rc::make_mut(&mut self.tile_draw_bins).active_batch_ids(tiles, draw_batch_ids)
    }

    pub(crate) fn build_lengths(
        &mut self,
        canvas: &Canvas,
        text: Option<&PreparedTextData>,
        plan: &ExecPlan,
        reused_plan: bool,
        cached_stack_depths: Option<(usize, usize)>,
        flat_text_changes: Option<&PreparedTextChanges>,
    ) -> GpuBufferLengths {
        // Lengths and tile draw bins must describe the same scene; building both here avoids
        // recounting every draw/tile intersection later in prepare.
        let path_counts = self.path_plans.update(
            canvas,
            canvas
                .buffer_changes
                .as_ref()
                .map(|changes| changes.paths.as_slice()),
        );

        let glyph_capacity = self.glyph_capacity.update(
            canvas,
            text,
            canvas.buffer_changes.as_ref(),
            flat_text_changes,
        );

        let mut lengths = GpuBufferLengths::from_scene_with_text_and_tile_draw_bins(
            canvas,
            text,
            plan,
            Rc::make_mut(&mut self.tile_draw_bins),
            &mut self.tile_draw_cursors,
            reused_plan,
            GpuLengthOverrides {
                path_plan_counts: Some(path_counts),
                coarse_glyph_capacity: Some(glyph_capacity),
                cached_stack_depths,
                ..Default::default()
            },
        );

        if canvas.buffer_changes.is_some() {
            lengths.coarse_ptcl_capacity = stable_work_capacity(
                &mut self.coarse_ptcl_capacity,
                &mut self.coarse_ptcl_underused_frames,
                lengths.coarse_ptcl_capacity,
            );

            lengths.coarse_glyph_capacity = stable_work_capacity(
                &mut self.coarse_glyph_capacity,
                &mut self.coarse_glyph_underused_frames,
                lengths.coarse_glyph_capacity,
            );
        } else {
            self.coarse_ptcl_capacity = lengths.coarse_ptcl_capacity;

            self.coarse_ptcl_underused_frames = 0;

            self.coarse_glyph_capacity = lengths.coarse_glyph_capacity;

            self.coarse_glyph_underused_frames = 0;
        }

        lengths
    }
}

const WORK_CAPACITY_SHRINK_DELAY: u16 = 120;

fn stable_work_capacity(capacity: &mut usize, underused_frames: &mut u16, live: usize) -> usize {
    if live > *capacity {
        *capacity = live.saturating_add(live / 2).max(live);

        *underused_frames = 0;
    } else if capacity.saturating_mul(10) > live.saturating_mul(18) {
        // Shrinking immediately makes alternating layer depth or glyph workloads move every
        // following work-buffer section twice per pair of frames. Require sustained low usage so
        // temporary topology changes retain stable offsets while genuinely smaller scenes still
        // release excess capacity.
        *underused_frames = underused_frames.saturating_add(1);

        if *underused_frames >= WORK_CAPACITY_SHRINK_DELAY {
            *capacity = live.saturating_add(live / 2).max(live);

            *underused_frames = 0;
        }
    } else {
        *underused_frames = 0;
    }

    *capacity
}
#[cfg(test)]
mod tests {
    use super::{SceneUploadStaging, WORK_CAPACITY_SHRINK_DELAY, stable_work_capacity};
    use crate::shared::execution::LayerStackEntry;

    #[test]
    fn layer_stack_refresh_converts_only_dirty_entries() {
        let mut staging = SceneUploadStaging::default();
        let mut entries = (0..1_000)
            .map(|draw| LayerStackEntry::Opacity {
                draw,
                opacity: 0.75,
            })
            .collect::<Vec<_>>();
        assert!(staging.refresh_layer_stack(&entries, None));
        let unchanged = staging.layer_stack[499].payload;
        entries[500] = LayerStackEntry::Opacity {
            draw: 500,
            opacity: 0.25,
        };
        let dirty = std::iter::once(500..501).collect::<Vec<_>>();
        assert!(!staging.refresh_layer_stack(&entries, Some(&dirty)));
        assert_eq!(staging.layer_stack[499].payload, unchanged);
        assert_eq!(
            staging.layer_stack[500].payload,
            u32::from(crate::shared::pixel::opacity_f32_to_u8(0.25))
        );
        assert!(!staging.refresh_layer_stack(&entries, Some(&[])));
        entries.push(LayerStackEntry::Clip { draw: 1_000 });
        assert!(staging.refresh_layer_stack(&entries, Some(&[])));
        assert_eq!(staging.layer_stack.len(), entries.len());
    }
    #[test]
    fn work_capacity_does_not_thrash_under_alternating_layer_depth() {
        let mut capacity = 1_000;
        let mut underused = 0;
        for _ in 0..WORK_CAPACITY_SHRINK_DELAY * 2 {
            assert_eq!(
                stable_work_capacity(&mut capacity, &mut underused, 400),
                1_000
            );
            assert_eq!(
                stable_work_capacity(&mut capacity, &mut underused, 900),
                1_000
            );
            assert_eq!(underused, 0);
        }
    }
    #[test]
    fn work_capacity_releases_sustained_excess_capacity() {
        let mut capacity = 1_000;
        let mut underused = 0;
        for _ in 1..WORK_CAPACITY_SHRINK_DELAY {
            assert_eq!(
                stable_work_capacity(&mut capacity, &mut underused, 400),
                1_000
            );
        }
        assert_eq!(
            stable_work_capacity(&mut capacity, &mut underused, 400),
            600
        );
        assert_eq!(underused, 0);

        assert_eq!(
            stable_work_capacity(&mut capacity, &mut underused, 800),
            1_200
        );
        assert_eq!(underused, 0);
    }
}
