//! Translate owned scene payloads without cloning filter/mask descriptors a second time.
use super::*;

impl SceneOffset {
    pub(super) fn new(pos: Point) -> Self {
        assert!(
            pos.x.is_finite() && pos.y.is_finite(),
            "canvas append position must be finite"
        );
        Self {
            dx: pos.x,
            dy: pos.y,
        }
    }

    pub(super) fn is_zero(self) -> bool {
        self.dx == 0.0 && self.dy == 0.0
    }

    pub(super) fn line(self, line: &mut Line) {
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        line.p0[0] += dx;
        line.p0[1] += dy;
        line.p1[0] += dx;
        line.p1[1] += dy;
    }

    pub(super) fn pixel_bounds(self, bounds: PixelBounds) -> PixelBounds {
        PixelBounds {
            x0: (bounds.x0 as f64 + self.dx).floor() as i32,
            y0: (bounds.y0 as f64 + self.dy).floor() as i32,
            x1: (bounds.x1 as f64 + self.dx).ceil() as i32,
            y1: (bounds.y1 as f64 + self.dy).ceil() as i32,
        }
    }

    pub(super) fn bounds(self, bounds: Bounds) -> Bounds {
        Bounds::new(
            (bounds.x0 as f64 + self.dx).floor() as i32,
            (bounds.y0 as f64 + self.dy).floor() as i32,
            (bounds.x1 as f64 + self.dx).ceil() as i32,
            (bounds.y1 as f64 + self.dy).ceil() as i32,
        )
    }

    fn rect(self, rect: Rect) -> Rect {
        Rect::new(
            rect.x0 + self.dx,
            rect.y0 + self.dy,
            rect.x1 + self.dx,
            rect.y1 + self.dy,
        )
    }

    fn transform(self, transform: Affine) -> Affine {
        Affine::translate((self.dx, self.dy)) * transform
    }

    pub(super) fn sdf(self, sdf: Sdf) -> Sdf {
        // SDF::translated subtracts its arguments for offscreen local-space
        // conversion. Appending moves local child geometry into parent space.
        sdf.translated(-(self.dx as f32), -(self.dy as f32))
    }

    pub(super) fn sdf_shadow(self, sdf_shadow: SdfShadow) -> SdfShadow {
        sdf_shadow.translated(-(self.dx as f32), -(self.dy as f32))
    }

    pub(super) fn brush(self, brush: Brush) -> Brush {
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        match brush {
            Brush::Solid(_) => brush,
            Brush::Linear(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Linear(gradient)
            }
            Brush::Radial(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Radial(gradient)
            }
            Brush::Sweep(mut gradient) => {
                gradient.center[0] += dx;
                gradient.center[1] += dy;
                Brush::Sweep(gradient)
            }
            Brush::FourCorner(mut gradient) => {
                gradient.bounds[0] += dx;
                gradient.bounds[1] += dy;
                gradient.bounds[2] += dx;
                gradient.bounds[3] += dy;
                Brush::FourCorner(gradient)
            }
            Brush::Mesh(mut gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
                Brush::Mesh(gradient)
            }
            Brush::Program(mut program) => {
                program.0.transform = self.brush_transform(program.0.transform);
                Brush::Program(program)
            }
            Brush::Pattern(mut pattern) => {
                pattern.transform = self.brush_transform(pattern.transform);
                Brush::Pattern(pattern)
            }
        }
    }

    fn brush_transform(self, transform: [f32; 6]) -> [f32; 6] {
        let [a, b, c, d, e, f] = transform;
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        [a, b, c, d, e - a * dx - c * dy, f - b * dx - d * dy]
    }

    fn layer(self, layer: Layer) -> Layer {
        match layer {
            Layer::Clip { .. }
            | Layer::ClipSdf { .. }
            | Layer::Isolate { .. }
            | Layer::Opacity { .. }
            | Layer::Blend { .. } => layer,
            Layer::Filter {
                filter,
                sample_region,
            } => Layer::Filter {
                filter: self.filter(filter),
                sample_region: self.region(sample_region),
            },
            Layer::Backdrop {
                filter,
                sample_region,
            } => Layer::Backdrop {
                filter: self.filter(filter),
                sample_region: self.region(sample_region),
            },
        }
    }

    fn mask(self, mask: Mask) -> Mask {
        Mask {
            region: self.region(mask.region),
            kind: mask.kind,
        }
    }

    fn filter(self, filter: Filter) -> Filter {
        match filter {
            Filter::ProgressiveBlur(blur) => {
                Filter::ProgressiveBlur(blur.translated(peniko::kurbo::Vec2::new(self.dx, self.dy)))
            }
            Filter::Chain {
                filters,
                fixed_region,
            } => Filter::Chain {
                filters: filters
                    .into_iter()
                    .map(|filter| self.filter(filter))
                    .collect(),
                fixed_region,
            },
            Filter::Graph {
                primitives,
                fixed_region,
            } => Filter::Graph {
                primitives: primitives
                    .into_iter()
                    .map(|primitive| FilterPrimitive {
                        input: primitive.input,
                        input2: primitive.input2,
                        region: self.bounds(primitive.region),
                        linear_rgb: primitive.linear_rgb,
                        kind: self.primitive_kind(primitive.kind),
                    })
                    .collect(),
                fixed_region,
            },
            Filter::ShaderPipeline(mut pipeline) => {
                pipeline.transform = self.brush_transform(pipeline.transform);
                Filter::ShaderPipeline(pipeline)
            }
            Filter::Flood { brush } => Filter::Flood {
                brush: self.brush(brush),
            },
            Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                brush,
            } => Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                brush: self.brush(brush),
            },
            Filter::DiffuseLighting(mut lighting) => {
                lighting.light_source = self.light_source(lighting.light_source);
                Filter::DiffuseLighting(lighting)
            }
            Filter::SpecularLighting(mut lighting) => {
                lighting.light_source = self.light_source(lighting.light_source);
                Filter::SpecularLighting(lighting)
            }
            _ => filter,
        }
    }

    fn light_source(self, source: LightSource) -> LightSource {
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        match source {
            LightSource::Distant { .. } => source,
            LightSource::Point { x, y, z } => LightSource::Point {
                x: x + dx,
                y: y + dy,
                z,
            },
            LightSource::Spot {
                x,
                y,
                z,
                points_at_x,
                points_at_y,
                points_at_z,
                specular_exponent,
                limiting_cone_angle,
            } => LightSource::Spot {
                x: x + dx,
                y: y + dy,
                z,
                points_at_x: points_at_x + dx,
                points_at_y: points_at_y + dy,
                points_at_z,
                specular_exponent,
                limiting_cone_angle,
            },
        }
    }

    fn primitive_kind(self, kind: FilterPrimitiveKind) -> FilterPrimitiveKind {
        match kind {
            FilterPrimitiveKind::ShaderPipeline {
                mut pipeline,
                inputs,
            } => {
                pipeline.transform = self.brush_transform(pipeline.transform);
                FilterPrimitiveKind::ShaderPipeline { pipeline, inputs }
            }
            FilterPrimitiveKind::Filter(filter) => {
                FilterPrimitiveKind::Filter(Box::new(self.filter(*filter)))
            }
            FilterPrimitiveKind::Image { brush } => FilterPrimitiveKind::Image {
                brush: self.brush(brush),
            },
            FilterPrimitiveKind::Tile { source_region } => FilterPrimitiveKind::Tile {
                source_region: self.bounds(source_region),
            },
            FilterPrimitiveKind::Turbulence(mut turbulence) => {
                turbulence.transform_x += self.dx as f32;
                turbulence.transform_y += self.dy as f32;
                turbulence.tile_x += self.dx as f32;
                turbulence.tile_y += self.dy as f32;
                FilterPrimitiveKind::Turbulence(turbulence)
            }
            _ => kind,
        }
    }

    fn region(self, region: Region) -> Region {
        match region {
            Region::Rect { rect, radius } => Region::rect(self.rect(rect), radius),
            Region::Path {
                path,
                transform,
                tolerance,
            } => Region::path(path, self.transform(transform), tolerance),
        }
    }

    pub(super) fn command(self, command: Command) -> Command {
        match command {
            Command::Draw(_) | Command::MaterializedRetainedScene { .. } => command,
            Command::Layer {
                retained,
                layer,
                children,
            } => Command::Layer {
                retained,
                layer: self.layer(layer),
                children,
            },
            Command::MaskLayer {
                retained,
                layer,
                content,
                mask,
            } => Command::MaskLayer {
                retained,
                layer: self.mask(layer),
                content,
                mask,
            },
        }
    }
}
