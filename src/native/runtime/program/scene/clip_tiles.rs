//! Conservative clip-local dispatch lists. Coverage remains in coarse/fine.
use crate::shared::execution::{ExecPlan, LayerStackEntry};
use crate::{Canvas, TILE_SIZE};
use std::ops::Range;

pub(super) fn tiles(
    canvas: &Canvas,
    plan: &ExecPlan,
    stack: Range<usize>,
    active: Option<&[u32]>,
) -> Option<Vec<u32>> {
    let width = canvas.physical_width();
    let height = canvas.physical_height();
    let mut bounds = (0i32, 0i32, width as i32, height as i32);
    if stack.is_empty() {
        return None;
    }
    for entry in &plan.layer_stack_data[stack] {
        // Group compositing may affect the destination outside its child bounds.
        // Restrict only pure clip stacks, whose coverage is zero outside the mask.
        let LayerStackEntry::Clip { draw } = *entry else {
            return None;
        };
        let b = canvas.draw_records[draw as usize].pixel_bounds;
        bounds.0 = bounds.0.max(b.x0);
        bounds.1 = bounds.1.max(b.y0);
        bounds.2 = bounds.2.min(b.x1);
        bounds.3 = bounds.3.min(b.y1);
    }
    if bounds.0 >= bounds.2 || bounds.1 >= bounds.3 {
        return Some(Vec::new());
    }
    let (x0, y0) = (bounds.0 as u32 / TILE_SIZE, bounds.1 as u32 / TILE_SIZE);
    let (x1, y1) = (
        (bounds.2 as u32).div_ceil(TILE_SIZE),
        (bounds.3 as u32).div_ceil(TILE_SIZE),
    );
    let stride = width.div_ceil(TILE_SIZE);
    let count = (x1 - x0) * (y1 - y0);
    let original = active.map_or(stride * height.div_ceil(TILE_SIZE), |tiles| {
        tiles.len() as u32
    });
    if active.is_none() && count >= original {
        return None;
    }
    Some(if let Some(active) = active {
        active
            .iter()
            .copied()
            .filter(|tile| {
                let (x, y) = (tile % stride, tile / stride);
                x >= x0 && x < x1 && y >= y0 && y < y1
            })
            .collect()
    } else {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| y * stride + x))
            .collect()
    })
}

pub(super) struct ClipDispatch {
    pub preallocated: bool,
    pub ranges: std::collections::HashMap<(u32, u32), Range<u32>>,
    pub data: Vec<u32>,
    pub slots: Vec<crate::shared::gpu_coarse::TileCoarseRecord>,
    pub kinds: Vec<u32>,
}

impl ClipDispatch {
    pub(super) fn discard_upload_data(&mut self) {
        self.data = Vec::new();
        self.slots = Vec::new();
        self.kinds = Vec::new();
    }

    pub(super) fn new(
        canvas: &Canvas,
        plan: &ExecPlan,
        active: Option<&[u32]>,
        records: &[crate::shared::gpu_coarse::TileDrawRecord],
        lengths: crate::shared::gpu_plan::GpuBufferLengths,
        clip_depth: usize,
    ) -> super::Result<Self> {
        use crate::shared::{execution::ExecOp, gpu_coarse::TileCoarseRecord};
        let base = crate::render::coarse::validate_work_layout(lengths)?;
        let mut result = Self {
            preallocated: false,
            ranges: Default::default(),
            data: Vec::new(),
            slots: Vec::new(),
            kinds: Vec::new(),
        };
        fn collect(ops: &[ExecOp], ranges: &mut Vec<Range<usize>>) {
            for op in ops {
                match op {
                    ExecOp::DrawBatch { layer_stack, .. } => ranges.push(layer_stack.clone()),
                    ExecOp::OffscreenLayer { children, .. } => collect(children, ranges),
                    ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                        collect(content, ranges);
                        collect(mask, ranges);
                    }
                    _ => {}
                }
            }
        }
        if cfg!(feature = "metal") || plan.layer_stack_data.is_empty() {
            return Ok(result);
        }
        let mut ranges = Vec::new();
        collect(&plan.ops, &mut ranges);
        for range in &ranges {
            let key = (u32::try_from(range.start)?, u32::try_from(range.end)?);
            if result.ranges.contains_key(&key) {
                continue;
            }
            if let Some(tiles) = tiles(canvas, plan, range.clone(), active) {
                let start = u32::try_from(
                    base.checked_add(result.data.len())
                        .ok_or("clip tile offset overflow")?,
                )?;
                result.data.extend(tiles);
                let end = u32::try_from(
                    base.checked_add(result.data.len())
                        .ok_or("clip tile offset overflow")?,
                )?;
                result.ranges.insert(key, start..end);
            }
        }
        // Only a complete pure-clip schedule can share immutable tile headers:
        // a regular coarse pass would overwrite them for subsequent clip batches.
        // Metal retains its existing schedule until its emitter is validated on Mac.
        let fixed = cfg!(any(feature = "dx12", feature = "vulkan"))
            && !lengths.text_enabled
            && !ranges.is_empty()
            && plan.ops.iter().all(|op| {
                matches!(
                    op,
                    ExecOp::DrawBatch { .. } | ExecOp::BeginClip | ExecOp::EndClip
                )
            })
            && ranges
                .iter()
                .all(|r| result.ranges.contains_key(&(r.start as u32, r.end as u32)));
        if fixed {
            result.preallocated = true;
            let mut cursor = 0u32;
            let overhead = u32::try_from(
                clip_depth
                    .checked_mul(2)
                    .and_then(|v| v.checked_add(1))
                    .ok_or("clip depth overflow")?,
            )?;
            for record in records {
                // Each binned non-text draw emits at most one particle. Reserve
                // begin/end pairs for maximum clip depth and a terminator per tile.
                let end = cursor
                    .checked_add(record.end)
                    .and_then(|v| v.checked_add(overhead))
                    .ok_or("clip particle capacity overflow")?;
                result.slots.push(TileCoarseRecord {
                    ptcl_count: end - cursor,
                    ptcl_start: cursor,
                    ptcl_end: end,
                    ..Default::default()
                });
                cursor = end;
            }
            if records.len() != lengths.tile_count || cursor as usize > lengths.coarse_ptcl_capacity
            {
                return Err("clip slots exceed scene capacity".into());
            }
            result.kinds.resize(
                lengths.tile_count,
                crate::shared::gpu_constants::TILE_KIND_INTERPRETER,
            );
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "clip_tiles_tests.rs"]
mod tests;
