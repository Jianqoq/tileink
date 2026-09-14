use std::collections::HashSet;

use peniko::kurbo::Rect;

use super::super::{Canvas, SceneOffset};
use super::damage_buffer::DamageBuffer;
use super::types::*;
use crate::shared::{
    bounds::Bounds,
    execution::Command,
    layer::{Layer, filter, region::Region},
};

impl RetainedDamage {
    pub(crate) fn add_node(&mut self, id: RetainedNodeId, bounds: Bounds) {
        if bounds.is_empty() {
            return;
        }
        self.node_bounds
            .entry(id)
            .and_modify(|current| *current = current.union(bounds))
            .or_insert(bounds);
    }

    pub(crate) fn add_unattributed(&mut self, bounds: Bounds) {
        push_unique_damage(&mut self.unattributed, bounds);
    }
}

impl RetainedFrame {
    pub(crate) fn node_state(&self, id: RetainedNodeId) -> Option<RetainedNodeState> {
        let mut delta = self.delta.as_deref();
        while let Some(current) = delta {
            if let Some(&index) = current.index.get(&id) {
                return current.patches[index].new;
            }
            delta = current.previous.as_deref();
        }
        self.node_index.get(&id).map(|&index| {
            const PAGE_SIZE: usize = 256;
            self.state_pages
                .get(&(index / PAGE_SIZE))
                .map_or(self.nodes[index], |page| page[index % PAGE_SIZE])
        })
    }

    pub(crate) fn node_revision(&self, id: RetainedNodeId) -> Option<NodeGeneration> {
        self.node_state(id).map(|node| node.revision)
    }
}

impl Canvas {
    /// Applies journal damage to the internal materialized canvas.
    pub(crate) fn invalidate_rect(&mut self, rect: Rect) {
        assert!(
            rect.x0.is_finite()
                && rect.y0.is_finite()
                && rect.x1.is_finite()
                && rect.y1.is_finite(),
            "invalidated rectangle must be finite"
        );
        if rect.is_zero_area() {
            return;
        }
        let rect = self.physical_rect(rect);
        let bounds = Bounds::new(
            rect.x0.floor() as i32,
            rect.y0.floor() as i32,
            rect.x1.ceil() as i32,
            rect.y1.ceil() as i32,
        )
        .intersect(Bounds::canvas(
            self.physical_width(),
            self.physical_height(),
        ));
        if !bounds.is_empty() {
            self.invalidated_bounds.push(bounds);
        }
    }

    pub(crate) fn retained_frame(&self) -> Option<RetainedFrame> {
        self.persistent_frame.clone()
    }

    pub(crate) fn visual_bounds(&self) -> Bounds {
        canvas_visual_bounds(self)
    }
    /// Propagates already-known output damage through filter dependencies.
    ///
    /// The retained diff identifies changed component pixels. This pass adds
    /// pixels whose value depends on those changes, notably blur/shadow output
    /// and backdrop regions. It is deliberately conservative for graph filters:
    /// an uncertain dependency redraws that layer, never unrelated root tiles.
    pub(crate) fn propagate_damage(&self, damage: &RetainedDamage) -> RetainedDamagePropagation {
        let mut propagated = DamageBuffer::new(&damage.unattributed);
        let mut dirty_backdrops = HashSet::new();
        propagate_list_damage(
            self,
            self.root_commands,
            damage,
            &mut propagated,
            &mut dirty_backdrops,
            None,
        );
        let mut propagated = propagated.into_vec();
        // Keep intermediate off-canvas dependencies until every enclosing
        // filter has consumed them, then constrain the delivered root damage.
        let root = Bounds::canvas(self.physical_width(), self.physical_height());
        for bounds in &mut propagated {
            *bounds = bounds.intersect(root);
        }
        propagated.retain(|bounds| !bounds.is_empty());
        RetainedDamagePropagation {
            bounds: propagated,
            dirty_backdrops,
        }
    }
}
fn canvas_visual_bounds(canvas: &Canvas) -> Bounds {
    list_visual_bounds(
        canvas,
        canvas.root_commands,
        SceneOffset { dx: 0.0, dy: 0.0 },
    )
}

fn list_visual_bounds(canvas: &Canvas, list_id: usize, offset: SceneOffset) -> Bounds {
    canvas.command_lists[list_id]
        .commands
        .iter()
        .fold(empty_bounds(), |bounds, command| {
            bounds.union(match command {
                Command::Draw(draw) => {
                    offset.bounds(pixel_bounds(canvas.draw_records[*draw].pixel_bounds))
                }
                Command::MaterializedRetainedScene { children, .. } => {
                    list_visual_bounds(canvas, *children, offset)
                }
                Command::Layer {
                    draw,
                    layer,
                    children,
                    ..
                } => layer_bounds(
                    canvas,
                    *draw,
                    layer,
                    list_visual_bounds(canvas, *children, offset),
                    offset,
                ),
                Command::MaskLayer { layer, content, .. } => {
                    list_visual_bounds(canvas, *content, offset)
                        .intersect(offset.bounds(region_bounds(&layer.region)))
                }
            })
        })
}

fn layer_bounds(
    canvas: &Canvas,
    draw: usize,
    layer: &Layer,
    child_bounds: Bounds,
    offset: SceneOffset,
) -> Bounds {
    match layer {
        Layer::Clip
        | Layer::ClipSdf { .. }
        | Layer::Isolate
        | Layer::Opacity(_)
        | Layer::Blend(_) => child_bounds
            .intersect(offset.bounds(pixel_bounds(canvas.draw_records[draw].pixel_bounds))),
        Layer::Filter {
            filter: value,
            sample_region,
        } => offset.bounds(filter::unclipped_filtered_region_bounds(
            value,
            sample_region,
        )),
        Layer::Backdrop {
            filter: value,
            sample_region,
        } => child_bounds.union(offset.bounds(filter::unclipped_filtered_region_bounds(
            value,
            sample_region,
        ))),
    }
}

fn region_bounds(region: &Region) -> Bounds {
    filter::region_bounds(region)
}

fn pixel_bounds(bounds: crate::shared::bounds::PixelBounds) -> Bounds {
    Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1)
}

fn empty_bounds() -> Bounds {
    Bounds::new(0, 0, 0, 0)
}

// Deleted commands have no current painter-order position. Seed their damage
// in every offscreen dependency domain; carrying the outer accumulated damage
// instead would incorrectly make isolated inputs depend on outside drawing.
fn propagate_list_damage(
    canvas: &Canvas,
    list_id: usize,
    pending: &RetainedDamage,
    damage: &mut DamageBuffer,
    dirty_backdrops: &mut HashSet<RetainedNodeId>,
    retained_owner: Option<RetainedNodeId>,
) {
    for command in &canvas.command_lists[list_id].commands {
        match command {
            Command::Draw(_) => {}
            Command::MaterializedRetainedScene { id, children, .. } => {
                propagate_list_damage(
                    canvas,
                    *children,
                    pending,
                    damage,
                    dirty_backdrops,
                    Some(*id),
                );
                append_node_damage(pending, *id, damage);
            }
            Command::Layer {
                retained,
                layer,
                children,
                ..
            } => {
                let retained_owner = retained.map(|key| key.id).or(retained_owner);
                // Fused clip coverage changes the pixels drawn before a descendant
                // backdrop samples them. Group/filter parameters affect only their
                // completed output and must remain after the child traversal.
                if matches!(layer, Layer::Clip | Layer::ClipSdf { .. })
                    && let Some(key) = retained
                {
                    append_node_damage(pending, key.id, damage);
                }
                match layer {
                    Layer::Backdrop {
                        filter: value,
                        sample_region,
                    } => {
                        let source_changed = propagate_filter_damage(value, sample_region, damage);
                        if source_changed
                            || retained_owner
                                .is_some_and(|id| pending.node_bounds.contains_key(&id))
                        {
                            dirty_backdrops.extend(retained_owner);
                        }
                        // The backdrop's own output is already painted before its children
                        // sample it. Parameter changes must reach nested backdrops here;
                        // appending only after children left their cached inputs stale.
                        if let Some(key) = retained {
                            append_node_damage(pending, key.id, damage);
                        }
                        // Children remain later in painter order and cannot invalidate
                        // this backdrop in the same frame.
                        propagate_list_damage(
                            canvas,
                            *children,
                            pending,
                            damage,
                            dirty_backdrops,
                            retained_owner,
                        );
                    }
                    Layer::Filter {
                        filter: value,
                        sample_region,
                    } => {
                        let scope = damage.enter_scope(&pending.unattributed);
                        propagate_list_damage(
                            canvas,
                            *children,
                            pending,
                            damage,
                            dirty_backdrops,
                            retained_owner,
                        );
                        propagate_filter_damage(value, sample_region, damage);
                        damage.finish_scope(scope);
                    }
                    Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_)
                        if !canvas.can_fuse(layer, *children) =>
                    {
                        // Use the compiler's actual offscreen boundary. Outside
                        // drawing/clip changes cannot enter an isolated input texture.
                        let scope = damage.enter_scope(&pending.unattributed);
                        propagate_list_damage(
                            canvas,
                            *children,
                            pending,
                            damage,
                            dirty_backdrops,
                            retained_owner,
                        );
                        damage.finish_scope(scope);
                    }
                    _ => propagate_list_damage(
                        canvas,
                        *children,
                        pending,
                        damage,
                        dirty_backdrops,
                        retained_owner,
                    ),
                }
                if let Some(retained) = retained
                    && !matches!(layer, Layer::Backdrop { .. })
                {
                    append_node_damage(pending, retained.id, damage);
                }
            }
            Command::MaskLayer {
                retained,
                content,
                mask,
                ..
            } => {
                let retained_owner = retained.map(|key| key.id).or(retained_owner);
                // Content and mask use independent input textures. Their output
                // damage joins only after both branches finish, so a content edit
                // cannot invalidate a backdrop that samples the mask branch.
                for children in [*content, *mask] {
                    let scope = damage.enter_scope(&pending.unattributed);
                    propagate_list_damage(
                        canvas,
                        children,
                        pending,
                        damage,
                        dirty_backdrops,
                        retained_owner,
                    );
                    damage.finish_scope(scope);
                }
                if let Some(retained) = retained {
                    append_node_damage(pending, retained.id, damage);
                }
            }
        }
    }
}

fn propagate_filter_damage(
    value: &filter::Filter,
    sample_region: &Region,
    damage: &mut DamageBuffer,
) -> bool {
    let dependency = filter::filter_dependency(value);
    let input = filter::filter_input_bounds(value, sample_region);
    let output = filter::unclipped_filtered_region_bounds(value, sample_region);
    let initial_len = damage.current().len();
    let mut changed = false;
    for index in 0..initial_len {
        let affected = dependency.affected_output(damage.current()[index], input, output);
        if !affected.is_empty() {
            changed = true;
            damage.push_unique(affected);
        }
    }
    changed
}

fn append_node_damage(pending: &RetainedDamage, id: RetainedNodeId, damage: &mut DamageBuffer) {
    if let Some(bounds) = pending.node_bounds.get(&id) {
        damage.push_unique(*bounds);
    }
}

fn push_unique_damage(damage: &mut Vec<Bounds>, bounds: Bounds) {
    if !bounds.is_empty() && !damage.contains(&bounds) {
        damage.push(bounds);
    }
}

#[cfg(test)]
mod filter_dependency_tests {
    use super::*;

    fn region() -> Region {
        Region::Rect {
            rect: peniko::kurbo::Rect::new(16.0, 16.0, 48.0, 48.0),
            radius: crate::Radius::ZERO,
        }
    }

    #[test]
    fn erosion_input_changes_damage_neighbouring_output_pixels() {
        let value = filter::Filter::Morphology {
            radius_x: 1.0,
            radius_y: 0.0,
            operator: filter::MorphologyOperator::Erode,
        };
        let mut damage = DamageBuffer::new(&[Bounds::new(32, 24, 33, 25)]);
        assert!(propagate_filter_damage(&value, &region(), &mut damage));
        assert!(
            damage.current().contains(&Bounds::new(31, 23, 34, 26)),
            "output extent zero does not mean pointwise damage"
        );
    }

    #[test]
    fn wrapped_source_damage_reaches_the_opposite_edge_without_dirtying_the_root() {
        let value = filter::Filter::ConvolveMatrix(filter::ConvolveMatrix {
            columns: 3,
            rows: 1,
            target_x: 1,
            target_y: 0,
            data: vec![0.0, 0.0, 1.0],
            divisor: 1.0,
            bias: 0.0,
            edge_mode: filter::ConvolveEdgeMode::Wrap,
            preserve_alpha: false,
        });
        let mut damage = DamageBuffer::new(&[Bounds::new(47, 24, 48, 25)]);
        assert!(propagate_filter_damage(&value, &region(), &mut damage));
        assert!(damage.current().contains(&Bounds::new(16, 16, 48, 48)));
        assert!(
            damage
                .current()
                .iter()
                .all(|bounds| *bounds == bounds.intersect(Bounds::new(16, 16, 48, 48)))
        );
        let mut outside = DamageBuffer::new(&[Bounds::new(96, 96, 97, 97)]);
        assert!(!propagate_filter_damage(&value, &region(), &mut outside));
        assert_eq!(outside.current(), &[Bounds::new(96, 96, 97, 97)]);
    }
}

#[cfg(test)]
mod nested_filter_damage_tests {
    use super::*;
    use peniko::kurbo::Rect;

    #[test]
    fn off_canvas_offset_damage_is_preserved_until_outer_wrap_sampling() {
        let region = |x0, x1| Region::Rect {
            rect: Rect::new(x0, 0.0, x1, 64.0),
            radius: crate::Radius::ZERO,
        };
        let mut canvas = Canvas::new(64, 64, 1.0);
        canvas.push_filter_layer(
            filter::Filter::ConvolveMatrix(filter::ConvolveMatrix {
                columns: 3,
                rows: 1,
                target_x: 1,
                target_y: 0,
                data: vec![1.0, 0.0, 0.0],
                divisor: 1.0,
                bias: 0.0,
                edge_mode: filter::ConvolveEdgeMode::Wrap,
                preserve_alpha: false,
            }),
            region(-16.0, 48.0),
        );
        canvas.push_filter_layer(
            filter::Filter::Offset { dx: 16.0, dy: 0.0 },
            region(-32.0, -31.0),
        );
        canvas.pop_layer();
        canvas.pop_layer();
        let mut damage = RetainedDamage::default();
        damage.add_unattributed(Bounds::new(-32, 0, -31, 64));
        let propagated = canvas.propagate_damage(&damage);
        assert!(
            propagated
                .bounds
                .iter()
                .any(|bounds| !bounds.intersect(Bounds::new(47, 0, 48, 64)).is_empty()),
            "an invisible intermediate can be sampled by a visible ancestor"
        );
        assert!(
            propagated
                .bounds
                .iter()
                .all(|bounds| *bounds == bounds.intersect(Bounds::canvas(64, 64))),
            "only the final root result is clipped to the canvas"
        );
    }
}
