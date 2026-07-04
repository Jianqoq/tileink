use super::*;

pub(super) fn apply(
    image: &mut Image,
    primitives: &[FilterPrimitive],
    bounds: Bounds,
    image_resources: Option<&ImageResourceStore>,
) {
    let source_graphic = image.clone();
    let source_alpha = source_alpha_image(&source_graphic);
    let mut outputs = Vec::with_capacity(primitives.len());

    for primitive in primitives {
        let output = apply_graph_primitive(
            primitive,
            bounds,
            &source_graphic,
            &source_alpha,
            &outputs,
            image_resources,
        );
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
    image_resources: Option<&ImageResourceStore>,
) -> Image {
    // Primitive subregions clip only the primitive output. Inputs still sample
    // from the full filter bounds, which is required for blur/offset chains.
    let region = primitive.region.intersect(bounds);
    match &primitive.kind {
        FilterPrimitiveKind::Image { brush } => brush_image(
            source_graphic.width,
            source_graphic.height,
            bounds,
            region,
            brush,
            image_resources,
        ),
        FilterPrimitiveKind::Identity => {
            let input = resolve_graph_input(primitive.input, source_graphic, source_alpha, outputs);
            clipped_image(input, bounds, region)
        }
        FilterPrimitiveKind::Filter(filter) => {
            let input = resolve_graph_input(primitive.input, source_graphic, source_alpha, outputs);
            let mut image = input.clone();
            super::apply_with_resources(&mut image, filter, bounds, image_resources);
            clipped_image(&image, bounds, region)
        }
        FilterPrimitiveKind::Blend { mode } => {
            let input = resolve_graph_input(primitive.input, source_graphic, source_alpha, outputs);
            let input2 =
                resolve_required_graph_input(primitive, source_graphic, source_alpha, outputs);
            blend_images(input, input2, bounds, region, *mode)
        }
        FilterPrimitiveKind::Composite { operator } => {
            let input = resolve_graph_input(primitive.input, source_graphic, source_alpha, outputs);
            let input2 =
                resolve_required_graph_input(primitive, source_graphic, source_alpha, outputs);
            composite_images(input, input2, bounds, region, *operator)
        }
        FilterPrimitiveKind::DisplacementMap(displacement) => {
            let input = resolve_graph_input(primitive.input, source_graphic, source_alpha, outputs);
            let input2 =
                resolve_required_graph_input(primitive, source_graphic, source_alpha, outputs);
            displacement_map_image(input, input2, bounds, region, displacement)
        }
        FilterPrimitiveKind::Tile { source_region } => {
            let input = resolve_graph_input(primitive.input, source_graphic, source_alpha, outputs);
            tile_image(input, bounds, region, *source_region)
        }
        FilterPrimitiveKind::Turbulence(turbulence) => turbulence::image(
            source_graphic.width,
            source_graphic.height,
            bounds,
            region,
            turbulence,
        ),
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

fn brush_image(
    width: u32,
    height: u32,
    bounds: Bounds,
    region: Bounds,
    brush: &Brush,
    image_resources: Option<&ImageResourceStore>,
) -> Image {
    let mut image = Image::new(width, height, peniko::Color::TRANSPARENT);
    if region.is_empty() {
        return image;
    }
    for y in region.y0..region.y1 {
        let local_y = (y - bounds.y0) as u32;
        for x in region.x0..region.x1 {
            let local_x = (x - bounds.x0) as u32;
            image.pixels[(local_y * width + local_x) as usize] =
                brush.sample_with_resources(x as f32 + 0.5, y as f32 + 0.5, image_resources);
        }
    }
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

fn tile_image(source: &Image, bounds: Bounds, region: Bounds, source_region: Bounds) -> Image {
    let mut image = Image::new(source.width, source.height, peniko::Color::TRANSPARENT);
    let source_region = source_region.intersect(bounds);
    if region.is_empty() || source_region.is_empty() {
        return image;
    }

    let tile_width = source_region.width() as i32;
    let tile_height = source_region.height() as i32;
    for y in region.y0..region.y1 {
        let local_y = (y - bounds.y0) as u32;
        let sy = source_region.y0 + (y - source_region.y0).rem_euclid(tile_height);
        let source_y = (sy - bounds.y0) as u32;
        for x in region.x0..region.x1 {
            let local_x = (x - bounds.x0) as u32;
            let sx = source_region.x0 + (x - source_region.x0).rem_euclid(tile_width);
            let source_x = (sx - bounds.x0) as u32;
            image.pixels[(local_y * source.width + local_x) as usize] =
                source.pixels[(source_y * source.width + source_x) as usize];
        }
    }
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

fn displacement_map_image(
    source: &Image,
    map: &Image,
    bounds: Bounds,
    region: Bounds,
    displacement: &DisplacementMap,
) -> Image {
    let mut image = Image::new(source.width, source.height, peniko::Color::TRANSPARENT);
    if region.is_empty() {
        return image;
    }

    let width = source.width as i32;
    let height = source.height as i32;
    for y in region.y0..region.y1 {
        let local_y = y - bounds.y0;
        for x in region.x0..region.x1 {
            let local_x = x - bounds.x0;
            let ix = (local_y as u32 * source.width + local_x as u32) as usize;
            let dx =
                displacement_channel(map.pixels[ix], displacement.x_channel, displacement) - 0.5;
            let dy =
                displacement_channel(map.pixels[ix], displacement.y_channel, displacement) - 0.5;
            let sx = (local_x as f32 + dx * displacement.scale_x).round() as i32;
            let sy = (local_y as f32 + dy * displacement.scale_y).round() as i32;
            if sx >= 0 && sx < width && sy >= 0 && sy < height {
                image.pixels[ix] = source.pixels[(sy as u32 * source.width + sx as u32) as usize];
            }
        }
    }
    image
}

fn displacement_channel(px: u32, channel: ColorChannel, displacement: &DisplacementMap) -> f32 {
    let rgba = straight_rgba8(px);
    match channel {
        ColorChannel::R => displacement_rgb_channel(rgba[0], displacement.linear_rgb),
        ColorChannel::G => displacement_rgb_channel(rgba[1], displacement.linear_rgb),
        ColorChannel::B => displacement_rgb_channel(rgba[2], displacement.linear_rgb),
        ColorChannel::A => rgba[3],
    }
}

fn displacement_rgb_channel(value: f32, linear_rgb: bool) -> f32 {
    if linear_rgb {
        srgb_to_linear(value)
    } else {
        value
    }
}
