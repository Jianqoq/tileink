//! Copy selected geometry and encoded payloads into compact offscreen storage.
use super::*;

#[allow(
    clippy::expect_used,
    reason = "Translated geometry copies encoder-owned records and each translated path has an owning draw"
)]
pub(super) fn translated_scene_for_draws(
    canvas: &Canvas,
    local: LocalSpace,
    draws: &[usize],
) -> (Canvas, HashMap<usize, usize>) {
    let mut translated = Canvas::new(local.surface.width(), local.surface.height(), 1.0);
    translated.scene_images.extend_from(&canvas.scene_images);
    let mut draw_map = HashMap::with_capacity(draws.len());
    let mut path_map = HashMap::<u32, u32>::new();
    let mut run_map = HashMap::<u32, u32>::new();
    for &old_draw in draws {
        let mut draw = canvas.draw_records[old_draw];
        draw.pixel_bounds = local.pixel_bounds(draw.pixel_bounds);
        // NONE denotes an intentionally absent payload. Present internal records
        // must decode; silently replacing corruption with transparent draws hides
        // the producer error and cannot preserve rendering semantics.
        if draw.brush_offset != DrawRecord::NONE {
            let brush = decode_encoded_brush(&canvas.brush_blob, draw.brush_offset, draw.brush_len)
                .expect("encoded offscreen brush");
            (draw.brush_offset, draw.brush_len) =
                push_encoded_brush(&mut translated.brush_blob, &local.brush(brush));
        }
        if draw.sdf_offset != DrawRecord::NONE {
            let sdf = decode_sdf(&canvas.sdf_blob, draw.sdf_offset, draw.sdf_len)
                .expect("encoded offscreen SDF");
            (draw.sdf_offset, draw.sdf_len) =
                push_encoded_sdf(&mut translated.sdf_blob, local.sdf(sdf));
        }
        if draw.sdf_shadow_offset != DrawRecord::NONE {
            let shadow = decode_sdf_shadow(
                &canvas.sdf_shadow_blob,
                draw.sdf_shadow_offset,
                draw.sdf_shadow_len,
            )
            .expect("encoded offscreen SDF shadow");
            (draw.sdf_shadow_offset, draw.sdf_shadow_len) =
                push_encoded_sdf_shadow(&mut translated.sdf_shadow_blob, local.sdf_shadow(shadow));
        }
        if let Some(old_path) = draw.path_id() {
            draw.path_id = *path_map.entry(old_path).or_insert_with(|| {
                let old = canvas.path_records[old_path as usize];
                let new = translated.path_records.len() as u32;
                let line_start = translated.lines.len() as u32;
                translated.lines.extend(
                    canvas.lines
                        [old.line_start as usize..(old.line_start + old.line_count) as usize]
                        .iter()
                        .copied()
                        .map(|line| {
                            let mut line = local.line(line);
                            line.path_id = new;
                            line
                        }),
                );
                translated.path_records.push(PathRecord {
                    path_id: new,
                    line_start,
                    ..old
                });
                new
            });
        }
        if let Some(old_run) = draw.glyph_run_id() {
            draw.glyph_run_id = *run_map.entry(old_run).or_insert_with(|| {
                let old = canvas.text_runs[old_run as usize];
                let new = translated.text_runs.len() as u32;
                let glyph_start = translated.text_glyphs.len() as u32;
                translated.text_glyphs.extend(
                    canvas.text_glyphs
                        [old.glyph_start as usize..(old.glyph_start + old.glyph_count) as usize]
                        .iter()
                        .copied()
                        .map(|glyph| {
                            glyph.translated(
                                -f64::from(local.surface.x0),
                                -f64::from(local.surface.y0),
                            )
                        }),
                );
                translated.text_runs.push(crate::text::TextRun {
                    glyph_start,
                    glyph_count: old.glyph_count,
                });
                new
            });
        }
        draw_map.insert(old_draw, translated.draw_records.len());
        translated.draw_records.push(draw);
    }
    rebuild_translated_path_backdrops(&mut translated);
    translated.path_cnt = translated.path_records.len() as u32;
    translated.backdrop_pool_capacity = translated
        .path_records
        .last()
        .map_or(0, |record| record.data_offset + record.data_len);
    translated.tile_cnt = translated
        .path_records
        .last()
        .map_or(0, |record| record.segment_start + record.segment_capacity);
    (translated, draw_map)
}

#[allow(
    clippy::expect_used,
    reason = "Translated geometry copies encoder-owned records and each translated path has an owning draw"
)]
fn rebuild_translated_path_backdrops(canvas: &mut Canvas) {
    let mut path_bounds: Vec<Option<PixelBounds>> = vec![None; canvas.path_records.len()];
    for draw in &canvas.draw_records {
        if let Some(path_id) = draw.path_id() {
            let slot = &mut path_bounds[path_id as usize];
            *slot = Some(match *slot {
                Some(bounds) => bounds.union(draw.pixel_bounds),
                None => draw.pixel_bounds,
            });
        }
    }

    let mut data_offset = 0;
    let mut segment_start = 0;
    let width_in_tiles = canvas.width_in_tiles();
    let height_in_tiles = canvas.height_in_tiles();
    for (path_id, bounds) in path_bounds.into_iter().enumerate() {
        // Translated paths are created only while copying their owning draws.
        let pixel_bounds = bounds.expect("translated path has an owning draw");
        let tile_bbox = pixel_bounds.tile_bbox(width_in_tiles, height_in_tiles);
        let data_len = tile_bbox.tile_count();
        let segment_capacity = translated_path_segment_capacity(canvas, path_id, tile_bbox);
        let record = &mut canvas.path_records[path_id];
        *record = PathRecord {
            data_offset,
            data_len,
            tile_x0: tile_bbox.x0,
            tile_y0: tile_bbox.y0,
            tile_x1: tile_bbox.x1,
            tile_y1: tile_bbox.y1,
            segment_start,
            segment_capacity,
            segment_count: 0,
            ..*record
        };
        data_offset += data_len;
        segment_start += segment_capacity;
    }
}

fn translated_path_segment_capacity(canvas: &Canvas, path_id: usize, tile_bbox: TileBbox) -> u32 {
    let record = &canvas.path_records[path_id];
    let lines =
        &canvas.lines[record.line_start as usize..(record.line_start + record.line_count) as usize];
    lines
        .iter()
        .map(|line| crate::shared::scan_line::line_scanned_tile_count(*line, tile_bbox))
        .sum()
}
