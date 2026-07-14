//! Offscreen, filter, backdrop, and mask layer execution.

use super::*;

impl Renderer {
    pub(super) fn execute_offscreen_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        draw: usize,
        layer: &Layer,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        match layer {
            Layer::Isolate => self.execute_masked_group_layer(
                commands,
                retained_id,
                canvas,
                plan,
                draw,
                outer_stack,
                children,
                None,
                None,
                target,
                filter_cursors,
            ),
            Layer::Opacity(opacity) => self.execute_masked_group_layer(
                commands,
                retained_id,
                canvas,
                plan,
                draw,
                outer_stack,
                children,
                Some(opacity.opacity),
                None,
                target,
                filter_cursors,
            ),
            Layer::Blend(blend) => self.execute_masked_group_layer(
                commands,
                retained_id,
                canvas,
                plan,
                draw,
                outer_stack,
                children,
                None,
                Some(blend.mode),
                target,
                filter_cursors,
            ),
            Layer::Filter {
                filter,
                sample_region,
            } => self.execute_filter_layer(
                commands,
                retained_id,
                canvas,
                plan,
                filter,
                sample_region,
                outer_stack,
                children,
                target,
                filter_cursors,
            ),
            Layer::Backdrop {
                filter,
                sample_region,
            } => self.execute_backdrop_layer(
                commands,
                retained_id,
                canvas,
                plan,
                filter,
                sample_region,
                outer_stack,
                children,
                target,
                filter_cursors,
            ),
            Layer::ClipSdf { .. } => false,
            Layer::Clip => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_masked_group_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        draw: usize,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        opacity: Option<f32>,
        blend: Option<peniko::BlendMode>,
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let bounds = draw_bounds(canvas, draw).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            return true;
        }

        let (meta, mut cached) = {
            let _scope = start_cpu_scope("plan.group.cache");
            let meta = retained_id.and_then(|id| {
                self.retained_surface_meta(
                    id,
                    RetainedSurfaceKind::Group,
                    self.size,
                    self.surface_origin,
                    bounds,
                )
            });
            let cached = self.take_matching_retained_surface(retained_id, meta);
            (meta, cached)
        };
        if !self.retained_surface_is_dirty(bounds)
            && let Some((id, surface)) = cached.take()
        {
            let ok = {
                let _scope = start_cpu_scope("plan.group.composite");
                self.composite_cached_group(
                    commands,
                    target,
                    &surface.primary,
                    surface.secondary.as_ref(),
                    bounds,
                    outer_stack,
                    blend,
                )
            };
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            filter_cursors.advance_ops(children);
            return ok;
        }
        let partial = cached
            .as_ref()
            .is_some_and(|(_, surface)| surface.secondary.is_some());
        if retained_id.is_some() {
            let rerendered_tiles = if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_tiles;
        }

        let source = {
            let _scope = start_cpu_scope("plan.group.children");
            if let Some((_, mut surface)) = cached {
                let source = {
                    let _scope = start_cpu_scope("plan.group.scratch");
                    let Some(source) = self.acquire_scratch() else {
                        return false;
                    };
                    self.install_scratch_render_target(source, surface.primary);
                    source
                };
                if let Some(bounds) = self.active_region(bounds) {
                    self.clear_render_region(commands, source, bounds, 0);
                }
                let rendered = profile_cpu("plan.group.render", || {
                    self.execute_ops(
                        commands,
                        canvas,
                        plan,
                        children,
                        source,
                        filter_cursors,
                        None,
                    )
                });
                if !rendered {
                    return false;
                }
                let mask = {
                    let _scope = start_cpu_scope("plan.group.scratch");
                    let Some(mask) = self.acquire_scratch() else {
                        self.release_scratch(source);
                        return false;
                    };
                    self.install_scratch_render_target(
                        mask,
                        surface
                            .secondary
                            .take()
                            .expect("group cache has a retained mask"),
                    );
                    mask
                };
                (source, Some(mask))
            } else {
                let source = profile_cpu("plan.group.render", || {
                    self.render_ops_to_scratch(commands, canvas, plan, children, filter_cursors)
                });
                let Some(source) = source else {
                    return false;
                };
                (source, None)
            }
        };
        let (source, cached_mask) = source;
        if let Some(opacity) = opacity
            && let Some(bounds) = self.active_region(bounds)
        {
            self.apply_color_filter_to_target(commands, source, bounds, FILTER_OPACITY, opacity);
        }

        let mask = if let Some(mask) = cached_mask {
            mask
        } else {
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(source);
                return false;
            };
            mask
        };
        if let Some(bounds) = self.active_region(bounds) {
            let _scope = start_cpu_scope("plan.group.mask");
            self.build_layer_mask(commands, mask, draw as u32, bounds);
        }
        let ok = {
            let _scope = start_cpu_scope("plan.group.composite");
            self.composite_group_targets(commands, target, source, mask, bounds, outer_stack, blend)
        };
        if retained_id.is_some() && meta.is_some() {
            let source = self.take_scratch_target(source);
            let mask = self.take_scratch_target(mask);
            if let (Some(source), Some(mask)) = (source, mask) {
                self.cache_retained_surface(retained_id, meta, source, Some(mask), None);
            }
        } else {
            self.release_scratch(mask);
            self.release_scratch(source);
        }
        ok
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_filter_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        filter: &Filter,
        sample_region: &crate::shared::layer::region::Region,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let target_bounds = Bounds::canvas(self.size.0, self.size.1);
        let Some(filter_bounds) =
            filter_model::filter_surface_bounds(filter, sample_region, target_bounds)
        else {
            filter_cursors.advance_filter_layer(sample_region, children, filter);
            return true;
        };

        let root_filter_cursors = filter_cursors.clone();
        filter_cursors.advance_filter_layer(sample_region, children, filter);
        let surface_size = (
            filter_bounds.surface.width(),
            filter_bounds.surface.height(),
        );
        let surface_origin = (filter_bounds.surface.x0, filter_bounds.surface.y0);
        let meta = retained_id.and_then(|id| {
            self.retained_surface_meta(
                id,
                RetainedSurfaceKind::Filter,
                surface_size,
                surface_origin,
                filter_bounds.output,
            )
        });
        let mut cached = self.take_matching_retained_surface(retained_id, meta);
        if !self.retained_surface_is_dirty(filter_bounds.output)
            && let Some((id, surface)) = cached.take()
        {
            let ok = self.composite_cached_filter_surface(
                commands,
                target,
                &surface.primary,
                surface_size,
                surface_origin,
                filter_bounds.output,
                outer_stack,
            );
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            return ok;
        }
        let local_damage = cached
            .as_ref()
            .filter(|(_, surface)| surface.secondary.is_some())
            .and_then(|_| self.local_damage_for_surface(filter_bounds.surface));
        // A full-canvas filter already uses the local surface coordinate space. Borrowing its
        // immutable scene and plan avoids cloning/translating every retained arena for one dirty
        // allocation; cropped or offset surfaces still take the general translation path.
        let translated_local = (filter_bounds.surface
            != Bounds::canvas(canvas.physical_width(), canvas.physical_height()))
        .then(|| {
            let candidate_draws = self
                .scene_upload
                .draws_in_bounds(filter_bounds.surface, plan);
            profile_cpu("prepare.local_scene", || {
                local_offscreen_scene(
                    canvas,
                    plan,
                    children,
                    filter_bounds.surface,
                    &candidate_draws,
                )
            })
        });
        let (local_canvas, local_plan, local_children) = translated_local
            .as_ref()
            .map_or((canvas, plan, children), |local| {
                (&local.canvas, &local.plan, local.children.as_slice())
            });
        let local_filter = profile_cpu("prepare.local_filter", || {
            local_filter(filter, filter_bounds.surface)
        });
        let local_bounds = Bounds::canvas(
            filter_bounds.surface.width(),
            filter_bounds.surface.height(),
        );
        let local_origin = (
            self.surface_origin.0 + filter_bounds.surface.x0,
            self.surface_origin.1 + filter_bounds.surface.y0,
        );
        let cache_surface = retained_id.is_some() && meta.is_some();
        let local_scratch_count = if cache_surface {
            3 + required_scratch_count(local_plan).max(filter_scratch_extra(&local_filter))
        } else {
            1 + required_scratch_count(local_plan).max(filter_scratch_extra(&local_filter))
        };
        let reuse_root_resources = translated_local.is_none()
            && target == WgpuRenderTargetId::Main
            && self.surface_origin == local_origin;
        let mut saved = if reuse_root_resources {
            self.prepare_scratch_buffers(local_scratch_count.max(1));
            None
        } else {
            Some(self.activate_local_scene_resources(
                local_canvas,
                local_plan,
                &local_filter,
                local_scratch_count,
                local_origin,
            ))
        };

        let source = WgpuRenderTargetId::Scratch(0);
        let filtered = WgpuRenderTargetId::Scratch(1);
        let partial_output =
            if let (Some((_, mut surface)), Some(output_damage)) = (cached, local_damage) {
                let source_history = surface
                    .secondary
                    .take()
                    .expect("partial filter cache has source history");
                self.install_scratch_target(0, source_history);
                self.install_scratch_target(1, surface.primary);
                let output_update = output_damage
                    .bounds_union(surface_size)
                    .unwrap_or(local_bounds);
                // Visible output damage can depend on source pixels outside the
                // root target (for example, a shape below the bottom edge blurred
                // back into view). Redraw the full source dependency window while
                // keeping the filtered write restricted to the visible output.
                self.retained.set_active_tiles(Some(output_damage.outset(
                    surface_size,
                    filter_model::filter_dependency_outset(&local_filter),
                )));
                self.prepare_active_tile_buffers();
                self.clear_render_region(commands, source, local_bounds, 0);
                Some((output_update, output_damage))
            } else {
                self.scratch_in_use[0] = true;
                self.clear_render_target(commands, source, 0);
                None
            };
        if !self.scan_and_cumsum(commands, local_canvas) {
            if let Some(saved) = saved.take() {
                self.restore_root_scene_resources(saved);
            }
            return false;
        }
        let mut local_filter_cursors = if reuse_root_resources {
            root_filter_cursors
        } else {
            WgpuFilterCursors::default()
        };
        let mut ok = self.execute_ops(
            commands,
            local_canvas,
            local_plan,
            local_children,
            source,
            &mut local_filter_cursors,
            None,
        );
        let is_partial_output = partial_output.is_some();
        if let Some((output_update, output_damage)) = partial_output {
            let process_bounds = output_update
                .outset(filter_model::filter_dependency_outset(&local_filter))
                .intersect(local_bounds);
            let Some(temp) = self.acquire_scratch() else {
                if let Some(saved) = saved.take() {
                    self.restore_root_scene_resources(saved);
                }
                return false;
            };
            ok = ok
                && self.copy_region_to_target(commands, source, temp, process_bounds)
                && self.apply_filter(
                    commands,
                    temp,
                    process_bounds,
                    &local_filter,
                    None,
                    &mut local_filter_cursors,
                );
            // The expanded source worklist includes every halo tile sampled by
            // the filter. Switch to the original output list before touching
            // retained filtered history so clean output tiles remain byte-for-
            // byte unchanged.
            self.retained.set_active_tiles(Some(output_damage));
            self.prepare_filter_active_tile_work();
            ok = ok && self.copy_region_to_target(commands, temp, filtered, output_update);
            self.release_scratch(temp);
        } else {
            if cache_surface {
                self.scratch_in_use[1] = true;
                ok = ok && self.copy_region_to_target(commands, source, filtered, local_bounds);
            }
            ok = ok
                && self.apply_filter(
                    commands,
                    source,
                    local_bounds,
                    &local_filter,
                    None,
                    &mut local_filter_cursors,
                );
        }

        let rerendered_local_tiles = self
            .retained
            .active_tiles()
            .map_or_else(|| tile_count_for_bounds(local_bounds), DamageTiles::len);
        let (source_buffer, source_history) = if cache_surface {
            if is_partial_output {
                (
                    self.take_scratch_target(filtered).unwrap(),
                    Some(self.take_scratch_target(source).unwrap()),
                )
            } else {
                (
                    self.take_scratch_target(source).unwrap(),
                    Some(self.take_scratch_target(filtered).unwrap()),
                )
            }
        } else {
            // Keep the remaining scratch allocation set attached to the local resource pool.
            // `take_scratch_target` replaces only the texture whose ownership escapes this render.
            (self.take_scratch_target(source).unwrap(), None)
        };
        if let Some(saved) = saved {
            self.scratch_in_use.clear();
            self.restore_root_scene_resources(saved);
        } else {
            self.scratch_in_use.fill(false);
        }
        if retained_id.is_some() {
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_local_tiles;
        }
        let ok = ok
            && self.composite_cached_filter_surface(
                commands,
                target,
                &source_buffer,
                surface_size,
                surface_origin,
                filter_bounds.output,
                outer_stack,
            );
        self.cache_retained_surface(retained_id, meta, source_buffer, source_history, None);
        ok
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_backdrop_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        filter: &Filter,
        sample_region: &crate::shared::layer::region::Region,
        outer_stack: std::ops::Range<usize>,
        children: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let bounds = filter_model::filtered_region_bounds(
            filter,
            sample_region,
            Bounds::canvas(self.size.0, self.size.1),
        );
        if bounds.is_empty() {
            filter_cursors.next_path_index(sample_region);
            return true;
        }
        let path_index = filter_cursors.next_path_index(sample_region);
        let meta = retained_id.and_then(|id| {
            self.retained_surface_meta(
                id,
                RetainedSurfaceKind::Backdrop,
                self.size,
                self.surface_origin,
                bounds,
            )
        });
        let mut cached = self.take_matching_retained_surface(retained_id, meta);
        let backdrop_dirty = self.retained.backdrop_is_dirty(retained_id);
        if !backdrop_dirty && let Some((id, surface)) = cached.take() {
            let ok = self.composite_cached_backdrop(
                commands,
                target,
                &surface.primary,
                surface.secondary.as_ref(),
                bounds,
                sample_region,
                outer_stack.clone(),
            );
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            filter_cursors.advance_filter(filter);
            return ok
                && self.execute_ops(
                    commands,
                    canvas,
                    plan,
                    children,
                    target,
                    filter_cursors,
                    None,
                );
        }
        let partial = cached.as_ref().is_some_and(|(_, surface)| {
            surface.backdrop_source.is_some()
                && (matches!(filter, Filter::Blur { sampling, .. } if sampling.factor() == 1)
                    || matches!(
                        filter,
                        Filter::RectLiquidGlass(glass) if glass.blur_sampling.factor() == 1
                    ))
        });
        if retained_id.is_some() {
            let rerendered_tiles = if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_tiles;
        }

        let direct_backdrop = retained_id.is_none() || self.retained.bypasses_backdrop_cache();
        if direct_backdrop
            && outer_stack.is_empty()
            && let Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } = filter
            && self.apply_downsampled_blur_rect_composite(
                commands,
                target,
                bounds,
                *std_dev_x,
                *std_dev_y,
                *sampling,
                sample_region,
            )
        {
            return self.execute_ops(
                commands,
                canvas,
                plan,
                children,
                target,
                filter_cursors,
                None,
            );
        }

        if direct_backdrop
            && outer_stack.is_empty()
            && let Filter::RectLiquidGlass(glass) = filter
            && self.apply_downsampled_liquid_glass_rect_composite(
                commands,
                target,
                bounds,
                *glass,
                sample_region,
            )
        {
            return self.execute_ops(
                commands,
                canvas,
                plan,
                children,
                target,
                filter_cursors,
                None,
            );
        }

        // A full-root redraw cannot reuse this frame's temporary source/output history. Keeping
        // it would add a target->history copy to every backdrop (and can double liquid-glass
        // cost under an outer clip) merely to discard or invalidate it on the next moving frame.
        let cache_surface =
            retained_id.is_some() && meta.is_some() && !self.retained.bypasses_backdrop_cache();
        let (backdrop, cached_mask, cached_source) = if let Some((_, mut surface)) = cached {
            let Some(backdrop) = self.acquire_scratch() else {
                return false;
            };
            self.install_scratch_render_target(backdrop, surface.primary);
            (
                backdrop,
                surface.secondary.take(),
                surface.backdrop_source.take(),
            )
        } else {
            let Some(backdrop) = self.acquire_scratch() else {
                return false;
            };
            (backdrop, None, None)
        };

        // A retained root texture stores the final previous frame. Clean tiles
        // therefore contain this backdrop and any later foreground content,
        // not the painter-order input the filter must sample. Preserve that
        // pre-backdrop input separately and update it only from dirty tiles
        // after earlier draw operations have been replayed.
        let source_history = if cache_surface {
            let Some(source) = self.acquire_scratch() else {
                self.release_scratch(backdrop);
                return false;
            };
            let source_update = if cached_source.is_some() {
                self.active_region(bounds)
            } else {
                Some(bounds)
            };
            if let Some(cached_source) = cached_source {
                self.install_scratch_render_target(source, cached_source);
            }
            let source_ok = source_update
                .is_none_or(|bounds| self.copy_region_to_target(commands, target, source, bounds));
            if !source_ok {
                self.release_scratch(source);
                self.release_scratch(backdrop);
                return false;
            }
            Some(source)
        } else {
            None
        };
        let filter_source = source_history.unwrap_or(target);
        // Filters without a partial-update implementation rebuild their whole cached surface.
        // Blur needs explicit intermediate halos. Liquid glass is safe with the root worklist
        // because its cached source is complete and retained damage already includes the full
        // blur/refraction dependency outset needed by every dirty output tile.
        let suspended_active = (!partial).then(|| self.suspend_incremental_filter_work());
        let filter_ok = if partial {
            let output = self
                .active_bounds_union()
                .unwrap_or(bounds)
                .intersect(bounds);
            match filter {
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    ..
                } => {
                    filter_cursors.advance_filter(filter);
                    self.apply_blur_from_source_partial(
                        commands,
                        filter_source,
                        backdrop,
                        output,
                        bounds,
                        *std_dev_x,
                        *std_dev_y,
                    )
                }
                Filter::RectLiquidGlass(glass) => {
                    rect_liquid_glass_region(Some(sample_region), bounds).is_some_and(|region| {
                        self.apply_liquid_glass_from_source_partial(
                            commands,
                            filter_source,
                            backdrop,
                            output,
                            bounds,
                            *glass,
                            region,
                        )
                    })
                }
                _ => unreachable!("only blur and liquid glass support partial backdrop updates"),
            }
        } else {
            match filter {
                Filter::Blur {
                    std_dev_x,
                    std_dev_y,
                    sampling,
                } => self.apply_blur_from_source(
                    commands,
                    filter_source,
                    backdrop,
                    bounds,
                    *std_dev_x,
                    *std_dev_y,
                    *sampling,
                ),
                _ => {
                    self.copy_region_to_target(commands, filter_source, backdrop, bounds)
                        && self.apply_filter(
                            commands,
                            backdrop,
                            bounds,
                            filter,
                            Some(sample_region),
                            filter_cursors,
                        )
                }
            }
        };
        if let Some(active) = suspended_active {
            self.restore_incremental_filter_work(active);
        }
        if !filter_ok {
            if let Some(source) = source_history {
                self.release_scratch(source);
            }
            self.release_scratch(backdrop);
            return false;
        }

        let mut retained_mask = None;
        let ok = if outer_stack.is_empty() {
            self.active_region(bounds).is_none_or(|bounds| {
                self.composite_src_over_rect_mask_direct(
                    commands,
                    target,
                    backdrop,
                    bounds,
                    sample_region,
                )
            })
        } else {
            let mask = if let Some(mask) = cached_mask {
                let Some(mask_target) = self.acquire_scratch() else {
                    if let Some(source) = source_history {
                        self.release_scratch(source);
                    }
                    self.release_scratch(backdrop);
                    return false;
                };
                self.install_scratch_render_target(mask_target, mask);
                mask_target
            } else {
                let Some(mask) = self.acquire_scratch() else {
                    if let Some(source) = source_history {
                        self.release_scratch(source);
                    }
                    self.release_scratch(backdrop);
                    return false;
                };
                if !self.build_region_mask(commands, mask, sample_region, path_index, bounds) {
                    self.release_scratch(mask);
                    if let Some(source) = source_history {
                        self.release_scratch(source);
                    }
                    self.release_scratch(backdrop);
                    return false;
                }
                mask
            };

            let ok = self.active_region(bounds).is_none_or(|bounds| {
                self.composite_src_over_with_stack(
                    commands,
                    target,
                    backdrop,
                    Some(mask),
                    bounds,
                    outer_stack.clone(),
                )
            });
            retained_mask = Some(mask);
            ok
        };
        if cache_surface {
            let backdrop = self.take_scratch_target(backdrop);
            let mask = retained_mask.and_then(|mask| self.take_scratch_target(mask));
            let source = source_history.and_then(|source| self.take_scratch_target(source));
            if let (Some(backdrop), Some(source)) = (backdrop, source) {
                self.cache_retained_surface(retained_id, meta, backdrop, mask, Some(source));
            }
        } else {
            if let Some(mask) = retained_mask {
                self.release_scratch(mask);
            }
            if let Some(source) = source_history {
                self.release_scratch(source);
            }
            self.release_scratch(backdrop);
        }
        ok && self.execute_ops(
            commands,
            canvas,
            plan,
            children,
            target,
            filter_cursors,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn execute_mask_layer(
        &mut self,
        commands: &mut WgpuCommandBatch,
        retained_id: Option<crate::canvas::RetainedSurfaceId>,
        canvas: &Canvas,
        plan: &ExecPlan,
        layer: &crate::shared::layer::mask::Mask,
        outer_stack: std::ops::Range<usize>,
        content: &[ExecOp],
        mask_ops: &[ExecOp],
        target: WgpuRenderTargetId,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        let bounds =
            region_bounds(&layer.region).intersect(Bounds::canvas(self.size.0, self.size.1));
        if bounds.is_empty() {
            filter_cursors.next_path_index(&layer.region);
            return true;
        }
        let path_index = filter_cursors.next_path_index(&layer.region);
        let meta = retained_id.and_then(|id| {
            self.retained_surface_meta(
                id,
                RetainedSurfaceKind::Mask,
                self.size,
                self.surface_origin,
                bounds,
            )
        });
        let mut cached = self.take_matching_retained_surface(retained_id, meta);
        if !self.retained_surface_is_dirty(bounds)
            && let Some((id, surface)) = cached.take()
        {
            let ok = self.composite_cached_group(
                commands,
                target,
                &surface.primary,
                surface.secondary.as_ref(),
                bounds,
                outer_stack,
                None,
            );
            self.retained.stats_mut().reused_offscreen_surfaces += 1;
            self.retained.insert_surface(id, surface);
            filter_cursors.advance_ops(content);
            filter_cursors.advance_ops(mask_ops);
            return ok;
        }
        let partial = cached
            .as_ref()
            .is_some_and(|(_, surface)| surface.secondary.is_some());
        if retained_id.is_some() {
            let rerendered_tiles = if partial {
                self.active_tile_count(bounds)
            } else {
                tile_count_for_bounds(bounds)
            };
            let stats = self.retained.stats_mut();
            stats.rerendered_offscreen_surfaces += 1;
            stats.rerendered_offscreen_tiles += rerendered_tiles;
        }

        let (content_target, cached_mask) = if let Some((_, mut surface)) = cached {
            let Some(content_target) = self.acquire_scratch() else {
                return false;
            };
            self.install_scratch_render_target(content_target, surface.primary);
            if let Some(update) = self.active_region(bounds) {
                self.clear_render_region(commands, content_target, update, 0);
            }
            if !self.execute_ops(
                commands,
                canvas,
                plan,
                content,
                content_target,
                filter_cursors,
                None,
            ) {
                self.release_scratch(content_target);
                return false;
            }
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(content_target);
                return false;
            };
            self.install_scratch_render_target(
                mask,
                surface
                    .secondary
                    .take()
                    .expect("mask cache has retained mask coverage"),
            );
            (content_target, Some(mask))
        } else {
            let Some(content_target) =
                self.render_ops_to_scratch(commands, canvas, plan, content, filter_cursors)
            else {
                return false;
            };
            (content_target, None)
        };
        let Some(mask_source) =
            self.render_ops_to_scratch(commands, canvas, plan, mask_ops, filter_cursors)
        else {
            self.release_scratch(content_target);
            return false;
        };

        let mask = if let Some(mask) = cached_mask {
            mask
        } else {
            let Some(mask) = self.acquire_scratch() else {
                self.release_scratch(mask_source);
                self.release_scratch(content_target);
                return false;
            };
            mask
        };
        if let Some(update) = self.active_region(bounds) {
            self.svg_mask_coverage(commands, mask_source, mask, update, layer.kind);
        }
        self.release_scratch(mask_source);

        let Some(region_mask) = self.acquire_scratch() else {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        };
        let region_ok = self.active_region(bounds).is_none_or(|update| {
            self.build_region_mask(commands, region_mask, &layer.region, path_index, update) && {
                self.apply_region_mask(commands, region_mask, mask, update);
                true
            }
        });
        self.release_scratch(region_mask);
        if !region_ok {
            self.release_scratch(mask);
            self.release_scratch(content_target);
            return false;
        }

        let ok = self.composite_group_targets(
            commands,
            target,
            content_target,
            mask,
            bounds,
            outer_stack,
            None,
        );
        if retained_id.is_some() && meta.is_some() {
            let content = self.take_scratch_target(content_target);
            let mask = self.take_scratch_target(mask);
            if let (Some(content), Some(mask)) = (content, mask) {
                self.cache_retained_surface(retained_id, meta, content, Some(mask), None);
            }
        } else {
            self.release_scratch(mask);
            self.release_scratch(content_target);
        }
        ok
    }
}
