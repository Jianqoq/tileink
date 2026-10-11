//! SVG masks and clipping, including complex clipPath isolation.
use super::{
    SvgBuilder, SvgError,
    geometry::{
        fill_rule, nonzero_rect_to_kurbo, rect_path, tiny_path_to_bez, transform_to_affine,
    },
};
use crate::{
    Brush, Canvas, FillRule, Radius, Region,
    shared::layer::mask::{Mask as LayerMask, MaskKind},
};
use peniko::{
    Color,
    kurbo::{Affine, BezPath, Rect},
};
use usvg::Node;

enum ClipPathLowering {
    Empty,
    Fused { path: BezPath, rule: FillRule },
    Mask,
}

impl SvgBuilder {
    pub(super) fn svg_mask_layer(
        &self,
        width: u32,
        height: u32,
        scale_factor: f32,
        mask: &usvg::Mask,
        transform: Affine,
    ) -> Result<(Canvas, LayerMask), SvgError> {
        let mut mask_scene = Canvas::new(width, height, scale_factor);
        let builder = self.with_base_transform(transform);
        if let Some(parent) = mask.mask() {
            let (parent_scene, parent_mask) =
                self.svg_mask_layer(width, height, scale_factor, parent, transform)?;
            mask_scene.push_mask_layer(parent_scene, parent_mask);
            builder.push_group(&mut mask_scene, mask.root())?;
            mask_scene.pop_layer();
        } else {
            builder.push_group(&mut mask_scene, mask.root())?;
        }
        let rect = nonzero_rect_to_kurbo(mask.rect());
        let [_, b, c, _, _, _] = transform.as_coeffs();
        // A rotated mask region is a parallelogram, not its axis-aligned bounding box.
        let region = if b == 0.0 && c == 0.0 {
            Region::rect(transform.transform_rect_bbox(rect), Radius::ZERO)
        } else {
            Region::path(rect_path(rect), transform, self.options.tolerance)
        };
        Ok((
            mask_scene,
            LayerMask {
                region,
                kind: match mask.kind() {
                    usvg::MaskType::Alpha => MaskKind::Alpha,
                    usvg::MaskType::Luminance => MaskKind::Luminance,
                },
            },
        ))
    }

    pub(super) fn push_clip_path_layers(
        &self,
        canvas: &mut Canvas,
        clip: &usvg::ClipPath,
        reference_transform: Affine,
    ) -> Result<usize, SvgError> {
        let mut pushed = 0;
        if let Some(parent) = clip.clip_path() {
            pushed += self.push_clip_path_layers(canvas, parent, reference_transform)?;
        }

        match self.clip_path_lowering(clip) {
            ClipPathLowering::Empty => canvas.push_mask_layer(
                Canvas::new(
                    canvas.logical_width,
                    canvas.logical_height,
                    canvas.scale_factor,
                ),
                LayerMask {
                    region: Region::rect(Rect::ZERO, Radius::ZERO),
                    kind: MaskKind::Alpha,
                },
            ),
            ClipPathLowering::Fused { path, rule } => canvas.push_clip_layer(
                path,
                reference_transform * transform_to_affine(clip.transform()),
                rule,
                self.options.tolerance,
            ),
            ClipPathLowering::Mask => {
                let (mask_scene, mask) = self.svg_clip_path_mask_layer(
                    canvas.logical_width,
                    canvas.logical_height,
                    canvas.scale_factor,
                    clip,
                    reference_transform,
                )?;
                canvas.push_mask_layer(mask_scene, mask);
            }
        }
        Ok(pushed + 1)
    }

    fn svg_clip_path_mask_layer(
        &self,
        width: u32,
        height: u32,
        scale_factor: f32,
        clip: &usvg::ClipPath,
        reference_transform: Affine,
    ) -> Result<(Canvas, LayerMask), SvgError> {
        let mut mask_scene = Canvas::new(width, height, scale_factor);
        self.push_clip_path_mask_group(
            &mut mask_scene,
            clip.root(),
            reference_transform * transform_to_affine(clip.transform()),
        )?;
        Ok((
            mask_scene,
            LayerMask {
                // Complex clipPath lowering must not crop group contents before filters run.
                // The alpha mask applies the actual clip shape after isolated content rendering.
                region: Region::rect(
                    Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
                    Radius::ZERO,
                ),
                kind: MaskKind::Alpha,
            },
        ))
    }

    fn push_clip_path_mask_group(
        &self,
        canvas: &mut Canvas,
        group: &usvg::Group,
        transform: Affine,
    ) -> Result<(), SvgError> {
        let transform = transform * transform_to_affine(group.transform());
        let mut pushed_layers = 0;
        if let Some(clip) = group.clip_path() {
            pushed_layers += self.push_clip_path_layers(canvas, clip, transform)?;
        }

        for child in group.children() {
            self.push_clip_path_mask_node(canvas, child, transform)?;
        }

        for _ in 0..pushed_layers {
            canvas.pop_layer();
        }
        Ok(())
    }

    fn push_clip_path_mask_node(
        &self,
        canvas: &mut Canvas,
        node: &Node,
        transform: Affine,
    ) -> Result<(), SvgError> {
        match node {
            Node::Group(group) => self.push_clip_path_mask_group(canvas, group, transform),
            Node::Path(path) => {
                if path.is_visible()
                    && let Some(fill) = path.fill()
                {
                    canvas.push_path(
                        tiny_path_to_bez(path.data()),
                        Brush::Solid(Color::BLACK),
                        transform,
                        fill_rule(fill.rule()),
                        self.options.tolerance,
                    );
                }
                Ok(())
            }
            Node::Text(text) => self.push_clip_path_mask_group(canvas, text.flattened(), transform),
            Node::Image(_) => Ok(()),
        }
    }

    fn clip_path_lowering(&self, clip: &usvg::ClipPath) -> ClipPathLowering {
        let mut paths = Vec::new();
        let mut needs_mask = false;
        self.collect_clip_path_candidates(clip.root(), &mut paths, &mut needs_mask);
        if paths.is_empty() && !needs_mask {
            ClipPathLowering::Empty
        } else if !needs_mask && paths.len() == 1 {
            let (path, rule) = paths.pop().unwrap();
            ClipPathLowering::Fused { path, rule }
        } else {
            ClipPathLowering::Mask
        }
    }

    fn collect_clip_path_candidates(
        &self,
        group: &usvg::Group,
        paths: &mut Vec<(BezPath, FillRule)>,
        needs_mask: &mut bool,
    ) {
        if group.clip_path().is_some() {
            *needs_mask = true;
            return;
        }

        for child in group.children() {
            match child {
                Node::Group(group) => self.collect_clip_path_candidates(group, paths, needs_mask),
                Node::Path(path) => {
                    if path.is_visible() {
                        let rule = path
                            .fill()
                            .map(|fill| fill_rule(fill.rule()))
                            .unwrap_or(FillRule::NonZero);
                        paths.push((
                            transform_to_affine(path.abs_transform())
                                * tiny_path_to_bez(path.data()),
                            rule,
                        ));
                    }
                }
                Node::Text(text) => {
                    self.collect_clip_path_candidates(text.flattened(), paths, needs_mask)
                }
                Node::Image(_) => {}
            }
        }
    }
}
