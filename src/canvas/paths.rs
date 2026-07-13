use super::*;

impl Canvas {
    pub(super) fn rounded_rect_path(rect: Rect, radius: Radius, tolerance: f64) -> BezPath {
        if radius.is_zero() {
            rect.to_path(tolerance)
        } else {
            peniko::kurbo::RoundedRect::new(
                rect.x0,
                rect.y0,
                rect.x1,
                rect.y1,
                (
                    radius.top_left as f64,
                    radius.top_right as f64,
                    radius.bottom_right as f64,
                    radius.bottom_left as f64,
                ),
            )
            .to_path(tolerance)
        }
    }

    pub(super) fn transform_path(path: BezPath, transform: Affine) -> BezPath {
        if transform == Affine::IDENTITY {
            path
        } else {
            transform * path
        }
    }

    pub(super) fn pixel_bounds_for_transformed_path(path: &BezPath) -> PixelBounds {
        let rect = path.bounding_box();
        PixelBounds {
            x0: rect.x0.floor() as i32,
            y0: rect.y0.floor() as i32,
            x1: rect.x1.ceil() as i32,
            y1: rect.y1.ceil() as i32,
        }
    }

    pub(super) fn push_path_inner(
        &mut self,
        path: BezPath,
        brush: impl Into<Brush>,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        bounds_override: Option<Bounds>,
    ) -> usize {
        self.push_path_inner_with_tag(
            path,
            transform,
            rule,
            tolerance,
            PathPushOptions {
                bounds_override,
                brush: brush.into(),
                emit_draw_command: true,
                tag: DrawTag::Brush,
            },
        )
    }

    pub(super) fn push_path_inner_with_tag(
        &mut self,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
        options: PathPushOptions,
    ) -> usize {
        self.ensure_command_root();
        let line_start = self.lines.len() as u32;
        let path_id = self.path_cnt;
        self.path_cnt += 1;
        let path = Self::transform_path(path, self.device_transform() * transform);
        PathFlatten::new(&path, self.device_tolerance(tolerance) as f32, path_id)
            .flatten(&mut self.lines);
        let line_count = self.lines.len() as u32 - line_start;
        let pixel_bounds = match options.bounds_override {
            Some(bounds) => {
                let bounds = self.physical_bounds(bounds);
                PixelBounds {
                    x0: bounds.x0,
                    y0: bounds.y0,
                    x1: bounds.x1,
                    y1: bounds.y1,
                }
            }
            None => Self::pixel_bounds_for_transformed_path(&path),
        };
        let path_record = PathRecord {
            path_id,
            line_count,
            line_start,
            flags: 0,
            data_offset: 0,
            data_len: 0,
            tile_x0: 0,
            tile_y0: 0,
            tile_x1: 0,
            tile_y1: 0,
            segment_start: 0,
            segment_capacity: 0,
            segment_count: 0,
            transform: Default::default(),
        };
        let tile_bbox = pixel_bounds.tile_bbox(self.width_in_tiles(), self.height_in_tiles());
        let tile_stride = tile_bbox.tile_stride();
        let tile_height = tile_bbox.tile_height();
        let backdrop_len = tile_stride * tile_height;
        let local_tile_cnt =
            self.segment_capacity_for_path_lines(line_start, line_count, tile_bbox);

        let backdrop_offset = self.backdrop_pool_capacity;
        self.backdrop_pool_capacity += backdrop_len;
        let segment_start = self.tile_cnt;
        self.tile_cnt += local_tile_cnt;
        self.path_records.push(PathRecord {
            data_offset: backdrop_offset,
            data_len: backdrop_len,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity: local_tile_cnt,
            segment_count: 0,
            ..path_record
        });

        let (brush_offset, brush_len) = self.push_brush(options.brush);
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id,
            glyph_run_id: DrawRecord::NONE,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset,
            brush_len,
            tag: options.tag.into(),
            fill_rule: rule.into(),
            pixel_bounds,
            local_pixel_bounds: pixel_bounds,
            solid_rect: 0,
            transform: Default::default(),
            inverse_transform: Default::default(),
        });
        if options.emit_draw_command {
            self.current_command_list_mut()
                .commands
                .push(Command::Draw(draw_ix));
        }
        draw_ix
    }

    pub(super) fn push_layer_path(
        &mut self,
        tag: DrawTag,
        path: BezPath,
        transform: Affine,
        rule: FillRule,
        tolerance: f64,
    ) -> usize {
        self.push_path_inner_with_tag(
            path,
            transform,
            rule,
            tolerance,
            PathPushOptions {
                bounds_override: None,
                brush: Brush::Solid(Color::TRANSPARENT),
                emit_draw_command: false,
                tag,
            },
        )
    }

    pub(super) fn push_sdf_draw(&mut self, sdf: Sdf, brush: impl Into<Brush>) -> usize {
        self.push_sdf_record(sdf, brush.into(), DrawTag::Brush, true)
    }

    pub(super) fn push_sdf_shadow_draw(
        &mut self,
        sdf_shadow: SdfShadow,
        brush: impl Into<Brush>,
    ) -> usize {
        self.push_sdf_shadow_record(sdf_shadow, brush.into(), DrawTag::Brush, true)
    }

    pub(super) fn push_sdf_record(
        &mut self,
        sdf: Sdf,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.push_physical_sdf_record(
            self.physical_sdf(sdf),
            self.physical_brush(brush),
            tag,
            emit_draw_command,
        )
    }

    pub(super) fn push_physical_sdf_record(
        &mut self,
        sdf: Sdf,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.ensure_command_root();
        let bounds = sdf.bounds();
        let (sdf_offset, sdf_len) = self.push_sdf(sdf);
        let (brush_offset, brush_len) = self.push_physical_brush(brush);
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: DrawRecord::NONE,
            sdf_offset,
            sdf_len,
            sdf_shadow_offset: DrawRecord::NONE,
            sdf_shadow_len: 0,
            brush_offset,
            brush_len,
            tag: tag.into(),
            fill_rule: SDF_RECORD_FILL_RULE.into(),
            pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            local_pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            solid_rect: 0,
            transform: Default::default(),
            inverse_transform: Default::default(),
        });
        if emit_draw_command {
            self.current_command_list_mut()
                .commands
                .push(Command::Draw(draw_ix));
        }
        draw_ix
    }

    pub(super) fn push_sdf_shadow_record(
        &mut self,
        sdf_shadow: SdfShadow,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.push_physical_sdf_shadow_record(
            self.physical_sdf_shadow(sdf_shadow),
            self.physical_brush(brush),
            tag,
            emit_draw_command,
        )
    }

    pub(super) fn push_physical_sdf_shadow_record(
        &mut self,
        sdf_shadow: SdfShadow,
        brush: Brush,
        tag: DrawTag,
        emit_draw_command: bool,
    ) -> usize {
        self.ensure_command_root();
        let bounds = sdf_shadow.bounds();
        let (sdf_shadow_offset, sdf_shadow_len) = self.push_sdf_shadow(sdf_shadow);
        let (brush_offset, brush_len) = self.push_physical_brush(brush);
        let draw_ix = self.push_draw_record(DrawRecord {
            path_id: DrawRecord::NONE,
            glyph_run_id: DrawRecord::NONE,
            sdf_offset: DrawRecord::NONE,
            sdf_len: 0,
            sdf_shadow_offset,
            sdf_shadow_len,
            brush_offset,
            brush_len,
            tag: tag.into(),
            fill_rule: SDF_RECORD_FILL_RULE.into(),
            pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            local_pixel_bounds: PixelBounds {
                x0: bounds.x0,
                y0: bounds.y0,
                x1: bounds.x1,
                y1: bounds.y1,
            },
            solid_rect: 0,
            transform: Default::default(),
            inverse_transform: Default::default(),
        });
        if emit_draw_command {
            self.current_command_list_mut()
                .commands
                .push(Command::Draw(draw_ix));
        }
        draw_ix
    }
}
