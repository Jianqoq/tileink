---
sidebar_position: 2
title: Immediate Canvas
---

# Immediate Canvas

`Canvas` is a contiguous recorder. `push_*` calls append fixed records, variable paint/SDF/text blobs, and layer command lists. `append` and `append_transformed` merge reusable child canvases.

Its strengths are direct APIs, dense memory, and high full-frame throughput. Its cost is O(total scene records and blobs) whenever the application rebuilds the whole canvas. Every pushed layer must be matched by `pop_layer`; an unclosed Canvas cannot be appended or inserted into a retained scene.

`DrawId` permits brush or color changes during the lifetime of the current Canvas, but IDs must not be reused after `reset`.

Compiled clips live only in fused batch stacks. Offscreen descriptors allow isolate, opacity, blend, filter and backdrop operations; only geometry groups own hidden draws, so filters/backdrops have no placeholder draw. Localization saturates integer bounds to the signed coordinate domain and converts path origins to floating point before negating them, preventing large clips and negative surface origins from wrapping and hiding content.

Recording commands also store draw references only for geometric layers. Filters/backdrops carry their filter and sample region, so parameter changes do not upload unrelated clip stacks. SDF clip geometry and bounds live solely in scene buffers, allowing geometry updates to reuse execution topology. Append translation consumes owned commands to avoid another deep copy of filter and mask descriptors.
