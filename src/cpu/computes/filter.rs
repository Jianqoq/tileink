use peniko::{Compose, Mix, kurbo::Shape};

use crate::shared::{
    bounds::Bounds,
    brush::Brush,
    image::{Image, rgba8_pack, unpack_rgba8},
    layer::{
        blend::{Blend, src_over_premul},
        filter::{
            COMPONENT_TRANSFER_TABLE_SIZE, ComponentTransferTable, CompositeOperator,
            ConvolveEdgeMode, ConvolveMatrix, DiffuseLighting, Filter, FilterInput,
            FilterPrimitive, FilterPrimitiveKind, LightSource, MorphologyOperator,
            SpecularLighting,
        },
        region::Region,
    },
    pixel::{pack_premul_rgba8, unpack_premul_rgba8},
};

pub(crate) fn apply(image: &mut Image, filter: &Filter, bounds: Bounds) {
    match filter {
        Filter::Chain { filters, .. } => {
            for filter in filters {
                apply(image, filter, bounds);
            }
        }
        Filter::Graph { primitives, .. } => apply_graph(image, primitives, bounds),
        Filter::Blur { radius_x, radius_y } => apply_gaussian_blur(image, *radius_x, *radius_y),
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
        Filter::Flood { brush } => apply_flood(image, bounds, brush),
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
        Filter::Offset { dx, dy } => apply_offset(image, dx.round() as i32, dy.round() as i32),
        Filter::Morphology {
            radius_x,
            radius_y,
            operator,
        } => apply_morphology(image, *radius_x, *radius_y, *operator),
        Filter::DropShadow {
            offset_x,
            offset_y,
            radius,
            brush,
        } => apply_drop_shadow(image, bounds, *offset_x, *offset_y, *radius, brush),
    }
}

fn apply_graph(image: &mut Image, primitives: &[FilterPrimitive], bounds: Bounds) {
    let source_graphic = image.clone();
    let source_alpha = source_alpha_image(&source_graphic);
    let mut outputs = Vec::with_capacity(primitives.len());

    for primitive in primitives {
        let output =
            apply_graph_primitive(primitive, bounds, &source_graphic, &source_alpha, &outputs);
        outputs.push(output);
    }

    if let Some(output) = outputs.pop() {
        *image = output;
    } else {
        image.pixels.fill(0);
    }
}

fn apply_graph_primitive(
    primitive: &FilterPrimitive,
    bounds: Bounds,
    source_graphic: &Image,
    source_alpha: &Image,
    outputs: &[Image],
) -> Image {
    // Primitive subregions clip only the primitive output. Inputs still sample
    // from the full filter bounds, which is required for blur/offset chains.
    let region = primitive.region.intersect(bounds);
    let input = resolve_graph_input(primitive.input, source_graphic, source_alpha, outputs);
    match &primitive.kind {
        FilterPrimitiveKind::Identity => clipped_image(input, bounds, region),
        FilterPrimitiveKind::Filter(filter) => {
            let mut image = input.clone();
            apply(&mut image, filter, bounds);
            clipped_image(&image, bounds, region)
        }
        FilterPrimitiveKind::Blend { mode } => {
            let input2 =
                resolve_required_graph_input(primitive, source_graphic, source_alpha, outputs);
            blend_images(input, input2, bounds, region, *mode)
        }
        FilterPrimitiveKind::Composite { operator } => {
            let input2 =
                resolve_required_graph_input(primitive, source_graphic, source_alpha, outputs);
            composite_images(input, input2, bounds, region, *operator)
        }
        FilterPrimitiveKind::Merge { inputs } => merge_images(
            inputs,
            source_graphic,
            source_alpha,
            outputs,
            bounds,
            region,
        ),
    }
}

fn resolve_required_graph_input<'a>(
    primitive: &'a FilterPrimitive,
    source_graphic: &'a Image,
    source_alpha: &'a Image,
    outputs: &'a [Image],
) -> &'a Image {
    resolve_graph_input(
        primitive
            .input2
            .expect("dual-input filter primitive is missing input2"),
        source_graphic,
        source_alpha,
        outputs,
    )
}

fn resolve_graph_input<'a>(
    input: FilterInput,
    source_graphic: &'a Image,
    source_alpha: &'a Image,
    outputs: &'a [Image],
) -> &'a Image {
    match input {
        FilterInput::SourceGraphic => source_graphic,
        FilterInput::SourceAlpha => source_alpha,
        FilterInput::Primitive(index) => &outputs[index],
    }
}

fn source_alpha_image(source: &Image) -> Image {
    Image {
        width: source.width,
        height: source.height,
        pixels: source.pixels.iter().map(|px| px & 0xff00_0000).collect(),
    }
}

fn clipped_image(source: &Image, bounds: Bounds, region: Bounds) -> Image {
    let mut image = Image::new(source.width, source.height, peniko::Color::TRANSPARENT);
    copy_region_pixels(source, &mut image, bounds, region);
    image
}

fn copy_region_pixels(source: &Image, target: &mut Image, bounds: Bounds, region: Bounds) {
    if region.is_empty() {
        return;
    }
    for y in region.y0..region.y1 {
        let local_y = (y - bounds.y0) as u32;
        for x in region.x0..region.x1 {
            let local_x = (x - bounds.x0) as u32;
            let ix = (local_y * source.width + local_x) as usize;
            target.pixels[ix] = source.pixels[ix];
        }
    }
}

fn blend_images(
    input1: &Image,
    input2: &Image,
    bounds: Bounds,
    region: Bounds,
    mode: Mix,
) -> Image {
    let mut image = Image::new(input1.width, input1.height, peniko::Color::TRANSPARENT);
    let blend = Blend::new(mode, Compose::SrcOver);
    for_each_region_pixel(bounds, region, input1.width, |ix| {
        image.pixels[ix] = blend.blend_pixel(input1.pixels[ix], input2.pixels[ix]);
    });
    image
}

fn composite_images(
    input1: &Image,
    input2: &Image,
    bounds: Bounds,
    region: Bounds,
    operator: CompositeOperator,
) -> Image {
    let mut image = Image::new(input1.width, input1.height, peniko::Color::TRANSPARENT);
    for_each_region_pixel(bounds, region, input1.width, |ix| {
        image.pixels[ix] = composite_pixel(input1.pixels[ix], input2.pixels[ix], operator);
    });
    image
}

fn merge_images(
    inputs: &[FilterInput],
    source_graphic: &Image,
    source_alpha: &Image,
    outputs: &[Image],
    bounds: Bounds,
    region: Bounds,
) -> Image {
    let mut image = Image::new(
        source_graphic.width,
        source_graphic.height,
        peniko::Color::TRANSPARENT,
    );
    let blend = Blend::new(Mix::Normal, Compose::SrcOver);
    for input in inputs {
        let source = resolve_graph_input(*input, source_graphic, source_alpha, outputs);
        for_each_region_pixel(bounds, region, source.width, |ix| {
            image.pixels[ix] = blend.blend_pixel(source.pixels[ix], image.pixels[ix]);
        });
    }
    image
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
    let a = straight_rgba8(input1);
    let b = straight_rgba8(input2);
    let out = [
        arithmetic_channel(a[0], b[0], k1, k2, k3, k4),
        arithmetic_channel(a[1], b[1], k1, k2, k3, k4),
        arithmetic_channel(a[2], b[2], k1, k2, k3, k4),
        arithmetic_channel(a[3], b[3], k1, k2, k3, k4),
    ];
    let alpha = out[3];
    pack_premul_rgba8([out[0] * alpha, out[1] * alpha, out[2] * alpha, alpha])
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

pub(crate) fn filtered_region_bounds(
    filter: &Filter,
    sample_region: &Region,
    canvas: Bounds,
) -> Bounds {
    let bounds = region_bounds(sample_region);
    let outset = filter_outset(filter);
    bounds.outset(outset).intersect(canvas)
}

fn filter_outset(filter: &Filter) -> i32 {
    match filter {
        Filter::Chain {
            filters,
            fixed_region,
        } => {
            if *fixed_region {
                0
            } else {
                filters.iter().map(filter_outset).sum()
            }
        }
        Filter::Graph { .. } => 0,
        Filter::Blur { radius_x, radius_y } => blur_outset(radius_x.max(*radius_y)),
        Filter::Offset { dx, dy } => dx.abs().ceil().max(dy.abs().ceil()) as i32,
        Filter::Morphology {
            radius_x,
            radius_y,
            operator,
        } => match operator {
            MorphologyOperator::Erode => 0,
            MorphologyOperator::Dilate => (*radius_x).max(*radius_y).max(0.0).ceil() as i32,
        },
        Filter::DropShadow {
            radius,
            offset_x,
            offset_y,
            ..
        } => blur_outset(*radius) + offset_x.abs().ceil().max(offset_y.abs().ceil()) as i32,
        _ => 0,
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

fn apply_flood(image: &mut Image, bounds: Bounds, brush: &Brush) {
    for y in 0..image.height {
        for x in 0..image.width {
            image.pixels[(y * image.width + x) as usize] = brush.sample(
                bounds.x0 as f32 + x as f32 + 0.5,
                bounds.y0 as f32 + y as f32 + 0.5,
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

fn apply_gaussian_blur(image: &mut Image, radius_x: f32, radius_y: f32) {
    let radius_x = radius_x.max(0.0);
    let radius_y = radius_y.max(0.0);
    if (radius_x <= 0.0 && radius_y <= 0.0) || image.width == 0 || image.height == 0 {
        return;
    }
    // SVG feGaussianBlur has independent X/Y standard deviations; a zero axis
    // is intentionally skipped so `stdDeviation="5 0"` stays horizontal-only.
    if radius_x > 0.0 {
        let kernel = gaussian_kernel(radius_x);
        if kernel.len() > 1 {
            image.pixels = blur_pass(image, &kernel, Axis::Horizontal);
        }
    }
    if radius_y > 0.0 {
        let kernel = gaussian_kernel(radius_y);
        if kernel.len() > 1 {
            image.pixels = blur_pass(image, &kernel, Axis::Vertical);
        }
    }
}

fn apply_drop_shadow(
    image: &mut Image,
    bounds: Bounds,
    offset_x: f32,
    offset_y: f32,
    radius: f32,
    brush: &crate::shared::brush::Brush,
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

    apply_gaussian_blur(&mut mask, radius, radius);
    for y in 0..image.height {
        for x in 0..image.width {
            let ix = (y * image.width + x) as usize;
            let alpha = f32::from(mask.rgba8_at(x, y)[3]) / 255.0;
            let mut shadow = unpack_premul_rgba8(brush.sample(
                bounds.x0 as f32 + x as f32 + 0.5,
                bounds.y0 as f32 + y as f32 + 0.5,
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

fn gaussian_kernel(radius: f32) -> Vec<f32> {
    let half_width = blur_outset(radius).max(1);
    let sigma = radius.max(0.0001);
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

fn region_bounds(region: &Region) -> Bounds {
    match region {
        Region::Rect { rect, .. } => Bounds::new(
            rect.x0.floor() as i32,
            rect.y0.floor() as i32,
            rect.x1.ceil() as i32,
            rect.y1.ceil() as i32,
        ),
        Region::Path {
            path, transform, ..
        } => {
            let rect = transform.transform_rect_bbox(path.bounding_box());
            Bounds::new(
                rect.x0.floor() as i32,
                rect.y0.floor() as i32,
                rect.x1.ceil() as i32,
                rect.y1.ceil() as i32,
            )
        }
    }
}

fn blur_outset(radius: f32) -> i32 {
    (radius.max(0.0) * 3.0).ceil() as i32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lum(c: [f32; 3]) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::layer::filter::{
        COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE, CompositeOperator,
        FilterInput, FilterPrimitive, FilterPrimitiveKind,
    };
    use peniko::{Color, Mix};

    #[test]
    fn drop_shadow_offsets_alpha_and_preserves_source() {
        let mut image = Image::new(8, 8, Color::TRANSPARENT);
        image.pixels[(2 * 8 + 2) as usize] = rgba8_pack([255, 255, 255, 255]);

        apply(
            &mut image,
            &Filter::DropShadow {
                offset_x: 2.0,
                offset_y: 1.0,
                radius: 0.0,
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
        image.pixels[(1 * 4 + 1) as usize] = rgba8_pack([255, 0, 0, 255]);

        apply(
            &mut image,
            &Filter::Offset { dx: 1.0, dy: -1.0 },
            Bounds::canvas(4, 4),
        );

        assert_eq!(image.rgba8_at(2, 0), [255, 0, 0, 255]);
        assert_eq!(image.rgba8_at(1, 1), [0, 0, 0, 0]);
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
                radius_x: 1.0,
                radius_y: 0.0,
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
            center_alpha < 255 && center_alpha >= 180,
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
    fn graph_arithmetic_composite_runs_on_straight_channels() {
        let mut image = Image::new(1, 1, Color::from_rgb8(255, 0, 0));
        apply(
            &mut image,
            &Filter::Graph {
                primitives: vec![
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: None,
                        region: Bounds::canvas(1, 1),
                        kind: FilterPrimitiveKind::Filter(Box::new(Filter::Flood {
                            brush: Brush::Solid(Color::from_rgb8(0, 0, 255)),
                        })),
                    },
                    FilterPrimitive {
                        input: FilterInput::SourceGraphic,
                        input2: Some(FilterInput::Primitive(0)),
                        region: Bounds::canvas(1, 1),
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
            Bounds::canvas(1, 1),
        );

        assert_eq!(image.rgba8_at(0, 0), [128, 0, 128, 255]);
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
        image.pixels[(1 * 3 + 1) as usize] = rgba8_pack([128, 0, 0, 128]);

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
        dilate.pixels[(1 * 4 + 2) as usize] = rgba8_pack([0, 128, 0, 128]);
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
