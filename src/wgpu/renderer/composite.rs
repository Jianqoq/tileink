//! Scratch-target management and compositing of rendered layer results.

use super::*;

impl Renderer {
    pub(super) fn build_layer_mask(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
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
        target: RenderTargetId,
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
        source: RenderTargetId,
        target: RenderTargetId,
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
        mask: RenderTargetId,
        target: RenderTargetId,
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
        target: RenderTargetId,
        source: RenderTargetId,
        mask: Option<RenderTargetId>,
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn composite_group_targets(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        source: RenderTargetId,
        mask: RenderTargetId,
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
        target: RenderTargetId,
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
        target: RenderTargetId,
        source: RenderTargetId,
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

    pub(super) fn composite_cached_backdrop_rect(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        source: &WgpuTarget,
        bounds: Bounds,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
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
        target: RenderTargetId,
        source: RenderTargetId,
        mask: RenderTargetId,
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
        target: RenderTargetId,
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
        target: RenderTargetId,
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
