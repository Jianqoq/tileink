//! A surface's backdrop is captured before its own opacity isolation; descendants remain isolated.
use super::{BackdropAdapter, BackdropLayer, execute_to};
use crate::{
    Canvas, Radius, Region,
    render::{filter_resources::cursors::FilterCursors, output::RenderTargetId},
    shared::{bounds::Bounds, execution::ExecPlan},
};

pub(super) fn execute<A: BackdropAdapter>(
    a: &mut A,
    canvas: &Canvas,
    plan: &ExecPlan,
    layer: &BackdropLayer<'_>,
    target: RenderTargetId,
    cursors: &mut FilterCursors,
    opacity: f32,
) -> Result<(), A::Error> {
    let size = a.size();
    let bounds = Bounds::canvas(size.0, size.1);
    let region = Region::rect(
        peniko::kurbo::Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1)),
        Radius::ZERO,
    );
    let source = a.acquire_scratch()?;
    let mut mask_slot = None;
    let result = (|| {
        a.clear_target(source)?;
        // Reuse only the background filter/history cache. Foreground and fade never enter it,
        // preventing cached opacity from accumulating and old foreground from sampling itself.
        let inner = BackdropLayer {
            retained_id: layer.retained_id,
            filter: layer.filter,
            region: layer.region,
            opacity: None,
            stack: 0..0,
            children: layer.children,
        };
        execute_to(a, canvas, plan, &inner, target, source, cursors)?;
        if opacity < 1.0 {
            let work = a.suspend_backdrop_work();
            let faded = a.apply_opacity(source, bounds, opacity);
            a.restore_backdrop_work(work);
            faded?;
        }
        if layer.stack.is_empty() {
            a.composite_backdrop_rect(target, source, bounds, &region)
        } else {
            let mask = a.acquire_scratch()?;
            mask_slot = Some(mask);
            a.region_mask(mask, &region, None, bounds)?;
            a.composite_targets(target, source, mask, bounds, layer.stack.clone(), None)
        }
    })();
    if let Some(mask) = mask_slot {
        a.release_scratch(mask);
    }
    a.release_scratch(source);
    result
}
