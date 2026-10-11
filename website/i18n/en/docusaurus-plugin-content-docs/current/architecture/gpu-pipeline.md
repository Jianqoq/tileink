---
title: GPU pipeline
---

# GPU pipeline

The renderer prepares paths, brushes, layers, text, and images from the same scene model for each native API. A path scan and prefix pass calculate work, coarse binning assigns draws to 16×16 tiles, and fine rasterization resolves coverage and blending. Filter passes use intermediate surfaces as needed. Retained frames reuse unchanged scene records and output history. Image readback is explicit through `NativeImageSubmission::readback`; host targets use submission receipts to coordinate presentation.


Coarse records the last solid-color particle proven to cover the complete tile with alpha 255. Fine can skip earlier particles, reducing raster work behind opaque panels. Particle allocations and glyph indices remain intact for retained range reuse. Partial clips, opacity/blend groups, filters, and backdrop dependencies retain their execution order. This removes fine raster work; scene preparation, uploads, and coarse allocation still run, so ordinary scenes without occlusion usually see little benefit.

The fused stack encodes typed clip, opacity, and blend entries. Only clips can carry SDF geometry; opacity and blend boundaries use paths. Each emitter first counts the current tile's valid wrappers, rejecting uncovered path or SDF bounds and eliding clips proven to cover the whole tile. It then emits begins in nesting order and ends in reverse order using the same immutable scene inputs. Metal and HLSL share this contract, so emission does not repeat handling for unknown stack kinds or rejected paths.
