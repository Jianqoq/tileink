# M4 native frame filters and backdrops

This step connects the shared recursive layer and filter-program schedulers to native DX12/Vulkan command recording. It does not mark M4 complete: text/frame resource preparation, the public NativeRenderer, pooled uniforms/submission integration and complete immediate SVG/example acceptance remain outstanding.

## Invariants

- `render::layers` owns group/filter/backdrop selection. Native code maps each `FilterKernel` to its validated production encoder; it never substitutes WGSL or a CPU image.
- `shared::filter_parameters` owns pure parameter conversion for both wgpu and native: convolution, displacement, turbulence, matrix, lighting and glass use the same mappings and units. Numeric shader constants remain in HLSLI.
- A local filter retains the exact Canvas/ExecPlan pair supplied by the shared localizer. A new local context allocates separate batch-owned targets, tables and geometry; scan is recorded only at `scan_filter_scene`, after the shared scheduler prepares its target. Parent context restoration occurs even when scan/recording fails. Failure invalidates the whole batch; callers must not submit partially recorded work.
- Explicit scene plans invalidate the previous cached Canvas fingerprint. Otherwise a later root render could reuse localized draw membership under the old fingerprint. The regression reproduces this root cause before the fix.
- Filter tables follow the shared traversal and append the enclosing filter after localized children. Whole-root plan scopes retain their original cursor offsets.
- Native brush upload is constructed from typed plan/filter data, and stores actual brush record starts. Dispatch rejects interior/padded offsets, source/destination aliases and foreign batches. Its image descriptors must match the immutable placement upload used to patch the brush records.
- Offscreen results remain GPU resources within the same ordered ComputeBatch; no intermediate readback or submission is introduced. Logical scratch release does not free submitted physical resources.
- Immediate backdrop execution preserves painter order. Partial retained backdrop history remains an explicit unsupported path pending M5; a fresh immediate Execution does not create such history.

## Validation

Initial complete Canvas comparison: twelve filter/backdrop combinations, all native/portable and atlas/table fine variants, against both production wgpu Renderers, passed with exact RGBA bytes (162.44 s). This is correctness validation, not a performance comparison.

Expanded tests add local point/spot lighting, nested convolution and turbulence tables, graph blend/displacement/tile/arithmetic composition and liquid glass. All 21 combinations passed exact four-API comparisons across all fine/filter variants (166.11 s).

Two additional four-API regressions passed (1.64 s): tile cells crossing logical surface boundaries and physically oversized pooled inputs with deliberately colored padding. The latter reproduced a wgpu bug: texture-load robustness only checks physical allocation bounds. Both shader implementations now explicitly return transparent pixels outside the logical input domain. This fixes the cause without changing comparison tolerances.

CPU regression and edge tests cover cached-plan pollution, failure before context activation, scan failure followed by parent restoration, invalid brush offsets, texture aliasing and foreign batches.

Final release validation passed 1,035 ordinary unit tests plus integrations, formatting, strict all-target Clippy and the native-only build. Full SVG and example rendering passed; all 3,471 PNGs were compared with the preserved baseline, with only the previously approved turbulence change. Standards and Spec reviews closed with no outstanding findings. No performance comparison was run.

The Windows session changed between runs. On 2026-09-19 both API inventories selected NVIDIA GeForce RTX 4090, device 9860/vendor 4318, LUID `0f42010000000000`; reported drivers were DX12 `32.0.16.1062` and Vulkan `610.62`. The same-GPU wgpu smoke suite passed before restarting the expanded native checks. The previous LUID is not silently accepted or replaced during a test.
