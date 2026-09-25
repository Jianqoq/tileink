//! Conservative clip-local dispatch lists. Coverage remains in coarse/fine.
use crate::shared::execution::{ExecPlan, LayerStackEntry};
use crate::{Canvas, TILE_SIZE};
use std::{collections::HashMap, ops::Range};

type PixelRect = (i32, i32, i32, i32);
type ContentBounds = HashMap<(usize, usize), Vec<PixelRect>>;

pub(super) fn tiles(
    canvas: &Canvas,
    plan: &ExecPlan,
    stack: Range<usize>,
    active: Option<&[u32]>,
    content_bounds: Option<&[PixelRect]>,
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
    let stride = width.div_ceil(TILE_SIZE);
    let original = active.map_or(stride * height.div_ceil(TILE_SIZE), |tiles| {
        tiles.len() as u32
    });
    let sparse_limit = original.div_ceil(3);
    let whole_clip = [bounds];
    // Retained damage can include content removed or reparented since the last
    // frame. An empty clip batch can still provide a mask for later draws.
    // Child bounds only constrain a complete repaint with known content.
    let content = match content_bounds {
        Some(bounds) if active.is_none() && !bounds.is_empty() => bounds,
        _ => &whole_clip,
    };
    let mut tile_rects = Vec::new();
    let mut candidate_tiles = 0u32;
    for &(x0, y0, x1, y1) in content {
        let rect = (
            bounds.0.max(x0),
            bounds.1.max(y0),
            bounds.2.min(x1),
            bounds.3.min(y1),
        );
        if rect.0 < rect.2 && rect.1 < rect.3 {
            let tile_rect = (
                rect.0 as u32 / TILE_SIZE,
                rect.1 as u32 / TILE_SIZE,
                (rect.2 as u32).div_ceil(TILE_SIZE),
                (rect.3 as u32).div_ceil(TILE_SIZE),
            );
            // A dense batch is cheaper through the original GPU path. Counting
            // overlaps twice is conservative and lets us stop before allocation.
            candidate_tiles = candidate_tiles.saturating_add(
                (tile_rect.2 - tile_rect.0).saturating_mul(tile_rect.3 - tile_rect.1),
            );
            if active.is_none() && candidate_tiles > sparse_limit {
                return None;
            }
            tile_rects.push(tile_rect);
        }
    }
    let selected: Vec<u32> = if let Some(active) = active {
        active
            .iter()
            .copied()
            .filter(|tile| {
                let (x, y) = (tile % stride, tile / stride);
                tile_rects
                    .iter()
                    .any(|&(x0, y0, x1, y1)| x >= x0 && x < x1 && y >= y0 && y < y1)
            })
            .collect()
    } else {
        let mut selected = Vec::new();
        for (x0, y0, x1, y1) in tile_rects {
            selected.extend((y0..y1).flat_map(|y| (x0..x1).map(move |x| y * stride + x)));
        }
        selected.sort_unstable();
        selected.dedup();
        selected
    };
    (selected.len() <= sparse_limit as usize).then_some(selected)
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
        fn collect(
            ops: &[ExecOp],
            canvas: &Canvas,
            ranges: &mut Vec<Range<usize>>,
            content_bounds: &mut ContentBounds,
            include_content: bool,
        ) {
            for op in ops {
                match op {
                    ExecOp::DrawBatch {
                        layer_stack, draws, ..
                    } => {
                        ranges.push(layer_stack.clone());
                        if include_content {
                            let bounds = content_bounds
                                .entry((layer_stack.start, layer_stack.end))
                                .or_default();
                            for &draw in draws.iter() {
                                let b = canvas.draw_records[draw].pixel_bounds;
                                if b.x0 < b.x1 && b.y0 < b.y1 {
                                    bounds.push((b.x0, b.y0, b.x1, b.y1));
                                }
                            }
                        }
                    }
                    ExecOp::OffscreenLayer { children, .. } => {
                        collect(children, canvas, ranges, content_bounds, include_content)
                    }
                    ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                        collect(content, canvas, ranges, content_bounds, include_content);
                        collect(mask, canvas, ranges, content_bounds, include_content);
                    }
                    _ => {}
                }
            }
        }
        // The prepared plan already knows its maximum active clip depth. A
        // clip-free retained frame must not rescan every layer stack.
        if clip_depth == 0 {
            return Ok(result);
        }
        let mut ranges = Vec::new();
        let mut content = std::collections::HashMap::new();
        collect(
            &plan.ops,
            canvas,
            &mut ranges,
            &mut content,
            active.is_none(),
        );
        for range in &ranges {
            let key = (u32::try_from(range.start)?, u32::try_from(range.end)?);
            if result.ranges.contains_key(&key) {
                continue;
            }
            let child_bounds = content.get(&(range.start, range.end)).map(Vec::as_slice);
            if let Some(tiles) = tiles(canvas, plan, range.clone(), active, child_bounds) {
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
        // every coarse pass must emit into the same preallocated slots.
        // All native emitters support these slots. Metal's former opt-out made
        // each small clip count, allocate, and draw the full viewport again.
        let fixed = !lengths.text_enabled
            && !ranges.is_empty()
            && plan.ops.iter().all(|op| {
                matches!(
                    op,
                    ExecOp::DrawBatch { .. } | ExecOp::BeginClip | ExecOp::EndClip
                )
            });
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
