use std::f32::consts::FRAC_1_SQRT_2;

use crate::shared::{euler::{CubicParams, EulerParams, EulerSeg, espc_int_approx, espc_int_inv_approx}, vec2::Vec2};

const DERIV_THRESH: f32 = 1e-6;
const DERIV_EPS: f32 = 1e-6;
const SUBDIV_LIMIT: f32 = 1.0 / 65536.0;

#[derive(Clone, Copy, Debug)]
pub struct CubicPoints {
    pub p0: Vec2,
    pub p1: Vec2,
    pub p2: Vec2,
    pub p3: Vec2,
}

impl CubicPoints {
    pub fn flatten_euler(
        cubic: CubicPoints,
        t_start: Vec2,
        t_end: Vec2,
        tol: f32,
        mut emit: impl FnMut(Vec2, Vec2),
    ) {
        let p0 = cubic.p0;
        let p1 = cubic.p1;
        let p2 = cubic.p2;
        let p3 = cubic.p3;
    
        if p0 == p1 && p0 == p2 && p0 == p3 {
            return;
        }
    
        let tol = tol.max(1.0e-6);
        let scale = 1.0_f32;
        let mut t0_u: u32 = 0;
        let mut dt: f32 = 1.;
        let mut last_p = p0;
        let mut last_q = p1 - p0;
        if last_q.length_squared() < DERIV_THRESH.powi(2) {
            last_q = eval_cubic_and_deriv(p0, p1, p2, p3, DERIV_EPS).1;
        }
        let mut last_t = 0.;
        let mut lp0 = t_start;
    
        loop {
            let t0 = (t0_u as f32) * dt;
            if t0 == 1. {
                break;
            }
            let mut t1 = t0 + dt;
            let this_p0 = last_p;
            let this_q0 = last_q;
            let (mut this_p1, mut this_q1) = eval_cubic_and_deriv(p0, p1, p2, p3, t1);
            if this_q1.length_squared() < DERIV_THRESH.powi(2) {
                let (new_p1, new_q1) = eval_cubic_and_deriv(p0, p1, p2, p3, t1 - DERIV_EPS);
                this_q1 = new_q1;
                if t1 < 1. {
                    this_p1 = new_p1;
                    t1 -= DERIV_EPS;
                }
            }
            let actual_dt = t1 - last_t;
            let cubic_params =
                CubicParams::from_points_derivs(this_p0, this_p1, this_q0, this_q1, actual_dt);
            if cubic_params.err * scale <= tol || dt <= SUBDIV_LIMIT {
                let euler_params = EulerParams::from_angles(cubic_params.th0, cubic_params.th1);
                let es = EulerSeg::from_params(this_p0, this_p1, euler_params);
    
                let (k0, k1) = (es.params.k0 - 0.5 * es.params.k1, es.params.k1);
                let normalized_offset = 0.0_f32;
                let dist_scaled = normalized_offset * es.params.ch;
                let scale_multiplier = 0.5
                    * FRAC_1_SQRT_2
                    * (scale * cubic_params.chord_len / (es.params.ch * tol)).sqrt();
                const K1_THRESH: f32 = 1e-3;
                const DIST_THRESH: f32 = 1e-3;
                let mut a = 0.0;
                let mut b = 0.0;
                let mut integral = 0.0;
                let mut int0 = 0.0;
                let (n_frac, robust) = if k1.abs() < K1_THRESH {
                    let k = k0 + 0.5 * k1;
                    let n_frac = (k * (k * dist_scaled + 1.0)).abs().sqrt();
                    (n_frac, EspcRobust::LowK1)
                } else if dist_scaled.abs() < DIST_THRESH {
                    let f = |x: f32| x * x.abs().sqrt();
                    a = k1;
                    b = k0;
                    int0 = f(b);
                    let int1 = f(a + b);
                    integral = int1 - int0;
                    let n_frac = (2. / 3.) * integral / a;
                    (n_frac, EspcRobust::LowDist)
                } else {
                    a = -2.0 * dist_scaled * k1;
                    b = -1.0 - 2.0 * dist_scaled * k0;
                    int0 = espc_int_approx(b);
                    let int1 = espc_int_approx(a + b);
                    integral = int1 - int0;
                    let k_peak = k0 - k1 * b / a;
                    let integrand_peak = (k_peak * (k_peak * dist_scaled + 1.0)).abs().sqrt();
                    let scaled_int = integral * integrand_peak / a;
                    (scaled_int, EspcRobust::Normal)
                };
                let n = (n_frac * scale_multiplier).ceil().clamp(1.0, 100.0);
    
                for i in 0..n as usize {
                    let lp1 = if i == n as usize - 1 && t1 == 1.0 {
                        t_end
                    } else {
                        let t = (i + 1) as f32 / n;
                        let s = match robust {
                            EspcRobust::LowK1 => t,
                            EspcRobust::LowDist => {
                                let c = (integral * t + int0).cbrt();
                                let inv = c * c.abs();
                                (inv - b) / a
                            }
                            EspcRobust::Normal => {
                                let inv = espc_int_inv_approx(integral * t + int0);
                                (inv - b) / a
                            }
                        };
                        es.eval_with_offset(s, normalized_offset)
                    };
                    emit(lp0, lp1);
                    lp0 = lp1;
                }
                last_p = this_p1;
                last_q = this_q1;
                last_t = t1;
                t0_u += 1;
                let shift = t0_u.trailing_zeros();
                t0_u >>= shift;
                dt *= (1 << shift) as f32;
            } else {
                t0_u = t0_u.saturating_mul(2);
                dt *= 0.5;
            }
        }
    }
}

fn eval_cubic_and_deriv(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> (Vec2, Vec2) {
    let m = 1.0 - t;
    let mm = m * m;
    let mt = m * t;
    let tt = t * t;
    let p = p0 * (mm * m) + (p1 * (3.0 * mm) + p2 * (3.0 * mt) + p3 * tt) * t;
    let q = (p1 - p0) * mm + (p2 - p1) * (2.0 * mt) + (p3 - p2) * tt;
    (p, q)
}

enum EspcRobust {
    Normal,
    LowK1,
    LowDist,
}