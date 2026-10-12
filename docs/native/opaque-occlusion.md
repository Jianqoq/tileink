# Opaque tile-prefix occlusion

This is raster work elimination within one coarse draw batch. The last full-tile
opaque source-over color makes earlier particles irrelevant to the final pixel.
The fine stage starts at the last proven opaque particle while preserving its
physical allocation and glyph destinations. The subsequent
[early-occlusion optimization](early-occlusion.md) also removes conservatively
covered references before coarse count/emission; the fine proof remains a fallback
for draws that do not qualify for the earlier CPU proof. Measurements below describe
the original fine-stage change only.

## Semantics and ownership

- Accept only existing `PTCL_COLOR` coverage proofs with packed alpha 255. Bounds,
  image opacity guesses, rounded edges and alpha 254 never establish occlusion.
- Retain partial clip, opacity and blend wrappers. A full clip can be elided by
  the existing proof, allowing its interior tiles to use occlusion.
- Scan backwards through the emitted stream while tracking clip/group depth.
  An opaque color inside a retained inline stack cannot erase its saved parent.
- Never cross batch, backdrop, filter or intermediate-surface dependencies.
- Keep physical particle starts/ends, counts and glyph offsets unchanged. Retained
  clip batches reuse preallocated ranges; changing their start corrupts subsequent
  emissions. A regression test reproduced that error before this separation.
- The existing fine descriptor packs its kind in bits 0–2 and a relative raster
  prefix in bits 3–31. All emission routes refresh it, including empty/rejected
  tiles. Raw-buffer address validation already bounds particle capacity below
  2^29; a host test proves every representable prefix fits.
- Scalar/chunk classification marks a later opaque color with `CHUNK_CLASS_OCCLUDER`
  using its existing OR reduction. A lone background followed by translucent,
  image or text particles needs no reverse scan. This removes most of the first
  experiment’s 2–3% translucent-scene overhead without another pass or allocation.

This addresses redundant fine rasterization directly. It is not a temporary pixel
workaround, and does not implement arbitrary object/region-union visibility culling.

## Metal measurements, 2026-10-11

Apple M5, Mac17,4, 16 GiB; GPU registry ID `0000000100000575`. Baseline:
`c34d11d3675070491910ba61a28f1089259e84d9`. Identical temporary drivers linked
against frozen baseline source and the candidate. Release build, one process and
one GPU workload at a time, validation disabled during timing. No concurrent
compiler/renderer jobs were observed. Each case warms 16 frames, then samples 80
frames in each of three alternating trials (240 samples per variant). Case order
uses deterministic shuffled seeds 311, 312 and 313. All samples are retained;
the third trial includes unattributed timing variability.

Wall time below is the render call plus explicit `NativeSubmission::wait`,
excluding scene construction, readback and pixel comparison. It includes CPU
recording, submission and completion; it is not a pure GPU timestamp or window FPS.
P50/P95/P99 use nearest rank over pooled samples. Units are milliseconds.

Detailed per-trial wall/submit distributions and executable hashes are in
[the JSON report](benchmarks/opaque-occlusion-metal-2026-10-11.json). Raw samples
and first-trial captured outputs remain under `target/occlusion-validation/measurements-final`;
all trial image hashes are recorded in `target/occlusion-validation/image-hashes.json`.

| Case | P50 before → after | Change | P95 before → after | P99 before → after |
| --- | ---: | ---: | ---: | ---: |
| `overdraw_1` | 0.314 → 0.293 | -6.7% | 0.495 → 0.354 | 0.832 → 0.748 |
| `overdraw_16` | 0.848 → 0.433 | -49.0% | 0.960 → 0.575 | 1.060 → 0.687 |
| `overdraw_128` | 4.490 → 1.416 | -68.5% | 5.338 → 1.651 | 5.654 → 2.119 |
| `overdraw_512` | 16.644 → 5.030 | -69.8% | 20.362 → 7.415 | 20.708 → 7.991 |
| `overdraw_16_4k` | 5.094 → 1.989 | -61.0% | 5.612 → 2.300 | 5.891 → 2.448 |
| `overdraw_128_4k` | 32.647 → 9.140 | -72.0% | 39.592 → 13.844 | 42.799 → 17.889 |
| `transparent_16` | 0.907 → 0.912 | +0.5% | 1.084 → 1.190 | 1.872 → 1.529 |
| `transparent_128` | 5.081 → 5.099 | +0.3% | 5.658 → 5.657 | 6.132 → 5.977 |
| `transparent_512` | 18.886 → 19.031 | +0.8% | 19.699 → 21.094 | 20.297 → 21.898 |
| `coverage_0` | 5.183 → 5.217 | +0.7% | 5.619 → 6.037 | 5.866 → 6.654 |
| `coverage_25` | 5.169 → 4.140 | -19.9% | 5.552 → 4.579 | 5.773 → 4.814 |
| `coverage_75` | 5.158 → 2.323 | -55.0% | 5.696 → 2.674 | 6.044 → 2.920 |
| `coverage_100` | 5.181 → 1.405 | -72.9% | 5.633 → 1.766 | 5.858 → 3.684 |
| `rounded_100` | 5.166 → 1.528 | -70.4% | 5.585 → 3.151 | 5.784 → 4.025 |
| `fractional_100` | 5.178 → 1.437 | -72.2% | 5.881 → 1.773 | 6.402 → 1.976 |
| `clip_100` | 5.604 → 2.100 | -62.5% | 8.559 → 2.778 | 10.400 → 2.928 |
| `opacity_100` | 8.481 → 8.301 | -2.1% | 9.004 → 9.105 | 9.644 → 9.525 |
| `blend_100` | 8.808 → 8.815 | +0.1% | 9.585 → 9.757 | 9.752 → 10.162 |
| `text_open` | 1.633 → 1.633 | -0.0% | 2.161 → 2.081 | 2.559 → 4.881 |
| `text_covered` | 1.747 → 0.722 | -58.7% | 2.121 → 1.002 | 2.335 → 1.221 |
| `images_open` | 0.506 → 0.484 | -4.4% | 0.917 → 0.933 | 1.086 → 1.090 |
| `images_covered` | 0.509 → 0.353 | -30.7% | 0.850 → 0.852 | 1.779 → 1.139 |
| `blur_full` | 3.244 → 3.163 | -2.5% | 4.474 → 4.094 | 8.290 → 4.878 |
| `backdrop_full` | 3.001 → 3.018 | +0.6% | 3.529 → 3.616 | 3.622 → 3.942 |
| `disjoint` | 0.493 → 0.494 | +0.2% | 0.856 → 0.885 | 1.255 → 1.201 |
| `chart` | 0.820 → 0.808 | -1.4% | 0.978 → 1.217 | 1.310 → 1.332 |
| `chart_modal` | 0.874 → 0.640 | -26.8% | 2.911 → 2.593 | 3.038 → 3.886 |
| `chart_4k` | 1.709 → 1.561 | -8.7% | 6.086 → 1.718 | 7.087 → 2.082 |
| `retained_static` | 0.000 → 0.000 | +0.0% | 0.000 → 0.000 | 0.000 → 0.000 |
| `retained_pan` | 1.093 → 1.067 | -2.3% | 1.379 → 1.463 | 2.229 → 1.863 |
| `retained_sparse` | 0.228 → 0.222 | -2.6% | 0.497 → 0.529 | 0.570 → 0.591 |
| `retained_pan_modal` | 1.185 → 0.979 | -17.3% | 1.293 → 1.210 | 1.399 → 1.294 |
| `svg_open` | 1.189 → 1.165 | -2.0% | 1.286 → 1.239 | 1.573 → 1.652 |
| `svg_covered` | 1.266 → 0.664 | -47.5% | 1.725 → 0.943 | 3.913 → 1.029 |
| `retained_cover_move` | 0.737 → 0.578 | -21.6% | 1.090 → 0.867 | 1.253 → 0.932 |
| `retained_cover_toggle` | 0.846 → 0.797 | -5.8% | 1.147 → 1.088 | 1.584 → 1.685 |

The unchanged retained case is a CPU history no-op (~0.2 µs), rounded to zero in
the table. Small timing changes and isolated tail spikes are not evidence of a
general speedup. Heavy occlusion improves consistently across trials; ordinary
chart/navigation scenes do not establish a significant overall application gain.

### Scene definitions

Unless suffixed `4k`, output is 1280×800; `4k` is 3840×2160. All colors and
placements are deterministic. Workloads use public Canvas/NativeRenderer APIs.

- `overdraw_N`: one opaque background plus N full-screen opaque solid rectangles.
- `transparent_N`: the same geometry with alpha 119 for the N overlays.
- `coverage_N`: 128 alpha-119 rounded full-screen layers, followed by an opaque
  rectangle covering N percent of width; background remains opaque.
- `rounded_100` / `fractional_100`: full cover with radius 70 / inset 0.375 px.
- `clip_100`: radius-41 outer clip; only full-coverage interior tiles can cull.
- `opacity_100` / `blend_100`: the coverage scene inside opacity 0.65 or Multiply
  source-over group; retain its group dependencies.
- `text_*`: repeated 18 px text on a 128×32 grid; `covered` adds an opaque
  foreground and a visible text run after it, preserving glyph index semantics.
- `images_*`: the same grid with bilinear 64×64 images at alpha 230, with/without
  an opaque foreground.
- `blur_full` / `backdrop_full`: 48 deterministic rounded rectangles and sigma-8
  blur; the cover is inside the filter / after the backdrop layer, respectively.
- `disjoint`: 512 separated solid rectangles in a 32×16 grid.
- `chart`: background panels, 12 grid lines, 640 analytic candles and 12 text runs.
  `modal` adds an opaque rounded rectangle covering the central 60% of each axis.
- `svg_*`: the repository tiger SVG scaled to the output, with/without opaque cover.
- `retained_static` / `retained_pan`: the chart unchanged / translated 0↔2 px.
- `retained_sparse`: chart background plus a 32×32 retained foreground translated
  2 px per alternate frame, exercising sparse dirty tiles.
- `retained_pan_modal`: translated chart plus its modal.
- `retained_cover_move` / `retained_cover_toggle`: chart background plus a retained
  rectangle covering 36% of the image, moved 2 px / moved and replaced with
  alternating alpha 0 and 255.

### Real application cross-check

Northstar ran with isolated capture storage and empty live profiles. Both release
executables used the same temporary buffered probe, which recorded actual native
renderer submission CPU time. The scenario navigated settings → strategies →
exchanges → backtests for 24 cycles, at 1280×800 logical size (Retina backing),
in each of three alternating trials. The first 16 submission samples per process
were excluded. The probe excludes layout and presentation; it does not measure FPS.

| CPU submission | Before | After | Change |
| --- | ---: | ---: | ---: |
| Mean | 3.043 ms | 3.064 ms | +0.7% |
| P50 | 3.786 ms | 3.819 ms | +0.9% |
| P95 | 6.531 ms | 6.624 ms | +1.4% |
| P99 | 7.131 ms | 7.249 ms | +1.7% |

Samples: 1009 before, 1013 after. Mean increases by about 21 µs (+0.7%).
This is approximately unchanged within the observed run variability; it is not
evidence of application-wide acceleration or improved frame tails. No live trading
actions occurred. Temporary app/framework probes were restored byte-for-byte.

## Correctness and completion checks

- Eight new GPU tests cover paged/flat lists, 0/1/255/256/257/513 particles, alpha
  0/254/255, wrappers, chunk reduction, repeated preallocated emission, incomplete
  streams, independent fine-only pixels, retained reveal and foreground glyph/images.
- The descriptor capacity boundary has a separate release host test.
- All 36 benchmark cases match baseline RGBA exactly at each trial’s final capture.
- All 1712 SVG fixture outputs and both progressive blur qualities match baseline
  RGBA exactly. Both native window smoke runs complete eight frames and a real resize.
- Final full-library result: 844 passed, 6 pre-existing failures (baseline: 835
  passed, 6 failures). The six full-library failures (five existing glyph/clip coarse
  oracles and the SDF oracle metadata hash) reproduce on frozen baseline. The separate 1600 px
  turbulence reference hash also fails identically. They are pre-existing failures,
  not a claim that the whole suite is green.
- SVG filter/masking/stroke/text/symbol, fractional glyph motion and progressive
  retained GPU integration checks pass; native examples are exercised separately.
- Formatting and release clippy run; existing unrelated lint warnings remain.
- Chinese/English pipeline documentation is updated. The website build could not
  run because npm and website dependencies are absent on this host.
- Matching HLSL source and structural/ABI checks do not establish DX12/Vulkan GPU
  behavior. This host has no pinned DXC; hardware performance results are Metal only.

Temporary benchmark source is removed after measurements; retained tests and this
report describe the maintained semantics and workload definitions.
