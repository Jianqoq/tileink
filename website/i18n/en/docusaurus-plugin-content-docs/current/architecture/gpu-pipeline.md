---
title: GPU pipeline
---

# GPU pipeline

The renderer prepares paths, brushes, layers, text, and images from the same scene model for each native API. A path scan and prefix pass calculate work, coarse binning assigns draws to 16×16 tiles, and fine rasterization resolves coverage and blending. Filter passes use intermediate surfaces as needed. Retained frames reuse unchanged scene records and output history. Image readback is explicit through `NativeImageSubmission::readback`; host targets use submission receipts to coordinate presentation.


CPU tile binning first proves complete tile coverage by solid opaque SDF rectangles, then removes covered draw references within the same raster batch. This reduces GPU coarse counting, particle emission and fine shading. Rounded corners, antialiasing edges and tiles without complete clip coverage remain; uncertain brushes and transforms defer to the existing GPU proof. CPU spatial membership retains all source content so movement, opacity edits and filter localization can recover the required draws. Single-batch retained indices can update each tile's visible start without rewriting the complete index arena on opacity changes.

Coarse still records the last solid-color particle proven to cover the complete tile with alpha 255 so Fine can skip earlier particles. This proof preserves particle allocations and glyph indices and supplements conservative CPU culling. Fused opacity/blend wrappers, filters and backdrop dependencies retain their execution order. Scene preparation, path scan and source uploads still run, so ordinary scenes without occlusion usually see little benefit. Metal and HLSL consume the same CPU tile references and execution boundaries.

The fused stack encodes typed clip, opacity, and blend entries. Only clips can carry SDF geometry; opacity and blend boundaries use paths. Each emitter first counts the current tile's valid wrappers, rejecting uncovered path or SDF bounds and eliding clips proven to cover the whole tile. It then emits begins in nesting order and ends in reverse order using the same immutable scene inputs. Metal and HLSL share this contract, so emission does not repeat handling for unknown stack kinds or rejected paths.
