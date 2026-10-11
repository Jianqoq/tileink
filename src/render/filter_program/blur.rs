use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    pub(super) fn use_downsampled_filter_work(
        &mut self,
        bounds: Bounds,
        factor: u32,
    ) -> Option<A::Work> {
        let active = self.adapter.active_tiles()?;
        let previous = self.adapter.filter_work()?;
        let mut low = DamageTiles::new(self.adapter.size());
        for damage in active.coalesced_rects(self.adapter.size()) {
            let damage = damage.intersect(bounds);
            if !damage.is_empty()
                && let Some(downsampled) = downsampled_bounds(damage, factor)
            {
                low.add_bounds(downsampled);
            }
        }
        self.adapter.prepare_filter_tile_work(low.list());
        Some(previous)
    }
    pub(super) fn restore_full_resolution_filter_work(&mut self, previous: Option<A::Work>) {
        if previous.is_some() {
            self.adapter.set_filter_work(previous);
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode_downsampled_blur_stages(
        &mut self,
        source: RenderTargetId,
        low: RenderTargetId,
        temp: RenderTargetId,
        source_bounds: Bounds,
        low_bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> Result<(), A::Error> {
        let factor = sampling.factor() as f32;
        self.adapter.encode(FilterKernel::DownsampleRegion {
            source,
            target: low,
            source_bounds,
            low_bounds,
            sampling,
        })?;
        self.adapter.encode(FilterKernel::BlurRegionToTarget {
            source: low,
            target: temp,
            bounds: low_bounds,
            std_dev: std_dev_x / factor,
            axis: 0,
        })?;
        self.adapter.encode(FilterKernel::BlurRegionToTarget {
            source: temp,
            target: low,
            bounds: low_bounds,
            std_dev: std_dev_y / factor,
            axis: 1,
        })
    }
    /// Horizontal intermediates include every row read by the vertical pass.
    pub(crate) fn apply_blur_from_source_partial(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
    ) -> Result<(), A::Error> {
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        match (std_dev_x > 0.0, std_dev_y > 0.0) {
            (true, true) => self.with_scratch(|s, temp| {
                let radius_y = filter_model::blur_outset(std_dev_y);
                let intermediate_bounds = Bounds {
                    y0: output_bounds.y0.saturating_sub_unsigned(radius_y),
                    y1: output_bounds.y1.saturating_add_unsigned(radius_y),
                    ..output_bounds
                }
                .intersect(Bounds::canvas(s.adapter.size().0, s.adapter.size().1));
                let output_work = s.adapter.filter_work();
                if output_work.is_some()
                    && let Some(active) = s.adapter.active_tiles()
                {
                    s.adapter
                        .prepare_filter_tile_work(active.outset_y(radius_y).list());
                }
                let horizontal = s.adapter.encode(FilterKernel::BlurRegionPartialToTarget {
                    source,
                    target: temp,
                    output_bounds: intermediate_bounds,
                    sample_bounds,
                    std_dev: std_dev_x,
                    axis: 0,
                });
                s.adapter.set_filter_work(output_work);
                horizontal?;
                s.adapter.encode(FilterKernel::BlurRegionPartialToTarget {
                    source: temp,
                    target,
                    output_bounds,
                    sample_bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                })
            }),
            (true, false) => self
                .adapter
                .encode(FilterKernel::BlurRegionPartialToTarget {
                    source,
                    target,
                    output_bounds,
                    sample_bounds,
                    std_dev: std_dev_x,
                    axis: 0,
                }),
            (false, true) => self
                .adapter
                .encode(FilterKernel::BlurRegionPartialToTarget {
                    source,
                    target,
                    output_bounds,
                    sample_bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                }),
            (false, false) => self.adapter.encode(FilterKernel::CopyRegionToTarget {
                source,
                target,
                bounds: output_bounds,
            }),
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn downsampled_blur_to_target(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        low: RenderTargetId,
        temp: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> Result<(), A::Error> {
        let factor = sampling.factor();
        let low_bounds =
            downsampled_bounds(bounds, factor).ok_or("downsampled blur has an empty region")?;
        if low_bounds.width() >= bounds.width() && low_bounds.height() >= bounds.height() {
            if source != target {
                self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source,
                    target,
                    bounds,
                })?;
            }
            self.adapter.encode(FilterKernel::BlurRegionToTarget {
                source: target,
                target: temp,
                bounds,
                std_dev: std_dev_x,
                axis: 0,
            })?;
            return self.adapter.encode(FilterKernel::BlurRegionToTarget {
                source: temp,
                target,
                bounds,
                std_dev: std_dev_y,
                axis: 1,
            });
        }
        let previous = self.use_downsampled_filter_work(bounds, factor);
        let result = self.encode_downsampled_blur_stages(
            source, low, temp, bounds, low_bounds, std_dev_x, std_dev_y, sampling,
        );
        self.restore_full_resolution_filter_work(previous);
        result?;
        self.adapter.encode(FilterKernel::UpsampleRegion {
            source: low,
            target,
            bounds,
            low_bounds,
            sampling,
        })
    }
    pub(super) fn apply_blur(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> Result<(), A::Error> {
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        if std_dev_x <= 0.0 && std_dev_y <= 0.0 {
            return Ok(());
        }
        if sampling.factor() > 1 && std_dev_x > 0.0 && std_dev_y > 0.0 {
            return self.with_scratch(|s, low| {
                s.with_scratch(|s, temp| {
                    s.downsampled_blur_to_target(
                        target, target, low, temp, bounds, std_dev_x, std_dev_y, sampling,
                    )
                })
            });
        }
        self.with_scratch(|s, temp| {
            let (std_dev, axis) = if std_dev_x > 0.0 {
                (std_dev_x, 0)
            } else {
                (std_dev_y, 1)
            };
            s.adapter.encode(FilterKernel::BlurRegionToTarget {
                source: target,
                target: temp,
                bounds,
                std_dev,
                axis,
            })?;
            if std_dev_x > 0.0 && std_dev_y > 0.0 {
                s.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                })
            } else {
                s.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                })
            }
        })
    }
    pub(crate) fn apply_blur_from_source(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> Result<(), A::Error> {
        if source == target {
            return self.apply_blur(target, bounds, std_dev_x, std_dev_y, sampling);
        }
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        if sampling.factor() > 1 && std_dev_x > 0.0 && std_dev_y > 0.0 {
            return self.with_scratch(|s, low| {
                s.with_scratch(|s, temp| {
                    s.downsampled_blur_to_target(
                        source, target, low, temp, bounds, std_dev_x, std_dev_y, sampling,
                    )
                })
            });
        }
        match (std_dev_x > 0.0, std_dev_y > 0.0) {
            (true, true) => self.with_scratch(|s, temp| {
                s.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source,
                    target: temp,
                    bounds,
                    std_dev: std_dev_x,
                    axis: 0,
                })?;
                s.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                })
            }),
            (true, false) => self.adapter.encode(FilterKernel::BlurRegionToTarget {
                source,
                target,
                bounds,
                std_dev: std_dev_x,
                axis: 0,
            }),
            (false, true) => self.adapter.encode(FilterKernel::BlurRegionToTarget {
                source,
                target,
                bounds,
                std_dev: std_dev_y,
                axis: 1,
            }),
            (false, false) => self.adapter.encode(FilterKernel::CopyRegionToTarget {
                source,
                target,
                bounds,
            }),
        }
    }
}
