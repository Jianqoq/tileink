//! Shared fine dispatch and spill addressing for both rendering adapters.
use crate::shared::{
    fine_config::FineConfig,
    gpu_coarse::{
        coarse_work_active_tile_list_word_offset, coarse_work_fine_tile_kind_word_offset,
    },
    gpu_constants::{FINE_GROUP_SPILL_FIELDS, FINE_WORKGROUP_SIZE, TILE_SIZE},
    gpu_plan::GpuBufferLengths,
};

#[derive(Clone, Copy, Default)]
pub(crate) struct FineParams {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) clear_color: u32,
    pub(crate) load_target: bool,
    pub(crate) clip_spill_depth: u32,
    pub(crate) group_spill_depth: u32,
    pub(crate) active_tile_count: Option<u32>,
    pub(crate) paint_sdf_shadow_base: u32,
    pub(crate) paint_brush_base: u32,
    pub(crate) text_image_base: u32,
    pub(crate) text_image_data_base: u32,
}

pub(crate) struct FinePlan {
    config: FineConfig,
    grid: [u32; 3],
    spill_words: usize,
}
impl FinePlan {
    pub(crate) fn new(
        lengths: GpuBufferLengths,
        params: FineParams,
        limit: u32,
    ) -> Result<Self, &'static str> {
        if limit == 0 || limit > 65535 {
            return Err("invalid fine dispatch limit");
        }
        super::coarse::validate_work_layout(lengths)?;
        let tile_count =
            u32::try_from(lengths.tile_count).map_err(|_| "fine tile count exceeds u32")?;
        let tiles_width = params.width.div_ceil(TILE_SIZE);
        let tiles_height = params.height.div_ceil(TILE_SIZE);
        if lengths.tiles_width != tiles_width as usize
            || lengths.tiles_height != tiles_height as usize
            || u64::from(tiles_width) * u64::from(tiles_height) != u64::from(tile_count)
        {
            return Err("fine dimensions do not match scene tiles");
        }
        let active = params.active_tile_count.unwrap_or(tile_count);
        if active > tile_count {
            return Err("fine active tiles exceed capacity");
        }
        let grid = if active == 0 {
            [0, 0, 0]
        } else {
            let x = active.min(limit);
            let y = active.div_ceil(x);
            if y > limit {
                return Err("fine dispatch exceeds device grid");
            }
            [x, y, 1]
        };
        let (group_base, spill_words) = spill_layout(
            tile_count,
            params.clip_spill_depth,
            params.group_spill_depth,
        )?;
        let word = |value: usize| u32::try_from(value).map_err(|_| "fine offset exceeds u32");
        let ptcl_capacity = word(lengths.coarse_ptcl_capacity)?;
        let kind = word(coarse_work_fine_tile_kind_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
            lengths.tile_draw_index_count,
            lengths.tile_draw_chunk_count,
        ))?;
        let active_base = word(coarse_work_active_tile_list_word_offset(
            lengths.tile_count,
            lengths.coarse_ptcl_capacity,
            lengths.coarse_glyph_capacity,
            lengths.tile_draw_index_count,
            lengths.tile_draw_chunk_count,
        ))?;
        Ok(Self {
            config: FineConfig {
                width: params.width,
                height: params.height,
                clear_color: params.clear_color,
                tile_count,
                tiles_width,
                tiles_height,
                load_target: u32::from(params.load_target),
                clip_spill_depth: params.clip_spill_depth,
                group_spill_depth: params.group_spill_depth,
                ptcl_capacity,
                paint_sdf_shadow_base: params.paint_sdf_shadow_base,
                paint_brush_base: params.paint_brush_base,
                text_image_base: params.text_image_base,
                text_image_data_base: params.text_image_data_base,
                group_spill_base: group_base,
                fine_tile_kind_base: kind,
                active_tile_count: active,
                dispatch_width: grid[0],
                active_tile_list_base: active_base,
                incremental: u32::from(params.active_tile_count.is_some()),
            },
            grid,
            spill_words,
        })
    }
    pub(crate) fn config(&self) -> &FineConfig {
        &self.config
    }
    pub(crate) fn grid(&self) -> [u32; 3] {
        self.grid
    }
    pub(crate) fn spill_words(&self) -> usize {
        self.spill_words
    }
}

/// Raw HLSL buffers address bytes with u32. Reject overflow before allocating or
/// converting the group base, rather than wrapping into another pixel's stack.
pub(crate) fn spill_layout(
    tiles: u32,
    clip_depth: u32,
    group_depth: u32,
) -> Result<(u32, usize), &'static str> {
    let lanes = u64::from(tiles) * u64::from(FINE_WORKGROUP_SIZE);
    let group_base = lanes
        .checked_mul(u64::from(clip_depth))
        .ok_or("fine clip spill overflow")?;
    let group_words = lanes
        .checked_mul(u64::from(group_depth))
        .and_then(|n| n.checked_mul(FINE_GROUP_SPILL_FIELDS as u64))
        .ok_or("fine group spill overflow")?;
    let words = group_base
        .checked_add(group_words)
        .filter(|n| *n <= u64::from(u32::MAX) / 4)
        .ok_or("fine spill exceeds raw-buffer address space")?;
    Ok((group_base as u32, words as usize))
}

#[cfg(test)]
#[path = "fine/tests.rs"]
mod tests;
