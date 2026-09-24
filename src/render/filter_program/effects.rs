use super::*;
impl<A: FilterAdapter> FilterExecutor<'_, A> {
    pub(crate) fn apply_filter(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        filter: &Filter,
        region: Option<&crate::shared::layer::region::Region>,
        linear_rgb: bool,
        filter_cursors: &mut FilterCursors,
    ) -> bool {
        match filter {
            Filter::ProgressiveBlur(blur) => self.adapter.encode(FilterKernel::ProgressiveBlur {
                target,
                bounds,
                blur: *blur,
            }),
            Filter::Graph { primitives, .. } => {
                self.apply_filter_graph(target, bounds, primitives, filter_cursors)
            }
            Filter::Chain { filters, .. } => filters.iter().all(|filter| {
                self.apply_filter(target, bounds, filter, region, linear_rgb, filter_cursors)
            }),
            Filter::RectLiquidGlass(glass) => {
                let Some(glass_region) = rect_liquid_glass_region(region, bounds) else {
                    return false;
                };
                self.apply_liquid_glass(target, bounds, *glass, glass_region)
            }
            Filter::Offset { dx, dy } => {
                let dx = filter_model::filter_offset_to_pixel_delta(*dx);
                let dy = filter_model::filter_offset_to_pixel_delta(*dy);
                if dx == 0 && dy == 0 {
                    return true;
                }
                let Some(temp) = self.adapter.acquire_scratch() else {
                    return false;
                };
                let ok = self.adapter.encode(FilterKernel::OffsetRegionToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    dx,
                    dy,
                }) && self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                });
                self.adapter.release_scratch(temp);
                ok
            }
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => self.apply_blur(target, bounds, *std_dev_x, *std_dev_y, *sampling),
            // Encoding failure must stop a chain before later kernels consume partial output.
            Filter::ColorMatrix(matrix) => {
                self.adapter.encode(FilterKernel::ApplyColorMatrixToTarget {
                    target,
                    bounds,
                    matrix: *matrix,
                })
            }
            Filter::ComponentTransfer(_) => {
                let table_index = filter_cursors.next_transfer_index();
                self.adapter
                    .encode(FilterKernel::ApplyComponentTransferToTarget {
                        target,
                        bounds,
                        table_index,
                        linear_rgb,
                    })
            }
            Filter::ConvolveMatrix(matrix) => {
                let kernel_offset = filter_cursors.next_convolve_offset(matrix);
                let Some(temp) = self.adapter.acquire_scratch() else {
                    return false;
                };
                let ok = self.adapter.encode(FilterKernel::ConvolveMatrixToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    matrix,
                    kernel_offset,
                    linear_rgb,
                }) && self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                });
                self.adapter.release_scratch(temp);
                ok
            }
            Filter::DiffuseLighting(lighting) => {
                let Some(temp) = self.adapter.acquire_scratch() else {
                    return false;
                };
                let ok = self.adapter.encode(FilterKernel::DiffuseLightingToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    lighting,
                }) && self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                });
                self.adapter.release_scratch(temp);
                ok
            }
            Filter::SpecularLighting(lighting) => {
                let Some(temp) = self.adapter.acquire_scratch() else {
                    return false;
                };
                let ok = self.adapter.encode(FilterKernel::SpecularLightingToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    lighting,
                }) && self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                });
                self.adapter.release_scratch(temp);
                ok
            }
            Filter::Flood { brush } => {
                let brush_offset = filter_cursors.next_brush_offset(brush);
                self.adapter.encode(FilterKernel::FloodRegionToTarget {
                    target,
                    bounds,
                    brush_offset,
                })
            }
            Filter::DropShadow {
                brush,
                offset_x,
                offset_y,
                std_dev,
                ..
            } => self.apply_drop_shadow(
                target,
                bounds,
                *offset_x,
                *offset_y,
                *std_dev,
                filter_cursors.next_brush_offset(brush),
            ),
            Filter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => {
                let raw_radius_x = radius_x.max(0.0).ceil() as u32;
                let raw_radius_y = radius_y.max(0.0).ceil() as u32;
                if raw_radius_x == 0 && raw_radius_y == 0 {
                    return true;
                }
                if *operator == filter_model::MorphologyOperator::Erode
                    && (raw_radius_x.saturating_mul(2) >= self.adapter.size().0
                        || raw_radius_y.saturating_mul(2) >= self.adapter.size().1)
                {
                    return self.adapter.encode(FilterKernel::ClearRenderRegion {
                        target,
                        bounds,
                        color: 0,
                    });
                }

                let Some(temp) = self.adapter.acquire_scratch() else {
                    return false;
                };
                let Some(output) = self.adapter.acquire_scratch() else {
                    self.adapter.release_scratch(temp);
                    return false;
                };
                let cleared = self.adapter.encode(FilterKernel::ClearRenderTarget {
                    target: temp,
                    color: 0,
                }) && self.adapter.encode(FilterKernel::ClearRenderTarget {
                    target: output,
                    color: 0,
                });
                let radius_x = raw_radius_x.min(self.adapter.size().0.saturating_sub(1));
                let radius_y = raw_radius_y.min(self.adapter.size().1.saturating_sub(1));
                let operator = *operator;
                let ok = cleared
                    && self.adapter.encode(FilterKernel::MorphologyAxisToTarget {
                        source: target,
                        target: temp,
                        bounds,
                        radius: radius_x,
                        operator,
                        axis: 0,
                    })
                    && self.adapter.encode(FilterKernel::MorphologyAxisToTarget {
                        source: temp,
                        target: output,
                        bounds,
                        radius: radius_y,
                        operator,
                        axis: 1,
                    })
                    && self.adapter.encode(FilterKernel::CopyRegionToTarget {
                        source: output,
                        target,
                        bounds,
                    });
                self.adapter.release_scratch(output);
                self.adapter.release_scratch(temp);
                ok
            }
            _ => {
                let Some((filter_kind, amount)) = color_filter(filter) else {
                    return false;
                };
                self.adapter.encode(FilterKernel::ApplyColorFilterToTarget {
                    target,
                    bounds,
                    kind: filter_kind,
                    amount,
                })
            }
        }
    }
    pub(super) fn apply_drop_shadow(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        offset_x: f32,
        offset_y: f32,
        std_dev: f32,
        brush_offset: u32,
    ) -> bool {
        let Some(shadow) = self.adapter.acquire_scratch() else {
            return false;
        };
        let cleared = self.adapter.encode(FilterKernel::ClearRenderRegion {
            target: shadow,
            bounds,
            color: 0,
        });
        if !cleared
            || !self
                .adapter
                .encode(FilterKernel::BuildDropShadowMaskToTarget {
                    source: target,
                    target: shadow,
                    bounds,
                    dx: offset_x.round() as i32,
                    dy: offset_y.round() as i32,
                })
        {
            self.adapter.release_scratch(shadow);
            return false;
        }

        let std_dev = std_dev.max(0.0);
        if std_dev > 0.0 {
            let Some(temp) = self.adapter.acquire_scratch() else {
                self.adapter.release_scratch(shadow);
                return false;
            };
            let ok = self.adapter.encode(FilterKernel::BlurRegionToTarget {
                source: shadow,
                target: temp,
                bounds,
                std_dev,
                axis: 0,
            }) && self.adapter.encode(FilterKernel::BlurRegionToTarget {
                source: temp,
                target: shadow,
                bounds,
                std_dev,
                axis: 1,
            });
            self.adapter.release_scratch(temp);
            if !ok {
                self.adapter.release_scratch(shadow);
                return false;
            }
        }

        let ok = self
            .adapter
            .encode(FilterKernel::CompositeDropShadowToTarget {
                target,
                shadow,
                bounds,
                brush_offset,
            });
        self.adapter.release_scratch(shadow);
        ok
    }
    pub(super) fn apply_blur(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        if std_dev_x <= 0.0 && std_dev_y <= 0.0 {
            return true;
        }

        let factor = sampling.factor();
        if factor > 1 && std_dev_x > 0.0 && std_dev_y > 0.0 {
            let Some(low) = self.adapter.acquire_scratch() else {
                return false;
            };
            let Some(temp) = self.adapter.acquire_scratch() else {
                self.adapter.release_scratch(low);
                return false;
            };
            let ok = self.downsampled_blur_to_target(
                target, target, low, temp, bounds, std_dev_x, std_dev_y, sampling,
            );
            self.adapter.release_scratch(temp);
            self.adapter.release_scratch(low);
            return ok;
        }

        let Some(temp) = self.adapter.acquire_scratch() else {
            return false;
        };
        let ok = match (std_dev_x > 0.0, std_dev_y > 0.0) {
            (true, true) => {
                self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    std_dev: std_dev_x,
                    axis: 0,
                }) && self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                })
            }
            (true, false) => {
                self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    std_dev: std_dev_x,
                    axis: 0,
                }) && self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                })
            }
            (false, true) => {
                self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: target,
                    target: temp,
                    bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                }) && self.adapter.encode(FilterKernel::CopyRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                })
            }
            (false, false) => true,
        };
        self.adapter.release_scratch(temp);
        ok
    }
    pub(crate) fn apply_blur_from_source(
        &mut self,
        source: RenderTargetId,
        target: RenderTargetId,
        bounds: Bounds,
        std_dev_x: f32,
        std_dev_y: f32,
        sampling: filter_model::BlurSampling,
    ) -> bool {
        if source == target {
            return self.apply_blur(target, bounds, std_dev_x, std_dev_y, sampling);
        }

        let std_dev_x = std_dev_x.max(0.0);
        let std_dev_y = std_dev_y.max(0.0);
        if std_dev_x <= 0.0 && std_dev_y <= 0.0 {
            return self.adapter.encode(FilterKernel::CopyRegionToTarget {
                source,
                target,
                bounds,
            });
        }

        let factor = sampling.factor();
        if factor > 1 && std_dev_x > 0.0 && std_dev_y > 0.0 {
            let Some(low) = self.adapter.acquire_scratch() else {
                return false;
            };
            let Some(temp) = self.adapter.acquire_scratch() else {
                self.adapter.release_scratch(low);
                return false;
            };
            let ok = self.downsampled_blur_to_target(
                source, target, low, temp, bounds, std_dev_x, std_dev_y, sampling,
            );
            self.adapter.release_scratch(temp);
            self.adapter.release_scratch(low);
            return ok;
        }

        match (std_dev_x > 0.0, std_dev_y > 0.0) {
            (true, true) => {
                let Some(temp) = self.adapter.acquire_scratch() else {
                    return false;
                };
                let ok = self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source,
                    target: temp,
                    bounds,
                    std_dev: std_dev_x,
                    axis: 0,
                }) && self.adapter.encode(FilterKernel::BlurRegionToTarget {
                    source: temp,
                    target,
                    bounds,
                    std_dev: std_dev_y,
                    axis: 1,
                });
                self.adapter.release_scratch(temp);
                ok
            }
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
            (false, false) => true,
        }
    }
    pub(super) fn apply_liquid_glass(
        &mut self,
        target: RenderTargetId,
        bounds: Bounds,
        glass: filter_model::RectLiquidGlass,
        region: filter_model::RectLiquidGlassRegion,
    ) -> bool {
        let Some(source) = self.adapter.acquire_scratch() else {
            return false;
        };
        let Some(blurred) = self.adapter.acquire_scratch() else {
            self.adapter.release_scratch(source);
            return false;
        };
        let mut ok = self.adapter.encode(FilterKernel::CopyRegionToTarget {
            source: target,
            target: source,
            bounds,
        });

        if ok && glass.blur_radius > 0 {
            let std_dev = glass.blur_radius as f32 * filter_model::LIQUID_GLASS_BLUR_STD_DEV_SCALE;
            ok = if glass.blur_sampling.factor() > 1 {
                let Some(temp) = self.adapter.acquire_scratch() else {
                    self.adapter.release_scratch(blurred);
                    self.adapter.release_scratch(source);
                    return false;
                };
                let ok = self.downsampled_blur_to_target(
                    source,
                    blurred,
                    temp,
                    blurred,
                    bounds,
                    std_dev,
                    std_dev,
                    glass.blur_sampling,
                );
                self.adapter.release_scratch(temp);
                ok
            } else {
                self.apply_blur_from_source(
                    source,
                    blurred,
                    bounds,
                    std_dev,
                    std_dev,
                    filter_model::BlurSampling::FULL_RES,
                )
            };
        } else if ok {
            ok = self.adapter.encode(FilterKernel::CopyRegionToTarget {
                source,
                target: blurred,
                bounds,
            });
        }

        ok = ok
            && self.adapter.encode(FilterKernel::LiquidGlassToTarget {
                source,
                blurred,
                target,
                bounds,
                glass,
                region,
            });
        self.adapter.release_scratch(blurred);
        self.adapter.release_scratch(source);
        ok
    }
}
fn color_filter(filter: &Filter) -> Option<(ColorFilterKind, f32)> {
    match filter {
        Filter::Brightness(amount) => Some((ColorFilterKind::Brightness, *amount)),
        Filter::Contrast(amount) => Some((ColorFilterKind::Contrast, *amount)),
        Filter::Grayscale(amount) => Some((ColorFilterKind::Grayscale, *amount)),
        Filter::HueRotate(amount) => Some((ColorFilterKind::HueRotate, *amount)),
        Filter::Invert(amount) => Some((ColorFilterKind::Invert, *amount)),
        Filter::Opacity(amount) => Some((ColorFilterKind::Opacity, *amount)),
        Filter::Saturate(amount) => Some((ColorFilterKind::Saturate, *amount)),
        Filter::Sepia(amount) => Some((ColorFilterKind::Sepia, *amount)),
        _ => None,
    }
}
