//! Graph edges bind native images directly. Only final primitive cropping may
//! copy pixels; importing dynamic inputs never uploads or reads them back.
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
        source_alpha: &mut Option<RenderTargetId>,
    ) -> Option<RenderTargetId> {
        if !pipeline.is_canvas_filter() || pipeline.input_count() != inputs.len() {
            return None;
        }
        let inputs = inputs
            .iter()
            .map(|&input| {
                self.resolve_filter_graph_input(source, input, outputs, source_alpha, bounds)
            })
            .collect::<Option<Vec<_>>>()?;
        let temp = self.adapter.acquire_scratch()?;
        if !self.adapter.encode(FilterKernel::ShaderPipelineToTarget {
            inputs: &inputs,
            target: temp,
            bounds: region,
            pipeline,
        }) {
            self.adapter.release_scratch(temp);
            return None;
        }
        let (width, height) = self.adapter.size();
        if region == Bounds::canvas(width, height) {
            return Some(temp);
        }
        // An Image/Groups dispatch can write outside the primitive region.
        // Clear a separate result and crop before publishing this graph edge.
        let output = self.copy_filter_graph_region(temp, bounds, region);
        self.adapter.release_scratch(temp);
        output
    }
}
