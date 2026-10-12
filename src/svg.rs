use std::{error::Error, fmt};

use peniko::{Compose, kurbo::Affine};
use usvg::Node;

use self::{
    filter::svg_filter_layers,
    geometry::{
        blend_mode_to_mix, inverse_affine, nonzero_rect_to_kurbo, rect_path, transform_to_affine,
    },
};
use crate::{Canvas, PixelGrid};

mod clip;
mod filter;
mod geometry;
mod image;
mod paint;
mod scene_image;

#[derive(Clone, Copy, Debug)]
pub struct SvgOptions {
    pub tolerance: f64,
    pub transform: Affine,
    /// Optional fitting for small UI glyphs. General SVG content preserves subpixel geometry.
    pub pixel_grid: Option<PixelGrid>,
}

const MAX_PATTERN_DEPTH: u8 = 16;
const MAX_IMAGE_DEPTH: u8 = 16;

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.1,
            transform: Affine::IDENTITY,
            pixel_grid: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SvgError {
    feature: String,
}

impl SvgError {
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self {
            feature: feature.into(),
        }
    }

    #[must_use]
    pub fn feature(&self) -> &str {
        &self.feature
    }
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported SVG feature: {}", self.feature)
    }
}

impl Error for SvgError {}

impl Canvas {
    /// Appends a parsed `usvg` tree by lowering the reduced SVG tree into Canvas primitives.
    ///
    /// The conversion is transactional: unsupported SVG features return an error before this
    /// canvas is modified. This prevents partial SVG output from looking like a successful render.
    ///
    /// # Errors
    /// Returns an error for unsupported SVG features or invalid transforms/filter parameters. The canvas remains unchanged on error.
    pub fn push_svg(&mut self, tree: &usvg::Tree) -> Result<(), SvgError> {
        self.push_svg_with_options(tree, SvgOptions::default())
    }

    ///
    /// # Errors
    /// Returns an error for unsupported SVG features or invalid transforms/filter parameters. The canvas remains unchanged on error.
    pub fn push_svg_with_options(
        &mut self,
        tree: &usvg::Tree,
        options: SvgOptions,
    ) -> Result<(), SvgError> {
        let mut svg_scene = Self::new(self.logical_width, self.logical_height, self.scale_factor);
        SvgBuilder::new(options).push_tree(&mut svg_scene, tree)?;
        self.append(&svg_scene, (0.0, 0.0));
        Ok(())
    }
}

struct SvgBuilder {
    options: SvgOptions,
    base_transform: Affine,
    pattern_depth: u8,
    image_depth: u8,
}

impl SvgBuilder {
    const fn new(options: SvgOptions) -> Self {
        Self {
            base_transform: options.transform,
            options,
            pattern_depth: 0,
            image_depth: 0,
        }
    }

    const fn with_base_transform(&self, base_transform: Affine) -> Self {
        Self {
            base_transform,
            options: self.options,
            pattern_depth: self.pattern_depth,
            image_depth: self.image_depth,
        }
    }

    fn push_tree(&self, canvas: &mut Canvas, tree: &usvg::Tree) -> Result<(), SvgError> {
        self.push_group(canvas, tree.root())
    }

    fn push_group(&self, canvas: &mut Canvas, group: &usvg::Group) -> Result<(), SvgError> {
        let group_transform = self.base_transform * transform_to_affine(group.abs_transform());
        let filter_layers = if group.filters().is_empty() {
            Vec::new()
        } else {
            let content_transform = group_transform
                * inverse_affine(transform_to_affine(group.transform()), "filter transform")?;
            svg_filter_layers(
                self,
                canvas,
                group.filters(),
                group_transform,
                content_transform,
                canvas.logical_width,
                canvas.logical_height,
            )?
        };
        let mask_layer = group
            .mask()
            .map(|mask| {
                self.svg_mask_layer(
                    canvas.logical_width,
                    canvas.logical_height,
                    canvas.scale_factor,
                    mask,
                    group_transform,
                )
            })
            .transpose()?;

        let mut pushed_layers = 0;
        if let Some(clip) = group.clip_path() {
            // usvg inserts a viewport-clip wrapper for <use> of a <symbol>. Its group absolute
            // transform is the parent's, while its untransformed child carries the <use>
            // transform. Apply the clip in the same coordinate space as that child.
            let clip_transform = match group.children() {
                [Node::Group(child)] if child.transform() == usvg::Transform::default() => {
                    self.base_transform * transform_to_affine(child.abs_transform())
                }
                _ => group_transform,
            };
            pushed_layers += self.push_clip_path_layers(canvas, clip, clip_transform)?;
        }

        let layer_path = || {
            self.base_transform * rect_path(nonzero_rect_to_kurbo(group.abs_layer_bounding_box()))
        };
        if group.isolate()
            && group.opacity().get() >= 1.0
            && group.blend_mode() == usvg::BlendMode::Normal
            && mask_layer.is_none()
            && filter_layers.is_empty()
        {
            canvas.push_isolate_layer(layer_path(), Affine::IDENTITY, self.options.tolerance);
            pushed_layers += 1;
        }
        if group.blend_mode() != usvg::BlendMode::Normal {
            canvas.push_blend_layer(
                layer_path(),
                Affine::IDENTITY,
                self.options.tolerance,
                blend_mode_to_mix(group.blend_mode()),
                Compose::SrcOver,
            );
            pushed_layers += 1;
        }
        if group.opacity().get() < 1.0 {
            canvas.push_opacity_layer(
                layer_path(),
                Affine::IDENTITY,
                self.options.tolerance,
                group.opacity().get(),
            );
            pushed_layers += 1;
        }
        if let Some((mask_scene, mask)) = mask_layer {
            canvas.push_mask_layer(mask_scene, mask);
            pushed_layers += 1;
        }
        for layer in filter_layers.into_iter().rev() {
            canvas.push_filter_layer(layer.filter, layer.region);
            pushed_layers += 1;
        }

        for child in group.children() {
            self.push_node(canvas, child)?;
        }

        for _ in 0..pushed_layers {
            canvas.pop_layer();
        }
        Ok(())
    }

    fn push_node(&self, canvas: &mut Canvas, node: &Node) -> Result<(), SvgError> {
        match node {
            Node::Group(group) => self.push_group(canvas, group),
            Node::Path(path) => self.push_path(canvas, path),
            Node::Text(text) => self.push_group(canvas, text.flattened()),
            Node::Image(image) => self.push_image(canvas, image),
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod deferred_resources;
