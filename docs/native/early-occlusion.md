# Occlusion before coarse count and emission

Native tile binning now removes covered draw references before GPU coarse count
and emission. This extends the existing fine-stage opaque-prefix optimization:
hidden draw/tile pairs no longer need coarse classification, particle emission or
fine shading. It addresses redundant work at its source rather than caching an
animation or changing compositing results.

## Proof and execution boundaries

- Only a solid brush with packed alpha 255 on a filled SDF rectangle can establish
  early coverage. Translation is supported; paths, glyphs, shadows, other brushes,
  scaling, rotation and uncertain numeric inputs do not establish this proof.
- The proof includes complete 16×16 tiles inside the rectangle's central interior.
  Rounded corners and antialiasing edges remain in the ordinary stream. Fractional
  inputs reserve a bounded float32 rounding margin.
- Every fused clip must also prove full coverage of the tile. A clip multiplies
  each draw's coverage separately, so even an opaque foreground cannot erase
  preceding contributions at a partial clip edge. Path clips conservatively defer
  to the existing GPU proof.
- Culling stays inside one raster batch. Inline layer commands disable its early
  proof. Fused opacity and blend wrappers remain around the surviving draws;
  filter, backdrop and intermediate-surface dependencies keep their order.
- CPU spatial membership stays complete. Localized filters can shift their tile
  origin and must prove visibility again. Retained edits that move, remove or
  change the paint of an occluder restore the required references.

An immediate canvas walks draws backwards, subtracting a proved covering
rectangle before enumerating tile memberships. It remembers one rectangle per
contiguous batch; it does not guess that a union of partial shapes is opaque.
The ordinary no-occluder loops remain separate so their vectorization is preserved.

Persistent bins retain full CPU membership. For one raster batch, the GPU keeps a
complete flat index arena and each tile record selects its visible suffix. Opacity
changes update records without rewriting the hidden index list. The suffix search
uses that tile's existing painter order directly, avoiding a global occluder scan
and repeated rank searches. Batches without an eligible foreground skip clip-proof
preparation, tiles without a removable predecessor keep their existing list, and
paint-only edits skip spatial density statistics. Batch-only changes do not dirty
unculled tile references: batch IDs have their own upload journal. Fresh unculled
flat records already select the complete list and need no suffix initialization
pass. A regression test first reproduced the redundant dirty journal before the
fix.

Full rebuilds directly create the final flat GPU index arena; CPU reverse membership
is independent of its format. Building pages and then copying another flat arena
both duplicated work and increased real-application rebuild cost. Immediately
constructing all pages on the first later edit also introduced a P95 latency spike.

The implementation measured below estimated index work from complete CPU membership: the flat
arena, all pages, and pages affected by this edit, including tile records. Short edit sequences
keep compact flat uploads. Their accumulated extra work relative to page deltas
pays toward the one-time page construction cost; sustained sparse motion eventually
establishes pages and then updates only affected ones. While flat, edits whose estimated page transfer cost cannot beat a full flat
rebuild do not accumulate that cost. There is no fixed frame or scene
threshold. Tests cover a short edit staying flat, eventual page promotion, and later
delta locality/fragmentation. This historical storage policy has since been
replaced by compact continuous ranges that support local uploads immediately;
see [cache-friendly preparation](cache-friendly-preparation.md).

Visibility changes dirty both the old and new proof regions. Transient retained
bounds exclude inactive storage slots: a focused regression first demonstrated that
stale geometry could inflate the complete membership estimate and misclassify a
dense edit. All these fixes address the underlying index maintenance; they are not
temporary measurement paths or animation caching.

The shared native upload path supplies both Metal and HLSL backends. The early
occlusion change did not alter shader source or public APIs. It does not itself
skip path scan, scene materialization or source upload, and persistent particle
capacity remains conservative. Early occlusion
therefore does not promise a speedup for every scene or eliminate all work on a
hidden object. The existing GPU fine-stage proof remains a fallback.

## Validation and measurements

Measurements use frozen baseline `fb213367dadf76596efa4508a7c4197b895ae2e1`
and candidate release executables on Apple M5 (Mac17,4, 16 GiB), Metal adapter
`0000000100000575`. Timing disables GPU validation and runs one renderer process
at a time, without concurrent compiler or GPU test jobs. Each of three alternating
trials warms 100 frames and measures 160 frames per case. Case order uses shuffled
seeds 107, 108 and 109. All samples are retained.

Wall time is the render call plus explicit submission completion, excluding scene
construction and image readback. Retained cases include their per-frame mutation.
CPU submission is recorded separately. Neither measure is window FPS or a pure
GPU timestamp. Distribution percentiles use nearest rank over pooled samples.

Unless suffixed `4k`, output is 960×640; `4k` is 3840×2160. Scene definitions:

- `opaque_128` / `opaque_512`: opaque background, 128/512 full-screen opaque
  rectangles, and a final opaque rectangle.
- `layers_covered` / `layers_covered_4k`: 128 alpha-120 full-screen layers followed
  by an opaque cover. `layers_open` / `layers_open_512` omit the cover.
- `coverage25`: cover spans 25% of viewport width. `rounded` uses radius 24;
  `fractional` insets the cover by 0.375 px. `alpha254` cannot establish coverage.
- `clip_covered`: fractional radius-41 outer clip; only its full interior can cull.
  `opacity_covered` / `blend_covered`: opacity 0.65 / Multiply source-over wrapper.
- `filter_covered`: the covered scene is inside sigma-8 blur. `backdrop_covered`
  adds a sigma-8 backdrop blur after the covered scene.
- `chart_open` / `chart_modal`: 1600 deterministic cubic path shapes, with/without
  an opaque rounded panel at (100,50)–(850,600).
- `retained_move` / `retained_toggle`: 128 alpha-120 background layers and that
  retained panel, moved 0↔2 px / replaced with alternating alpha 0 and 255.

Temporary benchmark drivers and real-application probes are removed after use.
Permanent tests cover source queries, clip edges, batch boundaries, fused wrappers,
brush/transform fallbacks, paint-only edits and reveal after movement. GPU tests
also compare with an independent fine-only particle stream and verify localized
filter inputs.

## Final results

Full distributions, per-trial values, executable/source hashes and validation
metadata are saved in [the benchmark JSON](benchmarks/early-occlusion-metal-2026-10-12.json).
Negative changes below mean less time. Both variants already include the committed
GPU fine-stage occlusion optimization; these results measure moving the proof earlier
and the required tile-index storage changes.

| Case | Wall P50 (ms), baseline → new | Change | Wall P95 (ms) | CPU submit P50 (ms) |
| --- | --- | --- | --- | --- |
| opaque_128 | 0.911 → 0.267 | -70.7% | 0.986 → 0.288 | 0.369 → 0.046 |
| opaque_512 | 3.151 → 0.279 | -91.1% | 3.570 → 0.316 | 1.372 → 0.059 |
| layers_covered | 0.903 → 0.266 | -70.5% | 1.013 → 0.287 | 0.365 → 0.045 |
| layers_covered_4k | 8.091 → 1.231 | -84.8% | 8.749 → 2.584 | 4.637 → 0.127 |
| layers_open | 3.560 → 3.570 | +0.3% | 4.144 → 4.070 | 0.376 → 0.375 |
| layers_open_512 | 13.831 → 13.830 | -0.0% | 14.425 → 14.560 | 1.411 → 1.408 |
| alpha254 | 3.582 → 3.582 | -0.0% | 4.079 → 4.092 | 0.384 → 0.383 |
| coverage25 | 2.920 → 2.926 | +0.2% | 3.456 → 3.428 | 0.385 → 0.409 |
| rounded | 0.943 → 0.657 | -30.3% | 1.059 → 0.744 | 0.367 → 0.111 |
| fractional | 0.905 → 0.269 | -70.2% | 0.993 → 0.289 | 0.367 → 0.045 |
| clip_covered | 1.091 → 0.844 | -22.6% | 1.177 → 0.910 | 0.402 → 0.178 |
| opacity_covered | 5.117 → 0.548 | -89.3% | 5.746 → 0.583 | 0.416 → 0.080 |
| blend_covered | 5.223 → 0.627 | -88.0% | 5.844 → 0.713 | 0.412 → 0.079 |
| filter_covered | 4.105 → 3.411 | -16.9% | 4.769 → 4.065 | 0.771 → 0.169 |
| backdrop_covered | 2.364 → 1.776 | -24.9% | 2.654 → 2.484 | 0.402 → 0.084 |
| chart_open | 1.386 → 1.404 | +1.3% | 1.579 → 1.524 | 0.363 → 0.376 |
| chart_modal | 0.936 → 0.907 | -3.1% | 0.991 → 1.008 | 0.362 → 0.364 |
| retained_move | 1.633 → 1.625 | -0.5% | 1.826 → 1.721 | 0.075 → 0.076 |
| retained_toggle | 2.347 → 2.324 | -0.9% | 4.596 → 4.668 | 0.061 → 0.073 |

The mixed `retained_toggle` distribution is bimodal; its pooled P50 does not describe
a typical state. With an opaque cover, wall P50 is 1.634 → 1.647 ms; transparent
cover P50 is 4.301 → 4.324 ms. Combined mean is 3.021 → 3.035 ms (+0.5%).
CPU submission P50 increases by about 12 μs. The ordinary transparent cases are
essentially unchanged, while the uncovered chart adds about 18 μs to wall P50.
Small partial coverage therefore does not guarantee a speedup.

## Real application

A copied Northstar / Axiom Terminal working tree runs 24 cycles through exchanges,
strategies, settings and backtests at a 1488×1056 logical window (Retina target
2976×2112). Isolated capture storage and empty live profiles keep the workload
repeatable. Both versions use the same temporary automation/timing probes and
build adaptations for current renderer types/diagnostic fields. Original application
sources are unchanged. Repository heads and source identities are in the JSON.

Three alternating trials retain 256 renderer-call samples per process, discarding
the first 16 (720 samples per variant). These are native renderer submission wall
milliseconds on the CPU, excluding UI layout, presentation and explicit completion;
they are neither window FPS nor GPU timestamps. Sample serialization occurs after
the timed call. Timing runs without GPU validation, compiler jobs or GPU tests.

| Renderer CPU metric | Baseline (ms) | New (ms) | Change |
| --- | --- | --- | --- |
| Mean | 2.974 | 2.801 | -5.8% |
| P50 | 3.955 | 4.153 | +5.0% |
| P95 | 6.927 | 5.461 | -21.2% |
| P99 | 7.290 | 5.650 | -22.5% |

Average and tail latency improve in this workload; the median increases by
0.198 ms (5.0%). Compact full uploads cost more on some individual edits but avoid
large padded index construction/transfers on heavier frames. This measured tradeoff
is retained explicitly; the result does not claim that every frame or scene is faster.

## Checks and cleanup

- 1241 release tests across 45 targets pass, including ignored physical-GPU tests,
  with one test thread and one Rayon thread. The focused tile-index suite has 34 tests.
- 1712 SVG PNGs have identical file hashes to the pre-change baseline. All 57
  benchmark PNG pairs match exact decoded RGBA bytes, including clip AA edges.
- Rendering examples pass: mesh gradient, program brush, program glass, shader
  pipeline/graph, both progressive-blur modes and Metal presentation with resize.
- `cargo fmt --all --check` and strict release Metal Clippy for all targets pass.
  Chinese and English website production builds pass.
- Metal runtime execution uses the pinned physical GPU and API validation.
  HLSL compilation covers DXIL and SPIR-V; DX12/Vulkan hardware execution and
  performance were not measured on this Mac.
- Temporary benchmark examples, stage timers and copied application sources are
  removed. Raw local samples, logs, images and frozen executables remain under
  `target/early-occlusion`; the report above is the permanent measurement artifact.
