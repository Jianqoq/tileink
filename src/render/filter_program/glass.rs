use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    pub(crate) fn apply_downsampled_blur_rect_composite(
        &mut self,
        target: RenderTargetId,
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

        let Some(low) = self.adapter.acquire_scratch() else {
            return false;
        };
        let Some(temp) = self.adapter.acquire_scratch() else {
            self.adapter.release_scratch(low);
            return false;
        };
        let low_work = self.use_downsampled_filter_work(bounds, factor);
        let low_ok = self.encode_downsampled_blur_stages(
            target, low, temp, bounds, low_bounds, std_dev_x, std_dev_y, sampling,
        );
        self.restore_full_resolution_filter_work(low_work);
        let ok = low_ok
            && self
                .adapter
                .encode(FilterKernel::UpsampleRectCompositeRegion {
                    source: low,
                    target,
                    bounds,
                    low_bounds,
                    sampling,
                    region,
                });
        self.adapter.release_scratch(temp);
        self.adapter.release_scratch(low);
        ok
    }
    pub(crate) fn apply_downsampled_liquid_glass_rect_composite(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: &crate::shared::layer::region::Region,
    ) -> bool {
        if glass.blur_radius == 0 || glass.blur_sampling.factor() <= 1 {
            return false;
        }
        let Some(glass_region) = rect_liquid_glass_region(Some(region), bounds) else {
            return false;
        };
        let factor = glass.blur_sampling.factor();
        let Some(low_bounds) = downsampled_bounds(bounds, factor) else {
            return false;
        };
        if low_bounds.width() >= bounds.width() && low_bounds.height() >= bounds.height() {
            return false;
        }

        self.composite_downsampled_glass(target, bounds, low_bounds, glass, glass_region)
    }
    fn composite_downsampled_glass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        low_bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        glass_region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        let Some(source) = self.adapter.acquire_scratch() else {
            return false;
        };
        let Some(low) = self.adapter.acquire_scratch() else {
            self.adapter.release_scratch(source);
            return false;
        };
        let Some(temp) = self.adapter.acquire_scratch() else {
            self.adapter.release_scratch(low);
            self.adapter.release_scratch(source);
            return false;
        };

        let factor = glass.blur_sampling.factor();
        let std_dev = glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
        let copied = self.adapter.encode(FilterKernel::CopyRegionToTarget {
            source: target,
            target: source,
            bounds,
        });
        let low_work = self.use_downsampled_filter_work(bounds, factor);
        let low_ok = copied
            && self.encode_downsampled_blur_stages(
                source,
                low,
                temp,
                bounds,
                low_bounds,
                std_dev,
                std_dev,
                glass.blur_sampling,
            );
        self.restore_full_resolution_filter_work(low_work);
        let sample_low = should_sample_downsampled_blur_in_liquid_glass(glass);
        let (blurred, blurred_bounds, sampling) = if sample_low {
            (low, low_bounds, glass.blur_sampling)
        } else {
            (temp, bounds, filter_model::BlurSampling::FULL_RES)
        };
        // Both sampling strategies share their allocation and worklist lifetime.
        // Materialization only inserts an upsample before the final glass kernel.
        let ok = low_ok
            && (sample_low
                || self.adapter.encode(FilterKernel::UpsampleRegion {
                    source: low,
                    target: temp,
                    bounds,
                    low_bounds,
                    sampling: glass.blur_sampling,
                }))
            && self
                .adapter
                .encode(FilterKernel::RectLiquidGlassCompositeRegion {
                    source,
                    blurred,
                    target,
                    bounds,
                    blurred_bounds,
                    sampling,
                    glass,
                    region: glass_region,
                });
        self.adapter.release_scratch(temp);
        self.adapter.release_scratch(low);
        self.adapter.release_scratch(source);
        ok
    }

    /// Recomputes only the damaged output of a cached full-resolution liquid-glass backdrop.
    ///
    /// `source` is the complete painter-order history captured before the backdrop. The blurred
    /// temporary needs a rectangular refraction halo beyond the compact root worklist, so that
    /// halo is generated with compact dispatch temporarily suspended; the final glass pass then
    /// returns to the exact dirty-tile worklist and leaves clean cached output untouched.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_liquid_glass_from_source_partial(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        debug_assert_eq!(glass.blur_sampling.factor(), 1);
        let Some(blurred) = self.adapter.acquire_scratch() else {
            return false;
        };
        let blur_output = output_bounds
            .outset(glass.sample_outset())
            .intersect(sample_bounds);
        let active = self.adapter.suspend_incremental_filter_work();
        let blur_ok = if glass.blur_radius == 0 {
            self.adapter.encode(FilterKernel::CopyRegionToTarget {
                source,
                target: blurred,
                bounds: blur_output,
            })
        } else {
            let std_dev = glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
            self.apply_blur_from_source_partial(
                source,
                blurred,
                blur_output,
                sample_bounds,
                std_dev,
                std_dev,
            )
        };
        self.adapter.restore_incremental_filter_work(active);
        let ok = blur_ok
            && self.adapter.encode(FilterKernel::LiquidGlassToTarget {
                source,
                blurred,
                target,
                bounds: output_bounds,
                glass,
                region,
            });
        self.adapter.release_scratch(blurred);
        ok
    }
}
