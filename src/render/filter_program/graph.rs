use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    pub(crate) fn apply_filter_graph(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        primitives: &[filter_model::FilterPrimitive],
        filter_cursors: &mut FilterCursors,
    ) -> bool {
        if primitives.is_empty() {
            return self.adapter.encode(FilterKernel::ClearRenderRegion {
                target,
                bounds,
                color: 0,
            });
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
                    self.adapter.release_scratch(output);
                }
                if let Some(source_alpha) = source_alpha {
                    self.adapter.release_scratch(source_alpha);
                }
                return false;
            };
            outputs.push(output);
        }

        let final_output = outputs[outputs.len() - 1];
        let ok = self.adapter.encode(FilterKernel::ClearRenderRegion {
            target,
            bounds,
            color: 0,
        }) && self.adapter.encode(FilterKernel::CopyRegionToTarget {
            source: final_output,
            target,
            bounds,
        });
        for output in outputs {
            self.adapter.release_scratch(output);
        }
        if let Some(source_alpha) = source_alpha {
            self.adapter.release_scratch(source_alpha);
        }
        ok
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_filter_graph_primitive(
        &mut self,
        source_graphic: RenderTargetId,
        bounds: Bounds,
        primitive: &filter_model::FilterPrimitive,
        outputs: &[RenderTargetId],
        source_alpha: &mut Option<RenderTargetId>,
        filter_cursors: &mut FilterCursors,
    ) -> Option<RenderTargetId> {
        let region = primitive.region.intersect(bounds);
        match &primitive.kind {
            filter_model::FilterPrimitiveKind::Image { brush } => {
                let output = self.acquire_graph_output(bounds)?;
                if self.adapter.encode(FilterKernel::FloodRegionToTarget {
                    target: output,
                    bounds: region,
                    brush_offset: filter_cursors.next_brush_offset(brush),
                }) {
                    Some(output)
                } else {
                    self.adapter.release_scratch(output);
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
                let temp = self.adapter.acquire_scratch()?;
                if !self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: input,
                    target: temp,
                    bounds,
                }) || !self.apply_filter(temp, bounds, filter, None, filter_cursors)
                {
                    self.adapter.release_scratch(temp);
                    return None;
                }
                let output = self.copy_filter_graph_region(temp, bounds, region);
                self.adapter.release_scratch(temp);
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
                let output = self.acquire_graph_output(bounds)?;
                if self.adapter.encode(FilterKernel::BlendFilterInputs {
                    input1: input,
                    input2,
                    target: output,
                    bounds: region,
                    mode: *mode,
                }) {
                    Some(output)
                } else {
                    self.adapter.release_scratch(output);
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
                let output = self.acquire_graph_output(bounds)?;
                if self.adapter.encode(FilterKernel::CompositeFilterInputs {
                    input1: input,
                    input2,
                    target: output,
                    bounds: region,
                    operator: *operator,
                }) {
                    Some(output)
                } else {
                    self.adapter.release_scratch(output);
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
                let output = self.acquire_graph_output(bounds)?;
                if self.adapter.encode(FilterKernel::TileFilterInput {
                    input,
                    target: output,
                    bounds: region,
                    source_region: *source_region,
                }) {
                    Some(output)
                } else {
                    self.adapter.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Merge { inputs } => {
                let output = self.acquire_graph_output(bounds)?;
                for input in inputs {
                    let Some(input) = self.resolve_filter_graph_input(
                        source_graphic,
                        *input,
                        outputs,
                        source_alpha,
                        bounds,
                    ) else {
                        // Root cause: resolving an invalid edge after acquiring the
                        // merge output used to bypass its scratch release.
                        self.adapter.release_scratch(output);
                        return None;
                    };
                    if !self.adapter.encode(FilterKernel::SourceOverFilterInput {
                        source: input,
                        target: output,
                        bounds: region,
                    }) {
                        self.adapter.release_scratch(output);
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
                let output = self.acquire_graph_output(bounds)?;
                if self
                    .adapter
                    .encode(FilterKernel::DisplacementMapFilterInputs {
                        input1: input,
                        input2,
                        target: output,
                        bounds: region,
                        displacement,
                    })
                {
                    Some(output)
                } else {
                    self.adapter.release_scratch(output);
                    None
                }
            }
            filter_model::FilterPrimitiveKind::Turbulence(turbulence) => {
                let output = self.acquire_graph_output(bounds)?;
                if self.adapter.encode(FilterKernel::TurbulenceToTarget {
                    target: output,
                    bounds: region,
                    turbulence,
                    table_index: filter_cursors.next_turbulence_index(),
                }) {
                    Some(output)
                } else {
                    self.adapter.release_scratch(output);
                    None
                }
            }
        }
    }
    pub(super) fn resolve_required_filter_graph_input(
        &mut self,
        source_graphic: RenderTargetId,
        primitive: &filter_model::FilterPrimitive,
        outputs: &[RenderTargetId],
        source_alpha: &mut Option<RenderTargetId>,
        bounds: Bounds,
    ) -> Option<RenderTargetId> {
        self.resolve_filter_graph_input(
            source_graphic,
            primitive.input2?,
            outputs,
            source_alpha,
            bounds,
        )
    }
    pub(super) fn resolve_filter_graph_input(
        &mut self,
        source_graphic: RenderTargetId,
        input: filter_model::FilterInput,
        outputs: &[RenderTargetId],
        source_alpha: &mut Option<RenderTargetId>,
        bounds: Bounds,
    ) -> Option<RenderTargetId> {
        match input {
            filter_model::FilterInput::SourceGraphic => Some(source_graphic),
            filter_model::FilterInput::Primitive(index) => outputs.get(index).copied(),
            filter_model::FilterInput::SourceAlpha => {
                if let Some(target) = *source_alpha {
                    return Some(target);
                }
                let alpha = self.adapter.acquire_scratch()?;
                if self.adapter.encode(FilterKernel::SourceAlphaToTarget {
                    source: source_graphic,
                    target: alpha,
                    bounds,
                }) {
                    *source_alpha = Some(alpha);
                    Some(alpha)
                } else {
                    self.adapter.release_scratch(alpha);
                    None
                }
            }
        }
    }
    pub(super) fn copy_filter_graph_region(
        &mut self,
        input: RenderTargetId,
        bounds: Bounds,
        region: Bounds,
    ) -> Option<RenderTargetId> {
        let output = self.acquire_graph_output(bounds)?;
        if self.adapter.encode(FilterKernel::CopyRegionToTarget {
            source: input,
            target: output,
            bounds: region.intersect(bounds),
        }) {
            Some(output)
        } else {
            self.adapter.release_scratch(output);
            None
        }
    }
}

impl<A: FilterAdapter> FilterExecutor<'_, A> {
    fn acquire_graph_output(&mut self, bounds: Bounds) -> Option<RenderTargetId> {
        let output = self.adapter.acquire_scratch()?;
        // A graph output is transparent outside its primitive region. Failed
        // clearing cannot publish stale scratch contents or continue the graph.
        if self.adapter.encode(FilterKernel::ClearRenderRegion {
            target: output,
            bounds,
            color: 0,
        }) {
            Some(output)
        } else {
            self.adapter.release_scratch(output);
            None
        }
    }
}
