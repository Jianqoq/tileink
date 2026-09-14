//! Filter source/history transitions shared by GPU adapters. The caller owns
//! source and history surfaces; this pass owns only its temporary partial surface.
use super::{
    damage_tiles::DamageTiles, filter_resources::cursors::FilterCursors, output::RenderTargetId,
    surfaces::SurfaceAdapter,
};
use crate::shared::{
    bounds::Bounds,
    layer::filter::{Filter, filter_dependency},
};

pub(crate) enum FilterHistory {
    None,
    Capture(RenderTargetId),
    Partial {
        filtered: RenderTargetId,
        update: Bounds,
        damage: DamageTiles,
    },
}

pub(crate) trait FilterPassAdapter: SurfaceAdapter {
    fn copy_filter_region(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> Result<(), Self::Error>;
    fn apply_filter_pass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        cursors: &mut FilterCursors,
    ) -> Result<(), Self::Error>;
    fn prepare_filter_output_work(&mut self) -> Result<(), Self::Error>;
}

pub(crate) fn execute<A: FilterPassAdapter>(
    adapter: &mut A,
    source: RenderTargetId,
    bounds: Bounds,
    filter: &Filter,
    history: FilterHistory,
    cursors: &mut FilterCursors,
) -> Result<(), A::Error> {
    match history {
        FilterHistory::None => adapter.apply_filter_pass(source, bounds, filter, cursors),
        FilterHistory::Capture(history) => {
            adapter.copy_filter_region(source, history, bounds)?;
            adapter.apply_filter_pass(source, bounds, filter, cursors)
        }
        FilterHistory::Partial {
            filtered,
            update,
            damage,
        } => {
            let process = update
                .outset(
                    filter_dependency(filter)
                        .local_radius()
                        .expect("partial filter reads a local neighbourhood"),
                )
                .intersect(bounds);
            let temp = adapter.acquire_scratch()?;
            let filtering = adapter
                .copy_filter_region(source, temp, process)
                .and_then(|()| adapter.apply_filter_pass(temp, process, filter, cursors));
            // Source dependencies include halo tiles. Always restore the output
            // worklist before updating retained history, including failed recording.
            adapter.retained_mut().set_active_tiles(Some(damage));
            let output_work = adapter.prepare_filter_output_work();
            let result = filtering
                .and(output_work)
                .and_then(|()| adapter.copy_filter_region(temp, filtered, update));
            // Every fallible recording step above shares this ownership boundary.
            // The adapter separately pins submitted physical allocations.
            adapter.release_scratch(temp);
            result
        }
    }
}

#[cfg(test)]
mod tests;
