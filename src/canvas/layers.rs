use super::*;

#[cfg(test)]
mod tests;

impl Canvas {
    pub fn push_clip_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) {
        // Rectangular clips are common in UI trees. Keeping them as generic
        // paths creates scan segments on tile boundaries and prevents coarse
        // from proving that a fully covered tile needs no clip wrappers. An
        // exact axis-aligned rectangle with pixel-aligned physical edges has
        // identical SDF coverage semantics, avoids path storage entirely, and
        // works for translated/reflected input paths as well.
        if let Some(rect) = axis_aligned_rect_path(&path, transform)
            && self.rect_has_pixel_aligned_edges(rect)
        {
            self.push_clip_sdf_rect_layer(rect, Radius::ZERO);
            return;
        }
        let draw = self.push_layer_path(DrawTag::Clip, path, transform, rule, tolerance);
        self.push_layer_command(Layer::Clip { draw });
    }

    /// Adds a rounded/sharp rectangle clip that is rasterized directly from an SDF.
    ///
    /// This avoids flattening simple rounded clips into path segments while keeping
    /// the SDF geometry as the source of truth until render time.
    pub fn push_clip_sdf_rect_layer(&mut self, rect: Rect, radius: Radius) {
        self.push_clip_sdf_layer(Sdf::Rect(SdfRect {
            start: Point::new(rect.x0, rect.y0),
            end: Point::new(rect.x1, rect.y1),
            radius,
        }));
    }

    pub(super) fn rect_has_pixel_aligned_edges(&self, rect: Rect) -> bool {
        let rect = self.physical_rect(rect);
        [rect.x0, rect.y0, rect.x1, rect.y1]
            .into_iter()
            .all(|value| (value - value.round()).abs() <= 1.0e-9)
    }

    pub fn push_clip_sdf_circle_layer(&mut self, circle: Circle) {
        self.push_clip_sdf_layer(Sdf::Circle(SdfCircle {
            center: circle.center,
            radius: circle.radius as f32,
        }));
    }

    pub fn push_clip_sdf_arc_layer(&mut self, arc: SdfArc) {
        self.push_clip_sdf_layer(Sdf::Rc(arc));
    }

    pub fn push_clip_sdf_line_layer(&mut self, line: SdfLine) {
        self.push_clip_sdf_layer(Sdf::Line(line));
    }

    /// Adds a clip layer backed by exact SDF geometry.
    ///
    /// Unlike path clips, SDF clips do not allocate path records, scan backdrops,
    /// or per-tile segments. The renderer rasterizes the mask directly from the
    /// SDF bounds, so future SDF primitives automatically work as clip layers.
    pub fn push_clip_sdf_layer(&mut self, sdf: Sdf) {
        let sdf = self.physical_sdf(sdf);
        let draw = self.push_physical_sdf_record(
            sdf,
            &Brush::Solid(Color::TRANSPARENT),
            DrawTag::Clip,
            false,
        );
        self.push_layer_command(Layer::ClipSdf { draw });
    }

    /// Adds an affine-transformed clip while preserving the SDF as local GPU geometry.
    ///
    /// Rectangular UI clips use this path so their coverage exactly matches transformed SDF
    /// rectangles instead of depending on a CPU-flattened path approximation. Returns `false`
    /// for a non-invertible transform without mutating the canvas.
    pub fn push_clip_sdf_layer_transformed(&mut self, sdf: Sdf, transform: Affine) -> bool {
        let transform = GpuAffine::from_logical(transform, self.scale_factor());
        let Some(inverse_transform) = transform.inverse() else {
            return false;
        };
        let sdf = self.physical_sdf(sdf);
        let draw = self.push_physical_sdf_record(
            sdf,
            &Brush::Solid(Color::TRANSPARENT),
            DrawTag::Clip,
            false,
        );
        let record = &mut self.draw_records[draw];
        record.transform = transform;
        record.inverse_transform = inverse_transform;
        record.pixel_bounds = transform.transform_bounds(record.local_pixel_bounds);
        self.push_layer_command(Layer::ClipSdf { draw });
        true
    }

    /// Starts an isolated source-over group.
    ///
    /// This is the renderer primitive for SVG/CSS `isolation:isolate` without
    /// opacity, blending, or filtering. The children are composited into a
    /// transparent offscreen buffer first, then the group is composited back
    /// through the supplied layer path and any outer clips.
    pub fn push_isolate_layer(&mut self, path: BezPath, transform: Affine, tolerance: f64) {
        let draw = self.push_layer_path(
            DrawTag::Isolate,
            path,
            transform,
            FillRule::NonZero,
            tolerance,
        );
        self.push_layer_command(Layer::Isolate { draw });
    }

    pub fn push_opacity_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        opacity: f32,
    ) {
        let draw = self.push_layer_path(
            DrawTag::Opacity,
            path,
            transform,
            FillRule::NonZero,
            tolerance,
        );
        self.push_layer_command(Layer::Opacity { draw, opacity });
    }

    pub fn push_blend_layer(
        &mut self,
        path: BezPath,
        transform: Affine,
        tolerance: f64,
        mix: Mix,
        compose: Compose,
    ) {
        let draw = self.push_layer_path(
            DrawTag::Blend,
            path,
            transform,
            FillRule::NonZero,
            tolerance,
        );
        self.push_layer_command(Layer::Blend {
            draw,
            mode: BlendMode::new(mix, compose),
        });
    }

    /// Starts a masked group using `mask_scene` as the mask source.
    ///
    /// The mask source is rendered isolated, converted to either alpha or
    /// luminance coverage, clipped to `mask.region`, then applied to this
    /// layer's content before compositing through any outer clips.
    ///
    /// # Panics
    /// Panics if the mask and destination use different scale factors.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Recording consumes the temporary mask scene and releases its source storage after appending"
    )]
    pub fn push_mask_layer(&mut self, mask_scene: Self, mask: Mask) {
        assert!(
            (self.scale_factor() - mask_scene.scale_factor()).abs() <= f32::EPSILON,
            "cannot use a mask canvas with a different scale factor"
        );
        let mask_commands = self.append_scene_as_command_list(&mask_scene);
        self.push_mask_command(self.physical_mask(mask), mask_commands);
    }

    /// Adds an offscreen filter group sampled from `sample_region`.
    ///
    /// Filters derive their final output bounds from this region. Blur and
    /// drop-shadow expand it internally so their output is not clipped back to
    /// the original geometry.
    ///
    /// # Panics
    /// Panics if the filter contains a rectangular liquid-glass backdrop effect. Use `push_backdrop_layer` for that effect.
    pub fn push_filter_layer(&mut self, filter: Filter, sample_region: Region) {
        let filter = self.physical_filter(filter);
        let sample_region = self.physical_region(sample_region);
        assert!(
            !filter.contains_rect_liquid_glass(),
            "RectLiquidGlass is a rounded-rectangle backdrop effect; use push_backdrop_layer with Region::Rect"
        );
        self.push_layer_command(Layer::Filter {
            filter,
            sample_region,
        });
    }

    /// Adds a backdrop filter group sampled from the already-rendered target.
    ///
    /// The filter samples pixels behind this layer from `sample_region`, clips
    /// the filtered backdrop back to that region, then renders this layer's
    /// children normally on top.
    ///
    /// # Panics
    /// Panics if a rectangular liquid-glass effect is supplied with a nonrectangular region.
    pub fn push_backdrop_layer(&mut self, filter: Filter, sample_region: Region) {
        let filter = self.physical_filter(filter);
        let sample_region = self.physical_region(sample_region);
        if filter.contains_rect_liquid_glass() {
            assert!(
                matches!(sample_region, Region::Rect { .. }),
                "RectLiquidGlass requires Region::Rect because it uses rounded-rectangle SDF normals"
            );
        }
        self.push_layer_command(Layer::Backdrop {
            filter,
            sample_region,
        });
    }

    pub fn pop_layer(&mut self) -> Option<LayerKind> {
        let layer = self.layer_stack.pop()?;
        self.current_commands = layer.parent;
        Some(layer.kind)
    }
}
fn axis_aligned_rect_path(path: &BezPath, transform: Affine) -> Option<Rect> {
    let mut points = Vec::with_capacity(5);
    let mut closed = false;
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(point) if points.is_empty() => points.push(transform * point),
            PathEl::LineTo(point) if !closed => points.push(transform * point),
            PathEl::ClosePath if !closed => closed = true,
            _ => return None,
        }
    }
    if !closed || points.len() < 4 || points.len() > 5 {
        return None;
    }

    if points
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite())
    {
        return None;
    }
    // Only the four corners define the clip. A near-duplicate closing endpoint
    // contributes to the recognition tolerance, but must not expand the rectangle.
    let rect = Rect::from_points(points[0], points[1])
        .union_pt(points[2])
        .union_pt(points[3]);
    let extent = points.get(4).map_or(rect, |point| rect.union_pt(*point));
    // Base tolerance on shape extent, not world-space translation. A large
    // translation must not make a slightly rotated quadrilateral look axial.
    let epsilon = extent.width().max(extent.height()).max(1.0) * 1.0e-12;
    let same = |a: Point, b: Point| (a.x - b.x).abs() <= epsilon && (a.y - b.y).abs() <= epsilon;
    if points.len() == 5 && same(points[0], points[4]) {
        points.pop();
    }
    if points.len() != 4 {
        return None;
    }

    let Rect { x0, y0, x1, y1 } = rect;
    if x1 - x0 <= epsilon || y1 - y0 <= epsilon {
        return None;
    }

    let mut corners = 0u8;
    for (index, point) in points.iter().enumerate() {
        let x_side = if (point.x - x0).abs() <= epsilon {
            0
        } else if (point.x - x1).abs() <= epsilon {
            1
        } else {
            return None;
        };
        let y_side = if (point.y - y0).abs() <= epsilon {
            0
        } else if (point.y - y1).abs() <= epsilon {
            1
        } else {
            return None;
        };
        let corner = 1 << (y_side * 2 + x_side);
        if corners & corner != 0 {
            return None;
        }
        corners |= corner;

        let next = points[(index + 1) % points.len()];
        let horizontal = (point.y - next.y).abs() <= epsilon;
        let vertical = (point.x - next.x).abs() <= epsilon;
        if horizontal == vertical {
            return None;
        }
    }
    (corners == 0b1111).then(|| Rect::new(x0, y0, x1, y1))
}
