use std::sync::Arc;

use peniko::{
    Color, Compose, Gradient, Mix,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};

use super::{CubeBufferLengths, CubeRenderTarget, WgpuRenderer};
use crate::cubecl::pipelines::coarse::TILE_WORKGROUP_SIZE;
use crate::cubecl::types::{
    CUBE_PTCL_BEGIN_BLEND, CUBE_PTCL_BEGIN_CLIP, CUBE_PTCL_BEGIN_OPACITY, CUBE_PTCL_COLOR,
    CUBE_PTCL_END, CUBE_PTCL_END_BLEND, CUBE_PTCL_END_CLIP, CUBE_PTCL_END_OPACITY, CUBE_PTCL_FILL,
    CUMSUM_CHUNK_SIZE,
};
use crate::shared::brush::{Brush, IDENTITY_TRANSFORM, PatternBrush};
use crate::shared::execution::ExecOp;
use crate::shared::image::{Image, rgba8_pack, unpack_rgba8};
use crate::shared::layer::{filter::Filter, region::Region};
use crate::shared::pixel::premul_f32_to_u32;
use crate::{CpuRenderer, FillRule, Radius, Scene};

fn mixed_shape_scene() -> Scene {
    let mut scene = Scene::new(360, 260);
    scene.push_rect(
        Rect::new(0.0, 0.0, 360.0, 260.0),
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(42.0, 38.0, 178.0, 128.0),
        Color::from_rgba8(37, 143, 93, 230),
        FillRule::NonZero,
    );
    scene.push_path(
        Circle::new((242.0, 86.0), 56.0).to_path(0.1),
        Color::from_rgba8(45, 111, 211, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        Rect::new(72.0, 156.0, 178.0, 218.0),
        Stroke::new(12.0),
        Color::from_rgb8(230, 89, 80),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        Circle::new((260.0, 184.0), 40.0),
        Stroke::new(10.0),
        Color::from_rgb8(222, 178, 106),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene
}

fn assert_images_close(expected: &Image, actual: &Image, tolerance: u8) {
    assert_eq!(
        (actual.width, actual.height),
        (expected.width, expected.height)
    );
    let mut mismatch_count = 0usize;
    let mut first_mismatch = None;
    for (ix, (&a, &b)) in expected.pixels.iter().zip(&actual.pixels).enumerate() {
        let ax = unpack_rgba8(a);
        let bx = unpack_rgba8(b);
        let close = ax
            .iter()
            .zip(bx)
            .all(|(&lhs, rhs)| lhs.abs_diff(rhs) <= tolerance);
        if !close {
            mismatch_count += 1;
            first_mismatch.get_or_insert((ix, ax, bx));
        }
    }
    assert_eq!(
        mismatch_count, 0,
        "{mismatch_count} pixels differ; first mismatch: {first_mismatch:?}"
    );
}

mod backdrop;
mod buffer_lengths;
mod coarse;
mod cumsum;
mod filter;
mod fine;
mod render;
mod scan;
