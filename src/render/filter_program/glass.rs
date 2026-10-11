use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    /// False means the direct algorithm is inapplicable; recording failures are errors.
    pub(crate) fn apply_downsampled_blur_rect_composite(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
        region: &crate::shared::layer::region::Region,
    ) -> Result<bool, A::Error> {
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        let factor = sampling.factor();
        if factor <= 1 || std_dev_x <= 0.0 || std_dev_y <= 0.0 {
            return Ok(false);
        }
        let Some(low_bounds) = downsampled_bounds(bounds, factor) else {
            return Ok(false);
        };
        if low_bounds.width() >= bounds.width() && low_bounds.height() >= bounds.height()
            || !matches!(region, crate::shared::layer::region::Region::Rect { .. })
        {
            return Ok(false);
        }
        self.with_scratch(|s, low| {
            s.with_scratch(|s, temp| {
                let previous = s.use_downsampled_filter_work(bounds, factor);
                let result = s.encode_downsampled_blur_stages(
                    target, low, temp, bounds, low_bounds, std_dev_x, std_dev_y, sampling,
                );
                s.restore_full_resolution_filter_work(previous);
                result?;
                s.adapter.encode(FilterKernel::UpsampleRectCompositeRegion {
                    source: low,
                    target,
                    bounds,
                    low_bounds,
                    sampling,
                    region,
                })
            })
        })?;
        Ok(true)
    }
    pub(crate) fn apply_downsampled_liquid_glass_rect_composite(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: &crate::shared::layer::region::Region,
    ) -> Result<bool, A::Error> {
        if glass.blur_radius == 0 || glass.blur_sampling.factor() <= 1 {
            return Ok(false);
        }
        let Some(glass_region) = rect_liquid_glass_region(Some(region), bounds) else {
            return Ok(false);
        };
        let Some(low_bounds) = downsampled_bounds(bounds, glass.blur_sampling.factor()) else {
            return Ok(false);
        };
        if low_bounds.width() >= bounds.width() && low_bounds.height() >= bounds.height() {
            return Ok(false);
        }
        self.composite_downsampled_glass(target, bounds, low_bounds, glass, glass_region)?;
        Ok(true)
    }
    fn composite_downsampled_glass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        low_bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> Result<(), A::Error> {
        self.with_scratch(|s, source| {
            s.with_scratch(|s, low| {
                s.with_scratch(|s, temp| {
                    s.adapter.encode(FilterKernel::CopyRegionToTarget {
                        source: target,
                        target: source,
                        bounds,
                    })?;
                    let std_dev =
                        glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
                    let previous =
                        s.use_downsampled_filter_work(bounds, glass.blur_sampling.factor());
                    let result = s.encode_downsampled_blur_stages(
                        source,
                        low,
                        temp,
                        bounds,
                        low_bounds,
                        std_dev,
                        std_dev,
                        glass.blur_sampling,
                    );
                    s.restore_full_resolution_filter_work(previous);
                    result?;
                    let (blurred, blurred_bounds, sampling) =
                        if should_sample_downsampled_blur_in_liquid_glass(glass) {
                            (low, low_bounds, glass.blur_sampling)
                        } else {
                            s.adapter.encode(FilterKernel::UpsampleRegion {
                                source: low,
                                target: temp,
                                bounds,
                                low_bounds,
                                sampling: glass.blur_sampling,
                            })?;
                            (temp, bounds, filter_model::BlurSampling::FULL_RES)
                        };
                    s.adapter
                        .encode(FilterKernel::RectLiquidGlassCompositeRegion {
                            source,
                            blurred,
                            target,
                            bounds,
                            blurred_bounds,
                            sampling,
                            glass,
                            region,
                        })
                })
            })
        })
    }
    /// The refraction halo is blurred without compact output work, then the
    /// exact dirty-tile list is restored before updating the cached glass image.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_liquid_glass_from_source_partial(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> Result<(), A::Error> {
        debug_assert_eq!(glass.blur_sampling.factor(), 1);
        self.with_scratch(|s, blurred| {
            let blur_output = output_bounds
                .outset(glass.sample_outset())
                .intersect(sample_bounds);
            let active = s.adapter.suspend_incremental_filter_work();
            let result = if glass.blur_radius == 0 {
                s.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source,
                    target: blurred,
                    bounds: blur_output,
                })
            } else {
                let std_dev =
                    glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
                s.apply_blur_from_source_partial(
                    source,
                    blurred,
                    blur_output,
                    sample_bounds,
                    std_dev,
                    std_dev,
                )
            };
            s.adapter.restore_incremental_filter_work(active);
            result?;
            s.adapter.encode(FilterKernel::LiquidGlassToTarget {
                source,
                blurred,
                target,
                bounds: output_bounds,
                glass,
                region,
            })
        })
    }
    pub(super) fn apply_liquid_glass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> Result<(), A::Error> {
        self.with_scratch(|s, source| {
            s.with_scratch(|s, blurred| {
                s.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: target,
                    target: source,
                    bounds,
                })?;
                if glass.blur_radius > 0 {
                    let std_dev =
                        glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
                    if glass.blur_sampling.factor() > 1 {
                        s.with_scratch(|s, temp| {
                            s.downsampled_blur_to_target(
                                source,
                                blurred,
                                temp,
                                blurred,
                                bounds,
                                std_dev,
                                std_dev,
                                glass.blur_sampling,
                            )
                        })?;
                    } else {
                        s.apply_blur_from_source(
                            source,
                            blurred,
                            bounds,
                            std_dev,
                            std_dev,
                            filter_model::BlurSampling::FULL_RES,
                        )?;
                    }
                } else {
                    s.adapter.encode(FilterKernel::CopyRegionToTarget {
                        source,
                        target: blurred,
                        bounds,
                    })?;
                }
                s.adapter.encode(FilterKernel::LiquidGlassToTarget {
                    source,
                    blurred,
                    target,
                    bounds,
                    glass,
                    region,
                })
            })
        })
    }
}
