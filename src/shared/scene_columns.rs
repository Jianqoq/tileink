use crate::shared::{
    brush::Brush,
    draw_record::{DrawRecord, DrawTag},
    fill::FillRule,
    gpu_sdf::{encode_sdf, encode_sdf_shadow},
    gpu_types::{
        DRAW_FLAG_FILL_RULE_EVEN_ODD, DRAW_FLAG_HAS_GLYPH, DRAW_FLAG_HAS_SDF,
        DRAW_FLAG_SOLID_COLOR_FAST_PATH, DRAW_FLAG_SOLID_RECT, GPU_DRAW_BLEND, GPU_DRAW_BRUSH,
        GPU_DRAW_CLIP, GPU_DRAW_ISOLATE, GPU_DRAW_OPACITY, GPU_DRAW_PATH_GLYPH,
    },
    line::Line,
    path::PathRecord,
    pixel::premul_f32_to_u32,
    sdf::{Sdf, SdfShadow},
};

const INVALID_REF: u32 = u32::MAX;

/// GPU upload columns derived from semantic canvas records.
///
/// `Canvas` owns only the semantic record form. Wgpu rebuilds this staging data
/// before upload so there is no second long-lived copy to keep coherent.
#[derive(Clone, Default)]
pub(crate) struct CanvasColumns {
    pub(crate) line_path_ids: Vec<u32>,
    pub(crate) line_p0x: Vec<f32>,
    pub(crate) line_p0y: Vec<f32>,
    pub(crate) line_p1x: Vec<f32>,
    pub(crate) line_p1y: Vec<f32>,
    pub(crate) path_flags: Vec<u32>,
    pub(crate) draw_path_ids: Vec<u32>,
    pub(crate) draw_glyph_run_ids: Vec<u32>,
    pub(crate) draw_glyph_run_ids_without_text: Vec<u32>,
    pub(crate) draw_flags: Vec<u32>,
    pub(crate) draw_flags_without_text: Vec<u32>,
    pub(crate) draw_brush_colors: Vec<u32>,
    pub(crate) draw_pixel_x0: Vec<i32>,
    pub(crate) draw_pixel_y0: Vec<i32>,
    pub(crate) draw_pixel_x1: Vec<i32>,
    pub(crate) draw_pixel_y1: Vec<i32>,
    pub(crate) sdf: DrawSdfColumns,
}

impl CanvasColumns {
    pub(crate) fn clear(&mut self) {
        self.line_path_ids.clear();
        self.line_p0x.clear();
        self.line_p0y.clear();
        self.line_p1x.clear();
        self.line_p1y.clear();
        self.path_flags.clear();
        self.draw_path_ids.clear();
        self.draw_glyph_run_ids.clear();
        self.draw_glyph_run_ids_without_text.clear();
        self.draw_flags.clear();
        self.draw_flags_without_text.clear();
        self.draw_brush_colors.clear();
        self.draw_pixel_x0.clear();
        self.draw_pixel_y0.clear();
        self.draw_pixel_x1.clear();
        self.draw_pixel_y1.clear();
        self.sdf.clear();
    }

    pub(crate) fn rebuild(
        &mut self,
        lines: &[Line],
        paths: &[PathRecord],
        draws: &[DrawRecord],
        brushes: &[Brush],
        sdfs: &[Sdf],
        sdf_shadows: &[SdfShadow],
    ) {
        self.clear();
        let sdf_count = draws
            .iter()
            .filter(|draw| draw.has_analytic_geometry())
            .count();
        self.reserve(lines.len(), paths.len(), draws.len(), sdf_count);
        self.extend_lines(lines);
        self.extend_paths(paths);
        for draw in draws {
            self.push_draw(draw, brushes, sdfs, sdf_shadows);
        }
    }

    pub(crate) fn push_draw(
        &mut self,
        draw: &DrawRecord,
        brushes: &[Brush],
        sdfs: &[Sdf],
        sdf_shadows: &[SdfShadow],
    ) {
        self.draw_path_ids.push(draw.path_id.unwrap_or(INVALID_REF));
        self.draw_glyph_run_ids
            .push(draw.glyph_run_id.unwrap_or(INVALID_REF));
        self.draw_glyph_run_ids_without_text.push(INVALID_REF);
        let brush = brushes.get(draw.brush_id as usize);
        self.draw_flags.push(draw_flags_word(draw, brush, true));
        self.draw_flags_without_text
            .push(draw_flags_word(draw, brush, false));
        self.draw_brush_colors.push(solid_fast_color(brush));
        self.draw_pixel_x0.push(draw.pixel_bounds.x0);
        self.draw_pixel_y0.push(draw.pixel_bounds.y0);
        self.draw_pixel_x1.push(draw.pixel_bounds.x1);
        self.draw_pixel_y1.push(draw.pixel_bounds.y1);
        self.sdf.push_draw(draw, sdfs, sdf_shadows);
    }

    pub(crate) fn push_path_record(&mut self, path: PathRecord) {
        let index = path.path_id as usize;
        if index >= self.path_flags.len() {
            self.path_flags.resize(index + 1, 0);
        }
        self.path_flags[index] = path.flags;
    }

    pub(crate) fn extend_paths(&mut self, paths: &[PathRecord]) {
        for &path in paths {
            self.push_path_record(path);
        }
    }

    pub(crate) fn push_line(&mut self, line: Line) {
        self.line_path_ids.push(line.path_id);
        self.line_p0x.push(line.p0[0]);
        self.line_p0y.push(line.p0[1]);
        self.line_p1x.push(line.p1[0]);
        self.line_p1y.push(line.p1[1]);
    }

    pub(crate) fn extend_lines(&mut self, lines: &[Line]) {
        for &line in lines {
            self.push_line(line);
        }
    }

    fn reserve(
        &mut self,
        line_count: usize,
        path_count: usize,
        draw_count: usize,
        sdf_count: usize,
    ) {
        self.line_path_ids.reserve(line_count);
        self.line_p0x.reserve(line_count);
        self.line_p0y.reserve(line_count);
        self.line_p1x.reserve(line_count);
        self.line_p1y.reserve(line_count);
        self.path_flags.reserve(path_count);
        self.draw_path_ids.reserve(draw_count);
        self.draw_glyph_run_ids.reserve(draw_count);
        self.draw_glyph_run_ids_without_text.reserve(draw_count);
        self.draw_flags.reserve(draw_count);
        self.draw_flags_without_text.reserve(draw_count);
        self.draw_brush_colors.reserve(draw_count);
        self.draw_pixel_x0.reserve(draw_count);
        self.draw_pixel_y0.reserve(draw_count);
        self.draw_pixel_x1.reserve(draw_count);
        self.draw_pixel_y1.reserve(draw_count);
        self.sdf.reserve_for_draws(draw_count, sdf_count);
    }
}

#[derive(Clone, Default)]
pub(crate) struct DrawSdfColumns {
    pub(crate) refs: Vec<u32>,
    pub(crate) kinds: Vec<u32>,
    pub(crate) x0: Vec<f32>,
    pub(crate) y0: Vec<f32>,
    pub(crate) x1: Vec<f32>,
    pub(crate) y1: Vec<f32>,
    pub(crate) r0: Vec<f32>,
    pub(crate) r1: Vec<f32>,
    pub(crate) r2: Vec<f32>,
    pub(crate) r3: Vec<f32>,
    pub(crate) stroke_top: Vec<f32>,
    pub(crate) stroke_right: Vec<f32>,
    pub(crate) stroke_bottom: Vec<f32>,
    pub(crate) stroke_left: Vec<f32>,
    pub(crate) shadow_offset_x: Vec<f32>,
    pub(crate) shadow_offset_y: Vec<f32>,
    pub(crate) shadow_expand: Vec<f32>,
    pub(crate) shadow_intensity: Vec<f32>,
}

impl DrawSdfColumns {
    fn clear(&mut self) {
        self.refs.clear();
        self.kinds.clear();
        self.x0.clear();
        self.y0.clear();
        self.x1.clear();
        self.y1.clear();
        self.r0.clear();
        self.r1.clear();
        self.r2.clear();
        self.r3.clear();
        self.stroke_top.clear();
        self.stroke_right.clear();
        self.stroke_bottom.clear();
        self.stroke_left.clear();
        self.shadow_offset_x.clear();
        self.shadow_offset_y.clear();
        self.shadow_expand.clear();
        self.shadow_intensity.clear();
    }

    fn push_draw(&mut self, draw: &DrawRecord, sdfs: &[Sdf], sdf_shadows: &[SdfShadow]) {
        let sdf = match (draw.sdf_id, draw.sdf_shadow_id) {
            (Some(sdf_id), None) => sdfs.get(sdf_id as usize).copied().map(encode_sdf),
            (None, Some(sdf_shadow_id)) => sdf_shadows
                .get(sdf_shadow_id as usize)
                .copied()
                .map(encode_sdf_shadow),
            (None, None) => None,
            (Some(_), Some(_)) => unreachable!("draw cannot store both SDF and SDF shadow"),
        };
        if let Some(sdf) = sdf {
            self.refs.push(self.kinds.len() as u32);
            self.push(sdf.kind, sdf.coords, sdf.radii, sdf.stroke, sdf.shadow);
        } else {
            self.refs.push(INVALID_REF);
        }
    }

    fn reserve_for_draws(&mut self, draw_count: usize, sdf_count: usize) {
        self.refs.reserve(draw_count);
        self.kinds.reserve(sdf_count);
        self.x0.reserve(sdf_count);
        self.y0.reserve(sdf_count);
        self.x1.reserve(sdf_count);
        self.y1.reserve(sdf_count);
        self.r0.reserve(sdf_count);
        self.r1.reserve(sdf_count);
        self.r2.reserve(sdf_count);
        self.r3.reserve(sdf_count);
        self.stroke_top.reserve(sdf_count);
        self.stroke_right.reserve(sdf_count);
        self.stroke_bottom.reserve(sdf_count);
        self.stroke_left.reserve(sdf_count);
        self.shadow_offset_x.reserve(sdf_count);
        self.shadow_offset_y.reserve(sdf_count);
        self.shadow_expand.reserve(sdf_count);
        self.shadow_intensity.reserve(sdf_count);
    }

    fn push(
        &mut self,
        kind: u32,
        xy: [f32; 4],
        radii: [f32; 4],
        stroke_widths: [f32; 4],
        shadow: [f32; 4],
    ) {
        self.kinds.push(kind);
        self.x0.push(xy[0]);
        self.y0.push(xy[1]);
        self.x1.push(xy[2]);
        self.y1.push(xy[3]);
        self.r0.push(radii[0]);
        self.r1.push(radii[1]);
        self.r2.push(radii[2]);
        self.r3.push(radii[3]);
        self.stroke_top.push(stroke_widths[0]);
        self.stroke_right.push(stroke_widths[1]);
        self.stroke_bottom.push(stroke_widths[2]);
        self.stroke_left.push(stroke_widths[3]);
        self.shadow_offset_x.push(shadow[0]);
        self.shadow_offset_y.push(shadow[1]);
        self.shadow_expand.push(shadow[2]);
        self.shadow_intensity.push(shadow[3]);
    }
}

pub(crate) fn draw_flags_word(draw: &DrawRecord, brush: Option<&Brush>, text_enabled: bool) -> u32 {
    let mut flags = draw_tag_word(draw);
    if draw.fill_rule == FillRule::EvenOdd {
        flags |= DRAW_FLAG_FILL_RULE_EVEN_ODD;
    }
    if draw.solid_rect {
        flags |= DRAW_FLAG_SOLID_RECT;
        if brush.and_then(Brush::solid_color).is_some() {
            flags |= DRAW_FLAG_SOLID_COLOR_FAST_PATH;
        }
    }
    if draw.has_analytic_geometry() {
        flags |= DRAW_FLAG_HAS_SDF;
    }
    if text_enabled && draw.glyph_run_id.is_some() {
        flags |= DRAW_FLAG_HAS_GLYPH;
    }
    flags
}

fn draw_tag_word(draw: &DrawRecord) -> u32 {
    match draw.tag {
        DrawTag::Brush => GPU_DRAW_BRUSH,
        DrawTag::PathGlyph => GPU_DRAW_PATH_GLYPH,
        DrawTag::Clip => GPU_DRAW_CLIP,
        DrawTag::Isolate => GPU_DRAW_ISOLATE,
        DrawTag::Opacity => GPU_DRAW_OPACITY,
        DrawTag::Blend => GPU_DRAW_BLEND,
    }
}

fn solid_fast_color(brush: Option<&Brush>) -> u32 {
    brush
        .and_then(Brush::solid_color)
        .map(|color| premul_f32_to_u32(color.premultiply().components))
        .unwrap_or(0)
}
