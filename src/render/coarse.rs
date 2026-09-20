//! Backend-independent coarse ordering. Both adapters consume the same plan so
//! binning, prefix allocation and particle emission cannot silently diverge.
use crate::shared::{
    gpu_coarse::coarse_work_active_tile_list_word_offset, gpu_constants::COARSE_WORKGROUP_SIZE,
    gpu_plan::GpuBufferLengths,
};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CoarseBatch {
    pub(crate) draw_start: u32,
    pub(crate) draw_end: u32,
    pub(crate) layer_stack_start: u32,
    pub(crate) layer_stack_end: u32,
    pub(crate) active_tile_count: Option<u32>,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CoarseConfig {
    pub(crate) tile_count: u32,
    pub(crate) tiles_width: u32,
    pub(crate) tiles_height: u32,
    pub(crate) draw_start: u32,
    pub(crate) draw_end: u32,
    pub(crate) layer_stack_start: u32,
    pub(crate) layer_stack_end: u32,
    pub(crate) ptcl_capacity: u32,
    pub(crate) glyph_capacity: u32,
    pub(crate) chunk_count: u32,
    pub(crate) text_run_count: u32,
    pub(crate) text_glyph_count: u32,
    pub(crate) tile_draw_index_count: u32,
    pub(crate) emit_chunk_capacity: u32,
    pub(crate) paint_brush_base: u32,
    pub(crate) text_enabled: u32,
    pub(crate) active_tile_count: u32,
    pub(crate) active_tile_list_base: u32,
    pub(crate) incremental: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoarseProgram {
    CountTiles,
    CountBins,
    PrefixChunks,
    ChunkOffsets,
    ApplyChunkOffsets,
    EmitChunkCounts,
    EmitPrefixChunks,
    EmitChunkOffsets,
    EmitApplyChunkOffsets,
    EmitFillRefs,
    EmitChunkParticleCounts,
    TileCountsFromEmitChunks,
    EmitChunkParticleOffsets,
    EmitTiles,
    EmitBins,
    EmitChunks,
    EmitChunkTileKinds,
}
impl CoarseProgram {
    pub(crate) fn entry(self) -> &'static str {
        match self {
            Self::CountTiles => "coarse_count",
            Self::CountBins => "coarse_count_bins",
            Self::PrefixChunks => "coarse_prefix_chunks",
            Self::ChunkOffsets => "coarse_chunk_offsets",
            Self::ApplyChunkOffsets => "coarse_apply_chunk_offsets",
            Self::EmitChunkCounts => "coarse_emit_chunk_counts",
            Self::EmitPrefixChunks => "coarse_emit_prefix_chunks",
            Self::EmitChunkOffsets => "coarse_emit_chunk_offsets",
            Self::EmitApplyChunkOffsets => "coarse_emit_apply_chunk_offsets",
            Self::EmitFillRefs => "coarse_emit_fill_refs",
            Self::EmitChunkParticleCounts => "coarse_emit_chunk_particle_counts",
            Self::TileCountsFromEmitChunks => "coarse_tile_counts_from_emit_chunks",
            Self::EmitChunkParticleOffsets => "coarse_emit_chunk_particle_offsets",
            Self::EmitTiles => "coarse_emit",
            Self::EmitBins => "coarse_emit_bins",
            Self::EmitChunks => "coarse_emit_chunks",
            Self::EmitChunkTileKinds => "coarse_emit_chunk_tile_kinds",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CoarseDispatch {
    pub(crate) program: CoarseProgram,
    pub(crate) grid: [u32; 3],
}

// Host schedule storage, unrelated to shader workgroup constants.
const MAX_PASSES: usize = 13;

pub(crate) struct CoarsePlan {
    pub(crate) config: CoarseConfig,
    passes: [CoarseDispatch; MAX_PASSES],
    len: usize,
}
impl CoarsePlan {
    pub(crate) fn new(
        lengths: GpuBufferLengths,
        batch: CoarseBatch,
        paint_brush_base: u32,
        emit_chunks: bool,
        limit: u32,
    ) -> Result<Self, &'static str> {
        if limit == 0 || limit > 65535 {
            return Err("invalid coarse dispatch limit");
        }
        validate_work_layout(lengths)?;
        let word = |v: usize| u32::try_from(v).map_err(|_| "coarse size exceeds u32");
        let tile_count = word(lengths.tile_count)?;
        let tiles_width = word(lengths.tiles_width)?;
        let tiles_height = word(lengths.tiles_height)?;
        if u64::from(tiles_width) * u64::from(tiles_height) != u64::from(tile_count) {
            return Err("coarse tile dimensions do not match capacity");
        }
        if batch.draw_start > batch.draw_end || batch.layer_stack_start > batch.layer_stack_end {
            return Err("reversed coarse record range");
        }
        let active = batch.active_tile_count.unwrap_or(tile_count);
        if active > tile_count {
            return Err("coarse active tiles exceed capacity");
        }
        let incremental = batch.active_tile_count.is_some();
        let chunk_count = word(lengths.coarse_chunk_count)?;
        if chunk_count != tile_count.div_ceil(COARSE_WORKGROUP_SIZE) {
            return Err("coarse chunk count does not cover tiles");
        }
        let prefix_count = if incremental {
            active.div_ceil(COARSE_WORKGROUP_SIZE)
        } else {
            chunk_count
        };
        // Check all section sizes before computing packed offsets (usize on host,
        // u32 word addresses in shaders). Callers allocate this shared layout.
        let ptcl_capacity = word(lengths.coarse_ptcl_capacity)?;
        let glyph_capacity = word(lengths.coarse_glyph_capacity)?;
        let tile_draw_index_count = word(lengths.tile_draw_index_count)?;
        let emit_chunk_capacity = word(lengths.tile_draw_chunk_count)?;
        let active_base = word(coarse_work_active_tile_list_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
            lengths.tile_draw_index_count,
            lengths.tile_draw_chunk_count,
        ))?;
        let mut plan = Self {
            config: CoarseConfig {
                tile_count,
                tiles_width,
                tiles_height,
                draw_start: batch.draw_start,
                draw_end: batch.draw_end,
                layer_stack_start: batch.layer_stack_start,
                layer_stack_end: batch.layer_stack_end,
                ptcl_capacity,
                glyph_capacity,
                chunk_count: prefix_count,
                text_run_count: word(lengths.text_run_count)?,
                text_glyph_count: word(lengths.text_glyph_count)?,
                tile_draw_index_count,
                emit_chunk_capacity,
                paint_brush_base,
                text_enabled: u32::from(lengths.text_enabled),
                active_tile_count: active,
                active_tile_list_base: active_base,
                incremental: u32::from(incremental),
            },
            passes: [CoarseDispatch {
                program: CoarseProgram::CountBins,
                grid: [0; 3],
            }; MAX_PASSES],
            len: 0,
        };
        if active == 0 {
            return Ok(plan);
        }
        use CoarseProgram::*;
        let emit = batch.draw_start < batch.draw_end && ptcl_capacity > 0;
        let chunked = emit_chunks && !incremental && emit && emit_chunk_capacity > 0;
        let count = if incremental {
            active
        } else {
            super::binning::coarse_bin_count(lengths)
        };
        if chunked {
            for program in [
                EmitChunkCounts,
                EmitPrefixChunks,
                EmitChunkOffsets,
                EmitApplyChunkOffsets,
                EmitFillRefs,
                EmitChunkParticleCounts,
                TileCountsFromEmitChunks,
            ] {
                plan.push(
                    program,
                    match program {
                        EmitChunkOffsets => 1,
                        EmitChunkParticleCounts => emit_chunk_capacity,
                        _ => chunk_count,
                    },
                    limit,
                )?;
            }
        } else {
            plan.push(
                if incremental { CountTiles } else { CountBins },
                count,
                limit,
            )?;
        }
        for program in [PrefixChunks, ChunkOffsets, ApplyChunkOffsets] {
            plan.push(
                program,
                if program == ChunkOffsets {
                    1
                } else {
                    prefix_count
                },
                limit,
            )?;
        }
        if chunked {
            plan.push(EmitChunkParticleOffsets, chunk_count, limit)?;
            plan.push(EmitChunks, emit_chunk_capacity, limit)?;
            plan.push(EmitChunkTileKinds, chunk_count, limit)?;
        } else if emit {
            plan.push(if incremental { EmitTiles } else { EmitBins }, count, limit)?;
        }
        Ok(plan)
    }
    fn push(&mut self, program: CoarseProgram, count: u32, limit: u32) -> Result<(), &'static str> {
        let grid = if matches!(
            program,
            CoarseProgram::EmitChunks | CoarseProgram::EmitChunkParticleCounts
        ) {
            let x = count.min(limit);
            [x, count.div_ceil(x), 1]
        } else {
            [count, 1, 1]
        };
        if grid[0] > limit || grid[1] > limit {
            return Err("coarse dispatch exceeds device grid");
        }
        self.passes[self.len] = CoarseDispatch { program, grid };
        self.len += 1;
        Ok(())
    }
    #[cfg(any(feature = "wgpu", test))]
    /// Resolve every live pass eagerly, before an adapter borrows its encoder.
    /// Mapping the owned array keeps storage tied to this plan's capacity and
    /// cannot silently truncate dispatches when another stage is added.
    pub(crate) fn resolve<T>(
        &self,
        mut map: impl FnMut(&CoarseDispatch) -> T,
    ) -> [Option<T>; MAX_PASSES] {
        let mut index = 0;
        self.passes.map(|pass| {
            let live = index < self.len;
            index += 1;
            live.then(|| map(&pass))
        })
    }
    pub(crate) fn passes(&self) -> &[CoarseDispatch] {
        &self.passes[..self.len]
    }
}

#[cfg(test)]
#[path = "coarse/tests.rs"]
mod tests;

#[path = "coarse/layout.rs"]
mod layout;
pub(crate) use layout::validate_work_layout;
