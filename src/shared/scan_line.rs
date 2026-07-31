use crate::{
    TILE_SCALE,
    shared::{bounds::TileBbox, line::Line},
};

pub(crate) struct ScanLinePlan {
    pub(crate) a: f32,
    pub(crate) b: f32,
    pub(crate) x0: f32,
    pub(crate) sign: f32,
    pub(crate) y0: f32,
    pub(crate) imin: u32,
    pub(crate) imax: u32,
}

pub(crate) fn line_scanned_tile_count(line: Line, bbox: TileBbox, _tiles_size: (u32, u32)) -> u32 {
    let Some(plan) = plan_scan_line(line, bbox) else {
        return 0;
    };
    let mut count = 0;
    for i in plan.imin..plan.imax {
        let z = (plan.a * i as f32 + plan.b).floor();
        let y = (plan.y0 + i as f32 - z) as i32;
        let x = (plan.x0 + plan.sign * z) as i32;
        count += u32::from(
            y >= bbox.y0 as i32 && y < bbox.y1 as i32 && x >= bbox.x0 as i32 && x < bbox.x1 as i32,
        );
    }
    count
}

fn plan_scan_line(line: Line, bbox: TileBbox) -> Option<ScanLinePlan> {
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
    if dy == 0.0 && s0.1.floor() == s0.1 {
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
    // Match Vello path_count: a segment whose upper endpoint only touches the
    // clipped top tile boundary is outside the scanned tile range. This epsilon
    // is intentionally much smaller than tile snapping epsilon so genuine
    // near-boundary geometry still contributes backdrop.
    if s0.1 >= bbox.y1 as f32
        || s1.1 <= bbox.y0 as f32 + TOP_TOUCH_EPSILON
        || xmin >= bbox.x1 as f32
    {
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

    if s0.0.max(s1.0) < bbox.x0 as f32 {
        imax = imin;
    } else {
        let fudge = if is_positive_slope { 0.0 } else { 1.0 };
        if xmin < bbox.x0 as f32 {
            let mut f = ((sign * (bbox.x0 as f32 - x0) - b + fudge) / a).round();
            if (x0 + sign * (a * f + b).floor() < bbox.x0 as f32) == is_positive_slope {
                f += 1.0;
            }
            if is_positive_slope {
                if f as u32 > imin {
                    imin = f as u32;
                }
            } else if (f as u32) < imax {
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

    Some(ScanLinePlan {
        a,
        b,
        x0,
        sign,
        y0,
        imin,
        imax,
    })
}

fn span(a: f32, b: f32) -> u32 {
    let hi = a.max(b).ceil();
    let lo = a.min(b).floor();
    (hi - lo).max(1.0) as u32
}

pub(crate) const TOP_TOUCH_EPSILON: f32 = 1.0e-12;
