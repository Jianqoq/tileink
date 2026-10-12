# Sampled programmable filters and native glass comparison

`ShaderProgram::new` accepts complete application-provided Metal/HLSL source and
compiles it as supplied. The public ABI and scheduling contract are documented
in [Native application shaders](program-brush-filter.md). Neither an expression
interpreter nor an expression-to-shader generator remains.

`examples/shaders/glass.metal` and `glass.hlsl` are standalone user kernels for a
rounded lens with refraction, tint, edge highlights and optional RGB dispersion.
The example composes the filter as a real backdrop layer. Ordinary filters read
their own layer; backdrops read earlier background. Input-reading kernels use
separate immutable input/output storage and copy the affected output region back.

Backdrop capture includes the complete declared input halo. Retained parameter
replacement replays that halo before filtering, excluding later foreground.
These are permanent fixes for two reproduced input-history bugs, covered by
regressions for halo reads and retained source/parameter updates.

## Current user-source comparison

Apple M5, Metal registry `0000000100000575`, validation disabled. The exact same
complete `examples/shaders/glass.metal` file is compiled by the Metal device
compiler and by the offline `metal`/`metallib` tools. Both use Metal 2.4, disabled
fast math and preserved invariance. There is no generated reference shader.
Each case has three alternating-order, serial trials, 16 warm-up and 80 timed
iterations per variant: 240 samples per cell. Timings measure GPU effect plus
identical copy-back, excluding compilation, upload, readback and scene rendering.

| Formula | Dispatch | Runtime compiler p50 | Offline compiler p50 | Difference |
| --- | --- | ---: | ---: | ---: |
| Basic, one sample | 320×200 | 0.01871 ms | 0.01867 ms | +0.22% |
| Basic, one sample | 1280×800 | 0.21375 ms | 0.21313 ms | +0.29% |
| Basic, one sample | 3840×2160 | 1.71212 ms | 1.71031 ms | +0.11% |
| RGB dispersion, three samples | 320×200 | 0.02250 ms | 0.02250 ms | <0.01% |
| RGB dispersion, three samples | 1280×800 | 0.26763 ms | 0.26752 ms | +0.04% |
| RGB dispersion, three samples | 3840×2160 | 2.15377 ms | 2.14944 ms | +0.20% |

All **18 runtime/offline output pairs have exactly equal RGBA bytes**. These
measurements support equivalent steady GPU execution for this user kernel on
this device; they do not include first-use compilation or establish equivalence
between different effects or backends.

The actual `NativeRenderer` workload additionally includes the checkerboard
scene from `program_glass`, backdrop capture, the user kernel, copy-back,
compositing and CPU submit/wait. Three alternating-order trials use 16 warm-up
and 80 timed frames per case. Baseline draws the same background without a
filter. Basic and dispersion do not include blur.

| Canvas / effect region | Background p50 | Basic glass p50 | Dispersion glass p50 |
| --- | ---: | ---: | ---: |
| 1280×800 / centered 320×200 | 0.578 ms | 0.629 ms | 0.633 ms |
| 1280×800 / full canvas | 0.551 ms | 1.028 ms | 1.082 ms |
| 3840×2160 / full canvas | 2.867 ms | 5.779 ms | 6.210 ms |

Native compilation removes interpreter execution cost. Backdrop capture,
scratch storage, copy-back and compositing still contribute to the whole-frame
cost. A native `ProgramBrush` additionally needs its GPU raster prepass and an
image sample during fine compositing.

Current raw samples, source/library/binary hashes and protocols are in
[the user-source JSON](benchmarks/user-shader-metal-2026-10-11.json).

## Verification

At the comparison revision, the release suite ran with one test thread and one Rayon thread,
including ignored physical-GPU tests: **967 passed, 9 failed**. The failures already occurred
in the prior implementation: one exact same-device SDF/WGSL comparison, five
coarse count/emit oracle comparisons, two tests requiring the unavailable pinned
DXC installation, and `scaled_turbulence_matches_canonical_rgba`. These failures
were not reported as passing.

These historical failures have since been resolved. The [cleanup record](cleanup-2026-10.md#phase-48-resolve-the-three-outstanding-test-failures)
details the last three: pinned DXC discovery and real HLSL compilation errors are fixed, and
scaled turbulence is checked against an independent SVG noise reference rather than an archived
GPU hash. Actual DXC checks now run for both DXIL and SPIR-V; the Metal turbulence output is unchanged.

New native-source semantic and regression tests pass. Coverage includes complete
source preservation, parameter bits and pipeline identity, compiler/entry/backend/
binding errors, failed-library cleanup, Brush and Flood coverage, coordinates,
sampling, compact dispatch, chains, graphs, retained updates and backdrop halos.
The full **1730 SVG images match the earlier renderer byte-for-byte after RGBA
decoding**. All release examples build; program paint, glass, mesh, progressive
blur and an eight-frame native window/resize smoke run execute successfully with
Metal validation. Formatting and release Clippy checks complete; existing lint
warnings remain.

GPU execution and measurements use Apple M5. At the comparison revision, the host
had neither pinned DXC nor DX12/Vulkan devices. Later cleanup verifies actual DXC
compilation and reflection; HLSL device execution and runtime/offline performance
equivalence still require those GPU platforms. Website content is updated; its build
is not run because the website dependencies are not installed.

## Historical interpreter comparison

The following results describe the removed experimental interpreter and its
same-expression compiled reference. They remain historical measurements; they
do not describe the current user-source API.

The previous experimental programmable path was a GPU interpreter. It did not
compile each user's expression graph into a specialized native shader. This
comparison emits the same expression graph as both serialized instructions for
the actual production interpreter and straight-line Metal code. Both retain
the same arithmetic validity checks, uniforms, transparent-border sampler,
premultiplied RGBA8 output and copy-back pass.

Apple M5, Metal registry `0000000100000575`, validation disabled. Each variant
uses private RGBA8 source, scratch and target textures and the same 256-thread
dispatch. Three trials alternate variant order, with 16 warm-up and 80 measured
iterations per process: 240 measurements per variant/case. No other GPU job or
compilation runs concurrently. Input is the repository's rendered tiger SVG,
resized identically for both variants.

The primary timing is **GPU command-buffer elapsed time for effect plus copy**.
It excludes scene rendering, upload, readback, compilation and presentation.
CPU encode/submit/wait timings are also recorded in the JSON. These GPU timings
must not be compared directly with earlier whole-`NativeRenderer.render` wall
times. A shared blur stage would add cost to both variants and reduce their
end-to-end ratio without removing interpreter overhead.

| Formula | Effect dispatch | Compiled p50 | Interpreter p50 | Ratio |
| --- | --- | ---: | ---: | ---: |
| Basic, one sample | 320×200 | 0.023 ms | 0.464 ms | 20.3× |
| Basic, one sample | 1280×800 | 0.276 ms | 7.117 ms | 25.8× |
| Basic, one sample | 3840×2160 | 2.194 ms | 58.792 ms | 26.8× |
| RGB dispersion, three samples | 320×200 | 0.030 ms | 0.554 ms | 18.2× |
| RGB dispersion, three samples | 1280×800 | 0.389 ms | 8.518 ms | 21.9× |
| RGB dispersion, three samples | 3840×2160 | 3.106 ms | 70.094 ms | 22.6× |

The small effect occupies `(480,300)..(800,500)` in a 1280×800 input image.
Large and 4K dispatches cover the whole input. Every native/interpreter pair
has **exactly equal decoded RGBA**, including all three trials: 18 comparisons,
zero differing pixels and zero maximum channel difference. A separate Metal
validation-layer smoke run also passes.

One compiled basic/small trial was slower than the other two (approximately
0.103 versus 0.023 ms); the aggregate p95 is 0.103 ms. All measurements are
retained, including this variation. The table reports aggregate medians rather
than selecting the fastest trial.

## Why the architecture changed

The old interpreter decoded operations, checked operands and dynamically indexed
registers per pixel. Native compilation exposes the whole shader to register
allocation and instruction scheduling. The interpreter and expression builder
have now been removed. Tileink accepts complete application Metal/HLSL source,
compiles it verbatim and caches the resulting native pipelines. Uniform changes
only update data. It does not generate effect code.

Historical raw measurements, formula/binary hashes and image comparisons remain
in [the old JSON](benchmarks/program-glass-metal-2026-10-11.json). Frozen binary
artifacts and output images remain under `target/glass-filter-validation`;
temporary measurement source is removed after verification.

## Ordinary application controls

Long sequential controls showed scheduling/thermal variance, including a +6.40%
aggregate for 128 transparent layers. A follow-up puts the committed baseline
and user-source renderer in the same process, alternating variant order on every
frame: three trials, 500 warm-up and 1000 timed frames per variant. All image
pairs match exactly. Combined p50 is 5.30254 vs 5.32258 ms (+0.38%). Raw samples
and the earlier measurements are retained in the current JSON report. This
control precedes the explicit pipeline extension.
