use super::*;

impl Canvas {
    /// Appends `other` with its local canvas origin placed at `pos`.
    ///
    /// Append translates the child canvas's geometry, brushes, and layer/filter
    /// regions into parent coordinates, then inserts its root commands into the
    /// current command list. It deliberately does not add a child-canvas clip;
    /// callers that need clipping can open a clip layer around the append.
    /// The borrowed child canvas is not mutated and remains reusable.
    pub fn append(&mut self, other: &Canvas, pos: impl Into<Point>) {
        self.prepare_append(other);
        self.append_translation_prepared(other, pos.into());
    }

    fn append_translation_prepared(&mut self, other: &Canvas, pos: Point) {
        let offset = SceneOffset::new(self.physical_point(pos));
        if offset.is_zero() || other.native_translation_is_safe() {
            self.append_scene_ref_unchecked(other, SceneAppendMode::MergeCurrent, offset);
        } else {
            // A nested scale/rotation makes local-space geometry translation non-commutative.
            // Compose the parent translation after that transform instead of scaling the offset.
            self.append_transformed_prepared(other, Affine::translate((pos.x, pos.y)));
        }
    }

    /// Appends a reusable child with a logical-space affine transform.
    ///
    /// Geometry and paint remain in the child's local buffers. The scan and fine shaders apply
    /// the transform, so rotation, scale, and shear do not require a CPU pass over path lines.
    pub fn append_transformed(&mut self, other: &Canvas, transform: Affine) {
        let coefficients = transform.as_coeffs();
        assert!(
            coefficients.iter().all(|value| value.is_finite())
                && (coefficients[0] * coefficients[3] - coefficients[1] * coefficients[2]).abs()
                    > f64::EPSILON,
            "canvas append transform must be finite and invertible"
        );
        self.prepare_append(other);
        let [a, b, c, d, x, y] = coefficients;
        if [a, b, c, d] == [1.0, 0.0, 0.0, 1.0] {
            self.append_translation_prepared(other, Point::new(x, y));
            return;
        }
        self.append_transformed_prepared(other, transform);
    }

    fn prepare_append(&mut self, other: &Canvas) {
        self.ensure_command_root();
        assert!(
            other.command_stack.len() == 1 && other.layer_stack.is_empty(),
            "cannot append a canvas with unclosed layers"
        );
        assert!(
            (self.scale_factor - other.scale_factor).abs() <= f32::EPSILON,
            "cannot append canvases with different scale factors"
        );
    }

    fn native_translation_is_safe(&self) -> bool {
        fn has_identity_linear_part(transform: GpuAffine) -> bool {
            [transform.a, transform.b, transform.c, transform.d] == [1.0, 0.0, 0.0, 1.0]
        }

        has_identity_linear_part(self.retained_transform)
            && self
                .path_records
                .iter()
                .all(|record| has_identity_linear_part(record.transform))
            && self
                .draw_records
                .iter()
                .all(|record| has_identity_linear_part(record.transform))
    }

    fn append_transformed_prepared(&mut self, other: &Canvas, transform: Affine) {
        let mut transformed =
            Canvas::new(self.logical_width, self.logical_height, self.scale_factor);
        // Zero-offset copying is valid for every nested transform and avoids recursively routing
        // an already transformed child back through public `append`.
        transformed.append_scene_ref_unchecked(
            other,
            SceneAppendMode::MergeCurrent,
            SceneOffset::new(Point::ZERO),
        );
        transformed.set_retained_transform(transform);
        self.append_scene_ref_unchecked(
            &transformed,
            SceneAppendMode::MergeCurrent,
            SceneOffset::new(Point::ZERO),
        );
    }

    pub(super) fn append_scene_ref_unchecked(
        &mut self,
        other: &Canvas,
        mode: SceneAppendMode,
        offset: SceneOffset,
    ) -> Option<CommandListId> {
        self.append_scene_ref_to_list_unchecked(other, self.current_command_list_id(), mode, offset)
    }

    pub(super) fn append_scene_ref_to_list_unchecked(
        &mut self,
        other: &Canvas,
        target_commands: CommandListId,
        mode: SceneAppendMode,
        offset: SceneOffset,
    ) -> Option<CommandListId> {
        let draw_offset = self.append_scene_data(other, offset);
        let command_list_offset = self.command_lists.len();
        let root_commands = other.root_commands;
        match mode {
            SceneAppendMode::MergeCurrent => {
                let child_list_offset = command_list_offset.saturating_sub(1);
                for (list_ix, list) in other.command_lists.iter().enumerate() {
                    if list_ix == root_commands {
                        continue;
                    }
                    self.command_lists
                        .push(Self::translated_remapped_command_list(
                            list,
                            draw_offset,
                            child_list_offset,
                            offset,
                        ));
                }

                self.command_lists[target_commands]
                    .commands
                    .reserve(other.command_lists[root_commands].commands.len());
                for command in &other.command_lists[root_commands].commands {
                    let command = Self::translated_remapped_command(
                        command,
                        draw_offset,
                        child_list_offset,
                        offset,
                    );
                    self.command_lists[target_commands].commands.push(command);
                }
                None
            }
            SceneAppendMode::AppendAsCommandList => {
                for list in &other.command_lists {
                    self.command_lists
                        .push(Self::translated_remapped_command_list(
                            list,
                            draw_offset,
                            command_list_offset,
                            offset,
                        ));
                }
                Some(command_list_offset + root_commands)
            }
        }
    }

    pub(super) fn translate_draw_for_append(draw: &mut DrawRecord, offset: SceneOffset) {
        if !draw.has_analytic_geometry() {
            draw.pixel_bounds = offset.pixel_bounds(draw.pixel_bounds);
        }
    }

    pub(super) fn pixel_bounds_from_bounds(bounds: Bounds) -> PixelBounds {
        PixelBounds {
            x0: bounds.x0,
            y0: bounds.y0,
            x1: bounds.x1,
            y1: bounds.y1,
        }
    }

    pub(super) fn path_pixel_bounds(&self, path_ix: usize) -> PixelBounds {
        let Some(record) = self.path_records.get(path_ix) else {
            return PixelBounds {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            };
        };
        let lines = &self.lines
            [record.line_start as usize..(record.line_start + record.line_count) as usize];
        if lines.is_empty() {
            return PixelBounds {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            };
        }

        let mut x0 = f32::INFINITY;
        let mut y0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        for line in lines {
            x0 = x0.min(line.p0[0]).min(line.p1[0]);
            y0 = y0.min(line.p0[1]).min(line.p1[1]);
            x1 = x1.max(line.p0[0]).max(line.p1[0]);
            y1 = y1.max(line.p0[1]).max(line.p1[1]);
        }
        PixelBounds {
            x0: x0.floor() as i32,
            y0: y0.floor() as i32,
            x1: x1.ceil() as i32,
            y1: y1.ceil() as i32,
        }
    }

    pub(super) fn segment_capacity_for_path_record(
        &self,
        record: &PathRecord,
        tile_bbox: crate::shared::bounds::TileBbox,
        width_in_tiles: u32,
        height_in_tiles: u32,
    ) -> u32 {
        if record.transform != GpuAffine::IDENTITY {
            // A transformed line can visit at most every row and column in its clipped path box.
            // This conservative O(1)-per-path bound lets GPU scan compute exact counts without a
            // CPU pass over local lines when a retained transform changes.
            return record.line_count.saturating_mul(
                tile_bbox
                    .tile_stride()
                    .saturating_add(tile_bbox.tile_height()),
            );
        }
        self.lines[record.line_start as usize..(record.line_start + record.line_count) as usize]
            .iter()
            .fold(0u32, |capacity, &line| {
                capacity.saturating_add(line_scanned_tile_count(
                    line,
                    tile_bbox,
                    (width_in_tiles, height_in_tiles),
                ))
            })
    }

    pub(super) fn append_scene_data(&mut self, other: &Canvas, offset: SceneOffset) -> usize {
        let line_offset = self.lines.len() as u32;
        let path_offset = self.path_cnt;
        let draw_offset = self.draw_records.len();
        let glyph_offset = self.text_glyphs.len() as u32;
        let text_run_offset = self.text_runs.len() as u32;
        let path_record_start = self.path_records.len();
        let draw_start = self.draw_records.len();

        self.scene_images.extend_from(&other.scene_images);

        self.lines.reserve(other.lines.len());
        for &line in &other.lines {
            let mut line = line;
            line.path_id = line.path_id.saturating_add(path_offset);
            if !offset.is_zero() {
                offset.line(&mut line);
            }
            self.lines.push(line);
        }

        self.path_records.reserve(other.path_records.len());
        for &record in &other.path_records {
            let mut record = record;
            record.path_id = record.path_id.saturating_add(path_offset);
            record.line_start = record.line_start.saturating_add(line_offset);
            record.data_offset = 0;
            record.data_len = 0;
            record.tile_x0 = 0;
            record.tile_y0 = 0;
            record.tile_x1 = 0;
            record.tile_y1 = 0;
            record.segment_start = 0;
            record.segment_capacity = 0;
            record.segment_count = 0;
            self.path_records.push(record);
        }

        self.draw_records.reserve(other.draw_records.len());
        for draw in &other.draw_records {
            let mut draw = *draw;
            if draw.path_id != DrawRecord::NONE {
                draw.path_id = draw.path_id.saturating_add(path_offset);
            }
            if draw.glyph_run_id != DrawRecord::NONE {
                draw.glyph_run_id = draw.glyph_run_id.saturating_add(text_run_offset);
            }
            if let Some(brush) = other.draw_brush_for_record(&draw) {
                let brush = if offset.is_zero() {
                    brush
                } else {
                    offset.brush(brush)
                };
                (draw.brush_offset, draw.brush_len) = self.push_physical_brush(brush);
            } else {
                draw.brush_offset = DrawRecord::NONE;
                draw.brush_len = 0;
            }
            if let Some(sdf) = other.draw_sdf(&draw) {
                let sdf = if offset.is_zero() {
                    sdf
                } else {
                    offset.sdf(sdf)
                };
                (draw.sdf_offset, draw.sdf_len) = self.push_sdf(sdf);
                draw.sdf_shadow_offset = DrawRecord::NONE;
                draw.sdf_shadow_len = 0;
                draw.local_pixel_bounds = Self::pixel_bounds_from_bounds(sdf.bounds());
                draw.pixel_bounds = draw.transform.transform_bounds(draw.local_pixel_bounds);
            } else if let Some(sdf_shadow) = other.draw_sdf_shadow(&draw) {
                let sdf_shadow = if offset.is_zero() {
                    sdf_shadow
                } else {
                    offset.sdf_shadow(sdf_shadow)
                };
                (draw.sdf_shadow_offset, draw.sdf_shadow_len) = self.push_sdf_shadow(sdf_shadow);
                draw.sdf_offset = DrawRecord::NONE;
                draw.sdf_len = 0;
                draw.local_pixel_bounds = Self::pixel_bounds_from_bounds(sdf_shadow.bounds());
                draw.pixel_bounds = draw.transform.transform_bounds(draw.local_pixel_bounds);
            } else {
                draw.sdf_offset = DrawRecord::NONE;
                draw.sdf_len = 0;
                draw.sdf_shadow_offset = DrawRecord::NONE;
                draw.sdf_shadow_len = 0;
            }
            if !offset.is_zero() {
                Self::translate_draw_for_append(&mut draw, offset);
            }
            self.draw_records.push(draw);
        }

        self.text_glyphs.reserve(other.text_glyphs.len());
        for &glyph in &other.text_glyphs {
            let glyph = if offset.is_zero() {
                glyph
            } else {
                glyph.translated(offset.dx, offset.dy)
            };
            self.text_glyphs.push(glyph);
        }

        self.text_runs.reserve(other.text_runs.len());
        for &run in &other.text_runs {
            let run = TextRun {
                glyph_start: run.glyph_start.saturating_add(glyph_offset),
                glyph_count: run.glyph_count,
            };
            self.text_runs.push(run);
        }

        self.path_cnt = self.path_cnt.saturating_add(other.path_cnt);
        self.append_path_backdrops_for_paths(
            path_record_start,
            other.path_records.len(),
            draw_start,
            other.draw_records.len(),
        );

        draw_offset
    }

    pub(super) fn append_path_backdrops_for_paths(
        &mut self,
        path_record_start: usize,
        path_record_count: usize,
        draw_start: usize,
        draw_count: usize,
    ) {
        if path_record_count == 0 {
            return;
        }

        let width_in_tiles = self.width_in_tiles();
        let height_in_tiles = self.height_in_tiles();
        let mut data_offset = self.backdrop_pool_capacity;
        let mut segment_start = self.tile_cnt;
        let path_record_end = path_record_start + path_record_count;
        let draw_end = draw_start + draw_count;

        for path_ix in path_record_start..path_record_end {
            let record = self.path_records[path_ix];
            let pixel_bounds = self.draw_records[draw_start..draw_end]
                .iter()
                .filter(|draw| draw.path_id == record.path_id)
                .map(|draw| draw.pixel_bounds)
                .reduce(PixelBounds::union)
                .unwrap_or_else(|| self.path_pixel_bounds(path_ix));
            let tile_bbox = pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles);
            let data_len = tile_bbox.tile_count();
            let segment_capacity = self.segment_capacity_for_path_record(
                &record,
                tile_bbox,
                width_in_tiles,
                height_in_tiles,
            );
            let record = &mut self.path_records[path_ix];
            record.data_offset = data_offset;
            record.data_len = data_len;
            record.tile_x0 = tile_bbox.x0;
            record.tile_y0 = tile_bbox.y0;
            record.tile_x1 = tile_bbox.x1;
            record.tile_y1 = tile_bbox.y1;
            record.segment_start = segment_start;
            record.segment_capacity = segment_capacity;
            record.segment_count = 0;
            data_offset = data_offset.saturating_add(data_len);
            segment_start = segment_start.saturating_add(segment_capacity);
        }

        self.backdrop_pool_capacity = data_offset;
        self.tile_cnt = segment_start;
    }

    pub(crate) fn remap_command(
        command: Command,
        draw_offset: usize,
        child_list_offset: usize,
    ) -> Command {
        match command {
            Command::Draw(draw_ix) => Command::Draw(draw_ix + draw_offset),
            Command::MaterializedRetainedScene {
                id,
                revision,
                children,
            } => Command::MaterializedRetainedScene {
                id,
                revision,
                children: children + child_list_offset,
            },
            Command::Layer {
                retained,
                draw,
                layer,
                children,
            } => Command::Layer {
                retained,
                draw: draw + draw_offset,
                layer,
                children: children + child_list_offset,
            },
            Command::MaskLayer {
                retained,
                layer,
                content,
                mask,
            } => Command::MaskLayer {
                retained,
                layer,
                content: content + child_list_offset,
                mask: mask + child_list_offset,
            },
        }
    }

    pub(super) fn translated_remapped_command_list(
        list: &CommandList,
        draw_offset: usize,
        child_list_offset: usize,
        offset: SceneOffset,
    ) -> CommandList {
        CommandList {
            commands: list
                .commands
                .iter()
                .map(|command| {
                    Self::translated_remapped_command(
                        command,
                        draw_offset,
                        child_list_offset,
                        offset,
                    )
                })
                .collect(),
        }
    }

    pub(super) fn translated_remapped_command(
        command: &Command,
        draw_offset: usize,
        child_list_offset: usize,
        offset: SceneOffset,
    ) -> Command {
        let mut command = command.clone();
        if !offset.is_zero() {
            offset.command(&mut command);
        }
        Self::remap_command(command, draw_offset, child_list_offset)
    }

    pub(super) fn append_scene_as_command_list(&mut self, other: &Canvas) -> CommandListId {
        self.append_scene_ref_unchecked(
            other,
            SceneAppendMode::AppendAsCommandList,
            SceneOffset::new(Point::new(0.0, 0.0)),
        )
        .expect("append mode returns a command list id")
    }
}
