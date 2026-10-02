//! Isolated filter scheduling, retained history and local resource ownership.
use super::{
    damage_tiles::DamageTiles,
    damage_tiles::tile_count_for_bounds,
    filter_pass::{self, FilterHistory, FilterPassAdapter},
    filter_resources::cursors::FilterCursors,
    filter_scene::PreparedFilterScene,
    operations,
    output::RenderTargetId,
    retained_surfaces::RetainedSurfaceKind,
};
use crate::{
    canvas::{Canvas, RetainedSurfaceId},
    shared::{
        bounds::Bounds,
        execution::{ExecOp, ExecPlan},
        layer::{
            filter::{Filter, filter_dependency, filter_surface_bounds},
            region::Region,
        },
    },
};
use std::ops::Range;

pub(crate) struct FilterLayer<'a> {
    pub(crate) retained_id: Option<RetainedSurfaceId>,
    pub(crate) filter: &'a Filter,
    pub(crate) region: &'a Region,
    pub(crate) stack: Range<usize>,
    pub(crate) children: &'a [ExecOp],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FilterPlacement {
    pub(crate) size: (u32, u32),
    pub(crate) origin: (i32, i32),
    pub(crate) bounds: Bounds,
}

pub(crate) trait FilterAdapter: FilterPassAdapter {
    type LocalState;
    fn filter_candidates(&self, bounds: Bounds, plan: &ExecPlan) -> Vec<u32>;
    /// On failure the previous context must remain active. A returned state is
    /// consumed exactly once by end_filter_scene, including recording failure.
    fn begin_filter_scene(
        &mut self,
        canvas: &Canvas,
        plan: &ExecPlan,
        filter: &Filter,
        scratch: usize,
        origin: (i32, i32),
        reuse_root: bool,
    ) -> Result<Self::LocalState, Self::Error>;
    /// Release all logical local scratch slots and restore the previous context.
    /// Submitted physical resources must remain pinned by the GPU adapter.
    fn end_filter_scene(&mut self, state: Self::LocalState);
    fn prepare_filter_source_work(&mut self) -> Result<(), Self::Error>;
    fn scan_filter_scene(&mut self, canvas: &Canvas) -> Result<(), Self::Error>;
    fn composite_filter(
        &mut self,
        target: RenderTargetId,
        source: &Self::Surface,
        placement: FilterPlacement,
        stack: Range<usize>,
    ) -> Result<(), Self::Error>;
}

pub(crate) fn execute<A: FilterAdapter>(
    adapter: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    layer: FilterLayer<'_>,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
) -> Result<(), A::Error> {
    let root_size = adapter.size();
    let Some(bounds) = filter_surface_bounds(
        layer.filter,
        layer.region,
        Bounds::canvas(root_size.0, root_size.1),
    ) else {
        cursors.advance_filter_layer(layer.region, layer.children, layer.filter);
        return Ok(());
    };
    let root_cursors = cursors.clone();
    cursors.advance_filter_layer(layer.region, layer.children, layer.filter);
    let placement = FilterPlacement {
        size: (bounds.surface.width(), bounds.surface.height()),
        origin: (bounds.surface.x0, bounds.surface.y0),
        bounds: bounds.output,
    };
    let meta = layer.retained_id.and_then(|id| {
        adapter.retained().surface_meta(
            id,
            RetainedSurfaceKind::Filter,
            placement.size,
            placement.origin,
            placement.bounds,
        )
    });
    let mut cached = adapter
        .retained_mut()
        .take_matching_surface(layer.retained_id, meta);
    if !adapter.retained().surface_is_dirty(bounds.output)
        && let Some((id, surface)) = cached.take()
    {
        let result = adapter.composite_filter(target, &surface.primary, placement, layer.stack);
        adapter.retained_mut().stats_mut().reused_offscreen_surfaces += 1;
        adapter.retained_mut().insert_surface(id, surface);
        return result;
    }
    let local_damage = cached
        .as_ref()
        // Whole-region addressing cannot use a cropped partial pass. Keep the
        // clean-cache fast path above; dirty work reconstructs this local domain.
        .filter(|(_, surface)| {
            surface.secondary.is_some() && filter_dependency(layer.filter).local_radius().is_some()
        })
        .and_then(|_| {
            adapter
                .retained()
                .local_damage_for_surface(bounds.surface, root_size)
        });
    let prepared = PreparedFilterScene::new(
        canvas,
        plan,
        layer.children,
        layer.filter,
        bounds.surface,
        || adapter.filter_candidates(bounds.surface, plan),
    );
    let (local_canvas, local_plan, local_children) = prepared.scene();
    let local_filter = prepared.filter();
    let local_bounds = Bounds::canvas(placement.size.0, placement.size.1);
    let parent_origin = adapter.origin();
    let local_origin = (
        parent_origin.0 + bounds.surface.x0,
        parent_origin.1 + bounds.surface.y0,
    );
    let cache_surface = layer.retained_id.is_some() && meta.is_some();
    let reuse_root =
        prepared.is_root_scene() && target == RenderTargetId::Main && parent_origin == local_origin;
    let state = adapter.begin_filter_scene(
        local_canvas,
        local_plan,
        local_filter,
        prepared.scratch_count(cache_surface),
        local_origin,
        reuse_root,
    )?;
    // One exit boundary restores the local context on every recording failure.
    // Failed source/filter work must never publish a partially updated cache image.
    let rendered = (|| {
        let source = adapter.acquire_scratch()?;
        let partial = if let (Some((_, mut surface)), Some(damage)) = (cached, local_damage) {
            adapter.install_scratch(
                source,
                surface
                    .secondary
                    .take()
                    .expect("partial filter cache has source history"),
            );
            let filtered = adapter.acquire_scratch()?;
            adapter.install_scratch(filtered, surface.primary);
            let update = damage.bounds_union(placement.size).unwrap_or(local_bounds);
            adapter.retained_mut().set_active_tiles(Some(
                damage.outset(
                    placement.size,
                    filter_dependency(local_filter)
                        .local_radius()
                        .expect("partial filter reads a local neighbourhood"),
                ),
            ));
            adapter.prepare_filter_source_work()?;
            adapter.clear_region(source, local_bounds)?;
            Some((filtered, update, damage))
        } else {
            adapter.clear_target(source)?;
            None
        };
        adapter.scan_filter_scene(local_canvas)?;
        // Fix the resource-layout mismatch at its source: uploading a complete
        // plan into another GPU pool does not compact its table indices. Paths
        // are preorder, so children also start after this filter's sample path.
        let mut local_cursors = if prepared.is_root_scene() {
            let mut cursors = root_cursors;
            cursors.next_path_index(layer.region);
            cursors
        } else {
            FilterCursors::default()
        };
        operations::execute_ops(
            adapter,
            local_canvas,
            local_plan,
            local_children,
            source,
            &mut local_cursors,
            None,
        )?;
        let (history, output, source_history) = if let Some((filtered, update, damage)) = partial {
            (
                FilterHistory::Partial {
                    filtered,
                    update,
                    damage,
                },
                filtered,
                Some(source),
            )
        } else if cache_surface {
            let history = adapter.acquire_scratch()?;
            (FilterHistory::Capture(history), source, Some(history))
        } else {
            (FilterHistory::None, source, None)
        };
        filter_pass::execute(
            adapter,
            source,
            local_bounds,
            local_filter,
            history,
            &mut local_cursors,
        )?;
        let tiles = adapter
            .retained()
            .active_tiles()
            .map_or_else(|| tile_count_for_bounds(local_bounds), DamageTiles::len);
        let output = adapter
            .take_scratch(output)
            .expect("recorded filter output owns its scratch slot");
        let source_history = source_history.map(|target| {
            adapter
                .take_scratch(target)
                .expect("recorded filter history owns its scratch slot")
        });
        Ok((output, source_history, tiles))
    })();
    adapter.end_filter_scene(state);
    let (output, history, tiles) = rendered?;
    if layer.retained_id.is_some() {
        let stats = adapter.retained_mut().stats_mut();
        stats.rerendered_offscreen_surfaces += 1;
        stats.rerendered_offscreen_tiles += tiles;
    }
    let result = adapter.composite_filter(target, &output, placement, layer.stack);
    adapter
        .retained_mut()
        .cache_surface(layer.retained_id, meta, output, history, None);
    result
}

#[cfg(test)]
mod tests;
