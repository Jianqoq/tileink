//! Group cache and execution order shared by the GPU adapters.

use std::ops::Range;

use super::{
    damage_tiles::tile_count_for_bounds, filter_resources::cursors::FilterCursors, operations,
    output::RenderTargetId, retained_surfaces::RetainedSurfaceKind,
};
use crate::{
    Canvas,
    canvas::RetainedSurfaceId,
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan},
    },
};

pub(crate) struct Group<'a> {
    pub(crate) retained_id: Option<RetainedSurfaceId>,
    pub(crate) draw: usize,
    pub(crate) outer_stack: Range<usize>,
    pub(crate) children: &'a [ExecOp],
    pub(crate) opacity: Option<f32>,
    pub(crate) blend: Option<peniko::BlendMode>,
}

/// Group-specific GPU work; cache and scratch operations are shared with masks.
pub(crate) trait GroupAdapter: super::surfaces::SurfaceAdapter {
    fn apply_opacity(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        opacity: f32,
    ) -> Result<(), Self::Error>;
    fn build_mask(
        &mut self,
        target: RenderTargetId,
        draw: u32,
        bounds: Bounds,
    ) -> Result<(), Self::Error>;
}

#[allow(
    clippy::expect_used,
    reason = "Matched masked group caches always contain retained mask coverage"
)]
pub(crate) fn execute<A: GroupAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    group: Group<'_>,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
) -> Result<(), A::Error> {
    let size = adapter.size();
    let bounds = draw_bounds(canvas, group.draw).intersect(Bounds::canvas(size.0, size.1));
    if bounds.is_empty() {
        // Resource tables follow the complete plan even when a group has no pixels.
        cursors.advance_ops(group.children);
        return Ok(());
    }
    let meta = group.retained_id.and_then(|id| {
        adapter.retained().surface_meta(
            id,
            RetainedSurfaceKind::Group,
            size,
            adapter.origin(),
            bounds,
        )
    });
    let mut cached = adapter
        .retained_mut()
        .take_matching_surface(group.retained_id, meta);
    if !adapter.retained().surface_is_dirty(bounds)
        && let Some((id, surface)) = cached.take()
    {
        let result = adapter.composite_cached(
            target,
            &surface.primary,
            surface.secondary.as_ref(),
            bounds,
            group.outer_stack,
            group.blend,
        );
        adapter.retained_mut().stats_mut().reused_offscreen_surfaces += 1;
        adapter.retained_mut().insert_surface(id, surface);
        cursors.advance_ops(group.children);
        return result;
    }
    let partial = cached
        .as_ref()
        .is_some_and(|(_, surface)| surface.secondary.is_some());
    if group.retained_id.is_some() {
        let tiles = if partial {
            adapter.retained().active_tiles().map_or_else(
                || tile_count_for_bounds(bounds),
                |tiles| tiles.count_in_bounds(bounds),
            )
        } else {
            tile_count_for_bounds(bounds)
        };
        let stats = adapter.retained_mut().stats_mut();
        stats.rerendered_offscreen_surfaces += 1;
        stats.rerendered_offscreen_tiles += tiles;
    }
    // Root-cause fix: every fallible recording exit returns its logical scratch
    // slots. Adapters retain submitted GPU-use leases independently until completion.
    let mut source_slot = None;
    let mut mask_slot = None;
    let result = (|| {
        let source = adapter.acquire_scratch()?;
        source_slot = Some(source);
        let retained_mask = if let Some((_, surface)) = cached {
            adapter.install_scratch(source, surface.primary);
            if adapter.retained().surface_is_dirty(bounds) {
                adapter.clear_region(source, bounds)?;
            }
            Some(surface.secondary.expect("group cache has a retained mask"))
        } else {
            adapter.clear_target(source)?;
            None
        };
        operations::execute_ops(adapter, canvas, plan, group.children, source, cursors, None)?;
        if let Some(retained_mask) = retained_mask {
            let mask = adapter.acquire_scratch()?;
            mask_slot = Some(mask);
            adapter.install_scratch(mask, retained_mask);
        }
        if let Some(opacity) = group.opacity
            && adapter.retained().surface_is_dirty(bounds)
        {
            adapter.apply_opacity(source, bounds, opacity)?;
        }
        let mask = if let Some(mask) = mask_slot {
            mask
        } else {
            let mask = adapter.acquire_scratch()?;
            mask_slot = Some(mask);
            mask
        };
        if adapter.retained().surface_is_dirty(bounds) {
            adapter.build_mask(mask, group.draw as u32, bounds)?;
        }
        let result =
            adapter.composite_targets(target, source, mask, bounds, group.outer_stack, group.blend);
        if group.retained_id.is_some() && meta.is_some() {
            source_slot = None;
            mask_slot = None;
            let source = adapter.take_scratch(source);
            let mask = adapter.take_scratch(mask);
            adapter
                .retained_mut()
                .cache_surface(group.retained_id, meta, source, Some(mask), None);
        }
        result
    })();
    if let Some(mask) = mask_slot {
        adapter.release_scratch(mask);
    }
    if let Some(source) = source_slot {
        adapter.release_scratch(source);
    }
    result
}

pub(crate) fn draw_bounds(canvas: &Canvas, draw_ix: usize) -> Bounds {
    let bounds = canvas.draw_records[draw_ix].pixel_bounds;
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

#[cfg(test)]
mod tests;
