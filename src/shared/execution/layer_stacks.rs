//! Fused layer stack storage, compaction and descriptor indexes.
use super::*;

impl ExecPlan {
    pub(crate) fn layer_stack_ranges_for_draw(&self, draw: usize) -> Vec<Range<usize>> {
        let Some(locations) = self.layer_stack_locations.get(&(draw as u32)) else {
            return Vec::new();
        };
        let mut ranges = Vec::<Range<usize>>::new();
        for &index in locations {
            if let Some(last) = ranges.last_mut()
                && last.end == index
            {
                last.end += 1;
            } else {
                ranges.push(index..index + 1);
            }
        }
        ranges
    }

    pub(super) fn refresh_layer_stack_locations(&mut self) {
        self.layer_stack_locations.clear();
        for (index, entry) in self.layer_stack_data.iter().enumerate() {
            let draw = match entry {
                LayerStackEntry::Clip { draw }
                | LayerStackEntry::Opacity { draw, .. }
                | LayerStackEntry::Blend { draw, .. } => *draw,
            };
            self.layer_stack_locations
                .entry(draw)
                .or_default()
                .push(index);
        }
    }
}

pub(super) fn offset_layer_stack_ranges(ops: &mut [ExecOp], offset: usize) {
    let offset_range = |range: &mut Range<usize>| {
        range.start += offset;
        range.end += offset;
    };
    for op in ops {
        match op {
            ExecOp::DrawBatch { layer_stack, .. } => offset_range(layer_stack),
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            } => {
                offset_range(outer_stack);
                offset_layer_stack_ranges(children, offset);
            }
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                offset_range(outer_stack);
                offset_layer_stack_ranges(content, offset);
                offset_layer_stack_ranges(mask, offset);
            }
        }
    }
}

pub(super) fn compact_layer_stacks(ops: &mut [ExecOp], layer_stacks: &mut Vec<LayerStackEntry>) {
    let old = std::mem::take(layer_stacks);
    remap_layer_stacks(ops, &old, layer_stacks);
}

fn remap_layer_stacks(
    ops: &mut [ExecOp],
    old: &[LayerStackEntry],
    compact: &mut Vec<LayerStackEntry>,
) {
    let remap = |range: &mut Range<usize>, compact: &mut Vec<LayerStackEntry>| {
        let stack = &old[range.clone()];
        if stack.is_empty() {
            *range = 0..0;
            return;
        }
        if compact.ends_with(stack) {
            *range = compact.len() - stack.len()..compact.len();
            return;
        }
        let start = compact.len();
        compact.extend_from_slice(stack);
        *range = start..compact.len();
    };
    for op in ops {
        match op {
            ExecOp::DrawBatch { layer_stack, .. } => remap(layer_stack, compact),
            ExecOp::OffscreenLayer {
                outer_stack,
                children,
                ..
            } => {
                remap(outer_stack, compact);
                remap_layer_stacks(children, old, compact);
            }
            ExecOp::OffscreenMaskLayer {
                outer_stack,
                content,
                mask,
                ..
            } => {
                remap(outer_stack, compact);
                remap_layer_stacks(content, old, compact);
                remap_layer_stacks(mask, old, compact);
            }
        }
    }
}
