use super::*;

pub(super) struct OffscreenLayerRef<'a> {
    pub(super) draw: usize,
    pub(super) layer: &'a Layer,
    pub(super) outer_stack: std::ops::Range<usize>,
    pub(super) children: &'a [ExecOp],
}

pub(super) struct MaskLayerRef<'a> {
    pub(super) layer: &'a Mask,
    pub(super) outer_stack: std::ops::Range<usize>,
    pub(super) content: &'a [ExecOp],
    pub(super) mask: &'a [ExecOp],
}

struct MaskedGroupLayer<'a> {
    draw: usize,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
    opacity: Option<f32>,
    composite: LayerComposite,
}

struct FilterLayerRef<'a> {
    filter: &'a Filter,
    sample_region: &'a Region,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
}

struct BackdropLayerRef<'a> {
    filter: &'a Filter,
    sample_region: &'a Region,
    outer_stack: std::ops::Range<usize>,
    children: &'a [ExecOp],
}

#[derive(Clone, Copy)]
enum LayerComposite {
    SrcOver,
    Blend(BlendMode),
}

impl<R: Runtime> Renderer<R> {
    pub(super) fn execute_offscreen_layer(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        offscreen: OffscreenLayerRef<'_>,
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        match offscreen.layer {
            Layer::Isolate => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw: offscreen.draw,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                    opacity: None,
                    composite: LayerComposite::SrcOver,
                },
                target,
                filter_cursors,
            ),
            Layer::Opacity(opacity) => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw: offscreen.draw,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                    opacity: Some(opacity.opacity),
                    composite: LayerComposite::SrcOver,
                },
                target,
                filter_cursors,
            ),
            Layer::Blend(blend) => self.execute_masked_group_layer(
                scene,
                plan,
                MaskedGroupLayer {
                    draw: offscreen.draw,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                    opacity: None,
                    composite: LayerComposite::Blend(blend.mode),
                },
                target,
                filter_cursors,
            ),
            Layer::Filter {
                filter,
                sample_region,
            } => self.execute_filter_layer_with_local_surface(
                scene,
                plan,
                FilterLayerRef {
                    filter,
                    sample_region,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                },
                target,
                filter_cursors,
            ),
            Layer::Backdrop {
                filter,
                sample_region,
            } => self.execute_backdrop_layer(
                scene,
                plan,
                BackdropLayerRef {
                    filter,
                    sample_region,
                    outer_stack: offscreen.outer_stack,
                    children: offscreen.children,
                },
                target,
                filter_cursors,
            ),
            Layer::ClipSdf { .. } => {
                panic!("CubeCL ClipSdf layers are not implemented; use the CPU renderer")
            }
            _ => panic!(
                "CubeCL offscreen execution only accepts Opacity, Filter, and Backdrop layers"
            ),
        }
    }

    fn execute_masked_group_layer(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        group: MaskedGroupLayer<'_>,
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        let bounds =
            draw_bounds(scene, group.draw).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            return;
        }

        let source = self.render_ops_to_scratch(scene, plan, group.children, filter_cursors);
        if let Some(opacity) = group.opacity {
            self.apply_color_filter(source, bounds, FILTER_OPACITY, opacity);
        }

        let mask = self.acquire_scratch();
        self.clear_buffer(mask, 0);
        self.build_layer_mask(mask, group.draw as u32, bounds);
        match group.composite {
            LayerComposite::SrcOver => self.composite_src_over_with_stack(
                target,
                source,
                Some(mask),
                bounds,
                group.outer_stack,
            ),
            LayerComposite::Blend(mode) => self.composite_blend_with_stack(
                target,
                source,
                mask,
                bounds,
                group.outer_stack,
                mode,
            ),
        }
        self.release_scratch(mask);
        self.release_scratch(source);
    }

    fn render_ops_to_scratch(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        ops: &[ExecOp],
        filter_cursors: &mut FilterCursors,
    ) -> CubeRenderTarget {
        let target = self.acquire_scratch();
        self.clear_buffer(target, 0);
        self.execute_ops(scene, plan, ops, target, filter_cursors);
        target
    }

    fn execute_filter_layer_with_local_surface(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        layer: FilterLayerRef<'_>,
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        let target_bounds = Bounds::canvas(self.size.0, self.size.1);
        let Some(filter_bounds) =
            filter_model::filter_surface_bounds(layer.filter, layer.sample_region, target_bounds)
        else {
            filter_cursors.advance_filter_layer(layer.sample_region, layer.children, layer.filter);
            return;
        };

        filter_cursors.advance_filter_layer(layer.sample_region, layer.children, layer.filter);
        let local = local_offscreen_scene(scene, plan, layer.children, filter_bounds.surface);
        let local_filter = local_filter(layer.filter, filter_bounds.surface);
        let local_bounds = Bounds::canvas(
            filter_bounds.surface.width(),
            filter_bounds.surface.height(),
        );
        let local_origin = (
            self.surface_origin.0 + filter_bounds.surface.x0,
            self.surface_origin.1 + filter_bounds.surface.y0,
        );
        let local_scratch_count =
            1 + required_scratch_count(&local.plan).max(filter_scratch_extra(&local_filter));
        let saved = self.activate_local_scene_resources(
            &local.scene,
            &local.plan,
            &local_filter,
            local_scratch_count,
            local_origin,
        );

        let source = CubeRenderTarget::Scratch(0);
        self.scratch_in_use[0] = true;
        self.clear_buffer(source, 0);
        ScanPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
        CumsumPipeline::run(&self.client, &self.scene, &mut self.scan, self.lengths);
        let mut local_filter_cursors = FilterCursors::default();
        self.execute_ops(
            &local.scene,
            &local.plan,
            &local.children,
            source,
            &mut local_filter_cursors,
        );
        self.apply_filter(
            source,
            local_bounds,
            &local_filter,
            &mut local_filter_cursors,
        );

        let mut local_scratch = std::mem::take(&mut self.scratch);
        let source_buffer = local_scratch.remove(0);
        self.scratch_in_use.clear();
        self.restore_root_scene_resources(saved);
        self.composite_surface_src_over_with_stack(
            target,
            &source_buffer,
            (
                filter_bounds.surface.width(),
                filter_bounds.surface.height(),
            ),
            (filter_bounds.surface.x0, filter_bounds.surface.y0),
            filter_bounds.output,
            layer.outer_stack,
        );
        self.surface_sources.push(source_buffer);
    }

    fn execute_backdrop_layer(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        layer: BackdropLayerRef<'_>,
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        let bounds = filter_model::filtered_region_bounds(
            layer.filter,
            layer.sample_region,
            Bounds::canvas(self.size.0, self.size.1),
        );
        let path_index = filter_cursors.next_path_index(layer.sample_region);
        let backdrop = self.acquire_scratch();
        self.clear_buffer(backdrop, 0);
        self.copy_region(target, backdrop, bounds);
        self.apply_filter(backdrop, bounds, layer.filter, filter_cursors);
        let mask = self.acquire_scratch();
        self.clear_buffer(mask, 0);
        self.build_region_mask(mask, layer.sample_region, path_index, bounds);
        self.composite_src_over_with_stack(
            target,
            backdrop,
            Some(mask),
            bounds,
            layer.outer_stack.clone(),
        );
        self.release_scratch(mask);
        self.release_scratch(backdrop);

        let content = self.render_ops_to_scratch(scene, plan, layer.children, filter_cursors);
        self.composite_src_over_with_stack(
            target,
            content,
            None,
            Bounds::canvas(self.size.0, self.size.1),
            layer.outer_stack,
        );
        self.release_scratch(content);
    }

    pub(super) fn execute_mask_layer(
        &mut self,
        scene: &Scene,
        plan: &ExecPlan,
        mask_layer: MaskLayerRef<'_>,
        target: CubeRenderTarget,
        filter_cursors: &mut FilterCursors,
    ) {
        let bounds = region_bounds(&mask_layer.layer.region)
            .intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            return;
        }
        let path_index = filter_cursors.next_path_index(&mask_layer.layer.region);

        let content = self.render_ops_to_scratch(scene, plan, mask_layer.content, filter_cursors);
        let mask_source = self.render_ops_to_scratch(scene, plan, mask_layer.mask, filter_cursors);

        let mask = self.acquire_scratch();
        self.clear_buffer(mask, 0);
        self.svg_mask_coverage(mask_source, mask, bounds, mask_layer.layer.kind);
        self.release_scratch(mask_source);

        let region_mask = self.acquire_scratch();
        self.clear_buffer(region_mask, 0);
        self.build_region_mask(region_mask, &mask_layer.layer.region, path_index, bounds);
        self.apply_region_mask(region_mask, mask, bounds);
        self.release_scratch(region_mask);

        self.composite_src_over_with_stack(
            target,
            content,
            Some(mask),
            bounds,
            mask_layer.outer_stack,
        );
        self.release_scratch(mask);
        self.release_scratch(content);
    }
}
