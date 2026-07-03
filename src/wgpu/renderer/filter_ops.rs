use peniko::Mix;

use crate::shared::{
    bounds::Bounds,
    layer::filter::{self as filter_model, Filter},
};

use super::super::{filter::encode_color_filter, filter_resources::WgpuFilterCursors};
use super::{Renderer, WgpuRenderTargetId, encode_morphology_operator, rect_liquid_glass_region};

impl Renderer {
    pub(super) fn apply_filter_graph(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        primitives: &[filter_model::FilterPrimitive],
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        if primitives.is_empty() {
            return self.clear_render_region(target, bounds, 0);
        }

        let mut source_alpha = None;
        let mut outputs = Vec::with_capacity(primitives.len());
        for primitive in primitives {
            let Some(output) = self.apply_filter_graph_primitive(
                target,
                bounds,
                primitive,
                &outputs,
                &mut source_alpha,
                filter_cursors,
            ) else {
                for output in outputs {
                    self.release_scratch(output);
                }
                if let Some(source_alpha) = source_alpha {
                    self.release_scratch(source_alpha);
                }
                return false;
            };
            outputs.push(output);
        }

        let final_output = outputs[outputs.len() - 1];
        let ok = self.clear_render_region(target, bounds, 0)
            && self.copy_region_to_target(final_output, target, bounds);
        for output in outputs {
            self.release_scratch(output);
        }
        if let Some(source_alpha) = source_alpha {
            self.release_scratch(source_alpha);
        }
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_filter_graph_primitive(
        &mut self,
        source_graphic: WgpuRenderTargetId,
        bounds: Bounds,
        primitive: &filter_model::FilterPrimitive,
        outputs: &[WgpuRenderTargetId],
        source_alpha: &mut Option<WgpuRenderTargetId>,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> Option<WgpuRenderTargetId> {
        let region = primitive.region.intersect(bounds);
        match &primitive.kind {
            filter_model::FilterPrimitiveKind::Image { .. } => {
                let output = self.acquire_scratch()?;
                self.clear_render_region(output, bounds, 0);
                if self.flood_region_to_target(output, region, filter_cursors.next_brush_index()) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Identity => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                self.copy_filter_graph_region(input, bounds, region)
            }
            filter_model::FilterPrimitiveKind::Filter(filter) => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let temp = self.acquire_scratch()?;
                if !self.copy_region_to_target(input, temp, bounds)
                    || !self.apply_filter(temp, bounds, filter, None, filter_cursors)
                {
                    self.release_scratch(temp);
                    return None;
                }
                let output = self.copy_filter_graph_region(temp, bounds, region);
                self.release_scratch(temp);
                output
            }
            filter_model::FilterPrimitiveKind::Blend { mode } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_region(output, bounds, 0);
                if self.blend_filter_inputs(input, input2, output, region, *mode) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Composite { operator } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_region(output, bounds, 0);
                if self.composite_filter_inputs(input, input2, output, region, *operator) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Tile { source_region } => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_region(output, bounds, 0);
                if self.tile_filter_input(input, output, region, *source_region) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Merge { inputs } => {
                let output = self.acquire_scratch()?;
                self.clear_render_region(output, bounds, 0);
                for input in inputs {
                    let input = self.resolve_filter_graph_input(
                        source_graphic,
                        *input,
                        outputs,
                        source_alpha,
                        bounds,
                    )?;
                    if !self.source_over_filter_input(input, output, region) {
                        self.release_scratch(output);
                        return None;
                    }
                }
                Some(output)
            }
            filter_model::FilterPrimitiveKind::DisplacementMap(displacement) => {
                let input = self.resolve_filter_graph_input(
                    source_graphic,
                    primitive.input,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let input2 = self.resolve_required_filter_graph_input(
                    source_graphic,
                    primitive,
                    outputs,
                    source_alpha,
                    bounds,
                )?;
                let output = self.acquire_scratch()?;
                self.clear_render_region(output, bounds, 0);
                if self.displacement_map_filter_inputs(input, input2, output, region, displacement)
                {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Turbulence(turbulence) => {
                let output = self.acquire_scratch()?;
                self.clear_render_region(output, bounds, 0);
                if self.turbulence_to_target(
                    output,
                    region,
                    turbulence,
                    filter_cursors.next_turbulence_index(),
                ) {
                    Some(output)
                } else {
                    self.release_scratch(output);
                    None
                }
            }
        }
    }

    fn resolve_required_filter_graph_input(
        &mut self,
        source_graphic: WgpuRenderTargetId,
        primitive: &filter_model::FilterPrimitive,
        outputs: &[WgpuRenderTargetId],
        source_alpha: &mut Option<WgpuRenderTargetId>,
        bounds: Bounds,
    ) -> Option<WgpuRenderTargetId> {
        self.resolve_filter_graph_input(
            source_graphic,
            primitive.input2?,
            outputs,
            source_alpha,
            bounds,
        )
    }

    fn resolve_filter_graph_input(
        &mut self,
        source_graphic: WgpuRenderTargetId,
        input: filter_model::FilterInput,
        outputs: &[WgpuRenderTargetId],
        source_alpha: &mut Option<WgpuRenderTargetId>,
        bounds: Bounds,
    ) -> Option<WgpuRenderTargetId> {
        match input {
            filter_model::FilterInput::SourceGraphic => Some(source_graphic),
            filter_model::FilterInput::Primitive(index) => outputs.get(index).copied(),
            filter_model::FilterInput::SourceAlpha => {
                if let Some(target) = *source_alpha {
                    return Some(target);
                }
                let alpha = self.acquire_scratch()?;
                if self.source_alpha_to_target(source_graphic, alpha, bounds) {
                    *source_alpha = Some(alpha);
                    Some(alpha)
                } else {
                    self.release_scratch(alpha);
                    None
                }
            }
        }
    }

    fn copy_filter_graph_region(
        &mut self,
        input: WgpuRenderTargetId,
        bounds: Bounds,
        region: Bounds,
    ) -> Option<WgpuRenderTargetId> {
        let output = self.acquire_scratch()?;
        self.clear_render_region(output, bounds, 0);
        if self.copy_region_to_target(input, output, region.intersect(bounds)) {
            Some(output)
        } else {
            self.release_scratch(output);
            None
        }
    }

    pub(super) fn apply_filter(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: Option<&crate::shared::layer::region::Region>,
        filter_cursors: &mut WgpuFilterCursors,
    ) -> bool {
        match filter {
            Filter::Graph { primitives, .. } => {
                self.apply_filter_graph(target, bounds, primitives, filter_cursors)
            }
            Filter::Chain { filters, .. } => filters
                .iter()
                .all(|filter| self.apply_filter(target, bounds, filter, region, filter_cursors)),
            Filter::RectLiquidGlass(glass) => {
                let Some(glass_region) = rect_liquid_glass_region(region, bounds) else {
                    return false;
                };
                self.apply_liquid_glass(target, bounds, *glass, glass_region)
            }
            Filter::Offset { dx, dy } => {
                let dx = filter_model::filter_offset_to_pixel_delta(*dx);
                let dy = filter_model::filter_offset_to_pixel_delta(*dy);
                if dx == 0 && dy == 0 {
                    return true;
                }
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                let ok = self.offset_region_to_target(target, temp, bounds, dx, dy)
                    && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => self.apply_blur(target, bounds, *std_dev_x, *std_dev_y, *sampling),
            Filter::ColorMatrix(matrix) => {
                self.apply_color_matrix_to_target(target, bounds, *matrix);
                true
            }
            Filter::ComponentTransfer(_) => {
                let table_index = filter_cursors.next_transfer_index();
                self.apply_component_transfer_to_target(target, bounds, table_index);
                true
            }
            Filter::ConvolveMatrix(matrix) => {
                let kernel_offset = filter_cursors.next_convolve_offset(matrix);
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                let ok =
                    self.convolve_matrix_to_target(target, temp, bounds, matrix, kernel_offset)
                        && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::DiffuseLighting(lighting) => {
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                let ok = self.diffuse_lighting_to_target(target, temp, bounds, lighting)
                    && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::SpecularLighting(lighting) => {
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                let ok = self.specular_lighting_to_target(target, temp, bounds, lighting)
                    && self.copy_region_to_target(temp, target, bounds);
                self.release_scratch(temp);
                ok
            }
            Filter::Flood { .. } => {
                let brush_index = filter_cursors.next_brush_index();
                self.flood_region_to_target(target, bounds, brush_index)
            }
            Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                ..
            } => self.apply_drop_shadow(
                target,
                bounds,
                *offset_x,
                *offset_y,
                *std_dev,
                filter_cursors.next_brush_index(),
            ),
            Filter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => {
                let raw_radius_x = radius_x.max(0.0).ceil() as u32;
                let raw_radius_y = radius_y.max(0.0).ceil() as u32;
                if raw_radius_x == 0 && raw_radius_y == 0 {
                    return true;
                }
                if *operator == filter_model::MorphologyOperator::Erode
                    && (raw_radius_x.saturating_mul(2) >= self.size.0
                        || raw_radius_y.saturating_mul(2) >= self.size.1)
                {
                    return self.clear_render_region(target, bounds, 0);
                }

                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                let Some(output) = self.acquire_scratch() else {
                    self.release_scratch(temp);
                    return false;
                };
                self.clear_render_target(temp, 0);
                self.clear_render_target(output, 0);
                let radius_x = raw_radius_x.min(self.size.0.saturating_sub(1));
                let radius_y = raw_radius_y.min(self.size.1.saturating_sub(1));
                let operator = encode_morphology_operator(*operator);
                let ok = self
                    .morphology_axis_to_target(target, temp, bounds, radius_x, operator, 0)
                    && self.morphology_axis_to_target(temp, output, bounds, radius_y, operator, 1)
                    && self.copy_region_to_target(output, target, bounds);
                self.release_scratch(output);
                self.release_scratch(temp);
                ok
            }
            _ => {
                let Some((filter_kind, amount)) = encode_color_filter(filter) else {
                    return false;
                };
                self.apply_color_filter_to_target(target, bounds, filter_kind, amount);
                true
            }
        }
    }

    pub(super) fn clear_render_target(&self, target: WgpuRenderTargetId, color: u32) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.clear_buffer(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            color,
        );
        true
    }

    pub(super) fn clear_render_region(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        color: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.clear_region(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            color,
        );
        true
    }

    fn flood_region_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        brush_index: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let brushes = self.filter_brush_bindings();
        filter.flood_region(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            brush_index,
            &brushes,
        );
        true
    }

    fn apply_drop_shadow(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        offset_x: f32,
        offset_y: f32,
        std_dev: f32,
        brush_index: u32,
    ) -> bool {
        let Some(shadow) = self.acquire_scratch() else {
            return false;
        };
        self.clear_render_region(shadow, bounds, 0);
        if !self.build_drop_shadow_mask_to_target(
            target,
            shadow,
            bounds,
            offset_x.round() as i32,
            offset_y.round() as i32,
        ) {
            self.release_scratch(shadow);
            return false;
        }

        let std_dev = std_dev.max(0.0);
        if std_dev > 0.0 {
            let Some(temp) = self.acquire_scratch() else {
                self.release_scratch(shadow);
                return false;
            };
            let ok = self.blur_region_to_target(shadow, temp, bounds, std_dev, 0)
                && self.blur_region_to_target(temp, shadow, bounds, std_dev, 1);
            self.release_scratch(temp);
            if !ok {
                self.release_scratch(shadow);
                return false;
            }
        }

        let ok = self.composite_drop_shadow_to_target(target, shadow, bounds, brush_index);
        self.release_scratch(shadow);
        ok
    }

    fn apply_blur(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        if std_dev_x <= 0.0 && std_dev_y <= 0.0 {
            return true;
        }

        let factor = sampling.factor();
        if factor > 1 && std_dev_x > 0.0 && std_dev_y > 0.0 {
            let Some(low) = self.acquire_scratch() else {
                return false;
            };
            let Some(temp) = self.acquire_scratch() else {
                self.release_scratch(low);
                return false;
            };
            let ok = self.downsampled_blur_to_target(
                target, target, low, temp, bounds, std_dev_x, std_dev_y, sampling,
            );
            self.release_scratch(temp);
            self.release_scratch(low);
            return ok;
        }

        let Some(temp) = self.acquire_scratch() else {
            return false;
        };
        let ok = match (std_dev_x > 0.0, std_dev_y > 0.0) {
            (true, true) => {
                self.blur_region_to_target(target, temp, bounds, std_dev_x, 0)
                    && self.blur_region_to_target(temp, target, bounds, std_dev_y, 1)
            }
            (true, false) => {
                self.blur_region_to_target(target, temp, bounds, std_dev_x, 0)
                    && self.copy_region_to_target(temp, target, bounds)
            }
            (false, true) => {
                self.blur_region_to_target(target, temp, bounds, std_dev_y, 1)
                    && self.copy_region_to_target(temp, target, bounds)
            }
            (false, false) => true,
        };
        self.release_scratch(temp);
        ok
    }

    pub(super) fn apply_blur_from_source(
        &mut self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        if source == target {
            return self.apply_blur(target, bounds, std_dev_x, std_dev_y, sampling);
        }

        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        if std_dev_x <= 0.0 && std_dev_y <= 0.0 {
            return self.copy_region_to_target(source, target, bounds);
        }

        let factor = sampling.factor();
        if factor > 1 && std_dev_x > 0.0 && std_dev_y > 0.0 {
            let Some(low) = self.acquire_scratch() else {
                return false;
            };
            let Some(temp) = self.acquire_scratch() else {
                self.release_scratch(low);
                return false;
            };
            let ok = self.downsampled_blur_to_target(
                source, target, low, temp, bounds, std_dev_x, std_dev_y, sampling,
            );
            self.release_scratch(temp);
            self.release_scratch(low);
            return ok;
        }

        match (std_dev_x > 0.0, std_dev_y > 0.0) {
            (true, true) => {
                let Some(temp) = self.acquire_scratch() else {
                    return false;
                };
                let ok = self.blur_region_to_target(source, temp, bounds, std_dev_x, 0)
                    && self.blur_region_to_target(temp, target, bounds, std_dev_y, 1);
                self.release_scratch(temp);
                ok
            }
            (true, false) => self.blur_region_to_target(source, target, bounds, std_dev_x, 0),
            (false, true) => self.blur_region_to_target(source, target, bounds, std_dev_y, 1),
            (false, false) => true,
        }
    }

    pub(super) fn apply_downsampled_blur_rect_composite(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        let factor = sampling.factor();
        if factor <= 1 || std_dev_x <= 0.0 || std_dev_y <= 0.0 {
            return false;
        }
        let Some(low_bounds) = downsampled_bounds(bounds, factor) else {
            return false;
        };
        if low_bounds.width() >= bounds.width() && low_bounds.height() >= bounds.height() {
            return false;
        }
        if !matches!(region, crate::shared::layer::region::Region::Rect { .. }) {
            return false;
        }

        let Some(low) = self.acquire_scratch() else {
            return false;
        };
        let Some(temp) = self.acquire_scratch() else {
            self.release_scratch(low);
            return false;
        };
        let ok = self.downsample_region_to_target(target, low, bounds, low_bounds, sampling)
            && self.blur_region_to_target(low, temp, low_bounds, std_dev_x / factor as f32, 0)
            && self.blur_region_to_target(temp, low, low_bounds, std_dev_y / factor as f32, 1)
            && self.upsample_rect_composite_to_target(
                low, target, bounds, low_bounds, sampling, region,
            );
        self.release_scratch(temp);
        self.release_scratch(low);
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn downsampled_blur_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        low: WgpuRenderTargetId,
        temp: WgpuRenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        let factor = sampling.factor();
        let Some(low_bounds) = downsampled_bounds(bounds, factor) else {
            return false;
        };
        if low_bounds.width() >= bounds.width() && low_bounds.height() >= bounds.height() {
            return if source == target {
                self.blur_region_to_target(source, temp, bounds, std_dev_x, 0)
                    && self.blur_region_to_target(temp, target, bounds, std_dev_y, 1)
            } else {
                self.copy_region_to_target(source, target, bounds)
                    && self.blur_region_to_target(target, temp, bounds, std_dev_x, 0)
                    && self.blur_region_to_target(temp, target, bounds, std_dev_y, 1)
            };
        }

        // Low-resolution pixels live at their global downsampled coordinates in
        // regular full-size scratch targets, so this path avoids per-filter
        // texture allocation while still reducing blur pass work.
        self.downsample_region_to_target(source, low, bounds, low_bounds, sampling)
            && self.blur_region_to_target(low, temp, low_bounds, std_dev_x / factor as f32, 0)
            && self.blur_region_to_target(temp, low, low_bounds, std_dev_y / factor as f32, 1)
            && self.upsample_region_to_target(low, target, bounds, low_bounds, sampling)
    }

    fn build_drop_shadow_mask_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.build_drop_shadow_mask(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            dx,
            dy,
        );
        true
    }

    fn source_alpha_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.source_alpha_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }

    fn source_over_filter_input(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.source_over_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }

    fn blend_filter_inputs(
        &self,
        input1: WgpuRenderTargetId,
        input2: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        mode: Mix,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.blend_region(
            &self.device,
            &self.queue,
            self.render_target_view(input1),
            self.render_target_view(input2),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            mode,
        );
        true
    }

    fn composite_filter_inputs(
        &self,
        input1: WgpuRenderTargetId,
        input2: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        operator: filter_model::CompositeOperator,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.composite_inputs_region(
            &self.device,
            &self.queue,
            self.render_target_view(input1),
            self.render_target_view(input2),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            operator,
        );
        true
    }

    fn displacement_map_filter_inputs(
        &self,
        input1: WgpuRenderTargetId,
        input2: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        displacement: &filter_model::DisplacementMap,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.displacement_map_region(
            &self.device,
            &self.queue,
            self.render_target_view(input1),
            self.render_target_view(input2),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            displacement,
        );
        true
    }

    fn turbulence_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        turbulence: &filter_model::Turbulence,
        table_index: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let tables = self.filter_turbulence_bindings();
        filter.turbulence_region(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            turbulence,
            table_index,
            &tables,
        );
        true
    }

    fn tile_filter_input(
        &self,
        input: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        source_region: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.tile_region(
            &self.device,
            &self.queue,
            self.render_target_view(input),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            source_region,
        );
        true
    }

    fn composite_drop_shadow_to_target(
        &self,
        target: WgpuRenderTargetId,
        shadow: WgpuRenderTargetId,
        bounds: Bounds,
        brush_index: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let brushes = self.filter_brush_bindings();
        filter.composite_drop_shadow(
            &self.device,
            &self.queue,
            self.render_target_view(target),
            self.render_target_view(shadow),
            self.size,
            self.lengths,
            bounds,
            brush_index,
            &brushes,
        );
        true
    }

    fn apply_color_matrix_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        matrix: [f32; 20],
    ) {
        if let Some(filter) = &self.filter {
            filter.apply_color_matrix(
                &self.device,
                &self.queue,
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                matrix,
            );
        }
    }

    fn apply_component_transfer_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        table_index: u32,
    ) {
        if let Some(filter) = &self.filter {
            filter.apply_component_transfer(
                &self.device,
                &self.queue,
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                table_index,
                self.filter_transfers.tables.buffer(),
            );
        }
    }

    fn convolve_matrix_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        matrix: &filter_model::ConvolveMatrix,
        kernel_offset: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.convolve_matrix_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            matrix,
            kernel_offset,
            self.filter_convolves.kernels.buffer(),
        );
        true
    }

    fn diffuse_lighting_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        lighting: &filter_model::DiffuseLighting,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.diffuse_lighting_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            lighting,
            self.surface_origin,
        );
        true
    }

    fn specular_lighting_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        lighting: &filter_model::SpecularLighting,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.specular_lighting_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            lighting,
            self.surface_origin,
        );
        true
    }

    fn apply_liquid_glass(
        &mut self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        let Some(source) = self.acquire_scratch() else {
            return false;
        };
        let Some(blurred) = self.acquire_scratch() else {
            self.release_scratch(source);
            return false;
        };
        let mut ok = self.copy_region_to_target(target, source, bounds);

        if ok && glass.blur_radius > 0 {
            let Some(temp) = self.acquire_scratch() else {
                self.release_scratch(blurred);
                self.release_scratch(source);
                return false;
            };
            let std_dev = glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
            ok = if glass.blur_sampling.factor() > 1 {
                self.downsampled_blur_to_target(
                    source,
                    blurred,
                    temp,
                    blurred,
                    bounds,
                    std_dev,
                    std_dev,
                    glass.blur_sampling,
                )
            } else {
                self.copy_region_to_target(source, blurred, bounds)
                    && self.blur_region_to_target(blurred, temp, bounds, std_dev, 0)
                    && self.blur_region_to_target(temp, blurred, bounds, std_dev, 1)
            };
            self.release_scratch(temp);
        } else if ok {
            ok = self.copy_region_to_target(source, blurred, bounds);
        }

        ok = ok && self.liquid_glass_to_target(source, blurred, target, bounds, glass, region);
        self.release_scratch(blurred);
        self.release_scratch(source);
        ok
    }

    fn liquid_glass_to_target(
        &self,
        source: WgpuRenderTargetId,
        blurred: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.rect_liquid_glass_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(blurred),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            glass,
            region,
        );
        true
    }

    fn blur_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        std_dev: f32,
        axis: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.blur_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            std_dev,
            axis,
        );
        true
    }

    fn downsample_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        source_bounds: Bounds,
        target_bounds: Bounds,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.downsample_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            source_bounds,
            target_bounds,
            sampling,
        );
        true
    }

    fn upsample_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        target_bounds: Bounds,
        source_bounds: Bounds,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.upsample_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            target_bounds,
            source_bounds,
            sampling,
        );
        true
    }

    fn upsample_rect_composite_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        target_bounds: Bounds,
        source_bounds: Bounds,
        sampling: filter_model::BlurSampling,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.upsample_rect_composite_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            target_bounds,
            source_bounds,
            sampling,
            region,
        )
    }

    fn morphology_axis_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        radius: u32,
        operator: u32,
        axis: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.morphology_axis_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            radius,
            operator,
            axis,
        );
        true
    }

    fn offset_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.offset_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            dx,
            dy,
        );
        true
    }

    pub(super) fn copy_region_to_target(
        &self,
        source: WgpuRenderTargetId,
        target: WgpuRenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.copy_region(
            &self.device,
            &self.queue,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }

    pub(super) fn apply_color_filter_to_target(
        &self,
        target: WgpuRenderTargetId,
        bounds: Bounds,
        filter_kind: u32,
        amount: f32,
    ) {
        if let Some(filter) = &self.filter {
            filter.apply_color_filter(
                &self.device,
                &self.queue,
                self.render_target_view(target),
                self.size,
                self.lengths,
                bounds,
                filter_kind,
                amount,
            );
        }
    }
}

fn downsampled_bounds(bounds: Bounds, downsample: u32) -> Option<Bounds> {
    let downsample = downsample.max(1) as i32;
    let bounds = Bounds::new(
        bounds.x0.div_euclid(downsample),
        bounds.y0.div_euclid(downsample),
        div_ceil_i32(bounds.x1, downsample),
        div_ceil_i32(bounds.y1, downsample),
    );
    (!bounds.is_empty()).then_some(bounds)
}

fn div_ceil_i32(value: i32, divisor: i32) -> i32 {
    -((-value).div_euclid(divisor))
}
