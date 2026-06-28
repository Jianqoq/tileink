use peniko::kurbo::Shape;

use crate::shared::{
    bounds::Bounds,
    brush::Brush,
    image::{Image, rgba8_pack},
    layer::{
        blend::src_over_premul,
        filter::{COMPONENT_TRANSFER_TABLE_SIZE, ComponentTransferTable, Filter},
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
        Filter::Blur(radius) => apply_gaussian_blur(image, *radius),
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
        Filter::DropShadow {
            offset_x,
            offset_y,
            radius,
            brush,
        } => apply_drop_shadow(image, bounds, *offset_x, *offset_y, *radius, brush),
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
        Filter::Blur(radius) => blur_outset(*radius),
        Filter::Offset { dx, dy } => dx.abs().ceil().max(dy.abs().ceil()) as i32,
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

fn apply_gaussian_blur(image: &mut Image, radius: f32) {
    let radius = radius.max(0.0);
    if radius <= 0.0 || image.width == 0 || image.height == 0 {
        return;
    }
    let kernel = gaussian_kernel(radius);
    if kernel.len() <= 1 {
        return;
    }
    let tmp = blur_pass(image, &kernel, Axis::Horizontal);
    image.pixels = blur_pass(
        &Image {
            width: image.width,
            height: image.height,
            pixels: tmp,
        },
        &kernel,
        Axis::Vertical,
    );
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

    apply_gaussian_blur(&mut mask, radius);
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
        COMPONENT_TRANSFER_TABLE_LEN, COMPONENT_TRANSFER_TABLE_SIZE,
    };
    use peniko::Color;

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
}
