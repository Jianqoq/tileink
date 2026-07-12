---
sidebar_position: 7
title: Debug and profile API
---

# Debug capture

`RenderDebugOptions::new`/`try_new` select an output directory; `with_tile` and `with_tile_overlay` add focused capture. Getters are `output_dir`, `tile`, and `tile_overlay`. `debug_capture_json` serializes `RenderDebugCapture`. Public capture records include debug text/images, tile summaries/dumps/paths, and line segments.

# Profiling

`WgpuRenderProfile` exposes `entries`, `cpu_time`, `gpu_time`, `incremental_stats`, and `summary`. `WgpuRenderProfileReport::new`, `push`, `iterations`, and `profile` aggregate frames.

`IncrementalRenderStats` reports redraw reason and ratios, output/history mode, queue submissions, retained nodes/batches/surfaces, scan and filter work, materialization reuse, rebuilt chunks/fragments, copied/uploaded bytes, tile pages, and arena utilization. Inspect `FullRedrawReason` rather than inferring the cause from `full_redraw` alone.
