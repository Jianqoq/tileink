//! Records one frame in its caller-owned batch. Native roots render in place;
//! submission, vector uploads and output copies belong to their actual owners.
use super::*;
use crate::render::{damage_tiles::DamageTiles, draw_batches::execute_in_place_root_batches};

pub(super) fn record<'gpu>(
    cache: &mut SceneCache,
    batch: &'gpu mut ComputeBatch,
    canvas: &Canvas,
    resources: FrameResources<'gpu>,
    options: FrameOptions,
    limit: u32,
) -> Result<ResourceId> {
    // Reject foreign image associations before recording any scene resources.
    resources.images.validate(batch)?;
    let prepared = cache.prepare(canvas);
    if resources
        .retained
        .active_tiles()
        .is_some_and(DamageTiles::is_empty)
    {
        return options
            .target
            .ok_or_else(|| "empty damage requires persistent native history".into());
    }
    let partial = resources.retained.active_tiles().is_some();
    if partial && options.target.is_none() {
        return Err("native partial frame requires persistent history".into());
    }
    let plan = prepared.plan_handle();
    let mut execution = Execution::prepare(prepared, batch, resources, options, limit)?;
    if options.target.is_some() {
        let (width, height) = canvas.physical_size();
        super::super::program::filter::encode(
            execution.batch,
            super::super::program::filter::BasicFilter::Clear,
            crate::shared::filter_config::FilterConfig {
                width,
                height,
                region_width: width,
                region_height: height,
                clear_color: options.clear_color,
                ..Default::default()
            },
            execution.retained.active_tiles().map(DamageTiles::list),
            None,
            execution.targets.get(RenderTargetId::Main)?.image(),
        )?;
    }
    let active = partial.then(|| {
        execution
            .scene
            .as_ref()
            .expect("prepared native scene")
            .active_batches
            .clone()
    });
    let direct = active.as_ref().map_or_else(
        || plan.all_direct_root_ops(),
        |selected| plan.active_direct_root_ops(selected),
    );
    if let Some(ops) = direct {
        execute_in_place_root_batches(&mut execution, canvas, &plan.ops, &ops)?;
    } else {
        shared_operations::execute_ops(
            &mut execution,
            canvas,
            &plan,
            &plan.ops,
            RenderTargetId::Main,
            &mut FilterCursors::default(),
            active.as_deref(),
        )?;
    }
    Ok(execution.targets.get(RenderTargetId::Main)?.image())
}
