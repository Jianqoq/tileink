---
sidebar_position: 5
title: Damage and output history
---

# Damage and output history

Incremental rasterization is valid only when old target pixels are trusted. Damage comes from revisions, transforms, bounds, hierarchy/order changes, explicit invalidation, filter/backdrop dependencies, renderer/resources, or surface identity and size.

Persistent frame snapshots may share immutable node arrays and record removals or updates in delta overlays. When the renderer falls back from a directly connected journal delta to snapshot comparison, it resolves those overlays before matching node IDs. A node removed by a delta and later reinserted with the same ID is therefore insertion damage, even when its old base-array entry still exists. Overlay-only nodes and explicit delta damage are included conservatively without escalating the frame to full damage. The fallback diff preserves a contiguous base-array scan and resolves only IDs changed by delta overlays or compacted state pages; it must not add an overlay hash lookup to every base node. Scratch storage is reused across frames, giving average `O(N + P)` time and `O(N + P)` extra scratch space; base-index marks cost one byte per node in each compared frame.

`IncrementalOutputMode` distinguishes renderer-owned internal history, caller-owned external history, direct transient output, and history rebuilding. A recreated or externally modified texture must receive a new `ExternalTextureHistoryId`.

`IncrementalRenderMode::ForceFull` is a correctness and performance oracle. It forces full raster damage while preserving normal retained materialization, chunk reuse, and incremental uploads.
