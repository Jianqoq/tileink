use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    #[allow(
        clippy::or_fun_call,
        reason = "Scratch release must execute even when the preceding operation failed"
    )]
    pub(crate) fn apply_filter_graph(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        primitives: &[filter_model::FilterPrimitive],
        cursors: &mut FilterCursors,
    ) -> Result<(), A::Error> {
        if primitives.is_empty() {
            return self.adapter.encode(FilterKernel::ClearRenderRegion {
                target,
                bounds,
                color: 0,
            });
        }
        let mut source_alpha = None;
        let mut outputs = Vec::with_capacity(primitives.len());
        let mut result = (|| {
            for primitive in primitives {
                let output = self.apply_filter_graph_primitive(
                    target,
                    bounds,
                    primitive,
                    &outputs,
                    &mut source_alpha,
                    cursors,
                )?;
                outputs.push(output);
            }
            self.adapter.encode(FilterKernel::ClearRenderRegion {
                target,
                bounds,
                color: 0,
            })?;
            self.adapter.encode(FilterKernel::CopyRegionToTarget {
                source: outputs[outputs.len() - 1],
                target,
                bounds,
            })
        })();
        // Release every graph lease even when recording or another release fails.
        for output in outputs.into_iter().chain(source_alpha) {
            result = result.and(self.adapter.release_scratch(output));
        }
        result
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "Explicit geometry/resource parameters preserve the renderer and native ABI contract."
    )]
    fn apply_filter_graph_primitive(
        &mut self,
        source: RenderTargetId,
        bounds: Bounds,
        primitive: &filter_model::FilterPrimitive,
        outputs: &[RenderTargetId],
        alpha: &mut Option<RenderTargetId>,
        cursors: &mut FilterCursors,
    ) -> Result<RenderTargetId, A::Error> {
        use filter_model::FilterPrimitiveKind as P;
        let region = primitive.region.intersect(bounds);
        match &primitive.kind {
            P::ShaderPipeline { pipeline, inputs } => self.apply_shader_graph_primitive(
                source, bounds, region, pipeline, inputs, outputs, alpha,
            ),
            P::Image { brush } => self.graph_output(bounds, |s, target| {
                s.adapter.encode(FilterKernel::FloodRegionToTarget {
                    target,
                    bounds: region,
                    brush_offset: cursors.next_brush_offset(brush),
                })
            }),
            P::Identity => {
                let input = self.resolve_filter_graph_input(
                    source,
                    primitive.input,
                    outputs,
                    alpha,
                    bounds,
                )?;
                self.copy_filter_graph_region(input, bounds, region)
            }
            P::Filter(filter) => {
                let input = self.resolve_filter_graph_input(
                    source,
                    primitive.input,
                    outputs,
                    alpha,
                    bounds,
                )?;
                self.build_output(|s, output| {
                    s.with_scratch(|s, temp| {
                        s.adapter.encode(FilterKernel::CopyRegionToTarget {
                            source: input,
                            target: temp,
                            bounds,
                        })?;
                        s.apply_filter(temp, bounds, filter, None, primitive.linear_rgb, cursors)?;
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
            P::Blend { .. } | P::Composite { .. } | P::DisplacementMap(_) => {
                let input1 = self.resolve_filter_graph_input(
                    source,
                    primitive.input,
                    outputs,
                    alpha,
                    bounds,
                )?;
                let input2 = self.resolve_filter_graph_input(
                    source,
                    primitive
                        .input2
                        .ok_or("filter graph primitive requires a second input")?,
                    outputs,
                    alpha,
                    bounds,
                )?;
                self.graph_output(bounds, |s, target| {
                    s.adapter.encode(match &primitive.kind {
                        P::Blend { mode } => FilterKernel::BlendFilterInputs {
                            input1,
                            input2,
                            target,
                            bounds: region,
                            mode: *mode,
                            linear_rgb: primitive.linear_rgb,
                        },
                        P::Composite { operator } => FilterKernel::CompositeFilterInputs {
                            input1,
                            input2,
                            target,
                            bounds: region,
                            operator: *operator,
                            linear_rgb: primitive.linear_rgb,
                        },
                        P::DisplacementMap(displacement) => {
                            FilterKernel::DisplacementMapFilterInputs {
                                input1,
                                input2,
                                target,
                                bounds: region,
                                displacement,
                            }
                        }
                        _ => unreachable!(),
                    })
                })
            }
            P::Tile { source_region } => {
                let input = self.resolve_filter_graph_input(
                    source,
                    primitive.input,
                    outputs,
                    alpha,
                    bounds,
                )?;
                self.graph_output(bounds, |s, target| {
                    s.adapter.encode(FilterKernel::TileFilterInput {
                        input,
                        target,
                        bounds: region,
                        source_region: *source_region,
                    })
                })
            }
            P::Merge { inputs } => self.graph_output(bounds, |s, target| {
                for &input in inputs {
                    let input =
                        s.resolve_filter_graph_input(source, input, outputs, alpha, bounds)?;
                    s.adapter.encode(FilterKernel::SourceOverFilterInput {
                        source: input,
                        target,
                        bounds: region,
                    })?;
                }
                Ok(())
            }),
            P::Turbulence(turbulence) => self.graph_output(bounds, |s, target| {
                s.adapter.encode(FilterKernel::TurbulenceToTarget {
                    target,
                    bounds: region,
                    turbulence,
                    table_index: cursors.next_turbulence_index(),
                })
            }),
        }
    }
    pub(super) fn resolve_filter_graph_input(
        &mut self,
        source: RenderTargetId,
        input: filter_model::FilterInput,
        outputs: &[RenderTargetId],
        alpha: &mut Option<RenderTargetId>,
        bounds: Bounds,
    ) -> Result<RenderTargetId, A::Error> {
        match input {
            filter_model::FilterInput::SourceGraphic => Ok(source),
            filter_model::FilterInput::Primitive(index) => outputs
                .get(index)
                .copied()
                .ok_or_else(|| "filter graph references a missing or forward primitive".into()),
            filter_model::FilterInput::SourceAlpha => {
                if let Some(target) = *alpha {
                    return Ok(target);
                }
                let target = self.build_output(|s, target| {
                    s.adapter.encode(FilterKernel::SourceAlphaToTarget {
                        source,
                        target,
                        bounds,
                    })
                })?;
                *alpha = Some(target);
                Ok(target)
            }
        }
    }
    pub(super) fn copy_filter_graph_region(
        &mut self,
        input: RenderTargetId,
        bounds: Bounds,
        region: Bounds,
    ) -> Result<RenderTargetId, A::Error> {
        self.graph_output(bounds, |s, target| {
            s.adapter.encode(FilterKernel::CopyRegionToTarget {
                source: input,
                target,
                bounds: region.intersect(bounds),
            })
        })
    }
    fn graph_output(
        &mut self,
        bounds: Bounds,
        action: impl FnOnce(&mut Self, RenderTargetId) -> Result<(), A::Error>,
    ) -> Result<RenderTargetId, A::Error> {
        // Primitive results are transparent outside their own region.
        self.build_output(|s, target| {
            s.adapter.encode(FilterKernel::ClearRenderRegion {
                target,
                bounds,
                color: 0,
            })?;
            action(s, target)
        })
    }
}
