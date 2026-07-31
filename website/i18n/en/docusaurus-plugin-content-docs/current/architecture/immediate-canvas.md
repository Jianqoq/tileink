---
sidebar_position: 2
title: Immediate Canvas
---

# Immediate Canvas

`Canvas` is a contiguous recorder. `push_*` calls append fixed records, variable paint/SDF/text blobs, and layer command lists. `append` and `append_transformed` merge reusable child canvases.

Its strengths are direct APIs, dense memory, and high full-frame throughput. Its cost is O(total scene records and blobs) whenever the application rebuilds the whole canvas. Every pushed layer must be matched by `pop_layer`; an unclosed Canvas cannot be appended or inserted into a retained scene.

`DrawId` permits brush or color changes during the lifetime of the current Canvas, but IDs must not be reused after `reset`.
