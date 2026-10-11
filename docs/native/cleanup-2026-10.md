# Native implementation cleanup — October 2026

This audit is ongoing. Phase 1 removes obsolete scheduler architecture and silent substitutes; it does not claim that every repository module has been audited.

## Removed in phase 1

- The test-only `CommandBatch`, `BatchAdapter`, uniform arenas, resource readiness ledger, early-root submission policy and portable root ping-pong executor. The native renderer never used these models. Conformance tests now exercise production `ComputeBatch` submissions, immutable per-dispatch uniforms, rejected work and receipt ownership.
- The generic frame adapter with no-op preparation methods, impossible submission-policy branches and one-use `Option` state. Native frame recording now executes directly in its caller-owned batch. Output routing and synchronization retain their actual ownership boundaries.
- Boolean/optional filter recording status and its out-of-band native error latch. `Result` preserves the original graph/allocation/recording error. Scratch helpers release every acquired intermediate on recording and cleanup failures, including unpublished graph outputs. This fixes the actual lost-error issue; it is not a temporary bridge.
- Offscreen reconstruction from old draw lists when a selected batch was absent. Selection now represents every requested batch, including empty batches, and shares one membership map. Geometry translation no longer ignores invalid internal indices or replaces malformed encoded brushes with transparent draws. Geometry, selection and tests have separate modules.
- The `TILEINK_DXC_PATH` compatibility alias. Build-time and runtime compiler selection use `TILEINK_NATIVE_DXC_PATH` consistently.

## Validation

Focused release tests pass. The broader single-thread Metal release suite reports 956 passing tests and eight previously observed failures: six Metal reference/particle oracle tests and two tests requiring unavailable DXC. The previously failing scaled-turbulence test passed this run; this cleanup does not claim to have fixed it. Obsolete model tests were removed with their implementations, and meaningful ordering, partial-frame, error and ownership coverage was moved to the current paths.

All 1730 SVG render outputs and seven rendering examples are byte-identical to commit `2ea5d59c`. The Metal window presentation smoke test passes. Release Clippy, example builds and the backend-free check pass; existing unrelated lint warnings remain. DX12/Vulkan GPU execution is not available on this host.

## Real renderer performance

Apple M5, pinned registry `0000000100000575`, release mode, one CPU thread, validation disabled. Baseline commit `2ea5d59c` and the cleanup renderer are linked into the same process. Each workload warms 100 paired frames and measures 600 frames per version, alternating execution order for every pair. Timings include CPU recording, submission and explicit GPU completion; compilation and readback are excluded. Each scene also requires exact baseline/cleanup pixel equality.

The measured p50 changes range from -1.33% to +0.76%. These small variations do not establish a speedup or a material slowdown. The purpose of this phase is simpler ownership and accurate errors while preserving rendering cost.

| Workload | Before p50 ms | After p50 ms | Change | Before p95 ms | After p95 ms |
|---|---:|---:|---:|---:|---:|
| ordinary | 0.724 | 0.715 | -1.33% | 0.797 | 0.786 |
| blur_small | 1.162 | 1.161 | -0.10% | 1.247 | 1.259 |
| blur_full | 4.116 | 4.120 | +0.11% | 4.939 | 4.953 |
| blur_4k | 29.230 | 29.222 | -0.03% | 30.492 | 30.460 |
| glass_small | 0.955 | 0.960 | +0.54% | 1.026 | 1.022 |
| glass_full | 1.805 | 1.806 | +0.07% | 2.626 | 2.621 |
| progressive | 2.317 | 2.322 | +0.18% | 3.189 | 3.199 |
| shadow | 3.943 | 3.973 | +0.76% | 4.829 | 4.844 |
| color_chain | 1.619 | 1.617 | -0.15% | 1.827 | 2.445 |
| graph | 5.151 | 5.179 | +0.56% | 6.179 | 6.212 |
| custom_glass | 0.816 | 0.812 | -0.44% | 0.931 | 0.932 |
| isolated_blur | 0.952 | 0.952 | +0.06% | 1.029 | 1.029 |
| isolated_path_blur | 1.141 | 1.142 | +0.05% | 1.216 | 1.222 |

[Raw measurements and protocol](benchmarks/cleanup-phase1-metal-2026-10-11.json). Temporary measurement drivers are removed after validation.

## Remaining audit work

- DX12/Vulkan still have separate probe frame/pipeline implementations; Metal already records those verification workloads through production `ComputeBatch`. Preserve the probe oracles while removing duplicate backends.
- Contextless private paths still substitute CPU uploads for pooled surfaces, persistent buffers and shader resources. Make allocator requirements explicit and migrate their conformance tests before removal.
- Continue auditing retained-scene reconciliation, SVG/text semantics, native resource lifetime boundaries, shaders, examples and scripts. Necessary SVG paint/font behavior, degenerate geometry semantics and unsafe GPU ownership/bounds validation must remain correct; a keyword match alone does not establish an obsolete implementation.
