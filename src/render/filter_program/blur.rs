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
    ) -> bool {
        let factor = sampling.factor() as f32;
        self.adapter.encode(FilterKernel::DownsampleRegion {
            source,
            target: low,
            source_bounds,
            low_bounds,
            sampling,
        }) && self.adapter.encode(FilterKernel::BlurRegionToTarget {
            source: low,
            target: temp,
            bounds: low_bounds,
            std_dev: std_dev_x / factor,
            axis: 0,
        }) && self.adapter.encode(FilterKernel::BlurRegionToTarget {
            source: temp,
            target: low,
            bounds: low_bounds,
            std_dev: std_dev_y / factor,
            axis: 1,
        })
    }
    /// Updates only `output_bounds` of an existing blur target while sampling
    /// the complete source texture. The horizontal intermediate is expanded
    /// vertically so the second pass never observes an uninitialized row.
    pub(crate) fn apply_blur_from_source_partial(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        output_bounds: Bounds,
        sample_bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
    ) -> bool {
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        match (std_dev_x > 0.0, std_dev_y > 0.0) {
            (true, true) => {
                let Some(temp) = self.adapter.acquire_scratch() else {
                    return false;
                };
                let radius_y = (std_dev_y * 3.0).ceil() as i32;
                let intermediate_bounds = Bounds::new(
                    output_bounds.x0,
                    output_bounds.y0 - radius_y,
                    output_bounds.x1,
                    output_bounds.y1 + radius_y,
                )
                .intersect(Bounds::canvas(self.adapter.size().0, self.adapter.size().1));
                let output_work = self.adapter.filter_work();
                if output_work.is_some()
                    && let Some(active) = self.adapter.active_tiles()
                {
                    // The vertical pass samples beyond the output tile rows. Give
                    // the intermediate its own halo worklist, preserving sparse
                    // columns and the original list for the final output pass.
                    let intermediate_work = active.outset_y(radius_y);
                    self.adapter
                        .prepare_filter_tile_work(intermediate_work.list());
                }
                let horizontal_ok = self
                    .adapter
                    .encode(FilterKernel::BlurRegionPartialToTarget {
                        source,
                        target: temp,
                        output_bounds: intermediate_bounds,
                        sample_bounds,
                        std_dev: std_dev_x,
                        axis: 0,
                    });
                self.adapter.set_filter_work(output_work);
                let ok = horizontal_ok
                    && self
                        .adapter
                        .encode(FilterKernel::BlurRegionPartialToTarget {
                            source: temp,
                            target,
                            output_bounds,
                            sample_bounds,
                            std_dev: std_dev_y,
                            axis: 1,
                        });
                self.adapter.release_scratch(temp);
                ok
            }
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
    ) -> bool {
        let factor = sampling.factor();
        let Some(low_bounds) = downsampled_bounds(bounds, factor) else {
            return false;
        };
        if low_bounds.width() >= bounds.width() && low_bounds.height() >= bounds.height() {
            return (source == target
                || self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source,
                    target,
                    bounds,
                }))
                && self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    std_dev: std_dev_x,
                    axis: 0,
                })
                && self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                });
        }
        let previous = self.use_downsampled_filter_work(bounds, factor);
        let ok = self.encode_downsampled_blur_stages(
            source, low, temp, bounds, low_bounds, std_dev_x, std_dev_y, sampling,
        );
        self.restore_full_resolution_filter_work(previous);
        ok && self.adapter.encode(FilterKernel::UpsampleRegion {
            source: low,
            target,
            bounds,
            low_bounds,
            sampling,
        })
    }
}
