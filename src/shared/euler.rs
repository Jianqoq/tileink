// Copyright 2023 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT OR Unlicense
//
// Ported from `vello_shaders/src/cpu/euler.rs`.

//! Euler spiral fitting for Bézier flattening.

use std::f32::consts::FRAC_PI_4;

use crate::shared::vec2::Vec2;

pub(crate) const TANGENT_THRESH: f32 = 1e-6;

#[derive(Debug)]
pub(crate) struct CubicParams {
    pub(crate) th0: f32,
    pub(crate) th1: f32,
    pub(crate) chord_len: f32,
    pub(crate) err: f32,
}

#[derive(Debug)]
pub(crate) struct EulerParams {
    pub(crate) th0: f32,
    pub(crate) k0: f32,
    pub(crate) k1: f32,
    pub(crate) ch: f32,
}

#[derive(Debug)]
pub(crate) struct EulerSeg {
    pub(crate) p0: Vec2,
    pub(crate) p1: Vec2,
    pub(crate) params: EulerParams,
}

impl CubicParams {
    pub(crate) fn from_points_derivs(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2, dt: f32) -> Self {
        let chord = p1 - p0;
        let chord_squared = chord.length_squared();
        let chord_len = chord_squared.sqrt();
        if chord_squared < TANGENT_THRESH.powi(2) {
            let chord_err = ((9. / 32.0) * (q0.length_squared() + q1.length_squared())).sqrt() * dt;
            return Self {
                th0: 0.0,
                th1: 0.0,
                chord_len: TANGENT_THRESH,
                err: chord_err,
            };
        }
        let scale = dt / chord_squared;
        let h0 = Vec2::new(
            q0.x * chord.x + q0.y * chord.y,
            q0.y * chord.x - q0.x * chord.y,
        );
        let th0 = h0.atan2();
        let d0 = h0.length() * scale;
        let h1 = Vec2::new(
            q1.x * chord.x + q1.y * chord.y,
            q1.x * chord.y - q1.y * chord.x,
        );
        let th1 = h1.atan2();
        let d1 = h1.length() * scale;
        let cth0 = th0.cos();
        let cth1 = th1.cos();
        let err = if cth0 * cth1 < 0.0 {
            2.0
        } else {
            let e0 = (2. / 3.) / (1.0 + cth0).max(1e-9);
            let e1 = (2. / 3.) / (1.0 + cth1).max(1e-9);
            let s0 = th0.sin();
            let s1 = th1.sin();
            let s01 = cth0 * s1 + cth1 * s0;
            let amin = 0.15 * (2. * e0 * s0 + 2. * e1 * s1 - e0 * e1 * s01);
            let a = 0.15 * (2. * d0 * s0 + 2. * d1 * s1 - d0 * d1 * s01);
            let aerr = (a - amin).abs();
            let symm = (th0 + th1).abs();
            let asymm = (th0 - th1).abs();
            let dist = (d0 - e0).hypot(d1 - e1);
            let ctr = 4.625e-6 * symm.powi(5) + 7.5e-3 * asymm * symm.powi(2);
            let halo_symm = 5e-3 * symm * dist;
            let halo_asymm = 7e-2 * asymm * dist;
            ctr + 1.55 * aerr + halo_symm + halo_asymm
        };
        Self {
            th0,
            th1,
            chord_len,
            err: err * chord_len,
        }
    }
}

impl EulerParams {
    pub(crate) fn from_angles(th0: f32, th1: f32) -> Self {
        let k0 = th0 + th1;
        let dth = th1 - th0;
        let d2 = dth * dth;
        let k2 = k0 * k0;
        let mut a = 6.0;
        a -= d2 * (1. / 70.);
        a -= (d2 * d2) * (1. / 10780.);
        a += (d2 * d2 * d2) * 2.769_178_3e-7;
        let b = -0.1 + d2 * (1. / 4200.) + d2 * d2 * 1.695_967_7e-5;
        let c = -1. / 1400. + d2 * 6.849_159_4e-5 - k2 * 7.936_475e-6;
        a += (b + c * k2) * k2;
        let k1 = dth * a;

        let mut ch = 1.0;
        ch -= d2 * (1. / 40.);
        ch += (d2 * d2) * 0.000_342_261_92;
        ch -= (d2 * d2 * d2) * 1.934_947_5e-6;
        let b = -1. / 24. + d2 * 0.002_470_238_1 - d2 * d2 * 3.729_741e-5;
        let c = 1. / 1920. - d2 * 4.873_508_8e-5 - k2 * 3.100_193_7e-6;
        ch += (b + c * k2) * k2;
        Self { th0, k0, k1, ch }
    }

    fn eval_th(&self, t: f32) -> f32 {
        (self.k0 + 0.5 * self.k1 * (t - 1.0)) * t - self.th0
    }

    fn eval(&self, t: f32) -> Vec2 {
        let thm = self.eval_th(t * 0.5);
        let k0 = self.k0;
        let k1 = self.k1;
        let (u, v) = integ_euler_10((k0 + k1 * (0.5 * t - 0.5)) * t, k1 * t * t);
        let s = t / self.ch * thm.sin();
        let c = t / self.ch * thm.cos();
        Vec2::new(u * c - v * s, -v * c - u * s)
    }

    fn eval_with_offset(&self, t: f32, offset: f32) -> Vec2 {
        let th = self.eval_th(t);
        let v = Vec2::new(offset * th.sin(), offset * th.cos());
        self.eval(t) + v
    }
}

impl EulerSeg {
    pub(crate) fn from_params(p0: Vec2, p1: Vec2, params: EulerParams) -> Self {
        Self { p0, p1, params }
    }

    pub(crate) fn eval_with_offset(&self, t: f32, normalized_offset: f32) -> Vec2 {
        let chord = self.p1 - self.p0;
        let Vec2 { x, y } = self.params.eval_with_offset(t, normalized_offset);
        Vec2::new(
            self.p0.x + chord.x * x - chord.y * y,
            self.p0.y + chord.x * y + chord.y * x,
        )
    }
}

fn integ_euler_10(k0: f32, k1: f32) -> (f32, f32) {
    let t1_1 = k0;
    let t1_2 = 0.5 * k1;
    let t2_2 = t1_1 * t1_1;
    let t2_3 = 2. * (t1_1 * t1_2);
    let t2_4 = t1_2 * t1_2;
    let t3_4 = t2_2 * t1_2 + t2_3 * t1_1;
    let t3_6 = t2_4 * t1_2;
    let t4_4 = t2_2 * t2_2;
    let t4_5 = 2. * (t2_2 * t2_3);
    let t4_6 = 2. * (t2_2 * t2_4) + t2_3 * t2_3;
    let t4_7 = 2. * (t2_3 * t2_4);
    let t4_8 = t2_4 * t2_4;
    let t5_6 = t4_4 * t1_2 + t4_5 * t1_1;
    let t5_8 = t4_6 * t1_2 + t4_7 * t1_1;
    let t6_6 = t4_4 * t2_2;
    let t6_7 = t4_4 * t2_3 + t4_5 * t2_2;
    let t6_8 = t4_4 * t2_4 + t4_5 * t2_3 + t4_6 * t2_2;
    let t7_8 = t6_6 * t1_2 + t6_7 * t1_1;
    let t8_8 = t6_6 * t2_2;
    let mut u = 1.;
    u -= (1. / 24.) * t2_2 + (1. / 160.) * t2_4;
    u += (1. / 1920.) * t4_4 + (1. / 10752.) * t4_6 + (1. / 55296.) * t4_8;
    u -= (1. / 322560.) * t6_6 + (1. / 1658880.) * t6_8;
    u += (1. / 92897280.) * t8_8;
    let mut v = (1. / 12.) * t1_2;
    v -= (1. / 480.) * t3_4 + (1. / 2688.) * t3_6;
    v += (1. / 53760.) * t5_6 + (1. / 276480.) * t5_8;
    v -= (1. / 11612160.) * t7_8;
    (u, v)
}

const BREAK1: f32 = 0.8;
const BREAK2: f32 = 1.25;
const BREAK3: f32 = 2.1;
const SIN_SCALE: f32 = 1.097_699_2;
const QUAD_A1: f32 = 0.6406;
const QUAD_B1: f32 = -0.81;
const QUAD_C1: f32 = 0.914_811_8;
const QUAD_A2: f32 = 0.5;
const QUAD_B2: f32 = -0.156;
const QUAD_C2: f32 = 0.161_457_79;

pub(crate) fn espc_int_approx(x: f32) -> f32 {
    let y = x.abs();
    let a = if y < BREAK1 {
        (SIN_SCALE * y).sin() * (1.0 / SIN_SCALE)
    } else if y < BREAK2 {
        (8.0_f32.sqrt() / 3.0) * (y - 1.0) * (y - 1.0).abs().sqrt() + FRAC_PI_4
    } else {
        let (a, b, c) = if y < BREAK3 {
            (QUAD_A1, QUAD_B1, QUAD_C1)
        } else {
            (QUAD_A2, QUAD_B2, QUAD_C2)
        };
        a * y * y + b * y + c
    };
    a.copysign(x)
}

pub(crate) fn espc_int_inv_approx(x: f32) -> f32 {
    let y = x.abs();
    let a = if y < 0.701_070_8 {
        (x * SIN_SCALE).asin() * (1.0 / SIN_SCALE)
    } else if y < 0.903_249_3 {
        let b = y - FRAC_PI_4;
        let u = b.abs().powf(2. / 3.).copysign(b);
        u * (9.0_f32 / 8.).cbrt() + 1.0
    } else {
        let (u, v, w) = if y < 2.038_857_7 {
            const B: f32 = 0.5 * QUAD_B1 / QUAD_A1;
            (B * B - QUAD_C1 / QUAD_A1, 1.0 / QUAD_A1, B)
        } else {
            const B: f32 = 0.5 * QUAD_B2 / QUAD_A2;
            (B * B - QUAD_C2 / QUAD_A2, 1.0 / QUAD_A2, B)
        };
        (u + v * y).sqrt() - w
    };
    a.copysign(x)
}
