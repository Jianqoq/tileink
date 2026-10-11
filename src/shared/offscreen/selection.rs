//! Select current spatial candidates and the hidden records their layer stacks own.
use super::*;

pub(super) struct LocalDrawSelection {
    pub(super) draws: Vec<usize>,
    pub(super) batch_draws: HashMap<u32, Vec<usize>>,
}

pub(super) fn local_draw_selection(
    canvas: &Canvas,
    plan: &ExecPlan,
    children: &[ExecOp],
    candidates: &[u32],
    bounds: Bounds,
) -> LocalDrawSelection {
    let mut batches = HashMap::new();
    let mut hidden = HashSet::new();
    collect_local_plan_draws(
        children,
        &plan.layer_stack_data,
        bounds,
        &mut batches,
        &mut hidden,
    );
    let mut draws = Vec::new();
    let mut seen = HashSet::new();
    let batch_ids = canvas.raster_batch_ids(plan);
    // Every requested batch has an explicit selection, including empty batches.
    for &draw in candidates {
        let draw = draw as usize;
        let record = &canvas.draw_records[draw];
        if let Some(request) = batches.get_mut(&batch_ids[draw])
            && pixel_bounds_overlap(record.pixel_bounds, request.bounds)
            && seen.insert(draw)
        {
            draws.push(draw);
            request.selected.push(draw);
        }
    }
    // Spatial bins cover the root canvas and the original query only. Nested
    // filters can read beyond both. Supplement just those batches, in their
    // original painter order; ordinary bounded batches keep the indexed path.
    let indexed = bounds.intersect(Bounds::canvas(
        canvas.physical_width(),
        canvas.physical_height(),
    ));
    for request in batches.values_mut() {
        if request.bounds.intersect(indexed) == request.bounds {
            continue;
        }
        request.selected.clear();
        for &draw in request.draws {
            if pixel_bounds_overlap(canvas.draw_records[draw].pixel_bounds, request.bounds) {
                if seen.insert(draw) {
                    draws.push(draw);
                }
                request.selected.push(draw);
            }
        }
    }
    for draw in hidden {
        if seen.insert(draw) {
            draws.push(draw);
        }
    }
    let batch_draws = batches
        .into_iter()
        .map(|(id, request)| (id, request.selected))
        .collect();
    LocalDrawSelection { draws, batch_draws }
}

struct LocalBatchInput<'a> {
    bounds: Bounds,
    draws: &'a [usize],
    selected: Vec<usize>,
}

fn collect_local_plan_draws<'a>(
    ops: &'a [ExecOp],
    layer_stacks: &[crate::shared::execution::LayerStackEntry],
    bounds: Bounds,
    batches: &mut HashMap<u32, LocalBatchInput<'a>>,
    hidden: &mut HashSet<usize>,
) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                batch_id,
                layer_stack,
                draws,
                ..
            } => {
                batches
                    .entry(*batch_id)
                    .and_modify(|input| input.bounds = input.bounds.union(bounds))
                    .or_insert(LocalBatchInput {
                        bounds,
                        draws,
                        selected: Vec::new(),
                    });
                collect_layer_stack_draws(layer_stacks, layer_stack.clone(), hidden);
            }
            ExecOp::OffscreenLayer {
                draw,
                outer_stack,
                children,
                layer,
                ..
            } => {
                if let Some(draw) = layer_draw(layer, *draw) {
                    hidden.insert(draw);
                }
                collect_layer_stack_draws(layer_stacks, outer_stack.clone(), hidden);
                let source = if let Layer::Filter {
                    filter,
                    sample_region,
                } = layer
                {
                    crate::shared::layer::filter::filter_surface_bounds(
                        filter,
                        sample_region,
                        bounds,
                    )
                    .map_or(Bounds::new(0, 0, 0, 0), |surface| surface.surface)
                } else {
                    bounds
                };
                collect_local_plan_draws(children, layer_stacks, source, batches, hidden);
            }
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                collect_layer_stack_draws(layer_stacks, outer_stack.clone(), hidden);
                collect_local_plan_draws(content, layer_stacks, bounds, batches, hidden);
                collect_local_plan_draws(mask, layer_stacks, bounds, batches, hidden);
            }
        }
    }
}

fn collect_layer_stack_draws(
    entries: &[crate::shared::execution::LayerStackEntry],
    range: std::ops::Range<usize>,
    draws: &mut HashSet<usize>,
) {
    for entry in &entries[range] {
        let draw = match entry {
            crate::shared::execution::LayerStackEntry::Clip { draw }
            | crate::shared::execution::LayerStackEntry::Opacity { draw, .. }
            | crate::shared::execution::LayerStackEntry::Blend { draw, .. } => *draw,
        };
        draws.insert(draw as usize);
    }
}

fn pixel_bounds_overlap(bounds: PixelBounds, region: Bounds) -> bool {
    bounds.x0 < region.x1 && bounds.x1 > region.x0 && bounds.y0 < region.y1 && bounds.y1 > region.y0
}
