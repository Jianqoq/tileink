use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    pub(super) fn apply_drop_shadow(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        offset_x: f32,
        offset_y: f32,
        std_dev: f32,
        brush_offset: u32,
    ) -> Result<(), A::Error> {
        self.with_scratch(|s, shadow| {
            s.adapter.encode(FilterKernel::ClearRenderRegion {
                target: shadow,
                bounds,
                color: 0,
            })?;
            s.adapter
                .encode(FilterKernel::BuildDropShadowMaskToTarget {
                    source: target,
                    target: shadow,
                    bounds,
                    dx: offset_x.round() as i32,
                    dy: offset_y.round() as i32,
                })?;
            let std_dev = std_dev.max(0.0);
            if std_dev > 0.0 {
                s.with_scratch(|s, temp| {
                    s.adapter.encode(FilterKernel::BlurRegionToTarget {
                        source: shadow,
                        target: temp,
                        bounds,
                        std_dev,
                        axis: 0,
                    })?;
                    s.adapter.encode(FilterKernel::BlurRegionToTarget {
                        source: temp,
                        target: shadow,
                        bounds,
                        std_dev,
                        axis: 1,
                    })
                })?;
            }
            s.adapter.encode(FilterKernel::CompositeDropShadowToTarget {
                target,
                shadow,
                bounds,
                brush_offset,
            })
        })
    }
}
