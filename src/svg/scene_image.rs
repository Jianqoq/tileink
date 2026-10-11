//! Image nodes and feImage lower to resources owned by the Canvas scene.
use super::{
    MAX_IMAGE_DEPTH, SvgBuilder, SvgError, SvgOptions,
    geometry::{
        affine_to_array, filter_axis_scale, inverse_affine, rect_path, transform_rect_to_bounds,
        transform_to_affine,
    },
    image::{decode_encoded_image, decode_png_image, image_sampling, svg_image_raster_size},
};
use crate::{
    Brush, Canvas, FillRule,
    shared::{
        bounds::Bounds,
        brush::{PatternBrush, PatternSampling},
        image_resource::{ImageResourceId, ImageSource},
    },
};
use peniko::{
    Extend,
    kurbo::{Affine, Rect},
};
use usvg::Node;

impl SvgBuilder {
    pub(super) fn push_image(
        &self,
        canvas: &mut Canvas,
        image: &usvg::Image,
    ) -> Result<(), SvgError> {
        if !image.is_visible() {
            return Ok(());
        }

        let transform = self.base_transform * transform_to_affine(image.abs_transform());
        let size = image.size();
        let world_to_local = inverse_affine(transform, "image transform")?;
        let source: ImageSource = match image.kind() {
            usvg::ImageKind::PNG(data) => decode_png_image(data)?.into(),
            usvg::ImageKind::JPEG(data) => {
                decode_encoded_image(data, ::image::ImageFormat::Jpeg, "jpeg image")?.into()
            }
            usvg::ImageKind::GIF(data) => {
                decode_encoded_image(data, ::image::ImageFormat::Gif, "gif image")?.into()
            }
            usvg::ImageKind::WEBP(data) => {
                decode_encoded_image(data, ::image::ImageFormat::WebP, "webp image")?.into()
            }
            usvg::ImageKind::SVG(tree) => {
                let (width, height) = svg_image_raster_size(transform, size);
                self.svg_image_to_canvas(tree, width, height)?.into()
            }
        };

        let Some(image_key) = canvas.register_scene_image(source) else {
            return Ok(());
        };
        let Some(pattern) = PatternBrush::new_resource(
            ImageResourceId::scene(image_key),
            affine_to_array(
                Affine::scale_non_uniform(
                    1.0 / f64::from(size.width()),
                    1.0 / f64::from(size.height()),
                ) * world_to_local,
            ),
            Extend::Pad,
            image_sampling(image.rendering_mode()),
            255,
        ) else {
            return Ok(());
        };
        let brush = Brush::Pattern(pattern);
        canvas.push_path(
            rect_path(Rect::new(
                0.0,
                0.0,
                size.width() as f64,
                size.height() as f64,
            )),
            brush,
            transform,
            FillRule::NonZero,
            self.options.tolerance,
        );
        Ok(())
    }

    fn svg_image_to_canvas(
        &self,
        tree: &usvg::Tree,
        width: u32,
        height: u32,
    ) -> Result<Canvas, SvgError> {
        if self.image_depth >= MAX_IMAGE_DEPTH {
            return Err(SvgError::unsupported("recursive svg image"));
        }

        let size = tree.size();
        // SVG images are vector content. Rasterizing them at their intrinsic size and then
        // scaling the bitmap loses edge coverage when the image is enlarged by the outer SVG.
        let transform = Affine::scale_non_uniform(
            f64::from(width) / f64::from(size.width()),
            f64::from(height) / f64::from(size.height()),
        );
        let mut canvas = Canvas::new(width, height, 1.0);
        SvgBuilder {
            options: SvgOptions {
                tolerance: self.options.tolerance,
                transform,
                pixel_grid: None,
            },
            base_transform: transform,
            pattern_depth: self.pattern_depth,
            image_depth: self.image_depth + 1,
        }
        .push_tree(&mut canvas, tree)?;

        // Preserve the vector scene until the chosen executor can render it on
        // its own device. SVG lowering must not create a hidden GPU context.
        Ok(canvas)
    }

    pub(super) fn filter_image_to_brush(
        &self,
        canvas: &mut Canvas,
        image: &usvg::filter::Image,
        filter_bounds: Bounds,
        primitive_rect: Rect,
        region_transform: Affine,
        content_transform: Affine,
    ) -> Result<Brush, SvgError> {
        if self.image_depth >= MAX_IMAGE_DEPTH {
            return Err(SvgError::unsupported("recursive feImage"));
        }

        let width = filter_bounds.width().max(1);
        let height = filter_bounds.height().max(1);
        let mut filter_canvas = Canvas::new(width, height, 1.0);
        if !filter_bounds.is_empty() {
            let buffer_origin =
                Affine::translate((-f64::from(filter_bounds.x0), -f64::from(filter_bounds.y0)));
            let local_transform = if fe_image_root_is_primitive_local_image(image.root()) {
                // External raster/SVG feImage content is already laid out by usvg
                // in primitive-subregion-local coordinates. The renderer filter
                // buffer is axis-aligned in canvas pixels, so we place that local
                // image at the transformed primitive bbox and keep only the
                // filter-axis scale for child coordinates.
                let primitive_bounds = transform_rect_to_bounds(primitive_rect, region_transform);
                buffer_origin
                    * Affine::translate((
                        f64::from(primitive_bounds.x0),
                        f64::from(primitive_bounds.y0),
                    ))
                    * filter_axis_scale(region_transform)
            } else {
                // Internal href targets preserve their own document coordinates.
                // usvg does not move those roots into the primitive subregion, so
                // keep the existing content transform plus primitive origin.
                buffer_origin
                    * content_transform
                    * Affine::translate((primitive_rect.x0, primitive_rect.y0))
            };
            SvgBuilder {
                options: SvgOptions {
                    tolerance: self.options.tolerance,
                    transform: local_transform,
                    pixel_grid: None,
                },
                base_transform: local_transform,
                pattern_depth: self.pattern_depth,
                image_depth: self.image_depth + 1,
            }
            .push_group(&mut filter_canvas, image.root())?;
        }

        let Some(image_key) = canvas.register_scene_image(filter_canvas) else {
            return Err(SvgError::unsupported("empty feImage"));
        };
        let Some(pattern) = PatternBrush::new_resource(
            ImageResourceId::scene(image_key),
            affine_to_array(
                Affine::scale_non_uniform(1.0 / f64::from(width), 1.0 / f64::from(height))
                    * Affine::translate((
                        -f64::from(filter_bounds.x0),
                        -f64::from(filter_bounds.y0),
                    )),
            ),
            Extend::Pad,
            PatternSampling::Nearest,
            255,
        ) else {
            return Err(SvgError::unsupported("empty feImage"));
        };
        Ok(Brush::Pattern(pattern))
    }
}

fn fe_image_root_is_primitive_local_image(root: &usvg::Group) -> bool {
    let [Node::Group(group)] = root.children() else {
        return false;
    };
    is_generated_image_group_id(group.id())
        && group
            .children()
            .iter()
            .all(|child| matches!(child, Node::Image(_)))
}

fn is_generated_image_group_id(id: &str) -> bool {
    id.strip_prefix("image").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
    })
}
