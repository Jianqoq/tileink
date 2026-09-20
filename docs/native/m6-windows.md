# M6 Windows acceptance

This delivery is limited to available Windows hardware and DX12/Vulkan. Intel,
Linux, and fresh macOS M6 runs remain unverified. The existing Apple M2 M1–M5
receipt is documented in [Metal](metal.md); it is not a Windows M6 result.
Performance comparisons remain excluded at the user's request.

## Separate backend executables

`scripts/ps1/run_native_acceptance.ps1` builds the `windows_corpus` release test
separately with `wgpu`, `dx12`, and `vulkan`. No build enables two renderer
features. Each GPU process uses one explicit route and the same physical Windows
LUID. Software adapters and unavailable requested devices cannot certify a run.

```powershell
$env:TILEINK_PARITY_DXCOMPILER = '<pinned SDK dxcompiler.dll>'
$env:VK_LAYER_PATH = '<Vulkan validation layer directory>'
$env:VK_LAYER_VALIDATE_SYNC = '1'
# Native DXC can use the documented cache discovery or an explicit override.
$env:TILEINK_NATIVE_DXC_PATH = '<dxc.exe>'
.\scripts\ps1\run_native_acceptance.ps1 -Output '<new evidence directory>' -Python python
# Optional: select specific physical LUIDs or one suite for diagnosis.
# -Gpu '<16 hex digits>' -Suite retained
```

Python 3.9+ and the Rust/build dependencies are required; the orchestrator uses
only Python's standard library. Output must be outside the checkout or inside a
git-ignored directory. Existing output directories are rejected.

The complete matrix has seven routes: wgpu-DX12 native/portable textures and
embedded DXIL, wgpu-Vulkan native/portable textures, native DX12, and native Vulkan.
Every selected GPU runs each suite three times in fresh, serial processes:

| Suite | Expected outputs per route/run |
| --- | ---: |
| SVG | 1,712 |
| Examples | 45 |
| Retained | 29 frames × 3 target contracts × Auto/ForceFull = 174 |

Retained comparisons also compare all six target/mode variants to each other.
The scenes, font capture, SVG loading, readback and retained semantic contracts
reuse the maintained parity fixtures. Shader caches remain enabled; independent
processes do not imply deleting shader or driver caches.

## Evidence and failure rules

The orchestrator freezes source hashes before compiling, verifies them after
each build and during execution, and associates the exact executable hash with
its source snapshot. Resource/font manifests must agree across processes.
Runtime reports begin in a failed state and become successful only after every
expected case/variant is present and validation has completed.

Each output is raw premultiplied RGBA8 without row padding. Comparisons read the
actual bytes, including RGB at zero alpha; dimensions and SHA-256 are checked as
well. Missing, duplicate, corrupt or unexpected output is a failure. The runner
preserves all raw images, per-case comparison records, process logs, build logs,
adapter metadata and a final receipt. A pixel failure records both raw paths,
dimensions and exact difference counts. A nonzero process status or API error
diagnostic fails the run. Wgpu API validation is explicitly enabled in release.
DX12 also requires the debug interface to be available; wgpu can otherwise skip
validation silently. Vulkan validation-layer/debug-utils absence fails acceptance.
DX12 shader DEBUG is disabled because wgpu uses it to disable DXC optimization;
Vulkan DEBUG remains enabled for its validation messenger.

The optional `rounding` suite runs the two lighting/turbulence regressions through
the same device-pinned image comparator. It does not assume identical absolute
lighting values on different vendors. Both fixtures are also in the full SVG suite.
A suite-only run certifies that suite, not the full matrix. Compare the NVIDIA
reference separately against the frozen, approved M5 PNG baseline; agreement
between newly built backends alone does not prove baseline preservation.

## Engineering changes

- Backend-specific host helpers and generated constants compile only for their
  consumers. Wgpu scan-buffer debug collection lives in `src/debug/wgpu.rs`;
  public capture types remain shared. Existing semantic tests remain in the
  appropriate backend matrix.
- Native full tile-bin uploads now consume and recycle their dirty journals.
  This fixes retained geometry edits accumulating unused dirty entries; it is
  not a workaround. It preserves complete upload bytes and does not add a sort
  of ranges that the native full-snapshot path never uses. A 256-update regression
  includes batches abandoned before submission.
- Additional host checks exercise invalid scatter ranges, layer opacity and
  geometry domains, and direct/surface composite recording. The lifecycle GPU
  suite also exercises synchronous probe reuse after queued retirement.
- Brush buffer reads use nested positive bounds checks. The equivalent early-out
  OR expression lost valid pattern reads on AMD Vulkan driver 24.30.18: the image
  and brush uploads were intact, but the 8×4 vector-image regression rendered
  transparent pixels. This is a workaround for observed shader compilation-path
  behavior; DXC versus driver responsibility is not independently attributed.
  Bounds and addition-overflow protection remain intact. GPU tests cover the last
  valid word, nonzero and invalid bases, large offsets and untouched output tails.
- Gaussian weights advance through the same two single-tap steps in every
  implementation. A positive-zero runtime operand protects required product
  rounding from literal-zero folding in the tested compilers. The existing
  reserved filter word holds this operand; native recording rejects other bits
  before allocation. This is a compilation workaround, not a universal guarantee
  about arbitrary toolchains. MSL mirrors the filter changes but requires fresh
  Mac validation.
- DXIL no longer globally marks FMad precise: DXIL defines precise FMad as
  non-fused, conflicting with the interpolation contract. A GPU regression
  checks values immediately below half-channel boundaries. SPIR-V retains its
  strict compilation policy and explicit Fma operations.
- Local `precise` declarations in pattern/clip helpers propagated into production
  intersection FMad operations. Removing that propagation restores fused
  intersections. Full-renderer regressions cover tile edges, tiger and transformed
  image sampling; standalone helper compilation did not reproduce the problem.
- Gradient ramp range reduction explicitly fuses `t * last - index` in all three
  shader languages, avoiding a separately rounded fraction at channel boundaries.
- Lighting preserves the innermost product of its nested fused dot. Literal-zero
  folding changed the spotlight attenuation across APIs on AMD; the production
  regression covers the resulting green/alpha half-channel boundary.
- Turbulence preserves the second gradient product before the fused dot and
  interpolation, using the validated runtime positive-zero operand. A production
  renderer regression catches optimizer reassociation that changed alpha 7 to 8
  on AMD wgpu-DX12. This is a tested compilation workaround; new MSL changes still
  require Mac execution. Temporary probes and unsuccessful compensations were
  removed.

## Completion evidence

The [verification receipt](m6-windows-verification.json) records the evidence paths
and SHA-256 hashes. Both device matrices use the same frozen source snapshot.

| Device | Vulkan driver | Runs / routes | Raw RGBA outputs | Different pixels |
| --- | --- | --- | ---: | ---: |
| NVIDIA GeForce RTX 4090 | 610.62 | 3 / 7 | 40,551 | 0 |
| AMD Radeon(TM) Graphics | 24.30.18 | 3 / 7 | 40,551 | 0 |

All 126 corpus processes pass API validation and compare actual image bytes.
Native lifecycle GPU tests pass on both physical devices: 29 DX12 and 24 Vulkan
tests per device. Both native window examples present eight frames and resize
from 640×360 to 480×270 on the default adapter; this is not a two-device window
certification. Release tests pass separately for wgpu (1,102), DX12 (813) and
Vulkan (821), including example test targets. Final corpus tests and strict
all-target Clippy pass for all three features. Standalone HLSL header compilation
and invalid-source rejection pass for DXIL and SPIR-V. Native normal dependency
trees contain no wgpu runtime.

Doc-test commands succeed for all three features (currently zero doctests).
The final `.crate` archive was extracted
outside the checkout and passes release library checks separately with wgpu,
DX12 and Vulkan. The user's untracked `.vscode` directory is excluded from both
the package and the intended commit.

The final NVIDIA reference matches the submitted human-review candidate for all
1,786 baseline images. Relative to the approved M5 baseline, 17 SVGs and 5 examples
change a total of 1,483 pixels, with maximum channel delta 1; all 29 retained
baseline frames remain unchanged. This is separate from cross-backend acceptance,
which has zero differences. After receiving this review summary and the PNG review
link, the user authorized commit/push on 2026-09-20. This accepts the submitted PNG
changes and closes the scoped Windows M6 delivery. The delivery commit contains
this record; the broader platform matrix remains open.

After GPU verification, only completion documentation and the receipt may change;
the source audit records those exceptions. Unavailable environments do not count
as passes and do not close the all-platform M6 checklist. Mirrored MSL numerical
changes require fresh execution on Mac, regardless of the earlier M1–M5 receipt.
