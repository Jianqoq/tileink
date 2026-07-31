---
sidebar_position: 1
title: Tileink Documentation
slug: /
---

# Tileink

Tileink is a Rust, WGPU-compute, tile-based 2D renderer. It supports paths, analytic SDFs, text, images, gradients, layers, masks, filters, backdrops, and SVG through two scene models:

- `Canvas`: a contiguous immediate scene for small, static, or fully rebuilt content.
- `RetainedScene`: a persistent transactional scene for large applications with local changes.

Start with [installation](getting-started/installation.md), the [quick start](getting-started/quick-start.md), or the [architecture overview](architecture/overview.md). The [public API reference](api/overview.md) follows the actual re-exports in `src/lib.rs`; `pub(crate)` and non-re-exported items remain internal.
