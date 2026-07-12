---
sidebar_position: 5
title: Damage and output history
---

# Damage and output history

Incremental rasterization is valid only when old target pixels are trusted. Damage comes from revisions, transforms, bounds, hierarchy/order changes, explicit invalidation, filter/backdrop dependencies, renderer/resources, or surface identity and size.

`IncrementalOutputMode` distinguishes renderer-owned internal history, caller-owned external history, direct transient output, and history rebuilding. A recreated or externally modified texture must receive a new `ExternalTextureHistoryId`.

`IncrementalRenderMode::ForceFull` is a correctness and performance oracle. It forces full raster damage while preserving normal retained materialization, chunk reuse, and incremental uploads.
