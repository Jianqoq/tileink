//! Retained damage decisions for the native renderer.
use super::damage_tiles::DamageTiles;
use crate::{
    TILE_SIZE,
    canvas::{RetainedDamage, RetainedFrame},
    shared::bounds::Bounds,
};
use std::rc::Rc;
mod config;
mod frame_diff;
pub use config::{
    FullRedrawReason, IncrementalOutputMode, IncrementalRenderConfig, IncrementalRenderMode,
    IncrementalRenderStats,
};
#[cfg(test)]
use frame_diff::diff_frames;
use frame_diff::{FrameDiffScratch, diff_frames_reusing};

pub(crate) struct DamagePlan {
    pub(crate) tiles: DamageTiles,
    pub(crate) changed_tiles: DamageTiles,
    pub(crate) retained_damage: RetainedDamage,
    pub(crate) stats: IncrementalRenderStats,
    pub(crate) frame: Option<RetainedFrame>,
    /// `Some` means the consumed persistent delta already expanded backdrop output damage and
    /// identified every retained backdrop surface that must be rerendered.
    pub(crate) dirty_backdrops: Option<std::rc::Rc<[crate::RetainedNodeId]>>,
}

impl DamagePlan {
    pub(crate) fn include_dependent_bounds(
        &mut self,
        bounds: impl IntoIterator<Item = Bounds>,
        config: IncrementalRenderConfig,
    ) {
        for bounds in bounds {
            self.changed_tiles.add_bounds(bounds);
        }
        self.stats.changed_tiles = self.changed_tiles.len();
        self.stats.changed_ratio = tile_ratio(&self.changed_tiles);
        if !self.stats.full_redraw && self.stats.changed_ratio >= config.full_redraw_ratio {
            let (tiles_width, tiles_height) = self.changed_tiles.dimensions();
            let size = (tiles_width * TILE_SIZE, tiles_height * TILE_SIZE);
            self.tiles = DamageTiles::full(size);
            self.stats.full_redraw = true;
            self.stats.full_redraw_reason = Some(FullRedrawReason::DirtyTileThreshold);
        } else if !self.stats.full_redraw {
            self.tiles = self.changed_tiles.clone();
        }
        self.stats.dirty_tiles = self.tiles.len();
        self.stats.dirty_ratio = tile_ratio(&self.tiles);
    }
}

fn tile_ratio(tiles: &DamageTiles) -> f32 {
    if tiles.total_tiles() == 0 {
        0.0
    } else {
        tiles.len() as f32 / tiles.total_tiles() as f32
    }
}

#[derive(Default)]
pub(crate) struct IncrementalState {
    previous: Option<RetainedFrame>,
    renderer_state_invalid: bool,
    frame_diff: FrameDiffScratch,
}

impl IncrementalState {
    pub(crate) const fn invalidate_renderer_state(&mut self) {
        self.renderer_state_invalid = true;
    }

    pub(crate) fn plan(
        &mut self,
        frame: Option<RetainedFrame>,
        physical_size: (u32, u32),
        config: IncrementalRenderConfig,
        history_valid: bool,
    ) -> DamagePlan {
        let size = physical_size;
        let mut stats = IncrementalRenderStats {
            total_tiles: size.0.div_ceil(TILE_SIZE) * size.1.div_ceil(TILE_SIZE),
            tiles_width: size.0.div_ceil(TILE_SIZE),
            tiles_height: size.1.div_ceil(TILE_SIZE),
            retained_nodes: frame.as_ref().map_or(0, |frame| frame.nodes.len() as u32),
            ..Default::default()
        };

        let resolved =
            frame
                .as_ref()
                .zip(self.previous.as_ref())
                .map_or(Ok(None), |(current, previous)| {
                    current
                        .damage_history
                        .resolve(previous.version, current.version)
                });
        let reason = self
            .full_reason(frame.as_ref(), config.mode, history_valid)
            .or_else(|| {
                resolved
                    .is_err()
                    .then_some(FullRedrawReason::DamageHistoryUnavailable)
            });
        let dirty_backdrops = if let Ok(Some(damage)) = &resolved {
            Some(std::rc::Rc::clone(&damage.dirty_backdrops))
        } else {
            frame.as_ref().and_then(|current| {
                let previous_version = self.previous.as_ref()?.version;
                let delta = current.delta.as_ref()?;
                (delta.backdrop_damage_complete
                    && delta.from_version == previous_version
                    && current.version == delta.to_version
                    && !current.invalidate_all)
                    .then(|| std::rc::Rc::clone(&delta.dirty_backdrops))
            })
        };
        let (changed_tiles, retained_damage) = if let Ok(Some(damage)) = &resolved {
            let mut tiles = DamageTiles::new(size);
            for &bounds in &damage.bounds {
                tiles.add_bounds(bounds);
            }
            (tiles, RetainedDamage::default())
        } else {
            self.scene_damage(frame.as_ref(), size)
        };
        let threshold_exceeded =
            reason.is_none() && tile_ratio(&changed_tiles) >= config.full_redraw_ratio;
        let reason = if threshold_exceeded {
            Some(FullRedrawReason::DirtyTileThreshold)
        } else {
            reason
        };
        let tiles = if reason.is_some() {
            DamageTiles::full(size)
        } else {
            changed_tiles.clone()
        };

        stats.full_redraw = reason.is_some();
        stats.full_redraw_reason = reason;
        stats.dirty_tiles = tiles.len();
        stats.dirty_ratio = tile_ratio(&tiles);
        stats.changed_tiles = changed_tiles.len();
        stats.changed_ratio = tile_ratio(&changed_tiles);
        DamagePlan {
            tiles,
            changed_tiles,
            retained_damage,
            stats,
            frame,
            dirty_backdrops,
        }
    }

    fn scene_damage(
        &mut self,
        frame: Option<&RetainedFrame>,
        physical_size: (u32, u32),
    ) -> (DamageTiles, RetainedDamage) {
        let Some(current) = frame else {
            return (DamageTiles::full(physical_size), RetainedDamage::default());
        };
        let Some(previous) = self.previous.as_ref() else {
            return (DamageTiles::full(physical_size), RetainedDamage::default());
        };
        if self.renderer_state_invalid
            || previous.root != current.root
            || previous.logical_size != current.logical_size
            || previous.physical_size != current.physical_size
            || previous.scale_bits != current.scale_bits
            || current.invalidate_all
        {
            return (DamageTiles::full(physical_size), RetainedDamage::default());
        }

        if let Some(delta) = &current.delta
            && delta.from_version == previous.version
            && current.version == delta.to_version
        {
            let mut tiles = DamageTiles::new(physical_size);
            let mut retained = RetainedDamage::default();
            // Indexed deltas already contain dependency damage, and dependency-free
            // frames never run the command-tree oracle. Building its unused source
            // lists made bulk removals pay quadratic rectangle deduplication costs.
            let needs_sources =
                current.requires_damage_propagation && !delta.backdrop_damage_complete;
            for patch in delta.patches.iter() {
                if patch.old != patch.new {
                    for region in patch.damage_regions() {
                        tiles.add_bounds(region);
                    }
                    if !needs_sources {
                        continue;
                    }
                    let Some(bounds) = patch.damage_bounds() else {
                        continue;
                    };
                    if let Some(node) = patch.new {
                        retained.add_node(node.id, bounds);
                    } else {
                        // A removed node has no command in the current painter order.
                        // Match frame diff semantics: its old pixels still invalidate
                        // downstream filters and cached backdrop surfaces.
                        retained.add_unattributed(bounds);
                    }
                }
            }
            for &(id, bounds) in delta.damage.iter() {
                tiles.add_bounds(bounds);
                if needs_sources {
                    retained.add_node(id, bounds);
                }
            }
            for &bounds in &current.invalidated_bounds {
                tiles.add_bounds(bounds);
                if needs_sources {
                    retained.add_unattributed(bounds);
                }
            }
            return (tiles, retained);
        }

        // Shared base nodes can still have different overlay revisions after a rejected
        // submission. Only the same version can skip comparing those logical states.
        if previous.version == current.version && Rc::ptr_eq(&previous.nodes, &current.nodes) {
            let mut tiles = DamageTiles::new(physical_size);
            let mut retained = RetainedDamage::default();
            for &bounds in &current.invalidated_bounds {
                tiles.add_bounds(bounds);
                retained.add_unattributed(bounds);
            }
            return (tiles, retained);
        }

        let mut tiles = DamageTiles::new(physical_size);
        let mut retained = RetainedDamage::default();
        diff_frames_reusing(
            previous,
            current,
            &mut tiles,
            &mut retained,
            &mut self.frame_diff,
        );
        for bounds in &current.invalidated_bounds {
            tiles.add_bounds(*bounds);
            retained.add_unattributed(*bounds);
        }
        (tiles, retained)
    }

    pub(crate) fn commit(&mut self, frame: Option<RetainedFrame>) {
        self.previous = frame;
        self.renderer_state_invalid = false;
    }

    fn full_reason(
        &self,
        frame: Option<&RetainedFrame>,
        mode: IncrementalRenderMode,
        history_valid: bool,
    ) -> Option<FullRedrawReason> {
        if mode == IncrementalRenderMode::ForceFull {
            return Some(FullRedrawReason::Forced);
        }
        let Some(frame) = frame else {
            return Some(FullRedrawReason::NonRetainedCanvas);
        };
        if self.renderer_state_invalid && self.previous.is_some() {
            return Some(FullRedrawReason::RendererStateChanged);
        }
        let Some(previous) = self.previous.as_ref() else {
            return Some(FullRedrawReason::FirstFrame);
        };
        if previous.root != frame.root {
            return Some(FullRedrawReason::RootChanged);
        }
        if previous.logical_size != frame.logical_size
            || previous.physical_size != frame.physical_size
            || previous.scale_bits != frame.scale_bits
        {
            return Some(FullRedrawReason::SurfaceChanged);
        }
        if frame.invalidate_all {
            return Some(FullRedrawReason::ExplicitInvalidation);
        }
        if !history_valid {
            return Some(FullRedrawReason::FirstFrame);
        }
        None
    }
}

#[cfg(test)]
mod tests;
