use peniko::kurbo::{Affine, Point, Rect};

use super::Scene;
use crate::shared::{
    bounds::{Bounds, PixelBounds},
    brush::Brush,
    execution::Command,
    layer::{
        Layer,
        filter::{Filter, FilterPrimitiveKind},
        region::Region,
    },
    sdf::{
        Sdf, SdfShadow,
        candlestick::CandleStick as SdfCandleStick,
        rect::{Radius, Rect as SdfRect, RectShadowOptions, StrokeWidths},
    },
};

#[derive(Clone, Copy)]
struct SceneScale {
    scale: f64,
    dx: f64,
    dy: f64,
}

impl SceneScale {
    fn coord_f32(self, value: f32, offset: f64) -> f32 {
        (f64::from(value) * self.scale + offset) as f32
    }

    fn scalar_f32(self, value: f32) -> f32 {
        (f64::from(value) * self.scale) as f32
    }

    fn point(self, point: Point) -> Point {
        Point::new(
            point.x * self.scale + self.dx,
            point.y * self.scale + self.dy,
        )
    }

    fn rect(self, rect: Rect) -> Rect {
        Rect::new(
            rect.x0 * self.scale + self.dx,
            rect.y0 * self.scale + self.dy,
            rect.x1 * self.scale + self.dx,
            rect.y1 * self.scale + self.dy,
        )
    }

    fn bounds(self, bounds: Bounds) -> Bounds {
        Bounds::new(
            (bounds.x0 as f64 * self.scale + self.dx).floor() as i32,
            (bounds.y0 as f64 * self.scale + self.dy).floor() as i32,
            (bounds.x1 as f64 * self.scale + self.dx).ceil() as i32,
            (bounds.y1 as f64 * self.scale + self.dy).ceil() as i32,
        )
    }

    fn pixel_bounds(self, bounds: PixelBounds) -> PixelBounds {
        let bounds = self.bounds(Bounds::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1));
        PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
    }

    fn affine(self) -> Affine {
        Affine::translate((self.dx, self.dy)) * Affine::scale(self.scale)
    }

    fn radius(self, radius: Radius) -> Radius {
        Radius {
            top_left: self.scalar_f32(radius.top_left),
            top_right: self.scalar_f32(radius.top_right),
            bottom_left: self.scalar_f32(radius.bottom_left),
            bottom_right: self.scalar_f32(radius.bottom_right),
        }
    }

    fn stroke_widths(self, widths: StrokeWidths) -> StrokeWidths {
        StrokeWidths {
            top: self.scalar_f32(widths.top),
            right: self.scalar_f32(widths.right),
            bottom: self.scalar_f32(widths.bottom),
            left: self.scalar_f32(widths.left),
        }
    }

    fn shadow_options(self, options: RectShadowOptions) -> RectShadowOptions {
        RectShadowOptions {
            offset_x: self.scalar_f32(options.offset_x),
            offset_y: self.scalar_f32(options.offset_y),
            expand: self.scalar_f32(options.expand),
            intensity: options.intensity,
        }
    }

    fn sdf_rect(self, mut rect: SdfRect) -> SdfRect {
        rect.start = self.point(rect.start);
        rect.end = self.point(rect.end);
        rect.radius = self.radius(rect.radius);
        rect
    }

    fn sdf(self, sdf: Sdf) -> Sdf {
        match sdf {
            Sdf::Rect(rect) => Sdf::Rect(self.sdf_rect(rect)),
            Sdf::RectStroke(mut stroke) => {
                stroke.rect = self.sdf_rect(stroke.rect);
                stroke.widths = self.stroke_widths(stroke.widths);
                Sdf::RectStroke(stroke)
            }
            Sdf::Circle(mut circle) => {
                circle.center = self.point(circle.center);
                circle.radius = self.scalar_f32(circle.radius);
                Sdf::Circle(circle)
            }
            Sdf::CircleStroke(mut stroke) => {
                stroke.circle.center = self.point(stroke.circle.center);
                stroke.circle.radius = self.scalar_f32(stroke.circle.radius);
                stroke.half_width = self.scalar_f32(stroke.half_width);
                Sdf::CircleStroke(stroke)
            }
            Sdf::Arc(mut arc) => {
                arc.center = self.point(arc.center);
                arc.radius = self.scalar_f32(arc.radius);
                arc.width = self.scalar_f32(arc.width);
                Sdf::Arc(arc)
            }
            Sdf::CandleStick(candle) => Sdf::CandleStick(self.candlestick(candle)),
            Sdf::Line(mut line) => {
                line.start = self.point(line.start);
                line.end = self.point(line.end);
                line.width = self.scalar_f32(line.width);
                Sdf::Line(line)
            }
        }
    }

    fn sdf_shadow(self, shadow: SdfShadow) -> SdfShadow {
        match shadow {
            SdfShadow::Rect(mut shadow) => {
                shadow.rect = self.sdf_rect(shadow.rect);
                shadow.options = self.shadow_options(shadow.options);
                SdfShadow::Rect(shadow)
            }
            SdfShadow::Circle(mut shadow) => {
                shadow.circle.center = self.point(shadow.circle.center);
                shadow.circle.radius = self.scalar_f32(shadow.circle.radius);
                shadow.options = self.shadow_options(shadow.options);
                SdfShadow::Circle(shadow)
            }
            SdfShadow::Arc(mut shadow) => {
                shadow.arc = match self.sdf(Sdf::Arc(shadow.arc)) {
                    Sdf::Arc(arc) => arc,
                    _ => unreachable!(),
                };
                shadow.options = self.shadow_options(shadow.options);
                SdfShadow::Arc(shadow)
            }
            SdfShadow::Line(mut shadow) => {
                shadow.line = match self.sdf(Sdf::Line(shadow.line)) {
                    Sdf::Line(line) => line,
                    _ => unreachable!(),
                };
                shadow.options = self.shadow_options(shadow.options);
                SdfShadow::Line(shadow)
            }
        }
    }

    fn candlestick(self, mut candle: SdfCandleStick) -> SdfCandleStick {
        candle.center_x = self.coord_f32(candle.center_x, self.dx);
        candle.high_y = self.coord_f32(candle.high_y, self.dy);
        candle.low_y = self.coord_f32(candle.low_y, self.dy);
        candle.body_top_y = self.coord_f32(candle.body_top_y, self.dy);
        candle.body_bottom_y = self.coord_f32(candle.body_bottom_y, self.dy);
        candle.body_width = scaled_odd_width(candle.body_width, self.scale as f32);
        candle
    }

    fn command(self, command: &mut Command) {
        match command {
            Command::Draw(_) => {}
            Command::Layer { layer, .. } => self.layer(layer),
            Command::MaskLayer { layer, .. } => {
                layer.region = self.region(layer.region.clone());
            }
        }
    }

    fn layer(self, layer: &mut Layer) {
        match layer {
            Layer::Clip | Layer::Isolate | Layer::Opacity(_) | Layer::Blend(_) => {}
            Layer::ClipSdf { sdf, bounds } => {
                *sdf = self.sdf(*sdf);
                *bounds = self.bounds(*bounds);
            }
            Layer::Filter {
                filter,
                sample_region,
            }
            | Layer::Backdrop {
                filter,
                sample_region,
            } => {
                self.filter(filter);
                *sample_region = self.region(sample_region.clone());
            }
        }
    }

    fn region(self, region: Region) -> Region {
        match region {
            Region::Rect { rect, radius } => Region::rect(self.rect(rect), self.radius(radius)),
            Region::Path {
                path,
                transform,
                tolerance,
            } => Region::path(path, self.affine() * transform, tolerance * self.scale),
        }
    }

    fn filter(self, filter: &mut Filter) {
        match filter {
            Filter::Chain { filters, .. } => {
                for filter in filters {
                    self.filter(filter);
                }
            }
            Filter::Graph { primitives, .. } => {
                for primitive in primitives {
                    primitive.region = self.bounds(primitive.region);
                    self.filter_primitive_kind(&mut primitive.kind);
                }
            }
            Filter::RectLiquidGlass(glass) => {
                glass.blur_radius = ((glass.blur_radius as f64 * self.scale).round() as u32).max(1);
                glass.refraction_thickness = self.scalar_f32(glass.refraction_thickness);
                glass.fresnel_range = self.scalar_f32(glass.fresnel_range);
                glass.glare_range = self.scalar_f32(glass.glare_range);
            }
            Filter::Blur {
                std_dev_x,
                std_dev_y,
            } => {
                *std_dev_x = self.scalar_f32(*std_dev_x);
                *std_dev_y = self.scalar_f32(*std_dev_y);
            }
            Filter::Flood { brush } => self.brush(brush),
            Filter::ConvolveMatrix(_) => {}
            Filter::DiffuseLighting(lighting) => {
                lighting.surface_scale = self.scalar_f32(lighting.surface_scale);
                lighting.light_source = self.light_source(lighting.light_source);
            }
            Filter::SpecularLighting(lighting) => {
                lighting.surface_scale = self.scalar_f32(lighting.surface_scale);
                lighting.light_source = self.light_source(lighting.light_source);
            }
            Filter::Offset { dx, dy } => {
                *dx = self.scalar_f32(*dx);
                *dy = self.scalar_f32(*dy);
            }
            Filter::Morphology {
                radius_x, radius_y, ..
            } => {
                *radius_x = self.scalar_f32(*radius_x);
                *radius_y = self.scalar_f32(*radius_y);
            }
            Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                brush,
            } => {
                *offset_x = self.scalar_f32(*offset_x);
                *offset_y = self.scalar_f32(*offset_y);
                *std_dev = self.scalar_f32(*std_dev);
                self.brush(brush);
            }
            Filter::Brightness(_)
            | Filter::Contrast(_)
            | Filter::ColorMatrix(_)
            | Filter::ComponentTransfer(_)
            | Filter::Grayscale(_)
            | Filter::HueRotate(_)
            | Filter::Invert(_)
            | Filter::Opacity(_)
            | Filter::Saturate(_)
            | Filter::Sepia(_) => {}
        }
    }

    fn filter_primitive_kind(self, kind: &mut FilterPrimitiveKind) {
        match kind {
            FilterPrimitiveKind::Filter(filter) => self.filter(filter),
            FilterPrimitiveKind::Image { brush } => self.brush(brush),
            FilterPrimitiveKind::DisplacementMap(map) => {
                map.scale_x = self.scalar_f32(map.scale_x);
                map.scale_y = self.scalar_f32(map.scale_y);
            }
            FilterPrimitiveKind::Tile { source_region } => {
                *source_region = self.bounds(*source_region);
            }
            FilterPrimitiveKind::Turbulence(turbulence) => self.turbulence(turbulence),
            FilterPrimitiveKind::Identity
            | FilterPrimitiveKind::Blend { .. }
            | FilterPrimitiveKind::Composite { .. }
            | FilterPrimitiveKind::Merge { .. } => {}
        }
    }

    fn turbulence(self, turbulence: &mut crate::shared::layer::filter::Turbulence) {
        turbulence.transform_x = self.coord_f32(turbulence.transform_x, self.dx);
        turbulence.transform_y = self.coord_f32(turbulence.transform_y, self.dy);
        turbulence.scale_x = self.scalar_f32(turbulence.scale_x);
        turbulence.scale_y = self.scalar_f32(turbulence.scale_y);
        turbulence.tile_x = self.coord_f32(turbulence.tile_x, self.dx);
        turbulence.tile_y = self.coord_f32(turbulence.tile_y, self.dy);
        turbulence.tile_width = self.scalar_f32(turbulence.tile_width);
        turbulence.tile_height = self.scalar_f32(turbulence.tile_height);
    }

    fn light_source(
        self,
        source: crate::shared::layer::filter::LightSource,
    ) -> crate::shared::layer::filter::LightSource {
        use crate::shared::layer::filter::LightSource;
        match source {
            LightSource::Distant { azimuth, elevation } => {
                LightSource::Distant { azimuth, elevation }
            }
            LightSource::Point { x, y, z } => LightSource::Point {
                x: self.coord_f32(x, self.dx),
                y: self.coord_f32(y, self.dy),
                z: self.scalar_f32(z),
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
                x: self.coord_f32(x, self.dx),
                y: self.coord_f32(y, self.dy),
                z: self.scalar_f32(z),
                points_at_x: self.coord_f32(points_at_x, self.dx),
                points_at_y: self.coord_f32(points_at_y, self.dy),
                points_at_z: self.scalar_f32(points_at_z),
                specular_exponent,
                limiting_cone_angle,
            },
        }
    }

    fn brush(self, brush: &mut Brush) {
        match brush {
            Brush::Solid(_) => {}
            Brush::Linear(gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
            }
            Brush::Radial(gradient) => {
                gradient.transform = self.brush_transform(gradient.transform);
            }
            Brush::Sweep(gradient) => {
                gradient.center[0] = self.coord_f32(gradient.center[0], self.dx);
                gradient.center[1] = self.coord_f32(gradient.center[1], self.dy);
            }
            Brush::FourCorner(gradient) => {
                gradient.bounds[0] = self.coord_f32(gradient.bounds[0], self.dx);
                gradient.bounds[1] = self.coord_f32(gradient.bounds[1], self.dy);
                gradient.bounds[2] = self.coord_f32(gradient.bounds[2], self.dx);
                gradient.bounds[3] = self.coord_f32(gradient.bounds[3], self.dy);
            }
            Brush::Pattern(pattern) => {
                pattern.transform = self.brush_transform(pattern.transform);
            }
        }
    }

    fn brush_transform(self, transform: [f32; 6]) -> [f32; 6] {
        let [a, b, c, d, e, f] = transform;
        let scale = self.scale as f32;
        let dx = self.dx as f32;
        let dy = self.dy as f32;
        [
            a / scale,
            b / scale,
            c / scale,
            d / scale,
            e - (a * dx + c * dy) / scale,
            f - (b * dx + d * dy) / scale,
        ]
    }
}

impl Scene {
    /// Returns a new scene whose geometry is uniformly scaled and centered into
    /// `target_width` x `target_height`.
    ///
    /// This transforms vector/SDF scene data before rendering. It intentionally
    /// does not upscale a rendered bitmap, so examples and previews keep crisp
    /// edges at the final output resolution.
    ///
    /// Bitmap text layouts must be rebuilt at the target font size before being
    /// pushed into the scene. Path text is normal vector geometry and can be
    /// scaled through this method.
    pub fn scaled_to_fit(mut self, target_width: u32, target_height: u32) -> Self {
        if self.width == target_width && self.height == target_height {
            return self;
        }
        assert!(
            self.text_glyphs.is_empty() && self.text_runs.is_empty(),
            "Scene::scaled_to_fit cannot scale bitmap text; rebuild TextLayout at the target font size or push text as paths"
        );
        if self.width == 0 || self.height == 0 || target_width == 0 || target_height == 0 {
            self.width = target_width;
            self.height = target_height;
            self.rebuild_backdrop_records();
            return self;
        }

        let scale = (target_width as f64 / self.width as f64)
            .min(target_height as f64 / self.height as f64);
        let transform = SceneScale {
            scale,
            dx: (target_width as f64 - self.width as f64 * scale) * 0.5,
            dy: (target_height as f64 - self.height as f64 * scale) * 0.5,
        };

        for line in &mut self.lines {
            line.p0[0] = transform.coord_f32(line.p0[0], transform.dx);
            line.p0[1] = transform.coord_f32(line.p0[1], transform.dy);
            line.p1[0] = transform.coord_f32(line.p1[0], transform.dx);
            line.p1[1] = transform.coord_f32(line.p1[1], transform.dy);
        }

        for draw in &mut self.draw_records {
            draw.pixel_bounds = transform.pixel_bounds(draw.pixel_bounds);
            draw.sdf = draw.sdf.map(|sdf| transform.sdf(sdf));
            draw.sdf_shadow = draw
                .sdf_shadow
                .map(|sdf_shadow| transform.sdf_shadow(sdf_shadow));
            transform.brush(&mut draw.brush);
        }

        for command_list in &mut self.command_lists {
            for command in &mut command_list.commands {
                transform.command(command);
            }
        }

        self.width = target_width;
        self.height = target_height;
        self.rebuild_backdrop_records();
        self
    }
}

fn scaled_odd_width(width: u32, scale: f32) -> u32 {
    let mut width = ((width as f32 * scale).round() as u32).max(1);
    if width.is_multiple_of(2) {
        width += 1;
    }
    width
}
