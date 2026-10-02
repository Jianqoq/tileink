---
title: API overview
---

# API overview

Use `Canvas` for immediate drawing and `RetainedScene` for transactional updates. `NativeContext` selects a device; `NativeRenderer` submits to an owned image, texture, or host render target. `TextFontSystem` and `TextContext` prepare text, while `Canvas::push_svg_with_options` lowers SVG input. Errors are returned as `NativeError`, `RetainedSceneError`, and `SvgError`.
