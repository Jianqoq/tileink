use super::{ComputeBatch, FilterConfig, ResourceId, Result, region};

#[derive(Clone, Copy)]
pub enum Glass {
    Effect,
    RectangleComposite,
}

/// Record liquid glass with explicit sharp/blurred inputs and a logical sampling domain.
pub fn encode(
    batch: &mut ComputeBatch,
    mode: Glass,
    config: FilterConfig,
    tiles: Option<&[u32]>,
    source: ResourceId,
    blurred: ResourceId,
    target: ResourceId,
) -> Result<()> {
    let values = [
        config.rect_x0,
        config.rect_y0,
        config.rect_x1,
        config.rect_y1,
        config.radius_top_left,
        config.radius_top_right,
        config.radius_bottom_left,
        config.radius_bottom_right,
        config.liquid_tint_r,
        config.liquid_tint_g,
        config.liquid_tint_b,
        config.liquid_tint_a,
        config.liquid_refraction_thickness,
        config.liquid_refraction_factor,
        config.liquid_refraction_dispersion,
        config.liquid_fresnel_range,
        config.liquid_fresnel_hardness,
        config.liquid_fresnel_factor,
        config.liquid_glare_range,
        config.liquid_glare_hardness,
        config.liquid_glare_convergence,
        config.liquid_glare_opposite_factor,
        config.liquid_glare_factor,
        config.liquid_glare_angle,
    ];
    if values.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite liquid glass parameter".into());
    }
    // Public tint channels and percent-like controls are normalized before GPU encoding.
    // Enforce that domain here so later LCH and intensity products cannot overflow.
    let normalized = [
        config.liquid_tint_r,
        config.liquid_tint_g,
        config.liquid_tint_b,
        config.liquid_tint_a,
        config.liquid_fresnel_hardness,
        config.liquid_fresnel_factor,
        config.liquid_glare_hardness,
        config.liquid_glare_convergence,
        config.liquid_glare_opposite_factor,
        config.liquid_glare_factor,
    ];
    if normalized.iter().any(|v| !(0.0..=1.0).contains(v)) {
        return Err("liquid glass normalized control exceeds unit range".into());
    }
    let max = f64::from(f32::MAX);
    for offset in [
        -std::f64::consts::FRAC_PI_4,
        7.0 * std::f64::consts::FRAC_PI_4,
    ] {
        if ((f64::from(config.liquid_glare_angle) + offset) * 2.0).abs() > max {
            return Err("liquid glass glare angle overflows shader arithmetic".into());
        }
    }
    // Bound actual center/span and squared-distance arithmetic, including normal
    // samples one pixel beyond the surface. This preserves safe large coordinates.
    let mut square_bound = 0.0;
    for (start, end, extent) in [
        (config.rect_x0, config.rect_x1, config.width),
        (config.rect_y0, config.rect_y1, config.height),
    ] {
        let (start, end) = (f64::from(start), f64::from(end));
        if (start + end).abs() > max || (end - start).abs() > max {
            return Err("liquid glass rectangle center or span overflows".into());
        }
        let center = (start + end) * 0.5;
        let half = ((end - start) * 0.5).max(0.0);
        let point = (-0.5 - center)
            .abs()
            .max((f64::from(extent) + 0.5 - center).abs());
        let bound = point + 2.0 * half;
        square_bound += bound * bound;
    }
    if square_bound > max {
        return Err("liquid glass rectangle distance overflows".into());
    }
    if config.upsample_filter > 1
        || config.mask_enabled > 1
        || config.source_x0 > config.source_x1
        || config.source_y0 > config.source_y1
        || config.source_x1 > config.width
        || config.source_y1 > config.height
    {
        return Err("invalid liquid glass sampling domain".into());
    }
    region::record(
        batch,
        match mode {
            Glass::Effect => "filter_liquid_glass_region",
            Glass::RectangleComposite => "filter_liquid_glass_rect_composite_region",
        },
        region::Geometry::Pixels,
        config,
        tiles,
        region::ReadBindings::textures(&[(1, source), (2, blurred)], [config.width, config.height]),
        target,
    )
}
