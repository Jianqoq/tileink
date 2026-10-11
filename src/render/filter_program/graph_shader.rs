//! Graph edges bind native images directly; only final primitive cropping copies pixels.
use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_shader_graph_primitive(
        &mut self,
        source: RenderTargetId,
        bounds: Bounds,
        region: Bounds,
        pipeline: &crate::ShaderPipeline,
        inputs: &[filter_model::FilterInput],
        outputs: &[RenderTargetId],
        alpha: &mut Option<RenderTargetId>,
    ) -> Result<RenderTargetId, A::Error> {
        if !pipeline.is_canvas_filter() || pipeline.input_count() != inputs.len() {
            return Err("filter graph pipeline input or output mismatch".into());
        }
        let inputs = inputs
            .iter()
            .map(|&input| self.resolve_filter_graph_input(source, input, outputs, alpha, bounds))
            .collect::<Result<Vec<_>, _>>()?;
        let (width, height) = self.adapter.size();
        if region == Bounds::canvas(width, height) {
            return self.build_output(|s, target| {
                s.adapter.encode(FilterKernel::ShaderPipelineToTarget {
                    inputs: &inputs,
                    target,
                    bounds: region,
                    pipeline,
                })
            });
        }
        // Arbitrary kernels may write outside the primitive region. Crop before
        // publishing the edge; the intermediate is released on every error path.
        self.build_output(|s, output| {
            s.with_scratch(|s, temp| {
                s.adapter.encode(FilterKernel::ShaderPipelineToTarget {
                    inputs: &inputs,
                    target: temp,
                    bounds: region,
                    pipeline,
                })?;
                s.adapter.encode(FilterKernel::ClearRenderRegion {
                    target: output,
                    bounds,
                    color: 0,
                })?;
                s.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target: output,
                    bounds: region,
                })
            })
        })
    }
}
