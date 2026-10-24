# Compact tile indices and incremental retained geometry

The CPU tile index and GPU work list now store draw references in contiguous
arenas. Retained path preparation also uses the geometry change journal to avoid
scanning unchanged edges. These changes address index allocation/transfer cost
and repeated geometry work directly; they do not cache rendered animations.

## Storage and execution

CPU source membership and GPU visibility references remain separate: source
queries need the complete painter-ordered membership even when an opaque cover
hides earlier draws. Each arena stores one offset/length/capacity record per tile
and a shared `Vec<u32>`. Full builds count memberships, compute exact offsets and
fill contiguous ranges. Local edits use reusable size-class ranges immediately,
without promoting a flat list to 257-word linked pages. Fragmentation over 30%
compacts storage and refreshes affected GPU offsets. Uploads cover actual changed
references; capacity tails are never read.

Metal and HLSL address these ranges directly. The coarse workgroup still processes
256 references, but this is a dispatch unit, not an allocation size or a linked
page. Dense chunks no longer chase pointers, and sparse tiles no longer reserve
256 IDs for a short list. Existing painter order, glyph indexing, clip/stack and
occlusion boundaries remain authoritative.

## Retained geometry

Reuse requires all of the following:

- The previous submission was accepted on the same ordered native queue.
- The materialized physical scene identity matches, independently of plan edits.
- The geometry journal starts at the last accepted version, or the version is unchanged.
- Existing backdrop, tile-range and edge buffers can hold the current extents.

A changed line selects its complete owning path. Selected work includes all edges,
scan chunks, tile backdrops and complete cumulative-sum rows of those paths. This
preserves cross-edge counts and prefixes, including rows spanning multiple chunks.
Unchanged rows are never accumulated again. Sparse dispatch requires the estimated
direct work to fall by at least half, accounting for index packing/upload and
indirection overhead. This avoids misclassifying near-full changes as sparse just
because physical arenas contain vacant slots. Dense edits use ordinary full dispatch;
paint-only frames import accepted geometry outputs without scan/cumsum dispatches.

Version gaps invalidate all derived scene journals together: path plans, tile
membership, glyph capacity, paint packing, text and layer records. GPU draw/paint/
batch uploads also use complete current inputs, while tile/paint/glyph storage and
native buffer capacities remain allocated. A pixel regression first reproduced a missed
color/placement edit followed by an unrelated edit; rescanning paths alone did
not restore those other inputs. An independent run against the unoptimized
baseline also reproduced stale geometry after a skipped materialization version.
These regressions establish the synchronization defect; the application capture
difference below is not attributed to a specific cause without a frame trace.

CPU text preparation tracks its own materialization stamp because it can advance
before GPU submission. Continuous journals update only changed glyph/run ranges;
a skipped or foreign version reconciles the complete current text. A regression
reproduces a skipped glyph position followed by an empty journal and verifies
that the next continuous update still uses the incremental path.

Abandoned batches and output-buffer growth force complete geometry initialization. Path
removal can skip geometry work but still invalidate cached cumulative metadata;
the next full scan refreshes that metadata instead of trusting a consumed journal.
These are permanent correctness rules, covered by focused regressions.

## Measurement method

The baseline is commit `3c3004444b0c9d9693aaaf3017f28f80d8440aeb`, which already
contains early occlusion. Release executables run on Apple M5, Metal adapter
`0000000100000575`, with GPU validation disabled for timing. Three alternating
trials warm 100 frames and measure 160 frames per renderer case. Each case renders
960×640. No compiler jobs or other GPU tests run during measurements.

Wall milliseconds include render, retained mutation where applicable, and explicit
submission completion. CPU milliseconds omit explicit completion. Neither metric
is window FPS or a pure GPU timestamp. Percentiles use nearest rank over pooled
samples; raw samples and executable/source hashes are retained in the benchmark
JSON. Temporary measurement source is removed after use.

Cases cover 2048 cubic shapes, 480 spaced translucent rectangles, 128/512 full-screen
translucent layers, static vectors under a moving SDF foreground with/without a
fractional rounded clip, and 1024 paths partitioned across 128 nodes with one or
all nodes moving. The latter cases separate sparse and dense geometry updates.

The real workload is a copied Northstar / Axiom Terminal working tree switching
through exchanges, strategies, settings and backtests 24 times at a 1488×1056
logical window (2976×2112 Retina target). Isolated storage and empty live profiles
make the navigation repeatable. Both variants use identical temporary automation,
renderer timing probes and API build adaptations; original application sources
remain untouched. Each process records 256 native renderer calls, discards the
first 16, and contributes 240 samples. These CPU call times exclude UI layout,
presentation and explicit GPU completion. Serialization occurs after the timed call.

## Results and validation

| Case | Wall P50 (ms), baseline → new | Change | Wall P95 (ms) | CPU P50 (ms) |
| --- | --- | --- | --- | --- |
| immediate_deep512 | 15.156 → 15.133 | -0.2% | 19.555 → 16.089 | 2.847 → 2.849 |
| immediate_rects | 0.325 → 0.329 | +1.2% | 0.345 → 0.354 | 0.053 → 0.054 |
| immediate_vectors | 1.440 → 1.373 | -4.7% | 1.509 → 1.438 | 0.453 → 0.429 |
| retained_deep128 | 0.397 → 0.390 | -1.5% | 0.605 → 0.415 | 0.041 → 0.038 |
| retained_deep512 | 0.959 → 0.962 | +0.2% | 1.026 → 1.025 | 0.078 → 0.077 |
| retained_path_dense | 1.546 → 1.579 | +2.1% | 1.736 → 1.744 | 0.974 → 1.015 |
| retained_path_sparse | 0.426 → 0.252 | -40.8% | 0.466 → 0.276 | 0.076 → 0.057 |
| retained_sparse_tiles | 0.237 → 0.216 | -8.5% | 0.373 → 0.242 | 0.040 → 0.037 |
| retained_vectors_clip | 0.627 → 0.307 | -51.0% | 0.782 → 0.320 | 0.100 → 0.069 |
| retained_vectors_paint | 0.588 → 0.248 | -57.8% | 0.671 → 0.266 | 0.086 → 0.055 |

The largest gains come from skipping unchanged geometry. The sparse cost estimate
applies a conservative threshold before packing indices, and a regression covers
dense work with vacant physical slots. Dense-update CPU P50 changes from 0.974 to
1.015 ms (+4.2%); individual small differences should not be read as universal gains.

All 30 renderer image pairs have identical decoded RGBA bytes. The complete
SVG corpus also remains unchanged (1712 PNG file hashes).

## Real application diagnostic

| Renderer CPU metric | Baseline (ms) | New (ms) | Change |
| --- | --- | --- | --- |
| mean | 4.155 | 4.145 | -0.2% |
| p50 | 4.846 | 4.942 | +2.0% |
| p95 | 10.659 | 10.646 | -0.1% |
| p99 | 12.379 | 12.517 | +1.1% |

This application comparison has an output limitation: baseline captures retain
only the central empty-state text (4277 of 6285312 pixels have nonzero alpha),
while candidate captures show the complete interface. All three runs reproduce
the difference. These CPU measurements are diagnostic evidence, not a strict
matched-output performance comparison or an application image-equivalence check.
The user reviewed the candidate screenshot and confirmed that it displays correctly.
The user also observed the nearly blank black application window; a process check
confirmed that the running executable was the old baseline.

Complete per-trial/raw samples, source identities and executable hashes are in
[the benchmark JSON](benchmarks/cache-scan-metal-2026-10-12.json).

## Checks and cleanup

- 1258 release tests pass across 45 repository targets,
  including ignored physical-GPU tests. Test and Rayon threads are 1.
- Focused text preparation (27 tests) and scan selection (3 tests) pass, including
  skipped-version reconciliation and dense updates with vacant arena slots.
- The 1712 SVG PNG file hashes and 30 benchmark RGBA pairs remain unchanged.
- Mesh/program/glass/shader graph/pipeline examples, both progressive-blur modes,
  and Metal presentation with eight frames and a real resize pass.
- Formatting, strict release Metal Clippy for all targets, and Chinese/English
  website production builds pass. Default DX12-only Clippy is not a supported
  native configuration on this Mac; without Metal it reports unused native internals.
- Metal runtime uses physical Apple M5 with API validation outside measurements.
  DXIL/SPIR-V compilation passes; DX12/Vulkan hardware performance was not measured.
- Temporary measurement source and copied application trees are removed after use.
  Raw logs, images, samples and frozen executables remain under `target/cache-scan`.
