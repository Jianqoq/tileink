---
sidebar_position: 3
title: Liquid Glass
---

# Liquid Glass

Use `Filter::RectLiquidGlass` inside a backdrop layer and provide a tight rounded `Region`. The backdrop samples previously painted content, so painter order is part of the visual semantics. In retained mode, moving the glass node damages old/new bounds plus affected backdrop dependency tiles.

The complete parameterized example is identical to the code on the Chinese page and to `examples/wgpu/liquid_glass.rs` in the repository.
