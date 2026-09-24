---
title: Debug and incremental statistics
---

# Debug and incremental statistics

`NativeRenderer::incremental_render_config` and `incremental_render_stats` describe retained redraw decisions. Use `set_incremental_render_config` to tune the policy, and `invalidate_retained_history` when external state makes output history invalid. Run release-mode tests serially and use API validation when diagnosing device or shader faults.
