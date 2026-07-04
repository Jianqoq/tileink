use peniko::{Compose, Mix};

use crate::shared::{
    bounds::Bounds,
    brush::Brush,
    image::{Image, rgba8_pack, unpack_rgba8},
    image_resource::ImageResourceStore,
    layer::{
        blend::{Blend, src_over_premul},
        filter::{
            BlurDownsampleFilter, BlurSampling, BlurUpsampleFilter, COMPONENT_TRANSFER_TABLE_SIZE,
            ColorChannel, ComponentTransferTable, CompositeOperator, ConvolveEdgeMode,
            ConvolveMatrix, DiffuseLighting, DisplacementMap, Filter, FilterInput, FilterPrimitive,
            FilterPrimitiveKind, LightSource, MorphologyOperator, SpecularLighting,
            TURBULENCE_LATTICE_SIZE, TURBULENCE_TABLE_LEN, Turbulence, TurbulenceKind,
            filter_offset_to_pixel_delta, rect_liquid_glass_region, turbulence_gradient_index,
            turbulence_lattice,
        },
        region::Region,
    },
    pixel::{pack_premul_rgba8, unpack_premul_rgba8},
};

mod graph;
mod liquid_glass;
mod turbulence;

#[cfg(test)]
fn apply(image: &mut Image, filter: &Filter, bounds: Bounds) {
    apply_with_resources(image, filter, bounds, None);
}

pub(crate) fn apply_with_resources(
    image: &mut Image,
    filter: &Filter,
    bounds: Bounds,
    image_resources: Option<&ImageResourceStore>,
) {
    apply_with_region(
        image,
        filter,
        bounds,
        (image.width, image.height),
        None,
        image_resources,
    );
}

pub(crate) fn apply_backdrop_with_resources(
    image: &mut Image,
    filter: &Filter,
    bounds: Bounds,
    surface_size: (u32, u32),
    region: &Region,
    image_resources: Option<&ImageResourceStore>,
) {
    apply_with_region(
        image,
        filter,
        bounds,
        surface_size,
        Some(region),
        image_resources,
    );
}

fn apply_with_region(
    image: &mut Image,
    filter: &Filter,
    bounds: Bounds,
    surface_size: (u32, u32),
    region: Option<&Region>,
    image_resources: Option<&ImageResourceStore>,
) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                apply_with_region(image, filter, bounds, surface_size, region, image_resources);
            }
        }
        Filter::Graph { primitives, .. } => {
            graph::apply(image, primitives, bounds, image_resources)
        }
        Filter::RectLiquidGlass(glass) => liquid_glass::apply(
            image,
            bounds,
            surface_size,
            *glass,
            rect_liquid_glass_region(region, bounds),
        ),
        Filter::Blur {
            std_dev_x,
            std_dev_y,
            sampling,
        } => apply_gaussian_blur_with_sampling(image, *std_dev_x, *std_dev_y, *sampling, bounds),
        Filter::ColorMatrix(matrix) => {
            for px in &mut image.pixels {
                *px = apply_color_matrix_pixel(*px, *matrix);
            }
        }
        Filter::ComponentTransfer(table) => {
            for px in &mut image.pixels {
                *px = apply_component_transfer_pixel(*px, table);
            }
        }
        Filter::ConvolveMatrix(matrix) => apply_convolve_matrix(image, matrix),
        Filter::DiffuseLighting(lighting) => apply_diffuse_lighting(image, bounds, lighting),
        Filter::SpecularLighting(lighting) => apply_specular_lighting(image, bounds, lighting),
        Filter::Flood { brush } => apply_flood(image, bounds, brush, image_resources),
        Filter::Brightness(amount)
        | Filter::Contrast(amount)
        | Filter::Grayscale(amount)
        | Filter::HueRotate(amount)
        | Filter::Invert(amount)
        | Filter::Opacity(amount)
        | Filter::Saturate(amount)
        | Filter::Sepia(amount) => {
            for px in &mut image.pixels {
                *px = apply_color_filter_pixel(*px, filter, *amount);
            }
        }
        Filter::Offset { dx, dy } => apply_offset(
            image,
            filter_offset_to_pixel_delta(*dx),
            filter_offset_to_pixel_delta(*dy),
        ),
        Filter::Morphology {
            radius_x,
            radius_y,
            operator,
        } => apply_morphology(image, *radius_x, *radius_y, *operator),
        Filter::DropShadow {
            offset_x,
            offset_y,
            std_dev,
            brush,
        } => apply_drop_shadow(
            image,
            bounds,
            *offset_x,
            *offset_y,
            *std_dev,
            brush,
            image_resources,
        ),
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn apply_morphology(image: &mut Image, radius_x: f32, radius_y: f32, operator: MorphologyOperator) {
    let radius_x = radius_x.max(0.0).ceil() as usize;
    let radius_y = radius_y.max(0.0).ceil() as usize;
    if radius_x == 0 && radius_y == 0 {
        return;
    }

    let width = image.width as usize;
    let height = image.height as usize;
    if width == 0 || height == 0 {
        return;
    }

    let source = image
        .pixels
        .iter()
        .copied()
        .map(straight_rgba8)
        .collect::<Vec<_>>();
    let mut temp = vec![[0.0; 4]; source.len()];
    let mut out = vec![[0.0; 4]; source.len()];
    morphology_axis(
        &source,
        &mut temp,
        width,
        height,
        radius_x,
        MorphologyAxis::X,
        operator,
    );
    morphology_axis(
        &temp,
        &mut out,
        width,
        height,
        radius_y,
        MorphologyAxis::Y,
        operator,
    );
    for (pixel, rgba) in image.pixels.iter_mut().zip(out) {
        let alpha = rgba[3];
        *pixel = pack_premul_rgba8([rgba[0] * alpha, rgba[1] * alpha, rgba[2] * alpha, alpha]);
    }
}

#[derive(Clone, Copy)]
enum MorphologyAxis {
    X,
    Y,
}

fn morphology_axis(
    source: &[[f32; 4]],
    target: &mut [[f32; 4]],
    width: usize,
    height: usize,
    radius: usize,
    axis: MorphologyAxis,
    operator: MorphologyOperator,
) {
    let (line_count, line_len) = match axis {
        MorphologyAxis::X => (height, width),
        MorphologyAxis::Y => (width, height),
    };
    if line_len == 0 {
        return;
    }

    let mut deques: [Vec<usize>; 4] = std::array::from_fn(|_| Vec::with_capacity(line_len));
    let mut heads = [0usize; 4];
    for line in 0..line_count {
        for channel in 0..4 {
            deques[channel].clear();
            heads[channel] = 0;
        }
        let mut pushed_end = 0usize;
        for pos in 0..line_len {
            let end = pos.saturating_add(radius).min(line_len - 1);
            while pushed_end <= end {
                let sample = source[morphology_axis_index(line, pushed_end, width, axis)];
                for channel in 0..4 {
                    while deques[channel].len() > heads[channel] {
                        let back = *deques[channel].last().unwrap();
                        let back_value =
                            source[morphology_axis_index(line, back, width, axis)][channel];
                        if !morphology_replaces(sample[channel], back_value, operator) {
                            break;
                        }
                        deques[channel].pop();
                    }
                    deques[channel].push(pushed_end);
                }
                pushed_end += 1;
            }

            let out_ix = morphology_axis_index(line, pos, width, axis);
            if operator == MorphologyOperator::Erode
                && (pos < radius || pos.saturating_add(radius) >= line_len)
            {
                target[out_ix] = [0.0; 4];
                continue;
            }

            let start = pos.saturating_sub(radius);
            let mut out = [0.0; 4];
            for channel in 0..4 {
                while heads[channel] < deques[channel].len()
                    && deques[channel][heads[channel]] < start
                {
                    heads[channel] += 1;
                }
                out[channel] = source
                    [morphology_axis_index(line, deques[channel][heads[channel]], width, axis)]
                    [channel];
            }
            target[out_ix] = out;
        }
    }
}

fn morphology_axis_index(line: usize, pos: usize, width: usize, axis: MorphologyAxis) -> usize {
    match axis {
        MorphologyAxis::X => line * width + pos,
        MorphologyAxis::Y => pos * width + line,
    }
}

fn morphology_replaces(candidate: f32, current: f32, operator: MorphologyOperator) -> bool {
    match operator {
        MorphologyOperator::Erode => candidate <= current,
        MorphologyOperator::Dilate => candidate >= current,
    }
}

fn apply_convolve_matrix(image: &mut Image, matrix: &ConvolveMatrix) {
    if matrix.columns == 0
        || matrix.rows == 0
        || matrix.target_x >= matrix.columns
        || matrix.target_y >= matrix.rows
        || matrix.divisor == 0.0
        || matrix.data.len() != (matrix.columns * matrix.rows) as usize
        || image.width == 0
        || image.height == 0
    {
        return;
    }

    let source = image.clone();
    for y in 0..source.height {
        for x in 0..source.width {
            image.pixels[(y * image.width + x) as usize] =
                convolve_matrix_pixel(&source, x as i32, y as i32, matrix);
        }
    }
}

fn convolve_matrix_pixel(source: &Image, x: i32, y: i32, matrix: &ConvolveMatrix) -> u32 {
    let mut out = [0.0; 4];
    for ky in 0..matrix.rows {
        for kx in 0..matrix.columns {
            let kernel_ix =
                ((matrix.rows - 1 - ky) * matrix.columns + (matrix.columns - 1 - kx)) as usize;
            let weight = matrix.data[kernel_ix];
            let sx = x + kx as i32 - matrix.target_x as i32;
            let sy = y + ky as i32 - matrix.target_y as i32;
            let sample = convolve_sample(source, sx, sy, matrix.edge_mode);
            for channel in 0..4 {
                out[channel] += sample[channel] * weight;
            }
        }
    }

    for channel in &mut out {
        *channel = (*channel / matrix.divisor + matrix.bias).clamp(0.0, 1.0);
    }
    let alpha = if matrix.preserve_alpha {
        straight_rgba8(source.pixels[(y as u32 * source.width + x as u32) as usize])[3]
    } else {
        out[3]
    };
    pack_premul_rgba8([out[0] * alpha, out[1] * alpha, out[2] * alpha, alpha])
}

fn convolve_sample(source: &Image, x: i32, y: i32, edge_mode: ConvolveEdgeMode) -> [f32; 4] {
    let Some((x, y)) = convolve_sample_coord(source.width, source.height, x, y, edge_mode) else {
        return [0.0; 4];
    };
    straight_rgba8(source.pixels[(y * source.width + x) as usize])
}

fn convolve_sample_coord(
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    edge_mode: ConvolveEdgeMode,
) -> Option<(u32, u32)> {
    let width = width as i32;
    let height = height as i32;
    match edge_mode {
        ConvolveEdgeMode::None => {
            (x >= 0 && y >= 0 && x < width && y < height).then_some((x as u32, y as u32))
        }
        ConvolveEdgeMode::Duplicate => {
            Some((x.clamp(0, width - 1) as u32, y.clamp(0, height - 1) as u32))
        }
        ConvolveEdgeMode::Wrap => Some((x.rem_euclid(width) as u32, y.rem_euclid(height) as u32)),
    }
}

fn apply_diffuse_lighting(image: &mut Image, bounds: Bounds, lighting: &DiffuseLighting) {
    if image.width == 0 || image.height == 0 {
        return;
    }

    // SVG diffuse lighting treats source alpha as a height map and replaces the
    // primitive output with opaque lit RGB, so reads must come from the original input.
    let source = image.clone();
    for y in 0..source.height {
        for x in 0..source.width {
            image.pixels[(y * image.width + x) as usize] =
                diffuse_lighting_pixel(&source, bounds, x, y, lighting);
        }
    }
}

fn diffuse_lighting_pixel(
    source: &Image,
    bounds: Bounds,
    x: u32,
    y: u32,
    lighting: &DiffuseLighting,
) -> u32 {
    let alpha = alpha_at(source, x, y);
    let z = alpha * lighting.surface_scale;
    let [nx, ny, nz] = surface_normal(source, x, y, lighting.surface_scale);
    let world_x = bounds.x0 as f32 + x as f32 + 0.5;
    let world_y = bounds.y0 as f32 + y as f32 + 0.5;
    let light = light_vector(lighting.light_source, world_x, world_y, z);
    let Some(([lx, ly, lz], attenuation)) = light else {
        return 0xff00_0000;
    };

    let amount = lighting.diffuse_constant * attenuation * (nx * lx + ny * ly + nz * lz).max(0.0);
    pack_premul_rgba8([
        (lighting.lighting_color[0] * amount).clamp(0.0, 1.0),
        (lighting.lighting_color[1] * amount).clamp(0.0, 1.0),
        (lighting.lighting_color[2] * amount).clamp(0.0, 1.0),
        1.0,
    ])
}

fn apply_specular_lighting(image: &mut Image, bounds: Bounds, lighting: &SpecularLighting) {
    if image.width == 0 || image.height == 0 {
        return;
    }

    // SVG specular lighting uses the same alpha-derived normal map as diffuse
    // lighting, but writes transparent black when no specular highlight exists.
    let source = image.clone();
    for y in 0..source.height {
        for x in 0..source.width {
            image.pixels[(y * image.width + x) as usize] =
                specular_lighting_pixel(&source, bounds, x, y, lighting);
        }
    }
}

fn specular_lighting_pixel(
    source: &Image,
    bounds: Bounds,
    x: u32,
    y: u32,
    lighting: &SpecularLighting,
) -> u32 {
    let alpha = alpha_at(source, x, y);
    let z = alpha * lighting.surface_scale;
    let normal = surface_normal(source, x, y, lighting.surface_scale);
    let world_x = bounds.x0 as f32 + x as f32 + 0.5;
    let world_y = bounds.y0 as f32 + y as f32 + 0.5;
    let light = light_vector(lighting.light_source, world_x, world_y, z);
    let Some((light, attenuation)) = light else {
        return 0;
    };
    let Some(half) = normalize3([light[0], light[1], light[2] + 1.0]) else {
        return 0;
    };

    let normal_dot_half =
        (normal[0] * half[0] + normal[1] * half[1] + normal[2] * half[2]).max(0.0);
    let amount = lighting.specular_constant
        * attenuation
        * normal_dot_half.powf(lighting.specular_exponent.max(0.0));
    let r = (lighting.lighting_color[0] * amount).clamp(0.0, 1.0);
    let g = (lighting.lighting_color[1] * amount).clamp(0.0, 1.0);
    let b = (lighting.lighting_color[2] * amount).clamp(0.0, 1.0);
    pack_premul_rgba8([r, g, b, r.max(g).max(b)])
}

fn surface_normal(source: &Image, x: u32, y: u32, surface_scale: f32) -> [f32; 3] {
    let dx = alpha_gradient_x(source, x, y) * surface_scale;
    let dy = alpha_gradient_y(source, x, y) * surface_scale;
    normalize3([-dx, -dy, 1.0]).unwrap_or([0.0, 0.0, 1.0])
}

fn alpha_gradient_x(source: &Image, x: u32, y: u32) -> f32 {
    if source.width < 2 {
        return 0.0;
    }

    let x = x as i32;
    let y = y as i32;
    let one_sided = x == 0 || x == source.width as i32 - 1;
    let left = (x - 1).max(0) as u32;
    let center = x as u32;
    let right = (x + 1).min(source.width as i32 - 1) as u32;
    let mut weighted_diff = 0.0;
    let mut weight_sum = 0.0;
    for offset in -1..=1 {
        let sy = y + offset;
        if sy < 0 || sy >= source.height as i32 {
            continue;
        }
        let weight = if offset == 0 { 2.0 } else { 1.0 };
        let diff = if x == 0 {
            alpha_at(source, right, sy as u32) - alpha_at(source, center, sy as u32)
        } else if x == source.width as i32 - 1 {
            alpha_at(source, center, sy as u32) - alpha_at(source, left, sy as u32)
        } else {
            alpha_at(source, right, sy as u32) - alpha_at(source, left, sy as u32)
        };
        weighted_diff += weight * diff;
        weight_sum += weight;
    }
    let edge_scale = if one_sided { 2.0 } else { 1.0 };
    weighted_diff * edge_scale / weight_sum.max(f32::EPSILON)
}

fn alpha_gradient_y(source: &Image, x: u32, y: u32) -> f32 {
    if source.height < 2 {
        return 0.0;
    }

    let x = x as i32;
    let y = y as i32;
    let one_sided = y == 0 || y == source.height as i32 - 1;
    let top = (y - 1).max(0) as u32;
    let center = y as u32;
    let bottom = (y + 1).min(source.height as i32 - 1) as u32;
    let mut weighted_diff = 0.0;
    let mut weight_sum = 0.0;
    for offset in -1..=1 {
        let sx = x + offset;
        if sx < 0 || sx >= source.width as i32 {
            continue;
        }
        let weight = if offset == 0 { 2.0 } else { 1.0 };
        let diff = if y == 0 {
            alpha_at(source, sx as u32, bottom) - alpha_at(source, sx as u32, center)
        } else if y == source.height as i32 - 1 {
            alpha_at(source, sx as u32, center) - alpha_at(source, sx as u32, top)
        } else {
            alpha_at(source, sx as u32, bottom) - alpha_at(source, sx as u32, top)
        };
        weighted_diff += weight * diff;
        weight_sum += weight;
    }
    let edge_scale = if one_sided { 2.0 } else { 1.0 };
    weighted_diff * edge_scale / weight_sum.max(f32::EPSILON)
}

fn alpha_at(source: &Image, x: u32, y: u32) -> f32 {
    ((source.pixels[(y * source.width + x) as usize] >> 24) & 255) as f32 / 255.0
}

fn light_vector(light_source: LightSource, x: f32, y: f32, z: f32) -> Option<([f32; 3], f32)> {
    match light_source {
        LightSource::Distant { azimuth, elevation } => {
            let azimuth = azimuth.to_radians();
            let elevation = elevation.to_radians();
            Some((
                [
                    azimuth.cos() * elevation.cos(),
                    azimuth.sin() * elevation.cos(),
                    elevation.sin(),
                ],
                1.0,
            ))
        }
        LightSource::Point {
            x: lx,
            y: ly,
            z: lz,
        } => normalize3([lx - x, ly - y, lz - z]).map(|light| (light, 1.0)),
        LightSource::Spot {
            x: lx,
            y: ly,
            z: lz,
            points_at_x,
            points_at_y,
            points_at_z,
            specular_exponent,
            limiting_cone_angle,
        } => {
            let light = normalize3([lx - x, ly - y, lz - z])?;
            let direction = normalize3([points_at_x - lx, points_at_y - ly, points_at_z - lz])?;
            let focus =
                (-(light[0] * direction[0] + light[1] * direction[1] + light[2] * direction[2]))
                    .max(0.0);
            if let Some(angle) = limiting_cone_angle
                && focus < angle.to_radians().cos()
            {
                return None;
            }
            Some((light, focus.powf(specular_exponent.max(0.0))))
        }
    }
}

fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (len > f32::EPSILON).then_some([v[0] / len, v[1] / len, v[2] / len])
}

fn for_each_region_pixel(bounds: Bounds, region: Bounds, width: u32, mut f: impl FnMut(usize)) {
    if region.is_empty() {
        return;
    }
    for y in region.y0..region.y1 {
        let local_y = (y - bounds.y0) as u32;
        for x in region.x0..region.x1 {
            let local_x = (x - bounds.x0) as u32;
            f((local_y * width + local_x) as usize);
        }
    }
}

fn composite_pixel(input1: u32, input2: u32, operator: CompositeOperator) -> u32 {
    match operator {
        CompositeOperator::Over => {
            Blend::new(Mix::Normal, Compose::SrcOver).blend_pixel(input1, input2)
        }
        CompositeOperator::In => {
            Blend::new(Mix::Normal, Compose::SrcIn).blend_pixel(input1, input2)
        }
        CompositeOperator::Out => {
            Blend::new(Mix::Normal, Compose::SrcOut).blend_pixel(input1, input2)
        }
        CompositeOperator::Atop => {
            Blend::new(Mix::Normal, Compose::SrcAtop).blend_pixel(input1, input2)
        }
        CompositeOperator::Xor => Blend::new(Mix::Normal, Compose::Xor).blend_pixel(input1, input2),
        CompositeOperator::Arithmetic { k1, k2, k3, k4 } => {
            arithmetic_composite_pixel(input1, input2, k1, k2, k3, k4)
        }
    }
}

fn arithmetic_composite_pixel(input1: u32, input2: u32, k1: f32, k2: f32, k3: f32, k4: f32) -> u32 {
    let a = unpack_premul_rgba8(input1);
    let b = unpack_premul_rgba8(input2);
    pack_premul_rgba8([
        arithmetic_channel(a[0], b[0], k1, k2, k3, k4),
        arithmetic_channel(a[1], b[1], k1, k2, k3, k4),
        arithmetic_channel(a[2], b[2], k1, k2, k3, k4),
        arithmetic_channel(a[3], b[3], k1, k2, k3, k4),
    ])
}

fn arithmetic_channel(a: f32, b: f32, k1: f32, k2: f32, k3: f32, k4: f32) -> f32 {
    (k1 * a * b + k2 * a + k3 * b + k4).clamp(0.0, 1.0)
}

fn straight_rgba8(px: u32) -> [f32; 4] {
    let [r, g, b, a] = unpack_rgba8(px);
    if a == 0 {
        [0.0, 0.0, 0.0, 0.0]
    } else {
        let inv_alpha = 255.0 / a as f32;
        [
            r as f32 * inv_alpha / 255.0,
            g as f32 * inv_alpha / 255.0,
            b as f32 * inv_alpha / 255.0,
            a as f32 / 255.0,
        ]
    }
}

fn apply_color_filter_pixel(px: u32, filter: &Filter, amount: f32) -> u32 {
    let mut c = unpack_premul_rgba8(px);
    if matches!(filter, Filter::Opacity(_)) {
        let opacity = amount.clamp(0.0, 1.0);
        return pack_premul_rgba8([
            c[0] * opacity,
            c[1] * opacity,
            c[2] * opacity,
            c[3] * opacity,
        ]);
    }
    if c[3] <= 0.0 {
        return px;
    }

    let alpha = c[3];
    let mut rgb = [c[0] / alpha, c[1] / alpha, c[2] / alpha];
    match filter {
        Filter::Brightness(_) => {
            for v in &mut rgb {
                *v *= amount;
            }
        }
        Filter::Contrast(_) => {
            for v in &mut rgb {
                *v = (*v - 0.5) * amount + 0.5;
            }
        }
        Filter::Grayscale(_) => {
            let t = amount.clamp(0.0, 1.0);
            let l = lum(rgb);
            for v in &mut rgb {
                *v = lerp(*v, l, t);
            }
        }
        Filter::HueRotate(_) => {
            let a = amount.to_radians();
            let co = a.cos();
            let si = a.sin();
            rgb = [
                (0.213 + co * 0.787 - si * 0.213) * rgb[0]
                    + (0.715 - co * 0.715 - si * 0.715) * rgb[1]
                    + (0.072 - co * 0.072 + si * 0.928) * rgb[2],
                (0.213 - co * 0.213 + si * 0.143) * rgb[0]
                    + (0.715 + co * 0.285 + si * 0.140) * rgb[1]
                    + (0.072 - co * 0.072 - si * 0.283) * rgb[2],
                (0.213 - co * 0.213 - si * 0.787) * rgb[0]
                    + (0.715 - co * 0.715 + si * 0.715) * rgb[1]
                    + (0.072 + co * 0.928 + si * 0.072) * rgb[2],
            ];
        }
        Filter::Invert(_) => {
            let t = amount.clamp(0.0, 1.0);
            for v in &mut rgb {
                *v = lerp(*v, 1.0 - *v, t);
            }
        }
        Filter::Saturate(_) => {
            let l = lum(rgb);
            for v in &mut rgb {
                *v = l + (*v - l) * amount;
            }
        }
        Filter::Sepia(_) => {
            let t = amount.clamp(0.0, 1.0);
            let sepia = [
                rgb[0] * 0.393 + rgb[1] * 0.769 + rgb[2] * 0.189,
                rgb[0] * 0.349 + rgb[1] * 0.686 + rgb[2] * 0.168,
                rgb[0] * 0.272 + rgb[1] * 0.534 + rgb[2] * 0.131,
            ];
            for i in 0..3 {
                rgb[i] = lerp(rgb[i], sepia[i], t);
            }
        }
        _ => {}
    }

    for v in &mut rgb {
        *v = v.clamp(0.0, 1.0);
    }
    c[0] = rgb[0] * alpha;
    c[1] = rgb[1] * alpha;
    c[2] = rgb[2] * alpha;
    pack_premul_rgba8(c)
}

fn apply_color_matrix_pixel(px: u32, matrix: [f32; 20]) -> u32 {
    // SVG filter matrices operate on straight RGBA, while render buffers are premultiplied.
    let c = unpack_premul_rgba8(px);
    let alpha = c[3];
    let rgba = if alpha > 0.0 {
        [c[0] / alpha, c[1] / alpha, c[2] / alpha, alpha]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };
    let out = [
        matrix[0] * rgba[0]
            + matrix[1] * rgba[1]
            + matrix[2] * rgba[2]
            + matrix[3] * rgba[3]
            + matrix[4],
        matrix[5] * rgba[0]
            + matrix[6] * rgba[1]
            + matrix[7] * rgba[2]
            + matrix[8] * rgba[3]
            + matrix[9],
        matrix[10] * rgba[0]
            + matrix[11] * rgba[1]
            + matrix[12] * rgba[2]
            + matrix[13] * rgba[3]
            + matrix[14],
        matrix[15] * rgba[0]
            + matrix[16] * rgba[1]
            + matrix[17] * rgba[2]
            + matrix[18] * rgba[3]
            + matrix[19],
    ];
    let out_alpha = out[3].clamp(0.0, 1.0);
    pack_premul_rgba8([
        out[0].clamp(0.0, 1.0) * out_alpha,
        out[1].clamp(0.0, 1.0) * out_alpha,
        out[2].clamp(0.0, 1.0) * out_alpha,
        out_alpha,
    ])
}

fn apply_component_transfer_pixel(px: u32, table: &ComponentTransferTable) -> u32 {
    let alpha = (px >> 24) & 255;
    let r = table[straight_channel_index(px & 255, alpha)] as f32 / 255.0;
    let g = table[COMPONENT_TRANSFER_TABLE_SIZE + straight_channel_index((px >> 8) & 255, alpha)]
        as f32
        / 255.0;
    let b = table
        [2 * COMPONENT_TRANSFER_TABLE_SIZE + straight_channel_index((px >> 16) & 255, alpha)]
        as f32
        / 255.0;
    let a = table[3 * COMPONENT_TRANSFER_TABLE_SIZE + alpha as usize] as f32 / 255.0;
    pack_premul_rgba8([r * a, g * a, b * a, a])
}

fn straight_channel_index(premul: u32, alpha: u32) -> usize {
    let safe_alpha = alpha.max(1);
    let mut index = ((premul * 255 + safe_alpha / 2) / safe_alpha).min(255);
    if alpha == 0 {
        index = 0;
    }
    index as usize
}

fn apply_flood(
    image: &mut Image,
    bounds: Bounds,
    brush: &Brush,
    image_resources: Option<&ImageResourceStore>,
) {
    for y in 0..image.height {
        for x in 0..image.width {
            image.pixels[(y * image.width + x) as usize] = brush.sample_with_resources(
                bounds.x0 as f32 + x as f32 + 0.5,
                bounds.y0 as f32 + y as f32 + 0.5,
                image_resources,
            );
        }
    }
}

fn apply_offset(image: &mut Image, dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }

    let source = image.pixels.clone();
    image.pixels.fill(0);
    for y in 0..image.height as i32 {
        for x in 0..image.width as i32 {
            let tx = x + dx;
            let ty = y + dy;
            if tx < 0 || ty < 0 || tx >= image.width as i32 || ty >= image.height as i32 {
                continue;
            }
            image.pixels[(ty as u32 * image.width + tx as u32) as usize] =
                source[(y as u32 * image.width + x as u32) as usize];
        }
    }
}

fn apply_gaussian_blur(image: &mut Image, std_dev_x: f32, std_dev_y: f32) {
    apply_gaussian_blur_full_resolution(image, std_dev_x, std_dev_y);
}

fn apply_gaussian_blur_with_sampling(
    image: &mut Image,
    std_dev_x: f32,
    std_dev_y: f32,
    sampling: BlurSampling,
    bounds: Bounds,
) {
    let std_dev_x = std_dev_x.max(0.0);
    let std_dev_y = std_dev_y.max(0.0);
    if (std_dev_x <= 0.0 && std_dev_y <= 0.0) || image.width == 0 || image.height == 0 {
        return;
    }

    let factor = sampling.factor();
    if factor == 1 || std_dev_x <= 0.0 || std_dev_y <= 0.0 {
        apply_gaussian_blur_full_resolution(image, std_dev_x, std_dev_y);
        return;
    }

    let Some(low_bounds) = downsampled_bounds(bounds, factor) else {
        return;
    };
    if low_bounds.width() >= image.width && low_bounds.height() >= image.height {
        apply_gaussian_blur_full_resolution(image, std_dev_x, std_dev_y);
        return;
    }

    let mut low = downsample_image(
        image,
        bounds,
        low_bounds,
        factor,
        sampling.downsample_filter,
    );
    apply_gaussian_blur_full_resolution(
        &mut low,
        std_dev_x / factor as f32,
        std_dev_y / factor as f32,
    );
    *image = upsample_image(
        &low,
        bounds,
        low_bounds,
        factor,
        sampling.upsample_filter,
        image.width,
        image.height,
    );
}

fn apply_gaussian_blur_full_resolution(image: &mut Image, std_dev_x: f32, std_dev_y: f32) {
    let std_dev_x = std_dev_x.max(0.0);
    let std_dev_y = std_dev_y.max(0.0);
    if (std_dev_x <= 0.0 && std_dev_y <= 0.0) || image.width == 0 || image.height == 0 {
        return;
    }
    // SVG feGaussianBlur has independent X/Y standard deviations; a zero axis
    // is intentionally skipped so `stdDeviation="5 0"` stays horizontal-only.
    if std_dev_x > 0.0 {
        let kernel = gaussian_kernel(std_dev_x);
        if kernel.len() > 1 {
            image.pixels = blur_pass(image, &kernel, Axis::Horizontal);
        }
    }
    if std_dev_y > 0.0 {
        let kernel = gaussian_kernel(std_dev_y);
        if kernel.len() > 1 {
            image.pixels = blur_pass(image, &kernel, Axis::Vertical);
        }
    }
}

fn downsampled_bounds(bounds: Bounds, factor: u32) -> Option<Bounds> {
    let factor = factor.max(1) as i32;
    let x0 = bounds.x0.div_euclid(factor);
    let y0 = bounds.y0.div_euclid(factor);
    let x1 = div_ceil_i32(bounds.x1, factor);
    let y1 = div_ceil_i32(bounds.y1, factor);
    let bounds = Bounds::new(x0, y0, x1, y1);
    (!bounds.is_empty()).then_some(bounds)
}

fn div_ceil_i32(value: i32, divisor: i32) -> i32 {
    -((-value).div_euclid(divisor))
}

fn downsample_image(
    source: &Image,
    bounds: Bounds,
    low_bounds: Bounds,
    factor: u32,
    filter: BlurDownsampleFilter,
) -> Image {
    let mut low = Image::new(
        low_bounds.width(),
        low_bounds.height(),
        peniko::Color::TRANSPARENT,
    );
    let factor = factor as i32;
    for ly in 0..low.height {
        let cell_y0 = ((low_bounds.y0 + ly as i32) * factor).max(bounds.y0);
        let cell_y1 = ((low_bounds.y0 + ly as i32 + 1) * factor).min(bounds.y1);
        for lx in 0..low.width {
            let cell_x0 = ((low_bounds.x0 + lx as i32) * factor).max(bounds.x0);
            let cell_x1 = ((low_bounds.x0 + lx as i32 + 1) * factor).min(bounds.x1);
            low.pixels[(ly * low.width + lx) as usize] = match filter {
                BlurDownsampleFilter::Nearest => {
                    let sx = ((cell_x0 + cell_x1 - 1) / 2).clamp(bounds.x0, bounds.x1 - 1);
                    let sy = ((cell_y0 + cell_y1 - 1) / 2).clamp(bounds.y0, bounds.y1 - 1);
                    source.pixels[((sy - bounds.y0) as u32 * source.width + (sx - bounds.x0) as u32)
                        as usize]
                }
                BlurDownsampleFilter::Box => {
                    let mut acc = [0.0; 4];
                    let mut count = 0.0;
                    for sy in cell_y0..cell_y1 {
                        for sx in cell_x0..cell_x1 {
                            let ix = ((sy - bounds.y0) as u32 * source.width
                                + (sx - bounds.x0) as u32)
                                as usize;
                            let px = unpack_premul_rgba8(source.pixels[ix]);
                            for channel in 0..4 {
                                acc[channel] += px[channel];
                            }
                            count += 1.0;
                        }
                    }
                    if count > 0.0 {
                        for channel in &mut acc {
                            *channel /= count;
                        }
                    }
                    pack_premul_rgba8(acc)
                }
            };
        }
    }
    low
}

fn upsample_image(
    low: &Image,
    bounds: Bounds,
    low_bounds: Bounds,
    factor: u32,
    filter: BlurUpsampleFilter,
    width: u32,
    height: u32,
) -> Image {
    let mut out = Image::new(width, height, peniko::Color::TRANSPARENT);
    let factor = factor as f32;
    for y in 0..height {
        let low_y = (((bounds.y0 as f32 + y as f32 + 0.5) / factor - 0.5)
            .clamp(low_bounds.y0 as f32, low_bounds.y1.saturating_sub(1) as f32))
            - low_bounds.y0 as f32;
        let y0 = low_y
            .floor()
            .clamp(0.0, low.height.saturating_sub(1) as f32) as u32;
        let y1 = (y0 + 1).min(low.height.saturating_sub(1));
        let ty = (low_y - low_y.floor()).clamp(0.0, 1.0);
        for x in 0..width {
            let low_x = (((bounds.x0 as f32 + x as f32 + 0.5) / factor - 0.5)
                .clamp(low_bounds.x0 as f32, low_bounds.x1.saturating_sub(1) as f32))
                - low_bounds.x0 as f32;
            let x0 = low_x.floor().clamp(0.0, low.width.saturating_sub(1) as f32) as u32;
            let x1 = (x0 + 1).min(low.width.saturating_sub(1));
            let tx = (low_x - low_x.floor()).clamp(0.0, 1.0);
            out.pixels[(y * width + x) as usize] = match filter {
                BlurUpsampleFilter::Nearest => {
                    let sx = low_x.round().clamp(0.0, low.width.saturating_sub(1) as f32) as u32;
                    let sy = low_y
                        .round()
                        .clamp(0.0, low.height.saturating_sub(1) as f32)
                        as u32;
                    pixel_at(low, sx, sy)
                }
                BlurUpsampleFilter::Bilinear => {
                    let top = lerp_premul_pixel(pixel_at(low, x0, y0), pixel_at(low, x1, y0), tx);
                    let bottom =
                        lerp_premul_pixel(pixel_at(low, x0, y1), pixel_at(low, x1, y1), tx);
                    lerp_premul_pixel(top, bottom, ty)
                }
            };
        }
    }
    out
}

fn pixel_at(image: &Image, x: u32, y: u32) -> u32 {
    image.pixels[(y * image.width + x) as usize]
}

fn lerp_premul_pixel(a: u32, b: u32, t: f32) -> u32 {
    let a = unpack_premul_rgba8(a);
    let b = unpack_premul_rgba8(b);
    pack_premul_rgba8([
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
        lerp(a[3], b[3], t),
    ])
}

fn apply_drop_shadow(
    image: &mut Image,
    bounds: Bounds,
    offset_x: f32,
    offset_y: f32,
    std_dev: f32,
    brush: &crate::shared::brush::Brush,
    image_resources: Option<&ImageResourceStore>,
) {
    // Drop-shadow is a filter over the source alpha: offset the alpha mask,
    // blur it, color it, then composite the original source back on top.
    let source = image.clone();
    let dx = offset_x.round() as i32;
    let dy = offset_y.round() as i32;
    let mut mask = Image::new(image.width, image.height, peniko::Color::TRANSPARENT);

    for y in 0..source.height as i32 {
        for x in 0..source.width as i32 {
            let tx = x + dx;
            let ty = y + dy;
            if tx < 0 || ty < 0 || tx >= source.width as i32 || ty >= source.height as i32 {
                continue;
            }
            let alpha = source.rgba8_at(x as u32, y as u32)[3];
            let ix = (ty as u32 * source.width + tx as u32) as usize;
            mask.pixels[ix] = rgba8_pack([alpha, alpha, alpha, alpha]);
        }
    }

    apply_gaussian_blur(&mut mask, std_dev, std_dev);
    for y in 0..image.height {
        for x in 0..image.width {
            let ix = (y * image.width + x) as usize;
            let alpha = f32::from(mask.rgba8_at(x, y)[3]) / 255.0;
            let mut shadow = unpack_premul_rgba8(brush.sample_with_resources(
                bounds.x0 as f32 + x as f32 + 0.5,
                bounds.y0 as f32 + y as f32 + 0.5,
                image_resources,
            ));
            shadow[0] *= alpha;
            shadow[1] *= alpha;
            shadow[2] *= alpha;
            shadow[3] *= alpha;
            let src = unpack_premul_rgba8(source.pixels[ix]);
            image.pixels[ix] = pack_premul_rgba8(src_over_premul(shadow, src));
        }
    }
}

#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

fn gaussian_kernel(std_dev: f32) -> Vec<f32> {
    let half_width = blur_outset(std_dev).max(1);
    let sigma = std_dev.max(0.0001);
    let two_sigma_sq = 2.0 * sigma * sigma;
    let mut kernel = Vec::with_capacity((half_width * 2 + 1) as usize);
    let mut sum = 0.0;
    for i in -half_width..=half_width {
        let x = i as f32;
        let w = (-x * x / two_sigma_sq).exp();
        kernel.push(w);
        sum += w;
    }
    if sum > 0.0 {
        for w in &mut kernel {
            *w /= sum;
        }
    }
    kernel
}

fn blur_pass(image: &Image, kernel: &[f32], axis: Axis) -> Vec<u32> {
    let mut out = vec![0; image.pixels.len()];
    let radius = (kernel.len() / 2) as i32;
    for y in 0..image.height {
        for x in 0..image.width {
            let mut acc = [0.0; 4];
            for (i, w) in kernel.iter().enumerate() {
                let d = i as i32 - radius;
                let sx = match axis {
                    Axis::Horizontal => x as i32 + d,
                    Axis::Vertical => x as i32,
                };
                let sy = match axis {
                    Axis::Horizontal => y as i32,
                    Axis::Vertical => y as i32 + d,
                };
                if sx < 0 || sy < 0 || sx >= image.width as i32 || sy >= image.height as i32 {
                    continue;
                }
                let ix = (sy as u32 * image.width + sx as u32) as usize;
                let px = unpack_premul_rgba8(image.pixels[ix]);
                for c in 0..4 {
                    acc[c] += px[c] * *w;
                }
            }
            out[(y * image.width + x) as usize] = pack_premul_rgba8(acc);
        }
    }
    out
}

fn blur_outset(std_dev: f32) -> i32 {
    (std_dev.max(0.0) * 3.0).ceil() as i32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lum(c: [f32; 3]) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::shared::brush::{IDENTITY_TRANSFORM, PatternBrush, PatternImage, PatternSampling};
    use crate::shared::layer::filter::{
        COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, CompositeOperator,
        FilterInput, FilterPrimitive, FilterPrimitiveKind,
    };
    use peniko::{Color, Extend, Mix};

    fn test_turbulence(kind: TurbulenceKind, seed: i32, num_octaves: u32) -> Turbulence {
        Turbulence {
            base_frequency_x: 0.25,
            base_frequency_y: 0.25,
            num_octaves,
            seed,
            stitch_tiles: false,
            kind,
            linear_rgb: false,
            transform_x: 0.0,
            transform_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            tile_x: 0.0,
            tile_y: 0.0,
            tile_width: 4.0,
            tile_height: 4.0,
        }
    }

    fn test_displacement_map(x_channel: ColorChannel, y_channel: ColorChannel) -> DisplacementMap {
        DisplacementMap {
            scale_x: 2.0,
            scale_y: 0.0,
            x_channel,
            y_channel,
            linear_rgb: false,
        }
    }

    #[test]
    fn drop_shadow_offsets_alpha_and_preserves_source() {
        let mut image = Image::new(8, 8, Color::TRANSPARENT);
        image.pixels[(2 * 8 + 2) as usize] = rgba8_pack([255, 255, 255, 255]);

        apply(
            &mut image,
            &Filter::DropShadow {
                offset_x: 2.0,
                offset_y: 1.0,
                std_dev: 0.0,
                brush: crate::shared::brush::Brush::Solid(Color::BLACK),
            },
            Bounds::canvas(8, 8),
        );

        assert_eq!(image.rgba8_at(2, 2), [255, 255, 255, 255]);
        assert_eq!(image.rgba8_at(4, 3), [0, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn offset_shifts_pixels_and_clears_uncovered_area() {
        let mut image = Image::new(4, 4, Color::TRANSPARENT);
        image.pixels[5] = rgba8_pack([255, 0, 0, 255]);

        apply(
            &mut image,
            &Filter::Offset { dx: 1.0, dy: -1.0 },
            Bounds::canvas(4, 4),
        );

        assert_eq!(image.rgba8_at(2, 0), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn offset_half_pixel_keeps_center_edge_ownership() {
        let mut image = Image::new(4, 1, Color::TRANSPARENT);
        image.pixels[1] = rgba8_pack([255, 0, 0, 255]);

        apply(
            &mut image,
            &Filter::Offset { dx: 1.5, dy: 0.0 },
            Bounds::canvas(4, 1),
        );

        assert_eq!(image.rgba8_at(2, 0), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(3, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn flood_replaces_filter_buffer_with_sampled_brush() {
        let mut image = Image::new(3, 2, Color::from_rgb8(255, 0, 0));

        apply(
            &mut image,
            &Filter::Flood {
                brush: Brush::Solid(Color::from_rgb8(0, 255, 0)),
            },
            Bounds::new(10, 20, 13, 22),
        );

        assert!(
            image
                .pixels
                .iter()
                .all(|px| *px == rgba8_pack([0, 255, 0, 255]))
        );
    }

    #[test]
    fn color_matrix_runs_on_unpremultiplied_channels() {
        let mut image = Image::new(1, 1, Color::TRANSPARENT);
        image.pixels[0] = rgba8_pack([128, 0, 0, 128]);

        apply(
            &mut image,
            &Filter::ColorMatrix([
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
            ]),
            Bounds::canvas(1, 1),
        );

        assert_eq!(image.rgba8_at(0, 0), [0, 0, 128, 128]);
    }

    #[test]
    fn component_transfer_runs_on_unpremultiplied_channels() {
        let mut table = Box::new([0; COMPONENT_TRANSFER_TABLE_LEN]);
        for i in 0..256 {
            table[i] = i as u32;
            table[COMPONENT_TRANSFER_TABLE_SIZE + i] = 255 - i as u32;
            table[2 * COMPONENT_TRANSFER_TABLE_SIZE + i] = 255;
            table[3 * COMPONENT_TRANSFER_TABLE_SIZE + i] = (i / 2) as u32;
        }
        let mut image = Image::new(1, 1, Color::TRANSPARENT);
        image.pixels[0] = rgba8_pack([128, 64, 0, 128]);

        apply(
            &mut image,
            &Filter::ComponentTransfer(table),
            Bounds::canvas(1, 1),
        );

        assert_eq!(image.rgba8_at(0, 0), [64, 32, 64, 64]);
    }

    #[test]
    fn anisotropic_blur_can_run_only_one_axis() {
        let mut image = Image::new(3, 3, Color::TRANSPARENT);
        image.pixels[(image.width + 1) as usize] = rgba8_pack([255, 255, 255, 255]);

        apply(
            &mut image,
            &Filter::Blur {
                std_dev_x: 1.0,
                std_dev_y: 0.0,
                sampling: BlurSampling::downsampled(4),
            },
            Bounds::canvas(3, 3),
        );

        assert!(image.rgba8_at(0, 1)[3] > 0);
        assert!(image.rgba8_at(1, 1)[3] > 0);
        assert!(image.rgba8_at(2, 1)[3] > 0);
        assert_eq!(image.rgba8_at(1, 0), [0, 0, 0, 0]);
        assert_eq!(image.rgba8_at(1, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn downsampled_blur_spreads_source_pixels() {
        let mut image = Image::new(6, 6, Color::TRANSPARENT);
        image.pixels[(2 * image.width + 2) as usize] = rgba8_pack([255, 255, 255, 255]);

        apply(
            &mut image,
            &Filter::Blur {
                std_dev_x: 2.0,
                std_dev_y: 2.0,
                sampling: BlurSampling::downsampled(2),
            },
            Bounds::canvas(6, 6),
        );

        assert!(image.rgba8_at(2, 2)[3] > 0);
        assert!(image.rgba8_at(5, 5)[3] > 0);
    }

    #[test]
    fn downsampled_blur_sampling_modes_change_reconstruction() {
        let mut nearest = Image::new(8, 8, Color::TRANSPARENT);
        nearest.pixels[(nearest.width + 1) as usize] = rgba8_pack([255, 255, 255, 255]);
        let mut box_filtered = nearest.clone();

        apply(
            &mut nearest,
            &Filter::Blur {
                std_dev_x: 4.0,
                std_dev_y: 4.0,
                sampling: BlurSampling {
                    factor: 4,
                    downsample_filter: BlurDownsampleFilter::Nearest,
                    upsample_filter: BlurUpsampleFilter::Nearest,
                },
            },
            Bounds::canvas(8, 8),
        );
        apply(
            &mut box_filtered,
            &Filter::Blur {
                std_dev_x: 4.0,
                std_dev_y: 4.0,
                sampling: BlurSampling::downsampled(4),
            },
            Bounds::canvas(8, 8),
        );

        assert!(nearest.rgba8_at(1, 1)[3] > box_filtered.rgba8_at(1, 1)[3]);
    }

    #[test]
    fn diffuse_lighting_uses_alpha_height_normals() {
        let mut image = Image::new(3, 1, Color::TRANSPARENT);
        image.pixels[0] = rgba8_pack([0, 0, 0, 0]);
        image.pixels[1] = rgba8_pack([0, 0, 0, 128]);
        image.pixels[2] = rgba8_pack([0, 0, 0, 255]);

        apply(
            &mut image,
            &Filter::DiffuseLighting(DiffuseLighting {
                surface_scale: 1.0,
                diffuse_constant: 1.0,
                lighting_color: [1.0, 0.0, 0.0],
                light_source: LightSource::Distant {
                    azimuth: 180.0,
                    elevation: 0.0,
                },
            }),
            Bounds::canvas(3, 1),
        );

        let center = image.rgba8_at(1, 0);
        assert!(
            center[0].abs_diff(180) <= 1 && center[1] == 0 && center[2] == 0 && center[3] == 255,
            "expected red diffuse response from alpha slope, got {center:?}"
        );
    }

    #[test]
    fn diffuse_lighting_point_light_uses_filter_bounds_as_user_space() {
        let mut image = Image::new(1, 1, Color::from_rgb8(0, 0, 0));

        apply(
            &mut image,
            &Filter::DiffuseLighting(DiffuseLighting {
                surface_scale: 0.0,
                diffuse_constant: 1.0,
                lighting_color: [0.0, 1.0, 0.0],
                light_source: LightSource::Point {
                    x: 10.5,
                    y: 20.5,
                    z: 1.0,
                },
            }),
            Bounds::new(10, 20, 11, 21),
        );

        assert_eq!(image.rgba8_at(0, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn specular_lighting_sets_alpha_to_max_component() {
        let mut image = Image::new(1, 1, Color::from_rgb8(0, 0, 0));

        apply(
            &mut image,
            &Filter::SpecularLighting(SpecularLighting {
                surface_scale: 0.0,
                specular_constant: 1.0,
                specular_exponent: 1.0,
                lighting_color: [0.5, 0.25, 0.0],
                light_source: LightSource::Point {
                    x: 10.5,
                    y: 20.5,
                    z: 1.0,
                },
            }),
            Bounds::new(10, 20, 11, 21),
        );

        assert_eq!(image.pixels[0], pack_premul_rgba8([0.5, 0.25, 0.0, 0.5]));
    }

    #[test]
    fn specular_lighting_uses_alpha_height_normals() {
        let mut image = Image::new(3, 1, Color::TRANSPARENT);
        image.pixels[0] = rgba8_pack([0, 0, 0, 0]);
        image.pixels[1] = rgba8_pack([0, 0, 0, 128]);
        image.pixels[2] = rgba8_pack([0, 0, 0, 255]);

        apply(
            &mut image,
            &Filter::SpecularLighting(SpecularLighting {
                surface_scale: 1.0,
                specular_constant: 1.0,
                specular_exponent: 1.0,
                lighting_color: [1.0, 1.0, 1.0],
                light_source: LightSource::Distant {
                    azimuth: 0.0,
                    elevation: 90.0,
                },
            }),
            Bounds::canvas(3, 1),
        );

        let center_alpha = image.rgba8_at(1, 0)[3];
        assert!(
            (180..255).contains(&center_alpha),
            "expected alpha slope to reduce specular highlight, got {center_alpha}"
        );
    }

    #[test]
    fn graph_resolves_inputs_and_clips_primitive_subregions() {
        let mut image = Image::new(4, 2, Color::from_rgb8(255, 0, 0));
        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::canvas(4, 2),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: Some(FilterInput::Primitive(0)),
                        region: Bounds::new(0, 0, 2, 2),
                        kind: FilterPrimitiveKind::Blend {
                            mode: Mix::Multiply,
                        },
                    },
                    FilterPrimitive {
                        input: FilterInput::Primitive(0),
                        input2: Some(FilterInput::SourceAlpha),
                        region: Bounds::new(2, 0, 4, 2),
                        kind: FilterPrimitiveKind::Composite {
                            operator: CompositeOperator::In,
                        },
                    },
                    FilterPrimitive {
                        input: FilterInput::Primitive(1),
                        input2: Some(FilterInput::Primitive(2)),
                        region: Bounds::canvas(4, 2),
                        kind: FilterPrimitiveKind::Composite {
                            operator: CompositeOperator::Over,
                        },
                    },
                ],
                fixed_region: true,
            },
            Bounds::canvas(4, 2),
        );

        assert_eq!(image.rgba8_at(0, 1), [0, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 1), [0, 0, 0, 255]);
        assert_eq!(image.rgba8_at(2, 1), [0, 0, 255, 255]);
        assert_eq!(image.rgba8_at(3, 1), [0, 0, 255, 255]);
    }

    #[test]
    fn graph_image_primitive_samples_absolute_brush_and_clips_region() {
        let mut image = Image::new(4, 2, Color::from_rgb8(255, 0, 0));
        let brush = Brush::Pattern(PatternBrush {
            image: PatternImage::Inline(Arc::new(Image {
                width: 2,
                height: 1,
                pixels: vec![rgba8_pack([0, 255, 0, 255]), rgba8_pack([0, 0, 255, 255])],
            })),
            transform: IDENTITY_TRANSFORM,
            extend: Extend::Pad,
            sampling: PatternSampling::Nearest,
            opacity: 255,
        });

        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceAlpha,
                    input2: None,
                    region: Bounds::new(0, 0, 2, 1),
                    kind: FilterPrimitiveKind::Image { brush },
                }],
                fixed_region: true,
            },
            Bounds::canvas(4, 2),
        );

        assert_eq!(image.rgba8_at(0, 0), [0, 255, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
        assert_eq!(image.rgba8_at(2, 0), [0, 0, 0, 0]);
        assert_eq!(image.rgba8_at(0, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn graph_turbulence_generates_deterministic_noise_and_clips_region() {
        fn render(seed: i32) -> Image {
            let mut image = Image::new(4, 4, Color::from_rgb8(255, 0, 0));
            apply(
                &mut image,
                &Filter::Graph {
                    primitives: vec![FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::new(1, 1, 3, 3),
                        kind: FilterPrimitiveKind::Turbulence(test_turbulence(
                            TurbulenceKind::Turbulence,
                            seed,
                            3,
                        )),
                    }],
                    fixed_region: true,
                },
                Bounds::canvas(4, 4),
            );
            image
        }

        let first = render(7);
        let second = render(7);
        let third = render(8);

        assert_eq!(first.pixels, second.pixels);
        assert_ne!(first.pixels, third.pixels);
        assert_eq!(first.rgba8_at(0, 0), [0, 0, 0, 0]);
        assert_eq!(first.rgba8_at(3, 3), [0, 0, 0, 0]);
        assert!(
            (1..3).any(|y| (1..3).any(|x| first.rgba8_at(x, y)[3] > 0)),
            "expected non-transparent turbulence inside primitive region"
        );
    }

    #[test]
    fn graph_turbulence_zero_octaves_handles_kind_baseline() {
        let mut turbulence = Image::new(1, 1, Color::TRANSPARENT);
        apply(
            &mut turbulence,
            &Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(1, 1),
                    kind: FilterPrimitiveKind::Turbulence(test_turbulence(
                        TurbulenceKind::Turbulence,
                        1,
                        0,
                    )),
                }],
                fixed_region: true,
            },
            Bounds::canvas(1, 1),
        );

        let mut fractal = Image::new(1, 1, Color::TRANSPARENT);
        apply(
            &mut fractal,
            &Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(1, 1),
                    kind: FilterPrimitiveKind::Turbulence(test_turbulence(
                        TurbulenceKind::FractalNoise,
                        1,
                        0,
                    )),
                }],
                fixed_region: true,
            },
            Bounds::canvas(1, 1),
        );

        assert_eq!(turbulence.rgba8_at(0, 0), [0, 0, 0, 0]);
        assert_eq!(fractal.rgba8_at(0, 0), [64, 64, 64, 128]);
    }

    #[test]
    fn graph_displacement_map_offsets_source_by_channel_value() {
        let mut image = Image {
            width: 4,
            height: 1,
            pixels: vec![
                rgba8_pack([10, 0, 0, 255]),
                rgba8_pack([20, 0, 0, 255]),
                rgba8_pack([40, 0, 0, 255]),
                rgba8_pack([80, 0, 0, 255]),
            ],
        };
        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: Some(FilterInput::SourceGraphic),
                    region: Bounds::new(0, 0, 4, 1),
                    kind: FilterPrimitiveKind::DisplacementMap(test_displacement_map(
                        ColorChannel::A,
                        ColorChannel::A,
                    )),
                }],
                fixed_region: true,
            },
            Bounds::new(0, 0, 4, 1),
        );
        assert_eq!(image.rgba8_at(0, 0), [20, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [40, 0, 0, 255]);
        assert_eq!(image.rgba8_at(2, 0), [80, 0, 0, 255]);
        assert_eq!(image.rgba8_at(3, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn graph_displacement_map_reads_rgb_map_channels_unpremultiplied() {
        let mut image = Image {
            width: 3,
            height: 1,
            pixels: vec![
                rgba8_pack([10, 0, 0, 255]),
                rgba8_pack([20, 0, 0, 255]),
                rgba8_pack([40, 0, 0, 255]),
            ],
        };
        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::new(0, 0, 3, 1),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgba8(255, 0, 0, 128)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: Some(FilterInput::Primitive(0)),
                        region: Bounds::new(0, 0, 3, 1),
                        kind: FilterPrimitiveKind::DisplacementMap(test_displacement_map(
                            ColorChannel::R,
                            ColorChannel::A,
                        )),
                    },
                ],
                fixed_region: true,
            },
            Bounds::new(0, 0, 3, 1),
        );
        assert_eq!(image.rgba8_at(0, 0), [20, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [40, 0, 0, 255]);
        assert_eq!(image.rgba8_at(2, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn graph_tile_repeats_source_region_and_clips_output_region() {
        let mut image = Image::new(6, 4, Color::TRANSPARENT);
        image.pixels[(6 + 1) as usize] = rgba8_pack([255, 0, 0, 255]);
        image.pixels[(6 + 2) as usize] = rgba8_pack([0, 255, 0, 255]);
        image.pixels[(2 * 6 + 1) as usize] = rgba8_pack([0, 0, 255, 255]);
        image.pixels[(2 * 6 + 2) as usize] = rgba8_pack([255, 255, 0, 255]);

        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::new(0, 0, 6, 4),
                    kind: FilterPrimitiveKind::Tile {
                        source_region: Bounds::new(1, 1, 3, 3),
                    },
                }],
                fixed_region: true,
            },
            Bounds::canvas(6, 4),
        );

        assert_eq!(image.rgba8_at(0, 0), [255, 255, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [0, 0, 255, 255]);
        assert_eq!(image.rgba8_at(4, 1), [0, 255, 0, 255]);
        assert_eq!(image.rgba8_at(5, 3), [255, 0, 0, 255]);
    }

    #[test]
    fn graph_tile_empty_source_region_outputs_transparent() {
        let mut image = Image::new(3, 2, Color::from_rgb8(255, 0, 0));

        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![FilterPrimitive {
                    input: FilterInput::SourceGraphic,
                    input2: None,
                    region: Bounds::canvas(3, 2),
                    kind: FilterPrimitiveKind::Tile {
                        source_region: Bounds::new(4, 4, 5, 5),
                    },
                }],
                fixed_region: true,
            },
            Bounds::canvas(3, 2),
        );

        assert!(image.pixels.iter().all(|px| *px == 0));
    }

    #[test]
    fn graph_arithmetic_composite_runs_on_premultiplied_channels() {
        let mut image = Image::new(2, 1, Color::from_rgb8(255, 0, 0));
        image.pixels[1] = 0;
        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::canvas(2, 1),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: Some(FilterInput::Primitive(0)),
                        region: Bounds::canvas(2, 1),
                        kind: FilterPrimitiveKind::Composite {
                            operator: CompositeOperator::Arithmetic {
                                k1: 0.0,
                                k2: 0.5,
                                k3: 0.5,
                                k4: 0.0,
                            },
                        },
                    },
                ],
                fixed_region: true,
            },
            Bounds::canvas(2, 1),
        );

        assert_eq!(image.rgba8_at(0, 0), [128, 0, 128, 255]);
        assert_eq!(image.rgba8_at(1, 0), [0, 0, 128, 128]);
    }

    #[test]
    fn graph_merge_composites_inputs_in_order_and_clips_region() {
        let mut image = Image::new(4, 2, Color::from_rgb8(255, 0, 0));
        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::canvas(4, 2),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::new(0, 0, 2, 2),
                        kind: FilterPrimitiveKind::Merge {
                            inputs: vec![FilterInput::Primitive(0), FilterInput::SourceGraphic],
                        },
                    },
                ],
                fixed_region: true,
            },
            Bounds::canvas(4, 2),
        );

        assert_eq!(image.rgba8_at(0, 1), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 1), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(2, 1), [0, 0, 0, 0]);
        assert_eq!(image.rgba8_at(3, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn morphology_dilate_expands_straight_rgba() {
        let mut image = Image::new(3, 3, Color::TRANSPARENT);
        image.pixels[4] = rgba8_pack([128, 0, 0, 128]);

        apply(
            &mut image,
            &Filter::Morphology {
                radius_x: 1.0,
                radius_y: 1.0,
                operator: MorphologyOperator::Dilate,
            },
            Bounds::canvas(3, 3),
        );

        assert_eq!(image.rgba8_at(0, 0), [128, 0, 0, 128]);
        assert_eq!(image.rgba8_at(1, 1), [128, 0, 0, 128]);
        assert_eq!(image.rgba8_at(2, 2), [128, 0, 0, 128]);
    }

    #[test]
    fn morphology_erode_treats_filter_buffer_edges_as_transparent() {
        let mut image = Image::new(3, 3, Color::from_rgb8(255, 0, 0));

        apply(
            &mut image,
            &Filter::Morphology {
                radius_x: 1.0,
                radius_y: 1.0,
                operator: MorphologyOperator::Erode,
            },
            Bounds::canvas(3, 3),
        );

        assert_eq!(image.rgba8_at(1, 1), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(0, 1), [0, 0, 0, 0]);
        assert_eq!(image.rgba8_at(1, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn morphology_handles_radius_larger_than_image() {
        let mut erode = Image::new(4, 3, Color::from_rgb8(255, 0, 0));
        apply(
            &mut erode,
            &Filter::Morphology {
                radius_x: 9999.0,
                radius_y: 9999.0,
                operator: MorphologyOperator::Erode,
            },
            Bounds::canvas(4, 3),
        );
        assert!(erode.pixels.iter().all(|pixel| *pixel == 0));

        let mut dilate = Image::new(4, 3, Color::TRANSPARENT);
        dilate.pixels[6] = rgba8_pack([0, 128, 0, 128]);
        apply(
            &mut dilate,
            &Filter::Morphology {
                radius_x: 9999.0,
                radius_y: 9999.0,
                operator: MorphologyOperator::Dilate,
            },
            Bounds::canvas(4, 3),
        );
        assert!(
            dilate
                .pixels
                .iter()
                .all(|pixel| *pixel == rgba8_pack([0, 128, 0, 128]))
        );
    }

    #[test]
    fn convolve_matrix_flips_kernel_and_respects_target() {
        let mut image = Image::new(3, 1, Color::TRANSPARENT);
        image.pixels = vec![
            rgba8_pack([10, 0, 0, 255]),
            rgba8_pack([20, 0, 0, 255]),
            rgba8_pack([40, 0, 0, 255]),
        ];

        apply(
            &mut image,
            &Filter::ConvolveMatrix(ConvolveMatrix {
                columns: 3,
                rows: 1,
                target_x: 1,
                target_y: 0,
                data: vec![1.0, 0.0, 0.0],
                divisor: 1.0,
                bias: 0.0,
                edge_mode: ConvolveEdgeMode::Duplicate,
                preserve_alpha: false,
            }),
            Bounds::canvas(3, 1),
        );

        assert_eq!(image.rgba8_at(0, 0), [20, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 0), [40, 0, 0, 255]);
        assert_eq!(image.rgba8_at(2, 0), [40, 0, 0, 255]);
    }

    #[test]
    fn convolve_matrix_edge_modes_control_out_of_bounds_sampling() {
        let filter = |edge_mode| {
            let mut image = Image::new(3, 1, Color::TRANSPARENT);
            image.pixels = vec![
                rgba8_pack([10, 0, 0, 255]),
                rgba8_pack([20, 0, 0, 255]),
                rgba8_pack([40, 0, 0, 255]),
            ];
            apply(
                &mut image,
                &Filter::ConvolveMatrix(ConvolveMatrix {
                    columns: 3,
                    rows: 1,
                    target_x: 1,
                    target_y: 0,
                    data: vec![1.0, 0.0, 0.0],
                    divisor: 1.0,
                    bias: 0.0,
                    edge_mode,
                    preserve_alpha: false,
                }),
                Bounds::canvas(3, 1),
            );
            image
        };

        assert_eq!(filter(ConvolveEdgeMode::None).rgba8_at(2, 0), [0, 0, 0, 0]);
        assert_eq!(
            filter(ConvolveEdgeMode::Wrap).rgba8_at(2, 0),
            [10, 0, 0, 255]
        );
    }

    #[test]
    fn convolve_matrix_preserve_alpha_keeps_source_alpha() {
        let mut image = Image::new(1, 1, Color::TRANSPARENT);
        image.pixels[0] = rgba8_pack([64, 0, 0, 128]);

        apply(
            &mut image,
            &Filter::ConvolveMatrix(ConvolveMatrix {
                columns: 1,
                rows: 1,
                target_x: 0,
                target_y: 0,
                data: vec![1.0],
                divisor: 1.0,
                bias: 0.25,
                edge_mode: ConvolveEdgeMode::Duplicate,
                preserve_alpha: true,
            }),
            Bounds::canvas(1, 1),
        );

        assert_eq!(image.rgba8_at(0, 0), [96, 32, 32, 128]);
    }
}
