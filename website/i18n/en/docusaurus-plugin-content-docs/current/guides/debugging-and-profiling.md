---
sidebar_position: 5
title: Debugging and profiling
---

# Debugging and profiling

Call `start_profile`, render, and `end_profile` for CPU stages. GPU timestamps may complete asynchronously; poll the device and call `poll_profile`. Important stages include `retained.materialize`, `retained.damage`, `prepare.lengths`, `prepare.upload_scene`, `plan.execute`, `scan`, `coarse`, `fine`, and `filter.*`.

`incremental_render_stats()` reports redraw reason, dirty tiles, rebuilt chunks/fragments, copied/uploaded bytes, rewritten tile pages, surface reuse, dispatches, and arena usage.

For structural diagnostics, pass `&RenderOptions` with `RenderDebugOptions` to `render_with_options`. Captures include tile/path/line dumps, overlays, images, and stable JSON via `debug_capture_json`.
