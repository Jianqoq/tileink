use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    pub(crate) fn apply_filter(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: Option<&crate::shared::layer::region::Region>,
        linear_rgb: bool,
        cursors: &mut FilterCursors,
    ) -> Result<(), A::Error> {
        match filter {
            Filter::ShaderPipeline(pipeline) => {
                if !pipeline.is_canvas_filter() || pipeline.input_count() > 1 {
                    return Err(
                        "filter pipeline requires one canvas output and at most one input".into(),
                    );
                }
                let inputs = if pipeline.reads_input() {
                    std::slice::from_ref(&target)
                } else {
                    &[]
                };
                let (width, height) = self.adapter.size();
                if !pipeline.requires_output_scratch() && bounds == Bounds::canvas(width, height) {
                    return self.adapter.encode(FilterKernel::ShaderPipelineToTarget {
                        inputs,
                        target,
                        bounds,
                        pipeline,
                    });
                }
                // Compute has no scissor; crop arbitrary user-kernel writes.
                self.with_scratch(|s, output| {
                    s.adapter.encode(FilterKernel::ShaderPipelineToTarget {
                        inputs,
                        target: output,
                        bounds,
                        pipeline,
                    })?;
                    s.adapter.encode(FilterKernel::CopyRegionToTarget {
                        source: output,
                        target,
                        bounds,
                    })
                })
            }
            Filter::ProgressiveBlur(blur) => self.adapter.encode(FilterKernel::ProgressiveBlur {
                target,
                bounds,
                blur: *blur,
            }),
            Filter::Graph { primitives, .. } => {
                self.apply_filter_graph(target, bounds, primitives, cursors)
            }
            Filter::Chain { filters, .. } => filters.iter().try_for_each(|filter| {
                self.apply_filter(target, bounds, filter, region, linear_rgb, cursors)
            }),
            Filter::RectLiquidGlass(glass) => {
                let region = rect_liquid_glass_region(region, bounds)
                    .ok_or("liquid glass requires a rectangular region")?;
                self.apply_liquid_glass(target, bounds, *glass, region)
            }
            Filter::Offset { dx, dy } => {
                let dx = filter_model::filter_offset_to_pixel_delta(*dx);
                let dy = filter_model::filter_offset_to_pixel_delta(*dy);
                if dx == 0 && dy == 0 {
                    return Ok(());
                }
                self.with_scratch(|s, temp| {
                    s.adapter.encode(FilterKernel::OffsetRegionToTarget {
                        source: target,
                        target: temp,
                        bounds,
                        dx,
                        dy,
                    })?;
                    s.adapter.encode(FilterKernel::CopyRegionToTarget {
                        source: temp,
                        target,
                        bounds,
                    })
                })
            }
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => self.apply_blur(target, bounds, *std_dev_x, *std_dev_y, *sampling),
            Filter::ColorMatrix(matrix) => {
                self.adapter.encode(FilterKernel::ApplyColorMatrixToTarget {
                    target,
                    bounds,
                    matrix: *matrix,
                    linear_rgb,
                })
            }
            Filter::ComponentTransfer(_) => {
                self.adapter
                    .encode(FilterKernel::ApplyComponentTransferToTarget {
                        target,
                        bounds,
                        table_index: cursors.next_transfer_index(),
                        linear_rgb,
                    })
            }
            Filter::ConvolveMatrix(matrix) => {
                let kernel_offset = cursors.next_convolve_offset(matrix);
                self.with_scratch(|s, temp| {
                    s.adapter.encode(FilterKernel::ConvolveMatrixToTarget {
                        source: target,
                        target: temp,
                        bounds,
                        matrix,
                        kernel_offset,
                        linear_rgb,
                    })?;
                    s.adapter.encode(FilterKernel::CopyRegionToTarget {
                        source: temp,
                        target,
                        bounds,
                    })
                })
            }
            Filter::DiffuseLighting(lighting) => self.with_scratch(|s, temp| {
                s.adapter.encode(FilterKernel::DiffuseLightingToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    lighting,
                })?;
                s.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                })
            }),
            Filter::SpecularLighting(lighting) => self.with_scratch(|s, temp| {
                s.adapter.encode(FilterKernel::SpecularLightingToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    lighting,
                })?;
                s.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                })
            }),
            Filter::Flood { brush } => self.adapter.encode(FilterKernel::FloodRegionToTarget {
                target,
                bounds,
                brush_offset: cursors.next_brush_offset(brush),
            }),
            Filter::DropShadow {
                brush,
                offset_x,
                offset_y,
                std_dev,
            } => self.apply_drop_shadow(
                target,
                bounds,
                *offset_x,
                *offset_y,
                *std_dev,
                cursors.next_brush_offset(brush),
            ),
            Filter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => {
                let (width, height) = self.adapter.size();
                let raw_x = radius_x.max(0.0).ceil() as u32;
                let raw_y = radius_y.max(0.0).ceil() as u32;
                if raw_x == 0 && raw_y == 0 {
                    return Ok(());
                }
                if *operator == filter_model::MorphologyOperator::Erode
                    && (raw_x.saturating_mul(2) >= width || raw_y.saturating_mul(2) >= height)
                {
                    return self.adapter.encode(FilterKernel::ClearRenderRegion {
                        target,
                        bounds,
                        color: 0,
                    });
                }
                self.with_scratch(|s, temp| {
                    s.with_scratch(|s, output| {
                        s.adapter.encode(FilterKernel::ClearRenderTarget {
                            target: temp,
                            color: 0,
                        })?;
                        s.adapter.encode(FilterKernel::ClearRenderTarget {
                            target: output,
                            color: 0,
                        })?;
                        s.adapter.encode(FilterKernel::MorphologyAxisToTarget {
                            source: target,
                            target: temp,
                            bounds,
                            radius: raw_x.min(width.saturating_sub(1)),
                            operator: *operator,
                            axis: 0,
                        })?;
                        s.adapter.encode(FilterKernel::MorphologyAxisToTarget {
                            source: temp,
                            target: output,
                            bounds,
                            radius: raw_y.min(height.saturating_sub(1)),
                            operator: *operator,
                            axis: 1,
                        })?;
                        s.adapter.encode(FilterKernel::CopyRegionToTarget {
                            source: output,
                            target,
                            bounds,
                        })
                    })
                })
            }
            Filter::Brightness(amount)
            | Filter::Contrast(amount)
            | Filter::Grayscale(amount)
            | Filter::HueRotate(amount)
            | Filter::Invert(amount)
            | Filter::Opacity(amount)
            | Filter::Saturate(amount)
            | Filter::Sepia(amount) => {
                let kind = match filter {
                    Filter::Brightness(_) => ColorFilterKind::Brightness,
                    Filter::Contrast(_) => ColorFilterKind::Contrast,
                    Filter::Grayscale(_) => ColorFilterKind::Grayscale,
                    Filter::HueRotate(_) => ColorFilterKind::HueRotate,
                    Filter::Invert(_) => ColorFilterKind::Invert,
                    Filter::Opacity(_) => ColorFilterKind::Opacity,
                    Filter::Saturate(_) => ColorFilterKind::Saturate,
                    Filter::Sepia(_) => ColorFilterKind::Sepia,
                    _ => unreachable!(),
                };
                self.adapter.encode(FilterKernel::ApplyColorFilterToTarget {
                    target,
                    bounds,
                    kind,
                    amount: *amount,
                    linear_rgb: linear_rgb && !matches!(kind, ColorFilterKind::Opacity),
                })
            }
        }
    }
}
