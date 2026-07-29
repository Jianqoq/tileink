use super::*;

impl Canvas {
    pub fn new(logical_width: u32, logical_height: u32, scale_factor: f32) -> Self {
        let scale = valid_scale_factor(scale_factor);
        Self {
            lines: Vec::new(),
            path_records: Vec::new(),
            draw_records: Vec::new(),
            brush_blob: Vec::new(),
            sdf_blob: Vec::new(),
            sdf_shadow_blob: Vec::new(),
            text_glyphs: Vec::new(),
            text_runs: Vec::new(),
            scene_images: ImageResourceStore::default(),
            command_lists: vec![CommandList::default()],
            root_commands: ROOT_COMMAND_LIST_ID,
            command_stack: vec![ROOT_COMMAND_LIST_ID],
            layer_stack: Vec::new(),
            path_cnt: 0,
            backdrop_pool_capacity: 0,
            tile_cnt: 0,
            logical_width,
            logical_height,
            scale_factor: scale,
            draw_generation: 0,
            persistent_root: None,
            invalidated_bounds: Vec::new(),
            invalidate_all: false,
            buffer_changes: None,
            plan_cache_key: None,
            compiled_plan: None,
            persistent_frame: None,
            painter_keys: None,
            stable_batch_ids: None,
            stable_batch_counts: None,
            retained_transform: GpuAffine::IDENTITY,
        }
    }

    /// Creates the internal materialized target owned by one persistent scene.
    pub(crate) fn new_persistent(
        logical_width: u32,
        logical_height: u32,
        scale_factor: f32,
        root_id: RetainedNodeId,
    ) -> Self {
        let mut canvas = Self::new(logical_width, logical_height, scale_factor);
        canvas.persistent_root = Some(root_id);
        canvas
    }
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (self.physical_width(), self.physical_height())
    }

    pub fn logical_size(&self) -> (u32, u32) {
        (self.logical_width, self.logical_height)
    }

    /// Changes only the viewport extent and rebuilds path allocations clipped to that viewport.
    /// Geometry, commands, brushes, images, and text remain valid when scale is unchanged.
    pub(crate) fn resize_surface(&mut self, logical_width: u32, logical_height: u32) {
        if (self.logical_width, self.logical_height) == (logical_width, logical_height) {
            return;
        }
        self.set_surface_extent(logical_width, logical_height);
        self.backdrop_pool_capacity = 0;
        self.tile_cnt = 0;
        self.append_path_backdrops_for_paths(
            0,
            self.path_records.len(),
            0,
            self.draw_records.len(),
        );
    }

    pub(crate) fn set_surface_extent(&mut self, logical_width: u32, logical_height: u32) {
        self.logical_width = logical_width;
        self.logical_height = logical_height;
    }

    /// Installs a retained node transform without rewriting local geometry or paint blobs.
    ///
    /// Scan transforms path lines on the GPU; fine rendering maps device samples back into local
    /// brush, SDF, image, and glyph coordinates. CPU work is limited to record bounds and scan
    /// allocation metadata needed by damage and tile binning.
    pub(crate) fn set_retained_transform(&mut self, logical_transform: Affine) {
        let transform = GpuAffine::from_logical(logical_transform, self.scale_factor);
        let inverse = transform
            .inverse()
            .expect("validated retained transforms are invertible");
        let previous = self.retained_transform;
        let delta = transform.compose(
            previous
                .inverse()
                .expect("installed retained transforms are invertible"),
        );
        if previous.a == transform.a
            && previous.b == transform.b
            && previous.c == transform.c
            && previous.d == transform.d
        {
            let offset = SceneOffset {
                dx: (transform.e - previous.e) as f64,
                dy: (transform.f - previous.f) as f64,
            };
            if !offset.is_zero() {
                for list in &mut self.command_lists {
                    for command in &mut list.commands {
                        offset.command(command);
                    }
                }
            }
        }
        for path in &mut self.path_records {
            path.transform = if path.transform == previous {
                transform
            } else {
                delta.compose(path.transform)
            };
        }
        for draw in &mut self.draw_records {
            if draw.transform == previous {
                draw.transform = transform;
                draw.inverse_transform = inverse;
            } else {
                draw.transform = delta.compose(draw.transform);
                draw.inverse_transform = draw
                    .transform
                    .inverse()
                    .expect("composed retained transforms are invertible");
            }
            draw.pixel_bounds = draw.transform.transform_bounds(draw.local_pixel_bounds);
        }
        self.backdrop_pool_capacity = 0;
        self.tile_cnt = 0;
        self.append_path_backdrops_for_paths(
            0,
            self.path_records.len(),
            0,
            self.draw_records.len(),
        );
        self.retained_transform = transform;
    }

    /// Returns whether every layer and command list opened while recording has been closed.
    /// Only a closed canvas can be appended or installed as a retained scene leaf.
    pub fn is_closed_for_append(&self) -> bool {
        self.command_stack.len() == 1 && self.layer_stack.is_empty()
    }

    pub fn physical_width(&self) -> u32 {
        scaled_canvas_extent(self.logical_width, self.scale_factor)
    }

    pub fn physical_height(&self) -> u32 {
        scaled_canvas_extent(self.logical_height, self.scale_factor)
    }

    pub(super) fn scale_f64(&self) -> f64 {
        f64::from(self.scale_factor)
    }

    pub(super) fn scale_f32(&self) -> f32 {
        self.scale_factor
    }

    pub(super) fn device_transform(&self) -> Affine {
        Affine::scale(self.scale_f64())
    }

    pub(super) fn device_tolerance(&self, tolerance: f64) -> f64 {
        tolerance * self.scale_f64()
    }

    pub(super) fn physical_point(&self, point: Point) -> Point {
        Point::new(point.x * self.scale_f64(), point.y * self.scale_f64())
    }

    pub(super) fn physical_rect(&self, rect: Rect) -> Rect {
        let scale = self.scale_f64();
        Rect::new(
            rect.x0 * scale,
            rect.y0 * scale,
            rect.x1 * scale,
            rect.y1 * scale,
        )
    }

    pub(super) fn physical_bounds(&self, bounds: Bounds) -> Bounds {
        let scale = self.scale_f32();
        Bounds::new(
            (bounds.x0 as f32 * scale).floor() as i32,
            (bounds.y0 as f32 * scale).floor() as i32,
            (bounds.x1 as f32 * scale).ceil() as i32,
            (bounds.y1 as f32 * scale).ceil() as i32,
        )
    }

    pub(super) fn physical_radius(&self, radius: Radius) -> Radius {
        let scale = self.scale_f32();
        Radius {
            top_left: radius.top_left * scale,
            top_right: radius.top_right * scale,
            bottom_left: radius.bottom_left * scale,
            bottom_right: radius.bottom_right * scale,
        }
    }

    pub(super) fn physical_stroke_widths(&self, widths: StrokeWidths) -> StrokeWidths {
        let scale = self.scale_f32();
        StrokeWidths {
            top: widths.top * scale,
            right: widths.right * scale,
            bottom: widths.bottom * scale,
            left: widths.left * scale,
        }
    }

    pub(super) fn physical_shadow_options(&self, options: RectShadowOptions) -> RectShadowOptions {
        let scale = self.scale_f32();
        RectShadowOptions {
            offset_x: options.offset_x * scale,
            offset_y: options.offset_y * scale,
            expand: options.expand * scale,
            intensity: options.intensity,
        }
    }

    pub(super) fn physical_filter(&self, filter: Filter) -> Filter {
        let scale = self.scale_f32();
        match filter {
            Filter::Chain {
                filters,
                fixed_region,
            } => Filter::Chain {
                filters: filters
                    .into_iter()
                    .map(|filter| self.physical_filter(filter))
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
                        region: self.physical_bounds(primitive.region),
                        kind: self.physical_filter_primitive_kind(primitive.kind),
                    })
                    .collect(),
                fixed_region,
            },
            Filter::RectLiquidGlass(mut glass) => {
                glass.blur_radius = ((glass.blur_radius as f32) * scale).round().max(1.0) as u32;
                glass.refraction_thickness *= scale;
                glass.fresnel_range *= scale;
                glass.glare_range *= scale;
                Filter::RectLiquidGlass(glass)
            }
            Filter::Blur {
                std_dev_x,
                std_dev_y,
                sampling,
            } => Filter::Blur {
                std_dev_x: std_dev_x * scale,
                std_dev_y: std_dev_y * scale,
                sampling,
            },
            Filter::Flood { brush } => Filter::Flood {
                brush: self.physical_brush(brush),
            },
            Filter::Offset { dx, dy } => Filter::Offset {
                dx: dx * scale,
                dy: dy * scale,
            },
            Filter::Morphology {
                radius_x,
                radius_y,
                operator,
            } => Filter::Morphology {
                radius_x: radius_x * scale,
                radius_y: radius_y * scale,
                operator,
            },
            Filter::DropShadow {
                offset_x,
                offset_y,
                std_dev,
                brush,
            } => Filter::DropShadow {
                offset_x: offset_x * scale,
                offset_y: offset_y * scale,
                std_dev: std_dev * scale,
                brush: self.physical_brush(brush),
            },
            filter => filter,
        }
    }

    pub(super) fn physical_filter_primitive_kind(
        &self,
        kind: FilterPrimitiveKind,
    ) -> FilterPrimitiveKind {
        let scale = self.scale_f32();
        match kind {
            FilterPrimitiveKind::Filter(filter) => {
                FilterPrimitiveKind::Filter(Box::new(self.physical_filter(*filter)))
            }
            FilterPrimitiveKind::Image { brush } => FilterPrimitiveKind::Image {
                brush: self.physical_brush(brush),
            },
            FilterPrimitiveKind::DisplacementMap(mut map) => {
                map.scale_x *= scale;
                map.scale_y *= scale;
                FilterPrimitiveKind::DisplacementMap(map)
            }
            FilterPrimitiveKind::Tile { source_region } => FilterPrimitiveKind::Tile {
                source_region: self.physical_bounds(source_region),
            },
            FilterPrimitiveKind::Turbulence(mut turbulence) => {
                turbulence.transform_x *= scale;
                turbulence.transform_y *= scale;
                turbulence.scale_x *= scale;
                turbulence.scale_y *= scale;
                turbulence.tile_x *= scale;
                turbulence.tile_y *= scale;
                turbulence.tile_width *= scale;
                turbulence.tile_height *= scale;
                FilterPrimitiveKind::Turbulence(turbulence)
            }
            kind => kind,
        }
    }

    pub(super) fn physical_region(&self, region: Region) -> Region {
        match region {
            Region::Rect { rect, radius } => Region::Rect {
                rect: self.physical_rect(rect),
                radius: self.physical_radius(radius),
            },
            Region::Path {
                path,
                transform,
                tolerance,
            } => Region::Path {
                path,
                transform: self.device_transform() * transform,
                tolerance: self.device_tolerance(tolerance),
            },
        }
    }

    pub(super) fn physical_mask(&self, mask: Mask) -> Mask {
        Mask {
            region: self.physical_region(mask.region),
            kind: mask.kind,
        }
    }

    pub(super) fn physical_brush(&self, brush: Brush) -> Brush {
        let inv_scale = 1.0 / self.scale_f32();
        match brush {
            Brush::Solid(_) => brush,
            Brush::Linear(mut gradient) => {
                gradient.transform = scale_brush_transform(gradient.transform, inv_scale);
                Brush::Linear(gradient)
            }
            Brush::Radial(mut gradient) => {
                gradient.transform = scale_brush_transform(gradient.transform, inv_scale);
                Brush::Radial(gradient)
            }
            Brush::Sweep(mut gradient) => {
                gradient.center[0] *= self.scale_f32();
                gradient.center[1] *= self.scale_f32();
                Brush::Sweep(gradient)
            }
            Brush::FourCorner(mut gradient) => {
                let scale = self.scale_f32();
                gradient.bounds[0] *= scale;
                gradient.bounds[1] *= scale;
                gradient.bounds[2] *= scale;
                gradient.bounds[3] *= scale;
                Brush::FourCorner(gradient)
            }
            Brush::Pattern(mut pattern) => {
                pattern.transform = scale_brush_transform(pattern.transform, inv_scale);
                Brush::Pattern(pattern)
            }
        }
    }

    pub(super) fn physical_sdf(&self, sdf: Sdf) -> Sdf {
        match sdf {
            Sdf::Rect(rect) => Sdf::Rect(self.physical_sdf_rect(rect)),
            Sdf::RectStroke(stroke) => Sdf::RectStroke(SdfRectStroke {
                rect: self.physical_sdf_rect(stroke.rect),
                widths: self.physical_stroke_widths(stroke.widths),
            }),
            Sdf::Circle(circle) => Sdf::Circle(SdfCircle {
                center: self.physical_point(circle.center),
                radius: circle.radius * self.scale_f32(),
            }),
            Sdf::CircleStroke(stroke) => Sdf::CircleStroke(SdfCircleStroke {
                circle: SdfCircle {
                    center: self.physical_point(stroke.circle.center),
                    radius: stroke.circle.radius * self.scale_f32(),
                },
                half_width: stroke.half_width * self.scale_f32(),
            }),
            Sdf::Rc(mut arc) => {
                arc.center = self.physical_point(arc.center);
                arc.radius *= self.scale_f32();
                arc.width *= self.scale_f32();
                Sdf::Rc(arc)
            }
            Sdf::CandleStick(mut candle) => {
                let scale = self.scale_f32();
                candle.center_x *= scale;
                candle.high_y *= scale;
                candle.low_y *= scale;
                candle.body_top_y *= scale;
                candle.body_bottom_y *= scale;
                candle.body_width = scaled_positive_u32(candle.body_width, scale);
                candle.wick_width = scaled_positive_u32(candle.wick_width, scale);
                Sdf::CandleStick(candle)
            }
            Sdf::Line(line) => Sdf::Line(self.physical_sdf_line(line)),
            Sdf::DashLine(mut line) => {
                line.line = self.physical_sdf_line(line.line);
                line.dash_length *= self.scale_f32();
                line.gap_length *= self.scale_f32();
                line.dash_offset *= self.scale_f32();
                Sdf::DashLine(line)
            }
            Sdf::Triangle(mut triangle) => {
                triangle.a = self.physical_point(triangle.a);
                triangle.b = self.physical_point(triangle.b);
                triangle.c = self.physical_point(triangle.c);
                triangle.corner_radius *= self.scale_f32();
                Sdf::Triangle(triangle)
            }
            Sdf::Checkerboard(mut checkerboard) => {
                checkerboard.start = self.physical_point(checkerboard.start);
                checkerboard.end = self.physical_point(checkerboard.end);
                checkerboard.cell_size *= self.scale_f32();
                Sdf::Checkerboard(checkerboard)
            }
            Sdf::Star(mut star) => {
                star.center = self.physical_point(star.center);
                star.outer_radius *= self.scale_f32();
                star.inner_radius *= self.scale_f32();
                star.corner_radius *= self.scale_f32();
                Sdf::Star(star)
            }
            Sdf::StarStroke(mut stroke) => {
                stroke.star.center = self.physical_point(stroke.star.center);
                stroke.star.outer_radius *= self.scale_f32();
                stroke.star.inner_radius *= self.scale_f32();
                stroke.star.corner_radius *= self.scale_f32();
                stroke.half_width *= self.scale_f32();
                Sdf::StarStroke(stroke)
            }
        }
    }

    pub(super) fn physical_sdf_shadow(&self, shadow: SdfShadow) -> SdfShadow {
        match shadow {
            SdfShadow::Rect(shadow) => SdfShadow::Rect(SdfRectShadow {
                rect: self.physical_sdf_rect(shadow.rect),
                options: self.physical_shadow_options(shadow.options),
            }),
            SdfShadow::Circle(shadow) => SdfShadow::Circle(SdfCircleShadow {
                circle: SdfCircle {
                    center: self.physical_point(shadow.circle.center),
                    radius: shadow.circle.radius * self.scale_f32(),
                },
                options: self.physical_shadow_options(shadow.options),
            }),
            SdfShadow::Rc(mut shadow) => {
                shadow.arc.center = self.physical_point(shadow.arc.center);
                shadow.arc.radius *= self.scale_f32();
                shadow.arc.width *= self.scale_f32();
                shadow.options = self.physical_shadow_options(shadow.options);
                SdfShadow::Rc(shadow)
            }
            SdfShadow::Line(shadow) => SdfShadow::Line(SdfLineShadow {
                line: self.physical_sdf_line(shadow.line),
                options: self.physical_shadow_options(shadow.options),
            }),
        }
    }

    pub(super) fn physical_sdf_rect(&self, rect: SdfRect) -> SdfRect {
        SdfRect {
            start: self.physical_point(rect.start),
            end: self.physical_point(rect.end),
            radius: self.physical_radius(rect.radius),
        }
    }

    pub(super) fn physical_sdf_line(&self, line: SdfLine) -> SdfLine {
        SdfLine {
            start: self.physical_point(line.start),
            end: self.physical_point(line.end),
            width: line.width * self.scale_f32(),
            cap: line.cap,
        }
    }
}
