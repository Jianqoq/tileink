use super::*;
impl Renderer {
    pub(in crate::wgpu::renderer) fn clear_render_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        color: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        if self.retained.active_tiles().is_some() {
            filter.clear_region(
                commands,
                self.render_target_view(target),
                self.size,
                self.lengths,
                Bounds::canvas(self.size.0, self.size.1),
                color,
            );
        } else {
            filter.clear_buffer(
                commands,
                self.render_target_view(target),
                self.size,
                self.lengths,
                color,
            );
        }
        true
    }
    pub(in crate::wgpu::renderer) fn clear_render_region(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        color: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.clear_region(
            commands,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            color,
        );
        true
    }
    pub(super) fn flood_region_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        brush_offset: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let brushes = self.filter_brush_bindings();
        filter.flood_region(
            commands,
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            brush_offset,
            &brushes,
        );
        true
    }
    pub(super) fn build_drop_shadow_mask_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.build_drop_shadow_mask(
            commands,
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
    pub(super) fn source_alpha_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.source_alpha_region(
            commands,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }
    pub(super) fn source_over_filter_input(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
            return false;
        };
        filter.source_over_region(
            commands,
            self.render_target_view(source),
            self.render_target_view(target),
            target_read,
            self.size,
            self.lengths,
            bounds,
        );
        true
    }
    pub(super) fn blend_filter_inputs(
        &self,
        commands: &mut WgpuCommandBatch,
        input1: RenderTargetId,
        input2: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        mode: Mix,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
            return false;
        };
        filter.blend_region(
            commands,
            self.render_target_view(input1),
            self.render_target_view(input2),
            self.render_target_view(target),
            target_read,
            self.size,
            self.lengths,
            bounds,
            mode,
        );
        true
    }
    pub(super) fn composite_filter_inputs(
        &self,
        commands: &mut WgpuCommandBatch,
        input1: RenderTargetId,
        input2: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        operator: filter_model::CompositeOperator,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.composite_inputs_region(
            commands,
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
    pub(super) fn displacement_map_filter_inputs(
        &self,
        commands: &mut WgpuCommandBatch,
        input1: RenderTargetId,
        input2: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        displacement: &filter_model::DisplacementMap,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.displacement_map_region(
            commands,
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
    pub(super) fn turbulence_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        turbulence: &filter_model::Turbulence,
        table_index: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let tables = self.filter_turbulence_bindings();
        filter.turbulence_region(
            commands,
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
    pub(super) fn tile_filter_input(
        &self,
        commands: &mut WgpuCommandBatch,
        input: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        source_region: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.tile_region(
            commands,
            self.render_target_view(input),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
            source_region,
        );
        true
    }
    pub(super) fn composite_drop_shadow_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        shadow: RenderTargetId,
        bounds: Bounds,
        brush_offset: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        let brushes = self.filter_brush_bindings();
        let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
            return false;
        };
        filter.composite_drop_shadow(
            commands,
            self.render_target_view(target),
            target_read,
            self.render_target_view(shadow),
            self.size,
            self.lengths,
            bounds,
            brush_offset,
            &brushes,
        );
        true
    }
    pub(super) fn apply_color_matrix_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        matrix: [f32; 20],
    ) -> bool {
        if let Some(filter) = &self.filter {
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
                return false;
            };
            filter.apply_color_matrix(
                commands,
                self.render_target_view(target),
                target_read,
                self.size,
                self.lengths,
                bounds,
                matrix,
            );
            true
        } else {
            false
        }
    }
    pub(super) fn apply_component_transfer_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        table_index: u32,
    ) -> bool {
        if let Some(filter) = &self.filter {
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
                return false;
            };
            filter.apply_component_transfer(
                commands,
                self.render_target_view(target),
                target_read,
                self.size,
                self.lengths,
                bounds,
                table_index,
                self.filter_transfers.tables.buffer(),
            );
            true
        } else {
            false
        }
    }
    pub(super) fn convolve_matrix_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        matrix: &filter_model::ConvolveMatrix,
        kernel_offset: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.convolve_matrix_region(
            commands,
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
    pub(super) fn diffuse_lighting_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        lighting: &filter_model::DiffuseLighting,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.diffuse_lighting_region(
            commands,
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
    pub(super) fn specular_lighting_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        lighting: &filter_model::SpecularLighting,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.specular_lighting_region(
            commands,
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
    pub(super) fn liquid_glass_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        blurred: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.rect_liquid_glass_region(
            commands,
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
    pub(super) fn blur_region_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev: f32,
        axis: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.blur_region(
            commands,
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn blur_region_partial_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        std_dev: f32,
        axis: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.blur_region_partial(
            commands,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            output_bounds,
            sample_bounds,
            std_dev,
            axis,
        );
        true
    }
    pub(super) fn morphology_axis_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        radius: u32,
        operator: u32,
        axis: u32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.morphology_axis_region(
            commands,
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
    pub(super) fn offset_region_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        dx: i32,
        dy: i32,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.offset_region(
            commands,
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
    pub(in crate::wgpu::renderer) fn copy_region_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
    ) -> bool {
        let Some(filter) = &self.filter else {
            return false;
        };
        filter.copy_region(
            commands,
            self.render_target_view(source),
            self.render_target_view(target),
            self.size,
            self.lengths,
            bounds,
        );
        true
    }
    pub(in crate::wgpu::renderer) fn apply_color_filter_to_target(
        &self,
        commands: &mut WgpuCommandBatch,
        target: RenderTargetId,
        bounds: Bounds,
        filter_kind: u32,
        amount: f32,
    ) -> bool {
        if let Some(filter) = &self.filter {
            let Some(target_read) = self.snapshot_filter_target(commands, target, bounds) else {
                return false;
            };
            filter.apply_color_filter(
                commands,
                self.render_target_view(target),
                target_read,
                self.size,
                self.lengths,
                bounds,
                filter_kind,
                amount,
            );
            true
        } else {
            false
        }
    }
}
