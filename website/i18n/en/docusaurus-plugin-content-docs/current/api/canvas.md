---
sidebar_position: 2
title: Canvas API
---

# `Canvas`

## Lifetime and composition

- `new(width, height, scale_factor)` creates a logical scene.
- `scale_factor`, `logical_size`, `physical_size`, `physical_width`, and `physical_height` query extent.
- `is_closed_for_append` verifies that every layer was popped; `reset` clears reusable storage.
- `append(other, position)` translates a child; `append_transformed(other, Affine)` supports full affine placement.

## Draw access

`draw_count`, `draw_id_at`, and `DrawId::index` expose draw identity. `draw_brush`, `draw_solid_color`, `set_draw_brush`, and `set_draw_color` inspect or update paint. IDs expire when the Canvas is reset.

## Layers

Path and analytic clips use `push_clip_layer`, `push_clip_sdf_rect_layer`, `push_clip_sdf_circle_layer`, `push_clip_sdf_arc_layer`, `push_clip_sdf_line_layer`, `push_clip_sdf_layer`, or `push_clip_sdf_layer_transformed`. Composition uses `push_isolate_layer`, `push_opacity_layer`, `push_blend_layer`, `push_mask_layer`, `push_filter_layer`, and `push_backdrop_layer`. Match each successful push with `pop_layer`.

## Geometry

- Rects: `push_rect`, `push_rect_stroke`, `push_rect_stroke_widths`, `push_rect_shadow`.
- Circles: `push_circle`, `push_circle_stroke`, `push_circle_shadow`.
- Analytic forms: `push_sdf_arc`, `push_arc_shadow`, `push_candlestick`, `push_line`, `push_dash_line`, `push_line_shadow`.
- Kurbo paths: `push_arc`, `push_stroke`, `push_path`.

Most methods accept `impl Into<Brush>`; potentially empty or invalid analytic geometry returns `Option<DrawId>`.

## Images, text, and SVG

`push_image(rect, image, extend, sampling)` embeds a shared image; `push_image_key(rect, key, extend, sampling)` uses the renderer registry. `push_text_layout` records text runs, while `push_text_layout_as_path` records scalable outlines. `push_svg` and `push_svg_with_options` transactionally lower a usvg tree.
