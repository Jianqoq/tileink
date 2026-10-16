---
sidebar_position: 5
title: Debugging and profiling
---

# Debugging and profiling

Use `NativeContextOptions { validation: true, ..Default::default() }` to request API validation. Rendering returns an explicit submission receipt. When CPU pixels are needed, call `render_to_image(...).readback()`.

`incremental_render_stats()` reports redraw reason, dirty tiles, rebuilt chunks/fragments, copied/uploaded bytes, surface reuse, draw batches, and arena usage.

Set `IncrementalRenderConfig::capture_active_tiles` to collect active tile IDs and bounds in the retained statistics. For performance changes, pin the physical device, scene, dimensions and toolchain, measure actual application frame time, and remove temporary measurement code after validation.

`IncrementalRenderConfig` provides `mode`, `full_redraw_ratio`, `capture_active_tiles`
and `retained_texture_budget_bytes`. The redraw ratio accepts finite values in
`(0, 1]`; the native retained texture cache defaults to a 256 MiB budget.
