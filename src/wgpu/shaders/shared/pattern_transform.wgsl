// Recover the rounding error of the second product before applying translation.
// A rotated pattern can cancel to exactly zero; a spurious negative residual
// would make nearest sampling wrap to the opposite edge after floor().
fn pattern_transform_component(a: f32, b: f32, offset: f32, x: f32, y: f32) -> f32 {
    let by = b * y;
    let dot = fma(a, x, by) + fma(b, y, -by);
    return dot + offset;
}
