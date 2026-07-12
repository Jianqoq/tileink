---
sidebar_position: 1
title: Public API overview
---

# Public API overview

This reference follows `src/lib.rs`. Use `cargo doc --no-deps --open` for generated field and trait documentation.

Top-level constants are `TILE_SIZE = 16`, `TILE_SCALE = 1.0 / TILE_SIZE`, and `BLOCK_SIZE = 256`. Primary entry points are [`Canvas`](canvas.md), [`WgpuRenderer`](renderer.md), [`RetainedScene`](retained-scene.md), `TextContext`, and the paint/geometry/filter types.

Tileink re-exports common cosmic-text attributes as `TextAlign`, `TextAttrs`, `TextCacheKeyFlags`, `TextFamily`, `TextFontSystem`, `TextStretch`, `TextStyle`, and `TextWeight`. Geometry and color types remain in peniko/kurbo.

Public errors are `RetainedSceneError`, `WgpuTextureRenderError`, `SvgError`, and `ImageSaveError`.
