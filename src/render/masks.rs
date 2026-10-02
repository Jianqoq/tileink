//! Shared mask cache, resource order and scratch ownership, independent of GPU API.
use crate::Canvas;
use crate::render::{
    damage_tiles::tile_count_for_bounds,
    filter_resources::cursors::FilterCursors,
    operations::{self, Masked},
    output::RenderTargetId,
    retained_surfaces::RetainedSurfaceKind,
    surfaces::SurfaceAdapter,
};
use crate::shared::{
    bounds::Bounds,
    execution::ExecPlan,
    layer::{filter::region_bounds, mask::MaskKind, region::Region},
};

pub(crate) trait MaskAdapter: SurfaceAdapter {
    fn mask_coverage(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        kind: MaskKind,
    ) -> Result<(), Self::Error>;
    fn region_mask(
        &mut self,
        target: RenderTargetId,
        region: &Region,
        path: Option<u32>,
        bounds: Bounds,
    ) -> Result<(), Self::Error>;
    fn apply_region(
        &mut self,
        region_mask: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<(), Self::Error>;
}

pub(crate) fn execute<A: MaskAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    op: Masked<'_>,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
) -> Result<(), A::Error> {
    let size = adapter.size();
    let bounds = region_bounds(&op.layer.region).intersect(Bounds::canvas(size.0, size.1));
    let path_index = cursors.next_path_index(&op.layer.region);
    if bounds.is_empty() {
        cursors.advance_ops(op.content);
        cursors.advance_ops(op.mask);
        return Ok(());
    }
    let meta = op.retained_id.and_then(|id| {
        adapter.retained().surface_meta(
            id,
            RetainedSurfaceKind::Mask,
            size,
            adapter.origin(),
            bounds,
        )
    });
    let mut cached = adapter
        .retained_mut()
        .take_matching_surface(op.retained_id, meta);
    if !adapter.retained().surface_is_dirty(bounds)
        && let Some((id, surface)) = cached.take()
    {
        let result = adapter.composite_cached(
            target,
            &surface.primary,
            surface.secondary.as_ref(),
            bounds,
            op.outer_stack,
            None,
        );
        adapter.retained_mut().stats_mut().reused_offscreen_surfaces += 1;
        adapter.retained_mut().insert_surface(id, surface);
        cursors.advance_ops(op.content);
        cursors.advance_ops(op.mask);
        return result;
    }
    let partial = cached
        .as_ref()
        .is_some_and(|(_, surface)| surface.secondary.is_some());
    if op.retained_id.is_some() {
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
    // Root-cause fix: cached coverage is already occupied when rendering its source
    // fails. One ownership boundary returns every acquired slot on every error.
    // Physical allocations used by earlier submissions remain pinned by the adapter.
    let mut content_slot = None;
    let mut mask_slot = None;
    let mut mask_source_slot = None;
    let mut region_slot = None;
    let result = (|| {
        let content = adapter.acquire_scratch()?;
        content_slot = Some(content);
        if let Some((_, mut surface)) = cached {
            adapter.install_scratch(content, surface.primary);
            if adapter.retained().surface_is_dirty(bounds) {
                adapter.clear_region(content, bounds)?;
            }
            operations::execute_ops(adapter, canvas, plan, op.content, content, cursors, None)?;
            let mask = adapter.acquire_scratch()?;
            mask_slot = Some(mask);
            adapter.install_scratch(
                mask,
                surface
                    .secondary
                    .take()
                    .expect("mask cache has retained mask coverage"),
            );
        } else {
            adapter.clear_target(content)?;
            operations::execute_ops(adapter, canvas, plan, op.content, content, cursors, None)?;
        }
        let mask_source = adapter.acquire_scratch()?;
        mask_source_slot = Some(mask_source);
        adapter.clear_target(mask_source)?;
        operations::execute_ops(adapter, canvas, plan, op.mask, mask_source, cursors, None)?;
        let mask = if let Some(mask) = mask_slot {
            mask
        } else {
            let mask = adapter.acquire_scratch()?;
            mask_slot = Some(mask);
            mask
        };
        if adapter.retained().surface_is_dirty(bounds) {
            adapter.mask_coverage(mask_source, mask, bounds, op.layer.kind)?;
        }
        adapter.release_scratch(mask_source);
        mask_source_slot = None;
        let region = adapter.acquire_scratch()?;
        region_slot = Some(region);
        if adapter.retained().surface_is_dirty(bounds) {
            adapter.region_mask(region, &op.layer.region, path_index, bounds)?;
            adapter.apply_region(region, mask, bounds)?;
        }
        adapter.release_scratch(region);
        region_slot = None;
        let result = adapter.composite_targets(target, content, mask, bounds, op.outer_stack, None);
        if op.retained_id.is_some() && meta.is_some() {
            content_slot = None;
            mask_slot = None;
            let content = adapter.take_scratch(content);
            let mask = adapter.take_scratch(mask);
            if let (Some(content), Some(mask)) = (content, mask) {
                adapter.retained_mut().cache_surface(
                    op.retained_id,
                    meta,
                    content,
                    Some(mask),
                    None,
                );
            }
        }
        result
    })();
    for slot in [region_slot, mask_source_slot, mask_slot, content_slot]
        .into_iter()
        .flatten()
    {
        adapter.release_scratch(slot);
    }
    result
}

#[cfg(test)]
mod tests;
