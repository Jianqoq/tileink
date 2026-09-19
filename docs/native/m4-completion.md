# Windows M4 completion — 2026-09-19

Windows M4 immediate rendering acceptance is complete. Native DX12/Vulkan execute
the shared Canvas/frame/filter schedule with HLSL, owned contexts and explicit
submission/readback. Wgpu remains the default. All 179 Windows program variants
have focused coverage; complete scenes now pass the corpus gate.

## Exact corpus acceptance

| Corpus | Completed | Routes | Different pixels / maximum channel delta |
| --- | ---: | ---: | --- |
| SVG | 1,712 / 1,712 | 6 | 0 / 0 |
| Example images | 45 / 45 | 6 | 0 / 0 |

Routes are wgpu DX12/Vulkan with native and portable texture modes, plus native
DX12/Vulkan. All use RTX 4090, LUID `0f42010000000000`, vendor 4318/device 9860;
DX12 driver `32.0.16.1062`, Vulkan driver `610.62`. Untested GPUs/drivers are not
certified. Native validation is enabled. The known DX12 optimized-clear advisory
is retained in logs; no unexplained validation errors remain.

Both manifests identify executable SHA-256
`c553807ecb8c3154fe527ebbacadd64fb22ce391fba4431dd20e48710d2cc417`.
Local evidence is under `G:/Code/northstar-trading-app/target/agent-work/`:

| Artifact | SHA-256 |
| --- | --- |
| `m4-native-svg-4/report.json` | `fa0bfe99e713a68279d150680295811953c51e395a2be4b33b36e779c9bb8da0` |
| `m4-native-svg-4/manifest.json` | `a1d35b8247b0a2846fee46c3aa175fc98c48bc079f9c3e8843f01d3b846e13b5` |
| `m4-native-examples-4/report.json` | `0fccfc4656bf799e77a6ed22060bf61c55410db5f1ee1acc555fb92e4e9540c6` |
| `m4-native-examples-4/manifest.json` | `e4909643aaa31e2fffd9cd6794687efe5f68c53f109a39a80922c585343b132e` |

See [runner commands](m4-corpus-runner.md). Actual RGBA bytes, including alpha,
are compared without tolerance or backend-specific references. Each route uses
the same example callbacks and frozen font/SVG inputs; capture failures cannot
silently select another renderer.

## Root fixes and shared structure

- Correct native morphology's axis field and clear empty tile inputs.
- Match fused byte-domain convolution and Box downsample arithmetic, including
  finite extreme divisors/bias. Independent numeric oracles cover these paths.
- Use the shared direct backdrop schedule on native. Pre-upsampling simple glass
  inserted an extra RGBA8 quantization: 12,683 changed pixels reproduced before
  the fix, then zero on all four APIs. Recording failures abort instead of trying
  another schedule or drawing foreground.
- Share scratch-slot leasing and explicit submission policy. Physical allocation
  and synchronization remain API-specific; native immediate batches own resources
  until completion. Persistent shader/pipeline caches remain enabled.
- Remove unconditional example profiling as requested. No performance comparison.

Detailed causes and regressions: [corpus boundaries](m4-corpus-boundaries.md).

## Verification and image review

Serial release verification passes: 1,048 library tests (130 explicitly ignored),
integration suites and 29 ordinary parity-runner tests. The exact simple-glass
GPU regression and native capture failure/recovery test run separately and pass.
Focused convolution, resample and SVG-boundary GPU regressions pass. Shared
backdrop scheduling has 18 passing tests, including abort-without-fallback.
The two retained removal/blur-halo GPU regressions also pass on DX12 and Vulkan
in both texture modes, checking the shared scratch lease change across frames.
Logs: `m4-retained-slots-{api}-{mode}.log`; these are correctness tests, not timings
used for performance comparisons.

Default, native-DX12-only, native-Vulkan-only and combined native-only checks,
formatting and strict native all-target Clippy pass. Logs: `m4-closeout-*.log`.
Standards and spec reviews closed without outstanding findings.

Full standard SVG/example generation retains all 3,471 PNG files. The previously
approved turbulence change affects 32,390 pixels. Five additional images were
reviewed and approved before commit: convolution bias -0.5 (2,880), downsample
blur (4,024), default glass (3,447), mixed glass (4,286), simple glass (3,659).
Each additional image has maximum channel delta 1. Immutable visual review:
`m4-corpus-png-review/review.md`. There are no other PNG changes.

## Remaining scope

M5 covers native retained frames, imported targets, resize/device-loss semantics,
continuous-frame GPU reuse and host interop. M6's broader GPU/platform matrix is
not implied by single-device acceptance. macOS keeps independent MSL source;
Apple compilation/GPU validation is deferred until a Mac is available. No gfx_ui
or trading-app migration is included.
