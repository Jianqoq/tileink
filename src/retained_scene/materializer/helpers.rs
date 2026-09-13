use super::*;

/// CPU-only retained materializer access for Criterion benchmarks.
///
/// This deliberately exposes no render data and is omitted from normal builds.
#[cfg(feature = "bench-internals")]
#[doc(hidden)]
pub struct RetainedMaterializerBenchmark {
    materializer: PersistentSceneMaterializer,
}

#[cfg(feature = "bench-internals")]
impl RetainedMaterializerBenchmark {
    pub fn new(scene: &RetainedScene) -> Self {
        Self {
            materializer: PersistentSceneMaterializer::new(scene),
        }
    }

    pub fn update_incremental(&mut self, scene: &RetainedScene) -> bool {
        let changes = scene.changes_since(self.materializer.version());
        self.materializer.update(scene, changes)
    }

    pub fn visit_tiles_for_bounds(&self, bounds: Bounds) -> (usize, usize) {
        self.materializer
            .tiles_for_bounds(bounds)
            .fold((0, 0), |(count, checksum), tile| {
                (count + 1, checksum.wrapping_add(tile))
            })
    }

    pub fn visit_node_physical_draws(
        &self,
        id: RetainedNodeId,
        repetitions: usize,
    ) -> (usize, usize) {
        let mut result = (0usize, 0usize);
        for _ in 0..repetitions {
            for draw in self.materializer.node_physical_draws(id) {
                result.0 += 1;
                result.1 = result.1.wrapping_add(draw);
            }
        }
        result
    }
}

pub(super) fn sync_arena<T: Copy>(
    dst: &mut Vec<T>,
    arena: &mut SceneArena<T>,
    vacant: T,
    ranges: &mut Vec<std::ops::Range<usize>>,
) {
    dst.resize(arena.values().len(), vacant);
    arena.take_dirty_ranges_into(ranges);
    for range in ranges.iter() {
        dst[range.clone()].copy_from_slice(&arena.values()[range.clone()]);
    }
    dst.truncate(arena.values().len());
}

pub(super) fn range_bytes<T>(ranges: &[std::ops::Range<usize>]) -> u64 {
    ranges
        .iter()
        .map(|range| (range.len() * std::mem::size_of::<T>()) as u64)
        .sum()
}

pub(super) fn changed_value_ranges<T: PartialEq>(
    old: &[T],
    new: &[T],
) -> Vec<std::ops::Range<usize>> {
    // These ranges are consumed as writes into `new`. A removed suffix is different from the old
    // value, but there is no destination element to upload and no live draw can address it.
    let len = new.len();
    let mut ranges = Vec::new();
    let mut start = None;
    for index in 0..len {
        let changed = old.get(index) != new.get(index);
        match (start, changed) {
            (None, true) => start = Some(index),
            (Some(begin), false) => {
                ranges.push(begin..index);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        ranges.push(begin..len);
    }
    ranges
}

pub(super) fn merge_index_ranges(
    mut ranges: Vec<std::ops::Range<usize>>,
) -> Vec<std::ops::Range<usize>> {
    ranges.retain(|range| !range.is_empty());
    ranges.sort_unstable_by_key(|range| range.start);
    let mut merged = Vec::<std::ops::Range<usize>>::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

/// Empty and singleton indexes can be validated in constant time. Reuse their
/// immutable mapping independently of patch revisions and bounds. Larger maps
/// rebuild directly: probing all slots before a late mismatch duplicated the
/// linear indexing work. Version nodes and patch payloads remain independent.
pub(super) fn retained_patch_index(
    patches: &[RetainedNodePatch],
    previous: Option<&Rc<HashMap<RetainedNodeId, usize>>>,
) -> Rc<HashMap<RetainedNodeId, usize>> {
    if patches.len() <= 1
        && let Some(previous) = previous
        && previous.len() == patches.len()
        && patches.iter().enumerate().all(|(index, patch)| {
            previous.get(&patch.new.or(patch.old).unwrap().id) == Some(&index)
        })
    {
        return Rc::clone(previous);
    }
    Rc::new(
        patches
            .iter()
            .enumerate()
            .map(|(index, patch)| (patch.new.or(patch.old).unwrap().id, index))
            .collect(),
    )
}

pub(super) fn prune_shadowed_delta(
    mut previous: Option<Rc<RetainedFrameDelta>>,
    index: &HashMap<RetainedNodeId, usize>,
) -> (Option<Rc<RetainedFrameDelta>>, u16) {
    while previous
        .as_ref()
        .is_some_and(|delta| delta.index.keys().all(|id| index.contains_key(id)))
    {
        previous = previous.unwrap().previous.clone();
    }
    let depth = previous.as_ref().map_or(1, |delta| delta.depth + 1);
    (previous, depth)
}

pub(super) fn collect_scene_leaves(
    scene: &RetainedScene,
    parent: RetainedNodeId,
    leaves: &mut Vec<RetainedNodeId>,
) {
    let node = &scene.nodes[&parent];
    for &child in node.content.values().chain(node.mask.values()) {
        match scene.nodes[&child].kind {
            NodeKind::Scene { .. } => leaves.push(child),
            NodeKind::Group | NodeKind::Layer(_) => collect_scene_leaves(scene, child, leaves),
        }
    }
}

pub(super) fn is_local_bounds_layer(layer: &RetainedLayerDescriptor) -> bool {
    matches!(
        layer,
        RetainedLayerDescriptor::ClipPath { .. }
            | RetainedLayerDescriptor::ClipSdf { .. }
            | RetainedLayerDescriptor::Isolate { .. }
            | RetainedLayerDescriptor::Opacity { .. }
            | RetainedLayerDescriptor::Blend { .. }
    )
}

pub(super) fn count_stable_batches(batches: &[u32]) -> Vec<u32> {
    let Some(maximum) = batches
        .iter()
        .copied()
        .filter(|&batch| batch != u32::MAX)
        .max()
    else {
        return Vec::new();
    };
    let mut counts = vec![0; maximum as usize + 1];
    for &batch in batches {
        if batch != u32::MAX {
            counts[batch as usize] += 1;
        }
    }
    counts
}

pub(super) fn exec_ops_have_batches(ops: &[crate::shared::execution::ExecOp]) -> bool {
    ops.iter().any(|op| match op {
        crate::shared::execution::ExecOp::DrawBatch { .. } => true,
        crate::shared::execution::ExecOp::OffscreenLayer { children, .. } => {
            exec_ops_have_batches(children)
        }
        crate::shared::execution::ExecOp::OffscreenMaskLayer { content, mask, .. } => {
            exec_ops_have_batches(content) || exec_ops_have_batches(mask)
        }
        _ => false,
    })
}

pub(super) fn set_stable_batch(
    batches: &mut [u32],
    counts: &mut Vec<u32>,
    physical: usize,
    batch: u32,
) -> bool {
    let previous = batches[physical];
    if previous == batch {
        return false;
    }
    if previous != u32::MAX {
        counts[previous as usize] -= 1;
    }
    if batch != u32::MAX {
        counts.resize(counts.len().max(batch as usize + 1), 0);
        counts[batch as usize] += 1;
    }
    batches[physical] = batch;
    true
}

pub(super) fn collect_container_batches(
    scene: &RetainedScene,
    id: RetainedNodeId,
    leaf_batches: &HashMap<RetainedNodeId, u32>,
    containers: &mut HashMap<RetainedParent, u32>,
) -> Option<u32> {
    let node = &scene.nodes[&id];
    let content =
        collect_branch_batch(scene, RetainedParent::content(id), leaf_batches, containers);
    if let Some(batch) = content {
        containers.insert(RetainedParent::content(id), batch);
    }
    if matches!(node.kind, NodeKind::Layer(RetainedLayerDescriptor::Mask(_))) {
        let mask = collect_branch_batch(scene, RetainedParent::mask(id), leaf_batches, containers);
        if let Some(batch) = mask {
            containers.insert(RetainedParent::mask(id), batch);
        }
    }
    matches!(node.kind, NodeKind::Group)
        .then_some(content)
        .flatten()
}

pub(super) fn collect_branch_batch(
    scene: &RetainedScene,
    parent: RetainedParent,
    leaf_batches: &HashMap<RetainedNodeId, u32>,
    containers: &mut HashMap<RetainedParent, u32>,
) -> Option<u32> {
    let mut batch = None;
    for &child in scene.nodes[&parent.node]
        .children(parent.branch)
        .expect("validated retained branch")
        .values()
    {
        let candidate = match scene.nodes[&child].kind {
            NodeKind::Scene { .. } => leaf_batches.get(&child).copied(),
            NodeKind::Group => collect_container_batches(scene, child, leaf_batches, containers),
            NodeKind::Layer(_) => {
                collect_container_batches(scene, child, leaf_batches, containers);
                None
            }
        };
        let Some(candidate) = candidate else {
            continue;
        };
        if batch.is_some_and(|batch| batch != candidate) {
            return None;
        }
        batch = Some(candidate);
    }
    batch
}

pub(super) fn painter_path(scene: &RetainedScene, mut id: RetainedNodeId) -> Rc<[u128]> {
    let mut reversed = Vec::new();
    while id != scene.root {
        let parent = scene.nodes[&id]
            .parent
            .expect("non-root retained node has parent");
        let children = scene.nodes[&parent.node]
            .children(parent.branch)
            .expect("validated retained branch");
        reversed.push((
            parent.branch,
            children
                .key_of(id)
                .expect("child exists in retained parent"),
        ));
        id = parent.node;
    }
    reversed.reverse();
    reversed
        .into_iter()
        .flat_map(|(branch, key)| [branch as u128, key])
        .collect::<Vec<_>>()
        .into()
}

pub(super) fn is_descendant_or_self(
    scene: &RetainedScene,
    mut id: RetainedNodeId,
    ancestor: RetainedNodeId,
) -> bool {
    loop {
        if id == ancestor {
            return true;
        }
        let Some(parent) = scene.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        id = parent.node;
    }
}

pub(super) fn inactive_draw() -> DrawRecord {
    DrawRecord {
        path_id: DrawRecord::NONE,
        glyph_run_id: DrawRecord::NONE,
        sdf_offset: DrawRecord::NONE,
        sdf_len: 0,
        sdf_shadow_offset: DrawRecord::NONE,
        sdf_shadow_len: 0,
        brush_offset: DrawRecord::NONE,
        brush_len: 0,
        tag: DrawTagWord::default(),
        fill_rule: FillRuleWord::default(),
        pixel_bounds: Default::default(),
        local_pixel_bounds: Default::default(),
        solid_rect: 0,
        transform: Default::default(),
        inverse_transform: Default::default(),
    }
}

pub(super) struct ChunkPlanMetadata {
    pub(super) plain_fragment: bool,
    pub(super) local_draw_order: LocalDrawOrder,
}

/// Compiles chunk-local execution metadata once and retains its draw-order allocation.
pub(super) fn chunk_plan_metadata(canvas: &Canvas) -> ChunkPlanMetadata {
    let plan = canvas.compile(crate::shared::execution::ROOT_COMMAND_LIST_ID);
    let plain_fragment = plan.layer_stack_data.is_empty()
        && plan
            .ops
            .iter()
            .all(|op| matches!(op, crate::shared::execution::ExecOp::DrawBatch { .. }));
    ChunkPlanMetadata {
        plain_fragment,
        local_draw_order: LocalDrawOrder::from_compiled(plan.draw_order),
    }
}

pub(super) fn scene_node_placement(node: &SceneNode) -> (Option<Rc<Canvas>>, Option<[u64; 6]>) {
    match &node.kind {
        NodeKind::Scene {
            canvas, transform, ..
        } => (
            Some(canvas.clone()),
            Some(transform.as_coeffs().map(f64::to_bits)),
        ),
        _ => (None, None),
    }
}

pub(super) fn command_has_filter_resources(command: &Command) -> bool {
    matches!(
        command,
        Command::Layer {
            layer: Layer::Filter { .. } | Layer::Backdrop { .. },
            ..
        }
    )
}

pub(super) fn chunk_has_surface_dependent_plan(canvas: &Canvas) -> bool {
    canvas.command_lists.iter().any(|list| {
        list.commands.iter().any(|command| {
            matches!(
                command,
                Command::Layer {
                    layer: Layer::Filter { .. } | Layer::Backdrop { .. },
                    ..
                } | Command::MaskLayer { .. }
            )
        })
    })
}

pub(super) fn backdrop_dependencies(canvas: &Canvas) -> Vec<BackdropDependency> {
    fn collect(canvas: &Canvas, list: usize, scoped: bool, result: &mut Vec<BackdropDependency>) {
        for command in &canvas.command_lists[list].commands {
            match command {
                Command::Layer {
                    layer, children, ..
                } => {
                    if let Layer::Backdrop {
                        filter: value,
                        sample_region,
                    } = layer
                    {
                        result.push(BackdropDependency {
                            scoped,
                            dependency: filter::filter_input_bounds(value, sample_region),
                            output: filter::unclipped_filtered_region_bounds(value, sample_region),
                            read: filter::filter_dependency(value),
                        });
                    }
                    collect(
                        canvas,
                        *children,
                        scoped || !matches!(layer, Layer::Backdrop { .. }),
                        result,
                    );
                }
                Command::MaskLayer { content, mask, .. } => {
                    collect(canvas, *content, true, result);
                    collect(canvas, *mask, true, result);
                }
                Command::MaterializedRetainedScene { children, .. } => {
                    collect(canvas, *children, scoped, result)
                }
                Command::Draw(_) => {}
            }
        }
    }
    let mut result = Vec::new();
    collect(canvas, canvas.root_commands, false, &mut result);
    result
}

pub(super) fn chunk_layer_influence_bounds(chunk: &SceneChunk) -> Bounds {
    let command = &chunk.canvas.command_lists[chunk.canvas.root_commands].commands[0];
    match command {
        Command::Layer {
            draw,
            layer:
                crate::shared::layer::Layer::Clip
                | crate::shared::layer::Layer::ClipSdf { .. }
                | crate::shared::layer::Layer::Isolate
                | crate::shared::layer::Layer::Opacity(_)
                | crate::shared::layer::Layer::Blend(_),
            ..
        } => {
            let bounds = chunk.canvas.draw_records[*draw].pixel_bounds;
            Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
        }
        Command::Layer {
            layer:
                crate::shared::layer::Layer::Filter {
                    filter: value,
                    sample_region,
                }
                | crate::shared::layer::Layer::Backdrop {
                    filter: value,
                    sample_region,
                },
            ..
        } => filter::unclipped_filtered_region_bounds(value, sample_region),
        Command::MaskLayer { layer, .. } => filter::region_bounds(&layer.region),
        _ => unreachable!("retained layer chunk has one root layer command"),
    }
}

/// Version bridges still own separate delta nodes. Reuse only empty immutable
/// payloads to remove repeated allocations without coalescing version history.
pub(super) fn reuse_empty_slice<T>(values: Vec<T>, previous: Option<&Rc<[T]>>) -> Rc<[T]> {
    if values.is_empty()
        && let Some(previous) = previous.filter(|previous| previous.is_empty())
    {
        return Rc::clone(previous);
    }
    values.into()
}

#[cfg(test)]
mod changed_value_range_tests {
    use super::changed_value_ranges;

    #[test]
    fn removed_suffix_does_not_emit_an_out_of_bounds_upload_range() {
        assert!(changed_value_ranges(&[1, 2, 3], &[1]).is_empty());
        let expected = std::iter::once(0..1).collect::<Vec<_>>();
        assert_eq!(changed_value_ranges(&[1, 2, 3], &[9]), expected);
    }

    #[test]
    fn appended_suffix_is_uploaded() {
        let expected = std::iter::once(1..3).collect::<Vec<_>>();
        assert_eq!(changed_value_ranges(&[1], &[1, 2, 3]), expected);
    }
}
