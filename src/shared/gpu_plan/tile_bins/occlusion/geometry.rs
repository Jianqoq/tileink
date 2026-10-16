use super::*;
use crate::shared::{
    execution::{ExecOp, LayerStackEntry},
    gpu_sdf::encoded_sdf,
    gpu_types::GPU_SDF_RECT,
};

pub(super) fn intersection(a: TileBbox, b: TileBbox) -> TileBbox {
    TileBbox {
        x0: a.x0.max(b.x0),
        y0: a.y0.max(b.y0),
        x1: a.x1.min(b.x1),
        y1: a.y1.min(b.y1),
    }
}

pub(super) fn batch_clips(canvas: &Canvas, plan: &ExecPlan, ops: &[ExecOp], batches: &mut [Batch]) {
    for op in ops {
        match op {
            ExecOp::DrawBatch {
                batch_id,
                layer_stack,
                ..
            } => {
                let Some(batch) = batches.get_mut(*batch_id as usize) else {
                    continue;
                };
                if !batch.has_occluder {
                    continue;
                }
                let mut clip = TileBbox {
                    x0: 0,
                    y0: 0,
                    x1: canvas.width_in_tiles(),
                    y1: canvas.height_in_tiles(),
                };
                for entry in &plan.layer_stack_data[layer_stack.clone()] {
                    if let LayerStackEntry::Clip { draw } = *entry {
                        // A clip masks each draw separately. An opaque brush cannot erase its
                        // predecessors at an AA edge; unknown/path clips keep the full stream.
                        clip = intersection(
                            clip,
                            rect_tiles(canvas, &canvas.draw_records[draw as usize]),
                        );
                    }
                }
                batch.clip = clip;
            }
            ExecOp::OffscreenLayer { children, .. } => batch_clips(canvas, plan, children, batches),
            ExecOp::OffscreenMaskLayer { content, mask, .. } => {
                batch_clips(canvas, plan, content, batches);
                batch_clips(canvas, plan, mask, batches);
            }
        }
    }
}

/// Match the native solid SDF rectangle route; arbitrary paths, shaders, images, shadows,
/// nontranslation transforms and AA edge tiles remain on the existing fine-stage proof.
pub(super) fn opaque_tiles(canvas: &Canvas, draw: &DrawRecord) -> TileBbox {
    if draw.tag() != DrawTag::Brush || draw.brush_len < 5 {
        return EMPTY;
    }
    let Some(brush) = canvas
        .brush_blob
        .get(draw.brush_offset as usize..)
        .filter(|brush| brush.len() >= 5)
    else {
        return EMPTY;
    };
    if brush[0] != GPU_BRUSH_SOLID || brush[4] >> 24 != 255 {
        return EMPTY;
    }
    rect_tiles(canvas, draw)
}

fn rect_tiles(canvas: &Canvas, draw: &DrawRecord) -> TileBbox {
    if draw.has_path()
        || draw.glyph_run_id().is_some()
        || draw.sdf_shadow_offset != DrawRecord::NONE
        || [
            draw.transform.a,
            draw.transform.b,
            draw.transform.c,
            draw.transform.d,
        ] != [1.0, 0.0, 0.0, 1.0]
    {
        return EMPTY;
    }
    let Some(sdf) = encoded_sdf(&canvas.sdf_blob, draw.sdf_offset, draw.sdf_len) else {
        return EMPTY;
    };
    if sdf.kind != GPU_SDF_RECT {
        return EMPTY;
    }
    let translation = [draw.transform.e, draw.transform.f];
    if sdf
        .coords
        .iter()
        .chain(&sdf.radii)
        .chain(&translation)
        .any(|v| !v.is_finite() || v.abs() > 65_536.0)
    {
        return EMPTY;
    }
    let [x0, y0, x1, y1] = sdf.coords.map(f64::from);
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let max_radius = ((x1 - x0).min(y1 - y0) * 0.5).max(0.0);
    let [tl, tr, bl, br] = sdf.radii.map(|v| f64::from(v).clamp(0.0, max_radius));
    // The central rectangle excludes rounded corners. Integer data is exact in the native
    // coverage proof's bounded float32 domain; fractional data reserves rounding error as well
    // as the half-pixel AA inset already implicit in whole-tile edge coordinates.
    let guard = if sdf
        .coords
        .iter()
        .chain(&sdf.radii)
        .chain(&translation)
        .all(|v| v.fract() == 0.0)
    {
        0.0
    } else {
        1.0 / 32.0
    };
    let tx = f64::from(translation[0]);
    let ty = f64::from(translation[1]);
    let tile = f64::from(crate::TILE_SIZE);
    let bbox = draw.tile_bbox(canvas.width_in_tiles(), canvas.height_in_tiles());
    TileBbox {
        x0: (((x0 + tl.max(bl) + tx + guard) / tile).ceil().max(0.0) as u32).max(bbox.x0),
        y0: (((y0 + tl.max(tr) + ty + guard) / tile).ceil().max(0.0) as u32).max(bbox.y0),
        x1: (((x1 - tr.max(br) + tx - guard) / tile).floor().max(0.0) as u32).min(bbox.x1),
        y1: (((y1 - bl.max(br) + ty - guard) / tile).floor().max(0.0) as u32).min(bbox.y1),
    }
}
