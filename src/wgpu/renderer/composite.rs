//! Scratch-target management and compositing of rendered layer results.

use super::*;

impl Renderer {
    pub(super) fn render_ops_to_scratch(
        &mut self,
        commands: &mut WgpuCommandBatch,
        canvas: &Canvas,
        plan: &ExecPlan,
        ops: &[ExecOp],
        filter_cursors: &mut WgpuFilterCursors,
    ) -> Option<WgpuRenderTargetId> {
        let target = self.acquire_scratch()?;
        self.clear_render_target(commands, target, 0);
        if self.execute_ops(commands, canvas, plan, ops, target, filter_cursors, None) {
            Some(target)
        } else {
            self.release_scratch(target);
            None
        }
    }

    pub(super) fn build_layer_mask(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        draw_ix: u32,
        bounds: Bounds,
    ) {
        if let Some(filter) = &self.filter {
            let bindings = self.scene_buffers.filter_bindings(&self.scan);
            filter.build_layer_mask(
                commands,
                self.render_target_view(target),
                self.size,
                self.lengths,
                &bindings,
                draw_ix,
                bounds,
            );
        }
    }

    pub(super) fn build_region_mask(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        region: &crate::shared::layer::region::Region,
        path_index: Option<u32>,
        bounds: Bounds,
    ) -> bool {
        let paths = self.filter_path_bindings();
        self.filter.as_ref().is_some_and(|filter| {
            filter.build_region_mask(
                commands,
                self.render_target_view(target),
                self.size,
                self.lengths,
                region,
                path_index,
                &paths,
                bounds,
            )
        })
    }

    pub(super) fn svg_mask_coverage(
        &self,
        commands: &mut WgpuCommandBatch,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        kind: crate::shared::layer::mask::MaskKind,
    ) {
        if let Some(filter) = &self.filter {
            filter.svg_mask_coverage(
                commands,
                self.render_target_view(source),
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                kind,
            );
        }
    }

    pub(super) fn apply_region_mask(
        &self,
        commands: &mut WgpuCommandBatch,
        mask: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) {
        if let Some(filter) = &self.filter {
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
                return;
            };
            filter.apply_region_mask(
                commands,
                self.render_target_view(mask),
                self.render_target_view(target),
                target_read,
                self.size,
                self.lengths,
                bounds,
            );
        }
    }

    pub(super) fn composite_src_over_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        mask: Option<WgpuRenderTargetId>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
            return false;
        };
        filter.composite_src_over_with_stack(
            commands,
            self.render_target_view(target),
            target_read,
            self.render_target_view(source),
            mask.map(|mask| self.render_target_view(mask)),
            self.size,
            self.lengths,
            &bindings,
            bounds,
            layer_stack,
        );
        true
    }

    pub(super) fn active_region(&self, bounds: Bounds) -> Option<Bounds> {
        match self.retained.active_tiles() {
            Some(active) if active.intersects_bounds(bounds) => Some(bounds),
            Some(_) => None,
            None => Some(bounds),
        }
    }

    pub(super) fn active_tile_count(&self, bounds: Bounds) -> u32 {
        self.retained.active_tiles().map_or_else(
            || tile_count_for_bounds(bounds),
            |tiles| tiles.count_in_bounds(bounds),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn composite_group_targets(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        mask: WgpuRenderTargetId,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> bool {
        self.active_region(bounds).is_none_or(|bounds| {
            if let Some(mode) = blend {
                self.composite_blend_with_stack(
                    commands,
                    target,
                    source,
                    mask,
                    bounds,
                    layer_stack.clone(),
                    mode,
                )
            } else {
                self.composite_src_over_with_stack(
                    commands,
                    target,
                    source,
                    Some(mask),
                    bounds,
                    layer_stack.clone(),
                )
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn composite_cached_group(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        mask: Option<&WgpuTarget>,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        blend: Option<peniko::BlendMode>,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let Some(mask) = mask else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        self.active_region(bounds).is_none_or(|bounds| {
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
                return false;
            };
            if let Some(mode) = blend {
                filter.composite_blend_with_stack(
                    commands,
                    self.render_target_view(target),
                    target_read,
                    source.view(),
                    mask.view(),
                    self.size,
                    self.lengths,
                    &bindings,
                    bounds,
                    layer_stack.clone(),
                    mode,
                );
            } else {
                filter.composite_src_over_with_stack(
                    commands,
                    self.render_target_view(target),
                    target_read,
                    source.view(),
                    Some(mask.view()),
                    self.size,
                    self.lengths,
                    &bindings,
                    bounds,
                    layer_stack.clone(),
                );
            }
            true
        })
    }

    pub(super) fn composite_src_over_rect_mask_direct(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        bounds: Bounds,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
            return false;
        };
        filter.composite_src_over_rect_mask_direct(
            commands,
            self.render_target_view(target),
            target_read,
            self.render_target_view(source),
            self.size,
            self.lengths,
            bounds,
            region,
        )
    }

    pub(super) fn composite_cached_backdrop(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        mask: Option<&WgpuTarget>,
        bounds: Bounds,
        region: &crate::shared::layer::region::Region,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        if !layer_stack.is_empty() {
            return self.composite_cached_group(
                commands,
                target,
                source,
                mask,
                bounds,
                layer_stack,
                None,
            );
        }
        let Some(filter) = &self.filter else {
            return false;
        };
        self.active_region(bounds).is_none_or(|bounds| {
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
                return false;
            };
            filter.composite_src_over_rect_mask_direct(
                commands,
                self.render_target_view(target),
                target_read,
                source.view(),
                self.size,
                self.lengths,
                bounds,
                region,
            )
        })
    }

    pub(super) fn composite_blend_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: WgpuRenderTargetId,
        mask: WgpuRenderTargetId,
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
        mode: peniko::BlendMode,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
            return false;
        };
        filter.composite_blend_with_stack(
            commands,
            self.render_target_view(target),
            target_read,
            self.render_target_view(source),
            self.render_target_view(mask),
            self.size,
            self.lengths,
            &bindings,
            bounds,
            layer_stack,
            mode,
        );
        true
    }

    pub(super) fn composite_surface_src_over_with_stack(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        source_size: (u32, u32),
        source_origin: (i32, i32),
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let bindings = self.scene_buffers.filter_bindings(&self.scan);
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
            return false;
        };
        filter.composite_src_over_surface_with_stack(
            commands,
            self.render_target_view(target),
            target_read,
            source.view(),
            self.size,
            source_size,
            source_origin,
            self.lengths,
            &bindings,
            bounds,
            layer_stack,
        );
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn composite_cached_filter_surface(
        &self,
        commands: &mut WgpuCommandBatch,
        target: WgpuRenderTargetId,
        source: &WgpuTarget,
        source_size: (u32, u32),
        source_origin: (i32, i32),
        bounds: Bounds,
        layer_stack: std::ops::Range<usize>,
    ) -> bool {
        self.active_region(bounds).is_none_or(|bounds| {
            self.composite_surface_src_over_with_stack(
                commands,
                target,
                source,
                source_size,
                source_origin,
                bounds,
                layer_stack.clone(),
            )
        })
    }
}
