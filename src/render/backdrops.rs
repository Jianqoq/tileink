//! Painter-order backdrop history and lifecycle, independent of GPU API.
use std::ops::Range;

use crate::{
    Canvas,
    canvas::RetainedSurfaceId,
    render::{
        damage_tiles::tile_count_for_bounds, filter_pass::FilterPassAdapter,
        filter_resources::cursors::FilterCursors, masks::MaskAdapter, operations,
        output::RenderTargetId, retained_surfaces::RetainedSurfaceKind,
    },
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan},
        layer::{
            filter::{Filter, filtered_region_bounds},
            region::Region,
        },
    },
};

pub(crate) struct BackdropLayer<'a> {
    pub(crate) retained_id: Option<RetainedSurfaceId>,
    pub(crate) filter: &'a Filter,
    pub(crate) region: &'a Region,
    pub(crate) stack: Range<usize>,
    pub(crate) children: &'a [ExecOp],
}

pub(crate) struct BackdropPass<'a> {
    pub(crate) source: RenderTargetId,
    pub(crate) target: RenderTargetId,
    pub(crate) bounds: Bounds,
    /// None rebuilds the complete filtered image. Some preserves clean output.
    pub(crate) partial_output: Option<Bounds>,
    pub(crate) filter: &'a Filter,
    pub(crate) region: &'a Region,
}

pub(crate) trait BackdropAdapter: MaskAdapter + FilterPassAdapter {
    type WorkState;
    /// False leaves the target unchanged and permits the ordinary filter path.
    fn try_direct_backdrop(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: &Region,
    ) -> bool;
    fn suspend_backdrop_work(&mut self) -> Self::WorkState;
    fn restore_backdrop_work(&mut self, state: Self::WorkState);
    fn filter_backdrop(
        &mut self,
        pass: BackdropPass<'_>,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error>;
    fn composite_backdrop_rect(
        &mut self,
        target: RenderTargetId,
        source: RenderTargetId,
        bounds: Bounds,
        region: &Region,
    ) -> Result<(), Self::Error>;
    fn composite_cached_backdrop_rect(
        &mut self,
        target: RenderTargetId,
        source: &Self::Surface,
        bounds: Bounds,
        region: &Region,
    ) -> Result<(), Self::Error>;
}

fn active_region<A: BackdropAdapter>(a: &A, bounds: Bounds) -> Option<Bounds> {
    // Preserve exact tile intersection and the original filter coordinate domain.
    // A union of sparse dirty tiles would join clean gaps and scan unrelated rows.
    a.retained().surface_is_dirty(bounds).then_some(bounds)
}

pub(crate) fn execute<A: BackdropAdapter>(
    a: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    layer: BackdropLayer<'_>,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
) -> Result<(), A::Error> {
    let size = a.size();
    let bounds = filtered_region_bounds(layer.filter, layer.region, Bounds::canvas(size.0, size.1));
    let path = cursors.next_path_index(layer.region);
    if bounds.is_empty() {
        // An empty effect does not clip its foreground or break root submission batching.
        cursors.advance_filter(layer.filter);
        return operations::execute_ops(a, canvas, plan, layer.children, target, cursors, None);
    }
    let meta = layer.retained_id.and_then(|id| {
        a.retained()
            .surface_meta(id, RetainedSurfaceKind::Backdrop, size, a.origin(), bounds)
    });
    let mut cached = a
        .retained_mut()
        .take_matching_surface(layer.retained_id, meta);
    // Root-cause fix: an empty outer clip stack does not imply a rectangular sample.
    // Path coverage must be built and retained on both first render and cache reuse.
    let direct_rect = layer.stack.is_empty() && matches!(layer.region, Region::Rect { .. });
    if !a.retained().backdrop_is_dirty(layer.retained_id)
        && let Some((id, surface)) = cached.take()
    {
        let result = active_region(a, bounds).map_or(Ok(()), |output| {
            if direct_rect {
                a.composite_cached_backdrop_rect(target, &surface.primary, output, layer.region)
            } else {
                a.composite_cached(
                    target,
                    &surface.primary,
                    surface.secondary.as_ref(),
                    output,
                    layer.stack.clone(),
                    None,
                )
            }
        });
        a.retained_mut().stats_mut().reused_offscreen_surfaces += 1;
        a.retained_mut().insert_surface(id, surface);
        cursors.advance_filter(layer.filter);
        result?;
        return operations::execute_ops(a, canvas, plan, layer.children, target, cursors, None);
    }
    let partial = cached.as_ref().is_some_and(|(_, surface)| {
        surface.backdrop_source.is_some() && (
            matches!(layer.filter, Filter::Blur { sampling, .. } if sampling.factor() == 1)
            || matches!(layer.filter, Filter::RectLiquidGlass(glass) if glass.blur_sampling.factor() == 1)
        )
    });
    if layer.retained_id.is_some() {
        let tiles = if partial {
            a.retained().active_tiles().map_or_else(
                || tile_count_for_bounds(bounds),
                |tiles| tiles.count_in_bounds(bounds),
            )
        } else {
            tile_count_for_bounds(bounds)
        };
        let stats = a.retained_mut().stats_mut();
        stats.rerendered_offscreen_surfaces += 1;
        stats.rerendered_offscreen_tiles += tiles;
    }
    let bypass = a.retained().bypasses_backdrop_cache();
    if (layer.retained_id.is_none() || bypass)
        && direct_rect
        && a.try_direct_backdrop(target, bounds, layer.filter, layer.region)
    {
        cursors.advance_filter(layer.filter);
        return operations::execute_ops(a, canvas, plan, layer.children, target, cursors, None);
    }
    let cache_surface = layer.retained_id.is_some() && meta.is_some() && !bypass;
    let mut backdrop_slot = None;
    let mut source_slot = None;
    let mut mask_slot = None;
    // A single ownership boundary returns every occupied slot on failure. No
    // partially recorded filter/coverage image can be published as valid history.
    let result = (|| {
        let backdrop = a.acquire_scratch()?;
        backdrop_slot = Some(backdrop);
        let (cached_mask, cached_source) = if let Some((_, mut surface)) = cached {
            a.install_scratch(backdrop, surface.primary);
            (surface.secondary.take(), surface.backdrop_source.take())
        } else {
            (None, None)
        };
        if cache_surface {
            let source = a.acquire_scratch()?;
            source_slot = Some(source);
            // Clean root pixels include the previous backdrop and foreground.
            // Keep their unfiltered painter-order input in an independent image.
            let update = if cached_source.is_some() {
                active_region(a, bounds)
            } else {
                Some(bounds)
            };
            if let Some(source_history) = cached_source {
                a.install_scratch(source, source_history);
            }
            if let Some(update) = update {
                a.copy_filter_region(target, source, update)?;
            }
        }
        let partial_output = partial.then(|| {
            a.retained()
                .active_tiles()
                .and_then(|tiles| tiles.bounds_union(size))
                .unwrap_or(bounds)
                .intersect(bounds)
        });
        let suspended = (!partial).then(|| a.suspend_backdrop_work());
        let filtered = a.filter_backdrop(
            BackdropPass {
                source: source_slot.unwrap_or(target),
                target: backdrop,
                bounds,
                partial_output,
                filter: layer.filter,
                region: layer.region,
            },
            cursors,
        );
        if let Some(state) = suspended {
            a.restore_backdrop_work(state);
        }
        filtered?;
        let composite = if direct_rect {
            active_region(a, bounds).map_or(Ok(()), |output| {
                a.composite_backdrop_rect(target, backdrop, output, layer.region)
            })
        } else {
            let mask = a.acquire_scratch()?;
            mask_slot = Some(mask);
            if let Some(cached_mask) = cached_mask {
                a.install_scratch(mask, cached_mask);
            } else {
                a.region_mask(mask, layer.region, path, bounds)?;
            }
            active_region(a, bounds).map_or(Ok(()), |output| {
                a.composite_targets(target, backdrop, mask, output, layer.stack.clone(), None)
            })
        };
        // A composite error leaves complete source/filter/coverage images valid.
        // Errors during their construction take the cleanup path above instead.
        if cache_surface {
            let output = a
                .take_scratch(backdrop_slot.take().unwrap())
                .expect("occupied backdrop output");
            let history = a
                .take_scratch(source_slot.take().unwrap())
                .expect("occupied backdrop history");
            let mask = mask_slot
                .take()
                .map(|slot| a.take_scratch(slot).expect("occupied backdrop coverage"));
            a.retained_mut()
                .cache_surface(layer.retained_id, meta, output, mask, Some(history));
        }
        composite
    })();
    for slot in [mask_slot, source_slot, backdrop_slot]
        .into_iter()
        .flatten()
    {
        a.release_scratch(slot);
    }
    result?;
    operations::execute_ops(a, canvas, plan, layer.children, target, cursors, None)
}

#[cfg(test)]
mod tests;
