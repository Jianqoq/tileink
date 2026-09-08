# Native backend implementation record

Status: **M0 in progress; native DX12/Vulkan features are not implemented.**
The four-way exact-pixel contract in [the plan](NATIVE_BACKEND_PLAN.md) is unchanged.

## Fixed reference

- Implementation baseline: `29396c32` (plan commit); original code baseline:
  `eabbe0b97b392582d663206c1f2aad51f76695aa`.
- Local hardware: NVIDIA GeForce RTX 4090; vendor 4318, device 9860;
  DX12/Vulkan LUID `bf3f010000000000`.
- Drivers: DX12 `32.0.16.1062`; Vulkan NVIDIA `610.62`.
- Runtime DXC: Windows SDK 10.0.26100.0 x64 `dxcompiler.dll`, version `1.8.2502.11`.
- WGPU 30.0.1 with registry WGPU HAL 30.0.0. No vendored or patched HAL is used by the fix.

The runner creates a fresh artifact directory for each run. It records the case manifest,
expected frame/route counts, selected adapter identity, source/binary/DXC hashes, texture
capabilities and the files actually used during SVG parsing. SVG trees are parsed once and
shared; external image reads are observed without replacing usvg's resolver semantics.
Resource contents and font-directory membership are rechecked. All raw RGBA channels count,
including RGB under alpha zero. Native/portable here describes WGPU texture execution only.

## Root causes found before native migration

### Illegal DX12 resource state

The filter source and auxiliary image were simultaneously bound as a read-only storage
texture (UAV) and sampled texture (SRV). The DX12 debug layer reported an invalid
`ResourceBarrier` state combination (`0xc8`), followed by command-list close failure
`E_INVALIDARG (0x80070057)` and an invalid device. An empty scene could therefore fail readback.

Filter inputs now use one sampled binding each; integer fetches use `textureLoad` at LOD 0
and linear fetches use the same resource. The target remains the appropriate native or
portable storage/output resource. Binding aliases were removed from both layouts and shaders.
This fixes resource semantics in Tileink; adding waits or a HAL workaround was unnecessary.
The layout regression failed before the fix; it and the explicitly enabled DX12 GPU
empty-scene/readback regression pass after it.

### Pattern coordinate cancellation

The initial complete SVG probe (1712 inputs, WGPU DX12/Vulkan native texture paths) found
126 mismatched frames. Most channel differences were 1, but
`painting/context/with-pattern-and-transform-in-use.svg` had 115 different pixels with a
maximum channel difference of 255. Independent isolated runs reproduced the same mismatch.
Removing the frame and unrelated paths preserved it; replacing the pattern with solid fill
removed it. The failing coordinates contain opposite transform coefficients and equal
pixel coordinates: their exact dot product is zero, but product rounding can leave a
negative residue before `floor`, selecting the texel across a repeat seam.

The production helper now compensates the second product's rounding error with explicit
`fma`, then applies translation. The complete SVG regression changed from 115 mismatched
pixels to **0**, and the separate compute test checks exact cancellation, translations and
axis transforms on both APIs. No tolerance, quantization or per-backend branch was added.
This fixes the reproduced sampling error; it does not establish universal floating-point
identity or resolve the other SVG differences by itself.

## Verification and remaining gate

The reference runner's ordinary tests and the explicit DX12 empty-scene and two-API
numeric regressions pass. Completed SVG, normal SVG/example regression, PNG review and
Criterion results are recorded below. A full run's `complete` flag does not imply
`passed`: every required RGBA comparison must still have zero differences.

M0 remains open until complete SVG/examples, retained frame sequences, texture/compiler
variants, repeat runs and baseline/resize evidence meet the plan. Native feature splitting,
HLSL compilation, native adapters and full native rendering (M1–M6) have not begun. Only the
listed NVIDIA device has been exercised; AMD/Intel/Linux/macOS certification is outstanding.

## Completed SVG cross-API probe

`m0-compensated-svg` completed all 1712 frames through four WGPU combinations on the GPU
above. The report correctly has `complete: true`, **`passed: false`**, 125 mismatched frames:

| Compared with WGPU DX12 native texture | Mismatched frames | Different pixels | Maximum channel delta |
| --- | ---: | ---: | ---: |
| WGPU Vulkan native texture | 124 | 1897 | 2 |
| WGPU DX12 portable texture | 1 | 144 | 255 |
| WGPU Vulkan portable texture | 124 | 1897 | 2 |

The portable DX12 failure is `filters/feMorphology/huge-radius.svg`; its cause has not yet
been established. It was outside the first two-route probe, so the aggregate counts are
not directly comparable. The pattern cancellation SVG is identical in all four combinations.
The remaining differences are retained as M0 investigation inputs, not waived or made into
separate goldens. The run used the absolute SDK compiler path; subsequent path validation
also resolves relative `--dxc` inputs before both DLL loading and manifest hashing.

## Current fix-batch verification

- Standard SVG rendering: all **1712** inputs completed, with exact native/portable WGPU
  texture comparisons passing.
- Standard examples: **45** generated outputs matched between native/portable WGPU modes.
- Strict repository PNG check against `29396c32`: **3488** files; 3486 byte-identical;
  two reviewed pixel changes (67 pixels in the rotated context pattern, one RGB +1 pixel in
  `structure/image/with-transform`). The comparator returns failure for these real changes;
  they were explicitly accepted in human review on 2026-09-07 and are not hidden by tolerance.
- Ordinary reference-runner tests: 15 passed; the two GPU-only tests are explicitly ignored
  in the ordinary command and were run separately on DX12/Vulkan. The DX12 empty-scene
  opt-in regression was also explicitly enabled and passed.
- Standards/spec reviews found and fixed PNG finalization, external-image/font resource
  tracking and compiler-path identity gaps, with regressions. Final focused reviews found
  no outstanding issue in those fixes. This is a review of the fix batch, not native-feature
  completion or a waiver of the 125 cross-API differences.

### Pattern performance check

Same RTX 4090 / Vulkan, identical Criterion benchmark sources, separate old/new binaries,
20 samples per case, 2-second warmup and 4-second collection. Every measured frame waits
for GPU completion; setup and readback are excluded. `performance-binaries.json` and
`performance-summary.json` retain the binary hashes and estimates under the local verification
artifact directory. Criterion classified all four as noise/no change, with no regression:

| WGPU texture mode / width | Before (µs) | After (µs) | Criterion result |
| --- | ---: | ---: | --- |
| native / 300 | 303.61 | 301.65 | Within noise threshold |
| native / 1600 | 549.08 | 549.03 | No change detected |
| portable / 300 | 300.60 | 304.69 | Within noise threshold |
| portable / 1600 | 546.49 | 539.98 | Within noise threshold |

The small portable case increased by about 4 µs in the point estimate; this was within
Criterion's noise classification. These results cover this pattern workload's steady state,
not all workloads, DX12 timing, resize PMax, cold-start latency or a native-API speedup.
Final verification passed: all 12 single-threaded release test groups from
`scripts/ps1/run_tests.ps1 -WgpuMode both`, `cargo fmt --all -- --check`, and
`cargo clippy --offline --release --all-targets -- -D warnings`. The final runtime
smoke test loaded the resolved SDK DXC path and matched all four WGPU combinations
for an empty SVG. Logs are retained under `target/native-m0-verification/`; the
full cross-API SVG gate above still fails and remains required for M0 completion.

## Reproduction

See [the runner commands](scripts/ps1/README.md#explicit-wgpu-dx12vulkan-reference).
Local diagnostic artifacts are under `target/backend-parity/` and are intentionally ignored
by Git. Keep the manifest with each report; do not merge results from different runs.
