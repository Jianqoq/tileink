use super::*;
// Concrete textures, shader uniforms, snapshots and command encoding stay in WGPU.
pub(super) struct WgpuFilterEncoder<'a> {
    pub(super) renderer: &'a mut Renderer,
    pub(super) commands: &'a mut WgpuCommandBatch,
}

impl FilterAdapter for WgpuFilterEncoder<'_> {
    type Work = FilterTileWork;
    fn size(&self) -> (u32, u32) {
        self.renderer.size
    }
    fn acquire_scratch(&mut self) -> Option<RenderTargetId> {
        self.renderer.acquire_scratch()
    }
    fn release_scratch(&mut self, target: RenderTargetId) {
        self.renderer.release_scratch(target);
    }
    fn active_tiles(&self) -> Option<&DamageTiles> {
        self.renderer.retained.active_tiles()
    }
    fn filter_work(&self) -> Option<FilterTileWork> {
        self.renderer
            .filter
            .as_ref()
            .and_then(|filter| filter.active_tile_work())
    }
    fn set_filter_work(&mut self, work: Option<FilterTileWork>) {
        if let Some(filter) = self.renderer.filter.as_mut() {
            filter.restore_active_tile_work(work);
        }
    }
    fn prepare_filter_tile_work(&mut self, tiles: &[u32]) {
        self.renderer.prepare_filter_tile_work(tiles);
    }
    fn suspend_incremental_filter_work(&mut self) -> Option<DamageTiles> {
        self.renderer.suspend_incremental_filter_work()
    }
    fn restore_incremental_filter_work(&mut self, active: Option<DamageTiles>) {
        self.renderer.restore_incremental_filter_work(active);
    }
    fn encode(&mut self, kernel: FilterKernel<'_>) -> bool {
        match kernel {
            FilterKernel::ClearRenderTarget { target, color } => {
                self.renderer
                    .clear_render_target(self.commands, target, color)
            }
            FilterKernel::ClearRenderRegion {
                target,
                bounds,
                color,
            } => self
                .renderer
                .clear_render_region(self.commands, target, bounds, color),
            FilterKernel::FloodRegionToTarget {
                target,
                bounds,
                brush_offset,
            } => self
                .renderer
                .flood_region_to_target(self.commands, target, bounds, brush_offset),
            FilterKernel::BuildDropShadowMaskToTarget {
                source,
                target,
                bounds,
                dx,
                dy,
            } => self.renderer.build_drop_shadow_mask_to_target(
                self.commands,
                source,
                target,
                bounds,
                dx,
                dy,
            ),
            FilterKernel::SourceAlphaToTarget {
                source,
                target,
                bounds,
            } => self
                .renderer
                .source_alpha_to_target(self.commands, source, target, bounds),
            FilterKernel::SourceOverFilterInput {
                source,
                target,
                bounds,
            } => self
                .renderer
                .source_over_filter_input(self.commands, source, target, bounds),
            FilterKernel::BlendFilterInputs {
                input1,
                input2,
                target,
                bounds,
                mode,
            } => self.renderer.blend_filter_inputs(
                self.commands,
                input1,
                input2,
                target,
                bounds,
                mode,
            ),
            FilterKernel::CompositeFilterInputs {
                input1,
                input2,
                target,
                bounds,
                operator,
            } => self.renderer.composite_filter_inputs(
                self.commands,
                input1,
                input2,
                target,
                bounds,
                operator,
            ),
            FilterKernel::DisplacementMapFilterInputs {
                input1,
                input2,
                target,
                bounds,
                displacement,
            } => self.renderer.displacement_map_filter_inputs(
                self.commands,
                input1,
                input2,
                target,
                bounds,
                displacement,
            ),
            FilterKernel::TurbulenceToTarget {
                target,
                bounds,
                turbulence,
                table_index,
            } => self.renderer.turbulence_to_target(
                self.commands,
                target,
                bounds,
                turbulence,
                table_index,
            ),
            FilterKernel::TileFilterInput {
                input,
                target,
                bounds,
                source_region,
            } => {
                self.renderer
                    .tile_filter_input(self.commands, input, target, bounds, source_region)
            }
            FilterKernel::CompositeDropShadowToTarget {
                target,
                shadow,
                bounds,
                brush_offset,
            } => self.renderer.composite_drop_shadow_to_target(
                self.commands,
                target,
                shadow,
                bounds,
                brush_offset,
            ),
            FilterKernel::ApplyColorMatrixToTarget {
                target,
                bounds,
                matrix,
            } => self
                .renderer
                .apply_color_matrix_to_target(self.commands, target, bounds, matrix),
            FilterKernel::ApplyComponentTransferToTarget {
                target,
                bounds,
                table_index,
            } => self.renderer.apply_component_transfer_to_target(
                self.commands,
                target,
                bounds,
                table_index,
            ),
            FilterKernel::ConvolveMatrixToTarget {
                source,
                target,
                bounds,
                matrix,
                kernel_offset,
            } => self.renderer.convolve_matrix_to_target(
                self.commands,
                source,
                target,
                bounds,
                matrix,
                kernel_offset,
            ),
            FilterKernel::DiffuseLightingToTarget {
                source,
                target,
                bounds,
                lighting,
            } => self.renderer.diffuse_lighting_to_target(
                self.commands,
                source,
                target,
                bounds,
                lighting,
            ),
            FilterKernel::SpecularLightingToTarget {
                source,
                target,
                bounds,
                lighting,
            } => self.renderer.specular_lighting_to_target(
                self.commands,
                source,
                target,
                bounds,
                lighting,
            ),
            FilterKernel::LiquidGlassToTarget {
                source,
                blurred,
                target,
                bounds,
                glass,
                region,
            } => self.renderer.liquid_glass_to_target(
                self.commands,
                source,
                blurred,
                target,
                bounds,
                glass,
                region,
            ),
            FilterKernel::BlurRegionToTarget {
                source,
                target,
                bounds,
                std_dev,
                axis,
            } => self.renderer.blur_region_to_target(
                self.commands,
                source,
                target,
                bounds,
                std_dev,
                axis,
            ),
            FilterKernel::BlurRegionPartialToTarget {
                source,
                target,
                output_bounds,
                sample_bounds,
                std_dev,
                axis,
            } => self.renderer.blur_region_partial_to_target(
                self.commands,
                source,
                target,
                output_bounds,
                sample_bounds,
                std_dev,
                axis,
            ),
            FilterKernel::MorphologyAxisToTarget {
                source,
                target,
                bounds,
                radius,
                operator,
                axis,
            } => {
                let operator = encode_morphology_operator(operator);
                self.renderer.morphology_axis_to_target(
                    self.commands,
                    source,
                    target,
                    bounds,
                    radius,
                    operator,
                    axis,
                )
            }
            FilterKernel::OffsetRegionToTarget {
                source,
                target,
                bounds,
                dx,
                dy,
            } => {
                self.renderer
                    .offset_region_to_target(self.commands, source, target, bounds, dx, dy)
            }
            FilterKernel::CopyRegionToTarget {
                source,
                target,
                bounds,
            } => self
                .renderer
                .copy_region_to_target(self.commands, source, target, bounds),
            FilterKernel::ApplyColorFilterToTarget {
                target,
                bounds,
                kind,
                amount,
            } => {
                let filter_kind = match kind {
                    ColorFilterKind::Brightness => crate::wgpu::filter::FILTER_BRIGHTNESS,
                    ColorFilterKind::Contrast => crate::wgpu::filter::FILTER_CONTRAST,
                    ColorFilterKind::Grayscale => crate::wgpu::filter::FILTER_GRAYSCALE,
                    ColorFilterKind::HueRotate => crate::wgpu::filter::FILTER_HUE_ROTATE,
                    ColorFilterKind::Invert => crate::wgpu::filter::FILTER_INVERT,
                    ColorFilterKind::Opacity => crate::wgpu::filter::FILTER_OPACITY,
                    ColorFilterKind::Saturate => crate::wgpu::filter::FILTER_SATURATE,
                    ColorFilterKind::Sepia => crate::wgpu::filter::FILTER_SEPIA,
                };
                {
                    self.renderer.apply_color_filter_to_target(
                        self.commands,
                        target,
                        bounds,
                        filter_kind,
                        amount,
                    )
                }
            }
            FilterKernel::DownsampleRegion {
                source,
                target,
                source_bounds,
                low_bounds,
                sampling,
            } => {
                let Some(filter) = &self.renderer.filter else {
                    return false;
                };
                filter.downsample_region(
                    self.commands,
                    self.renderer.render_target_view(source),
                    self.renderer.render_target_view(target),
                    self.renderer.size,
                    self.renderer.lengths,
                    source_bounds,
                    low_bounds,
                    sampling,
                );
                true
            }
            FilterKernel::UpsampleRegion {
                source,
                target,
                bounds,
                low_bounds,
                sampling,
            } => {
                let Some(filter) = &self.renderer.filter else {
                    return false;
                };
                filter.upsample_region(
                    self.commands,
                    self.renderer.render_target_view(source),
                    self.renderer.render_target_view(target),
                    self.renderer.size,
                    self.renderer.lengths,
                    bounds,
                    low_bounds,
                    sampling,
                );
                true
            }
            FilterKernel::UpsampleRectCompositeRegion {
                source,
                target,
                bounds,
                low_bounds,
                sampling,
                region,
            } => {
                let Some(filter) = &self.renderer.filter else {
                    return false;
                };
                let Some(target_read) =
                    self.renderer
                        .snapshot_filter_target(self.commands, target, bounds)
                else {
                    return false;
                };
                filter.upsample_rect_composite_region(
                    self.commands,
                    self.renderer.render_target_view(source),
                    self.renderer.render_target_view(target),
                    target_read,
                    self.renderer.size,
                    self.renderer.lengths,
                    bounds,
                    low_bounds,
                    sampling,
                    region,
                )
            }
            FilterKernel::RectLiquidGlassCompositeRegion {
                source,
                blurred,
                target,
                bounds,
                blurred_bounds,
                sampling,
                glass,
                region,
            } => {
                let Some(filter) = &self.renderer.filter else {
                    return false;
                };
                filter.rect_liquid_glass_composite_region(
                    self.commands,
                    self.renderer.render_target_view(source),
                    self.renderer.render_target_view(blurred),
                    self.renderer.render_target_view(target),
                    self.renderer.render_target_view(source),
                    self.renderer.size,
                    self.renderer.lengths,
                    bounds,
                    blurred_bounds,
                    sampling,
                    glass,
                    region,
                );
                true
            }
        }
    }
}
