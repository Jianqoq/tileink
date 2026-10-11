---
sidebar_position: 5
title: Debugging and profiling
---

# Debugging and profiling

Use `NativeContextOptions { validation: true, ..Default::default() }` to request API validation. Rendering returns an explicit submission receipt. When CPU pixels are needed, call `render_to_image(...).readback()`.

`incremental_render_stats()` reports redraw reason, dirty tiles, rebuilt chunks/fragments, copied/uploaded bytes, surface reuse, dispatches, and arena usage.

Set `IncrementalRenderConfig::capture_active_tiles` to collect active tile IDs and bounds in the retained statistics. For performance changes, pin the physical device, scene, dimensions and toolchain, measure actual application frame time, and remove temporary measurement code after validation.
