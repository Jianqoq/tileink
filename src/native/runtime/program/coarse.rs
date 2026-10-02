use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
};
use crate::render::coarse::{CoarsePlan, CoarseProgram};

/// GPU outputs from scan and uploads from the shared Canvas preparation.
/// These handles remain in the same ComputeBatch through coarse and fine.
pub(crate) struct CoarseBindings {
    pub(crate) draws: ResourceId,
    pub(crate) text: ResourceId,
    pub(crate) paint: ResourceId,
    pub(crate) paths: ResourceId,
    pub(crate) backdrops: ResourceId,
    pub(crate) ranges: ResourceId,
    pub(crate) layers: ResourceId,
    pub(crate) work: ResourceId,
    pub(crate) chunks: ResourceId,
    pub(crate) batches: ResourceId,
}

/// Record the shared schedule using the maintained HLSL resource interfaces.
///
/// # Safety
/// All scene records, tile lists, active indices, and referenced subranges must
/// be valid for the plan; work and chunk storage must cover its complete packed
/// layout. Scan must precede this call, and no source may alias writable storage.
/// On any recording error the caller must discard the batch.
pub(crate) unsafe fn encode(
    batch: &mut ComputeBatch,
    plan: &CoarsePlan,
    inputs: &CoarseBindings,
) -> Result<()> {
    if plan.passes().is_empty() {
        return Ok(());
    }
    let config = batch.buffer(bytemuck::bytes_of(&plan.config).to_vec())?;
    for pass in plan.passes() {
        use CoarseProgram::*;
        let CoarseBindings {
            draws,
            text,
            paint,
            paths,
            backdrops,
            ranges,
            layers,
            work,
            chunks,
            batches,
        } = *inputs;
        // Resource numbers belong to each maintained HLSL interface, not to
        // dispatch position. ComputeBatch verifies the exact ABI and aliases.
        let bindings: &[(u32, ResourceId)] = match pass.program {
            CountTiles | CountBins => &[
                (0, config),
                (1, draws),
                (2, text),
                (3, paths),
                (4, backdrops),
                (5, ranges),
                (6, layers),
                (7, work),
                (8, paint),
                (9, batches),
            ],
            PrefixChunks | ApplyChunkOffsets | EmitPrefixChunks | EmitApplyChunkOffsets => {
                &[(0, config), (7, work), (8, chunks)]
            }
            ChunkOffsets | EmitChunkOffsets => &[(0, config), (8, chunks)],
            EmitChunkCounts | EmitFillRefs | EmitChunkParticleOffsets => &[(0, config), (7, work)],
            EmitChunkParticleCounts => &[
                (0, config),
                (1, draws),
                (2, text),
                (3, paths),
                (4, backdrops),
                (5, ranges),
                (6, layers),
                (7, work),
                (9, paint),
                (10, batches),
            ],
            TileCountsFromEmitChunks => &[
                (0, config),
                (1, draws),
                (3, paths),
                (4, backdrops),
                (5, ranges),
                (6, layers),
                (7, work),
                (9, paint),
            ],
            EmitTiles | EmitBins | EmitChunks => &[
                (0, config),
                (1, draws),
                (2, text),
                (3, paint),
                (4, paths),
                (5, backdrops),
                (6, ranges),
                (7, layers),
                (8, work),
                (9, batches),
            ],
            EmitChunkTileKinds => &[
                (0, config),
                (1, draws),
                (3, paint),
                (4, paths),
                (5, backdrops),
                (6, ranges),
                (7, layers),
                (8, work),
            ],
        };
        // SAFETY: the caller owns record/capacity validity; the shared schedule
        // enforces stage ordering, nonzero grids and device dispatch bounds.
        unsafe {
            batch.dispatch(pass.program.entry(), bindings, pass.grid)?;
        }
    }
    Ok(())
}
