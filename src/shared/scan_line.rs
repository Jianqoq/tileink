use crate::{
    TILE_SCALE,
    shared::{bounds::TileBbox, line::Line},
};

pub(crate) struct ScanLinePlan {
    pub(crate) xy0: [f32; 2],
    pub(crate) xy1: [f32; 2],
    pub(crate) is_down: bool,
    pub(crate) a: f32,
    pub(crate) b: f32,
    pub(crate) x0: f32,
    pub(crate) sign: f32,
    pub(crate) y0: f32,
    pub(crate) delta: i32,
    pub(crate) imin: u32,
    pub(crate) imax: u32,
    pub(crate) ymin: i32,
    pub(crate) ymax: i32,
    pub(crate) top_clip_bump_x: Option<i32>,
    pub(crate) keep_horizontal_tile_edges: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct ScannedTile {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) global_ix: u32,
    pub(crate) top_edge: bool,
    pub(crate) initial_top_edge: bool,
}

pub(crate) fn for_each_scanned_tile(
    plan: &ScanLinePlan,
    bbox: TileBbox,
    tiles_size: (u32, u32),
    mut f: impl FnMut(ScannedTile),
) {
    let mut last_z = (plan.a * (plan.imin as f32 - 1.0) + plan.b).floor();
    for i in plan.imin..plan.imax {
        let z = (plan.a * i as f32 + plan.b).floor();
        let y = (plan.y0 + i as f32 - z) as i32;
        let x = (plan.x0 + plan.sign * z) as i32;
        if y < bbox.y0 as i32 || y >= bbox.y1 as i32 || x < bbox.x0 as i32 || x >= bbox.x1 as i32 {
            last_z = z;
            continue;
        }

        let initial_top_edge = i == plan.imin
            && plan.imin == 0
            && (plan.y0 - plan.xy0[1] * TILE_SCALE).abs() <= SCAN_EPSILON;
        let top_edge = if i == plan.imin {
            initial_top_edge
        } else {
            last_z == z
        };
        f(ScannedTile {
            x,
            y,
            global_ix: y as u32 * tiles_size.0 + x as u32,
            top_edge,
            initial_top_edge,
        });
        last_z = z;
    }
}

pub(crate) fn line_scanned_tile_count(
    line: Line,
    bbox: TileBbox,
    tiles_size: (u32, u32),
    keep_horizontal_tile_edges: bool,
) -> u32 {
    let Some(plan) = plan_scan_line(line, bbox, keep_horizontal_tile_edges) else {
        return 0;
    };
    let mut count = 0;
    for_each_scanned_tile(&plan, bbox, tiles_size, |_| count += 1);
    count
}

pub(crate) fn plan_scan_line(
    line: Line,
    bbox: TileBbox,
    keep_horizontal_tile_edges: bool,
) -> Option<ScanLinePlan> {
    let p0 = line.p0;
    let p1 = line.p1;
    let is_down = p1[1] >= p0[1];
    let (xy0, xy1) = if is_down { (p0, p1) } else { (p1, p0) };

    let s0 = (xy0[0] * TILE_SCALE, xy0[1] * TILE_SCALE);
    let s1 = (xy1[0] * TILE_SCALE, xy1[1] * TILE_SCALE);
    let count_x = span(s0.0, s1.0) - 1;
    let count = count_x + span(s0.1, s1.1);

    let dx = (s1.0 - s0.0).abs();
    let dy = s1.1 - s0.1;
    if dx + dy == 0.0 {
        return None;
    }
    // Filled paths normally ignore horizontal edges exactly on tile boundaries.
    // Stroke-generated thin horizontal outlines opt in to keeping them so the
    // paired opposite edge does not drive backdrop fill across complete tiles.
    if dy == 0.0 && s0.1.floor() == s0.1 && !keep_horizontal_tile_edges {
        return None;
    }

    let idxdy = 1.0 / (dx + dy);
    let mut a = dx * idxdy;
    let is_positive_slope = s1.0 >= s0.0;
    let sign = if is_positive_slope { 1.0 } else { -1.0 };
    let xt0 = (s0.0 * sign).floor();
    let c = s0.0 * sign - xt0;
    let y0 = s0.1.floor();
    let ytop = if s0.1 == s1.1 { s0.1.ceil() } else { y0 + 1.0 };
    let b = ((dy * c + dx * (ytop - s0.1)) * idxdy).min(0.999_999_94);
    let robust_err = (a * (count as f32 - 1.0) + b).floor() - count_x as f32;
    if robust_err != 0.0 {
        a -= 2e-7_f32.copysign(robust_err);
    }
    let x0 = xt0 * sign + if is_positive_slope { 0.0 } else { -1.0 };

    let xmin = s0.0.min(s1.0);
    if s0.1 >= bbox.y1 as f32 || s1.1 < bbox.y0 as f32 || xmin >= bbox.x1 as f32 {
        return None;
    }

    let mut imin = 0u32;
    if s0.1 < bbox.y0 as f32 {
        let mut iminf = ((bbox.y0 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + iminf - (a * iminf + b).floor() < bbox.y0 as f32 {
            iminf += 1.0;
        }
        imin = iminf as u32;
    }
    let mut imax = count;
    if s1.1 > bbox.y1 as f32 {
        let mut imaxf = ((bbox.y1 as f32 - y0 + b - a) / (1.0 - a)).round() - 1.0;
        if y0 + imaxf - (a * imaxf + b).floor() < bbox.y1 as f32 {
            imaxf += 1.0;
        }
        imax = imaxf as u32;
    }

    let delta = if is_down { -1 } else { 1 };
    let mut ymin = 0i32;
    let mut ymax = 0i32;
    if s0.0.max(s1.0) <= bbox.x0 as f32 {
        ymin = s0.1.ceil() as i32;
        ymax = s1.1.ceil() as i32;
        imax = imin;
    } else {
        let fudge = if is_positive_slope { 0.0 } else { 1.0 };
        if xmin < bbox.x0 as f32 {
            let mut f = ((sign * (bbox.x0 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox.x0 as f32) == is_positive_slope {
                f += 1.0;
            }
            let ynext = (y0 + f - (a * f + b).floor() + 1.0) as i32;
            if is_positive_slope {
                if f as u32 > imin {
                    ymin = (y0 + if y0 == s0.1 { 0.0 } else { 1.0 }) as i32;
                    ymax = ynext;
                    imin = f as u32;
                }
            } else if (f as u32) < imax {
                ymin = ynext;
                ymax = s1.1.ceil() as i32;
                imax = f as u32;
            }
        }
        if s0.0.max(s1.0) > bbox.x1 as f32 {
            let mut f = ((sign * (bbox.x1 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox.x1 as f32) == is_positive_slope {
                f += 1.0;
            }
            if is_positive_slope {
                imax = imax.min(f as u32);
            } else {
                imin = imin.max(f as u32);
            }
        }
    }
    imax = imin.max(imax);
    ymin = ymin.max(bbox.y0 as i32);
    ymax = ymax.min(bbox.y1 as i32);
    if ymin == bbox.y0 as i32 && ymax > ymin && is_top_left_corner_clip(s0, s1, bbox) {
        // The top-left clip corner is owned by the top edge. Keep the first
        // row as segment coverage so a stroke cap cannot become full-tile fill.
        ymin += 1;
    }
    let top_clip_bump_x = top_clip_backdrop_bump_x(s0, s1, bbox);

    Some(ScanLinePlan {
        xy0,
        xy1,
        is_down,
        a,
        b,
        x0,
        sign,
        y0,
        delta,
        imin,
        imax,
        ymin,
        ymax,
        top_clip_bump_x,
        keep_horizontal_tile_edges,
    })
}

fn top_clip_backdrop_bump_x(s0: (f32, f32), s1: (f32, f32), bbox: TileBbox) -> Option<i32> {
    let top_y = bbox.y0 as f32;
    if s0.1 >= top_y - SCAN_EPSILON || s1.1 <= top_y + SCAN_EPSILON {
        return None;
    }

    let top_x = s0.0 + (s1.0 - s0.0) * ((top_y - s0.1) / (s1.1 - s0.1));
    if top_x < bbox.x0 as f32 - SCAN_EPSILON || top_x >= bbox.x1 as f32 {
        return None;
    }

    // A line clipped by the backdrop's top boundary did not have an original
    // DDA top-edge event. The clipped boundary contributes only to tiles whose
    // left edge is at or to the right of the crossing; exact tile-boundary
    // crossings therefore stay on that boundary instead of advancing one tile.
    if top_x - bbox.x0 as f32 <= SCAN_EPSILON {
        Some(bbox.x0 as i32 + 1)
    } else {
        Some((top_x - SCAN_EPSILON).ceil() as i32)
    }
}

fn is_top_left_corner_clip(s0: (f32, f32), s1: (f32, f32), bbox: TileBbox) -> bool {
    if s0.1 >= bbox.y0 as f32 || s1.1 <= bbox.y0 as f32 || s0.0 == s1.0 {
        return false;
    }

    let left_x = bbox.x0 as f32;
    let top_y = bbox.y0 as f32;
    let top_x = s0.0 + (s1.0 - s0.0) * ((top_y - s0.1) / (s1.1 - s0.1));
    let left_y = s0.1 + (s1.1 - s0.1) * ((left_x - s0.0) / (s1.0 - s0.0));

    (top_x - left_x).abs() <= SCAN_EPSILON && (left_y - top_y).abs() <= SCAN_EPSILON
}

fn span(a: f32, b: f32) -> u32 {
    let hi = a.max(b).ceil();
    let lo = a.min(b).floor();
    (hi - lo).max(1.0) as u32
}

// Shared by DDA top-edge detection, top-clipped backdrop bumps, and tile-boundary
// segment snapping. This only absorbs arithmetic noise around an exact tile
// boundary; wider tolerances can create false backdrop carry for nearby geometry.
pub(crate) const SCAN_EPSILON: f32 = 1.0e-6;
