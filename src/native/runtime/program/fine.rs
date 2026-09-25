use super::super::{
    Result,
    compute::{ComputeBatch, Resource, ResourceId},
};
use crate::render::fine::FinePlan;

pub(crate) struct FineBindings {
    pub(crate) target: ResourceId,
    pub(crate) draws: ResourceId,
    pub(crate) paint: ResourceId,
    pub(crate) coarse: ResourceId,
    pub(crate) segments: ResourceId,
    pub(crate) text: ResourceId,
    pub(crate) spills: ResourceId,
    pub(crate) atlas: ResourceId,
    pub(crate) images: ResourceId,
    pub(crate) sampler: ResourceId,
}

/// # Safety
/// All particle/draw/text/paint/segment records and image indices must be valid
/// for this plan and its bound resources. Coarse must have completed before fine
/// reads it. Sparse tile IDs must be unique and within the physical tile range;
/// spill depths must cover actual clip/group nesting. Discard the batch on error.
pub(crate) unsafe fn encode(
    batch: &mut ComputeBatch,
    plan: &FinePlan,
    inputs: &FineBindings,
) -> Result<()> {
    if plan.grid()[0] == 0 {
        return Ok(());
    }
    let config = plan.config();
    batch.size(inputs.target)?; // Validate ownership before indexing resources.
    if !matches!(&batch.resources()[inputs.target.index()], Resource::Texture(t)
        if !t.array && t.size[0] >= config.width && t.size[1] >= config.height)
    {
        return Err("native fine target does not cover the viewport".into());
    }
    if batch.size(inputs.spills)? < plan.spill_words().max(1) * size_of::<u32>() {
        return Err("native fine spill storage is too small".into());
    }
    let config = batch.buffer(bytemuck::bytes_of(config).to_vec())?;
    // SAFETY: caller provides semantic bounds; FinePlan validates launch/spill
    // arithmetic and the checks above validate physical output capacity.
    unsafe {
        batch.dispatch(
            "fine_tile_main",
            &[
                (0, config),
                (1, inputs.target),
                (2, inputs.draws),
                (3, inputs.paint),
                (4, inputs.coarse),
                (5, inputs.segments),
                (6, inputs.text),
                (7, inputs.spills),
                (12, inputs.atlas),
                (13, inputs.sampler),
                (30, inputs.images),
            ],
            plan.grid(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/fine_plan.rs"]
mod tests;
