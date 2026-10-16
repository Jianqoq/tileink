# Native shader pipelines

`ShaderPipeline` exposes explicit resources and ordered compute passes. Application
kernels are complete user-provided Metal/HLSL source files, compiled verbatim.
There is no effect builder, shader source generator, or GPU interpreter.
`ShaderProgram` remains the convenient fixed-ABI single-pass brush/filter API.

The pipeline interface supports:

- Multiple immutable input images and nearest/linear clamp samplers.
- Raw read-only and read/write word buffers, reset from immutable CPU snapshots
  on each invocation. Writes remain on the GPU between passes.
- RGBA8, RGBA16F and RGBA32F intermediate textures; floating-point values retain
  negatives and values above one until the application's final output conversion.
- Write-only and in-place read/write textures; multiple outputs in one kernel.
- Caller-chosen workgroups, 2D extent dispatches and explicit 3D group counts.
- Fixed or surface-relative sizes, including rounded-up downsampled dimensions.
- Native resource pools, memory dependencies, chains/graphs, backdrops and
  retained updates. No CPU waits or readbacks between passes.

## Built-in filters

All built-in filter GPU stages now execute through `ShaderPipeline`, including
Gaussian/shared-memory blur and resampling, progressive blur, liquid glass,
shadows, flood/brushes, offsets, morphology, color adjustments/matrices/component
transfer, convolution, lighting, turbulence, displacement, tile, blend and
composite. Chain/graph planning and their mask/surface composition stages use
the same executor as well. Existing `Filter` constructors remain convenient
algorithm descriptions; they no longer dispatch through a separate filter GPU
execution path.

Built-ins now construct pipelines through the **public** `ShaderKernel::precompiled`,
`ShaderPipeline::new`, `ShaderResource::External` and `ShaderPass` API. There is no
private bound-resource wrapper or unchecked pipeline constructor. Application
kernels may use complete source through `ShaderKernel::new`; the execution path
and resource/binding validator are the same. Build-compiled kernels preserve
startup performance and their existing native ABI.

`Filter::Program`, `ProgramFilter` and `ProgramToTarget` have been removed.
`ShaderProgram::filter(uniforms)` is only a convenience constructor returning
`ShaderPipeline`, using the public Config/ActiveTiles/Buffer resources and Pixels
dispatch. It has no separate executor. Update its parameter resource with
`with_resource(3, ShaderResource::Buffer(words.into()))`, or construct a new
pipeline with `program.filter(words)`; program kernels share their compile cache.
Brush rasterization also constructs an ordinary pipeline, including cropped
output origins. Point/sample dependencies, DPI and compact tile semantics remain.

For direct native invocations, `NativeShaderBatch` exposes owned buffer/texture/
array/table/sampler creation and zero-copy persistent texture import. Pass opaque
`NativeShaderResource` handles to `record(&pipeline, &resources)` in External
index order, then `submit()?.wait()?`. A native invocation may write external
resources without a Canvas Output descriptor. It accepts External, Uniform,
Buffer and Sampler resources with explicit Groups dispatch; Canvas resolves its
own Input/Output/Config/ActiveTiles/extent resources. Canvas rejects native-only
pipelines. Foreign batch/device handles, wrong kinds/formats, short constant
blocks, wrong table counts and writable aliases are rejected. The pipeline plan
never stores batch handles and can be reused with new resources.

`batch.sampler(ShaderSampler::Linear)` returns a `NativeShaderResource` directly.
Recording a sampler only stores its immutable descriptor; native allocation
failures are reported by `submit`. No `?` is needed when recording the sampler.

`ShaderBindingKind::Uniform(size)` and `ShaderResource::Uniform(bytes)` support
arbitrary read-only native constant blocks, with reflected byte-size checks.
`TextureArray` and `TextureTable(count)` declare native array and descriptor table
bindings; only texture tables may use slot 30. All are available to application
source as well as precompiled kernels. This closes the previous public-interface
gap permanently, without copying GPU resources into CPU snapshots.

The stage planners retain data-dependent validation, compact tile lists,
shared-memory launch geometry, stable resampling phase and progressive level
selection. Those are algorithm contracts, not an alternative shader execution
engine. This is a permanent integration, with no interpreter, shader source
generator, runtime recompilation of built-ins or compatibility fallback.

Immutable built-in pass descriptions are cached by entry, with at most one plan
per compiled entry on each rendering thread. A changed dispatch grid replaces
the plan. GPU handles and parameter contents are resolved on every invocation,
so this cache cannot retain another frame's buffers or destination image.

Host tests exercise every compiled filter entry, native ABI preservation,
resource identity without extra allocation, read-only aliases, invalid/foreign
bindings, descriptor-table counts, writable table aliases and zero/overflowing
dispatches. Pixel and workload comparisons are recorded in
[the migration benchmark](benchmarks/builtin-filter-pipelines-metal-2026-10-11.json).

## Resource and execution contract

`ShaderPipeline::new(resources, passes)` validates every resource reference,
binding, format and dispatch. A pass maps kernel slots to resource indices.
`ShaderKernel::new(sources, entry, workgroup, bindings)` declares the entry and
interface; changing resource data does not recompile it. `with_resource` returns
an immutable replacement suitable for a retained layer descriptor update.

Bindings use unique slots 0..=29 across resource namespaces; texture tables may also use slot30; samplers use 0..=15.
Metal uses the matching buffer/texture/sampler index. HLSL uses the declared
resource name and b/t/u/s register in space0, Vulkan descriptor set0. HLSL raw
buffers use ByteAddressBuffer/RWByteAddressBuffer. Storage textures use float4;
SPIR-V source explicitly annotates their format as `rgba8`, `rgba16f`, or
`rgba32f`. The active reflected interface must match the declaration. Optimized
out read-only resources are allowed; declared writable resources must be active.
Actual workgroup and allocation limits are additionally checked by the backend.

The optional `ShaderResource::config(extent)` resource binds ShaderConfig at slot0.
Use `Config(ShaderConfigDescriptor { extent, sample_radius, uniform_words })` to
supply additional standard metadata. `ShaderDispatch::Pixels` uses the fixed
256-lane compact pixel ABI; Image/Region dispatches use ordinary 2D coordinates.
Pixels uses full-surface coordinates; tile IDs are read-only and cannot be mutated
through writable buffer bindings. Zero group counts are a validated no-op and
cannot initialize an output or intermediate for a later pass to read.

For Image/Region dispatch, its width/height are
the descriptor's resolved extent; Image dispatch covers that extent, compact tiles are
inactive, dispatch_width is the pass's X group count, and output_width/height
are the full logical surface extent. Region dispatch narrows execution to the
filter output plus a physical-pixel outset, rounded outward in the stable
surface grid. Add Config's region origin to dispatch-thread coordinates and
guard its end. Earlier passes must provide every later pass's read halo. `inverse_*` maps full-resolution surface
coordinates to logical canvas coordinates. Kernels use ordinary dispatch-thread
coordinates and guard rounded-up threads themselves. Input/output allocations
may have pooled spare capacity: guard against Config's logical dimensions,
rather than sampling spare pixels based on a native texture's physical size.

`Input(index)` is an immutable invocation image. Input indices are unique and
contiguous from zero, regardless of resource descriptor order. A standalone
filter binds its layer/backdrop at Input(0) and supports at most one invocation
input. Use a graph primitive for more dynamic GPU inputs; extra immutable CPU
image snapshots remain available in both contexts.

Output is premultiplied-sRGB RGBA8. Intermediates begin with
unspecified contents: their first writer must initialize every texel a later
pass reads. The scheduler rejects read-before-write and writable aliases in one
pass. In-place textures require an earlier writer; callers must avoid races
between neighbouring pixels. Raw shaders remain responsible for index bounds,
uniform barrier participation, complete writes and declared read footprints.

Resource lifetimes permit disjoint intermediates with equal size/format to share
one GPU allocation. If the last Input read precedes the first Output write, a
full-surface filter writes directly back to its target. Overlapping input/output
lifetimes use separate storage. Partial output uses a cropped copy-back because
compute dispatches have no scissor. Immutable image snapshots upload once and
are released when their owners disappear. Buffer storage is pooled but its
initial contents are restored on every invocation; no hidden frame history.

## Damage and sampling

`with_output_outset(outset)` declares symmetric output growth beyond the source
geometry, in logical canvas units. For an offset/shadow, include its displacement
and blur reach. It changes layer bounds, offscreen allocation, composition and
retained damage; it does not add shader instructions. DPI scales both the logical
coordinate transform and this output growth. It is independent of input radius:
`with_input_radius` controls reads, while output outset controls produced pixels.
Graph primitive regions remain explicit clips even when a pipeline declares growth.

By default, input-reading pipelines declare a whole-canvas dependency. This
supports reductions and arbitrary multiresolution access: a background edit
outside the output rectangle still invalidates dependent retained output.
`with_input_radius(radius)` optionally declares an aggregate per-axis logical
input displacement for the complete workload. Include every pass, resampling
and buffer-derived access; the renderer additionally includes the bilinear
footprint. That declaration narrows backdrop capture and damage propagation.
The shader is not rewritten or clamped to enforce it.

## Dynamic graph inputs

`FilterPrimitiveKind::ShaderPipeline { pipeline, inputs }` binds each ordered graph
edge to `ShaderResource::Input(index)`. The vector must have `pipeline.input_count()`
entries. It can contain SourceGraphic, SourceAlpha and any preceding Primitive
index, including repeated edges. SourceAlpha is produced once; input textures
are bound directly, with no CPU readback/upload or per-input copy. Forward/missing
edges and count mismatches fail rendering; partial outputs are cleared/cropped
before other graph nodes can read them. Failed allocation/recording releases all
scratch leases. Nested graphs resolve edges within their own graph.

```rust,ignore
FilterPrimitiveKind::ShaderPipeline {
    pipeline, // resource descriptors include Input(0), Input(1), Input(2)
    inputs: vec![
        FilterInput::SourceGraphic,
        FilterInput::Primitive(0),
        FilterInput::SourceAlpha,
    ],
}
```

For this variant, the enclosing primitive's shared `input`/`input2` fields are
unused. Its region still clips output. Graph images retain their usual
premultiplied sRGB storage; application source must perform any requested linear
color-space calculation, just as it owns all other effect math.

## Example and validation

Run `cargo run --release --no-default-features --features metal --example
shader_pipeline`. The example's four native kernels reduce resolution, apply
horizontal/vertical Gaussian convolution in RGBA16F, and upsample with an extra
tint image. The left backdrop is untinted; the right uses a blue tint. Source
files are `examples/shaders/blur_pipeline.metal` and `.hlsl`; resource/pass setup
is in `examples/common/shader_blur.rs`.

GPU regressions exercise negative/HDR round trips, odd sizes including 1x1,
extra images/samplers, writable buffers, shared-memory barriers, 3D workgroups,
in-place texture access, last-use reuse, repeated frames, changing images,
retained source/parameter changes and backdrop reads outside output geometry.
Focused regressions also cover stable compilation-cache fingerprints and cropped
partial output, including failure cleanup. These fix the actual resource and
lifecycle paths, with no temporary production workaround.

The final single-threaded release run passes 1,005 tests, with the same nine
failures as the pre-pipeline baseline and no new failures. The focused shader
run passes all 47 tests, and example tests pass all 13. All 1,730 SVG outputs and
seven rendering examples match the baseline pixel-for-pixel. Metal window
presentation with resize also passes; release clippy completes with existing
warnings. The no-backend release build also checks successfully.

New regressions cover public-only construction of every built-in filter kernel,
arbitrary constant blocks, texture arrays/tables, repeated native invocations,
foreign handles and writable aliases. They also reproduce and fix three actual
validation gaps: Config and raw 112-byte uniforms had the same compilation-cache
fingerprint, Metal accepted a reflected texture-table count mismatch, and zero
group dispatches incorrectly marked unwritten intermediate textures initialized.
The fixes apply to shared resource validation and reflection, with no temporary
production workaround. ShaderProgram and its kernel also share source storage
to avoid duplicating source strings while decoding brushes.

Dynamic-input regressions compare custom composition against built-in composition,
including nested graphs, repeated edges, SourceAlpha, primitive clips and retained
updates. Output-growth regressions compare custom offsets and shadows against
built-ins at 1x, 1.5x and 2x DPI, including off-canvas sources. A reproduced DPI bug
in retained damage propagation mixed logical filter/mask descriptors with physical
child bounds; it now reads the cached physical filter/region used by rendering. A second
regression fixes SourceAlpha scratch accounting in vector-input graph primitives.
Both are permanent fixes to shared execution paths.
Chained output growth also saturates before canvas clipping, so extremely large
valid declarations cannot wrap into negative bounds and discard output.

Run the `shader_graph` example for SourceAlpha → offset → blur followed by custom
two-input composition; its complete kernels are in `examples/shaders/composite.*`.

Metal execution is validated on Apple M5. DX12/Vulkan share the descriptors,
format-aware allocation, barriers and HLSL reflection implementation; device
execution must be validated on their respective platforms. The API describes
compute filter work, not arbitrary graphics render passes, bindless resources,
mips, multisampling or persistent cross-frame writable storage.

## Measured native execution

Apple M5, Metal validation disabled. The exact complete application file is
compiled verbatim at runtime or loaded as an offline Metal 2.4 library, with
fast math disabled and invariance preserved. Both use the full NativeRenderer,
including scene drawing, backdrop capture, four passes, RGBA16F intermediates,
extra tint image, compositing, submission and wait. Compilation and readback
are excluded. Two contexts share one process; execution order alternates each
frame. Three trials each use 500 warm-up and 500 timed frames per variant.

| Effect region | Runtime p50 | Offline p50 | Difference |
| --- | ---: | ---: | ---: |
| 320×200 in 1280×800 | 0.45221 ms | 0.45146 ms | +0.17% |
| 1280×800 | 0.89794 ms | 0.89631 ms | +0.18% |
| 3840×2160 | 4.95463 ms | 4.95512 ms | -0.01% |

All nine paired outputs match exactly. These measurements support equivalent
steady execution for this algorithm on this device; first-use compilation still
has a cost, and arbitrary shaders/algorithms are not guaranteed equal performance.
Earlier separate-process runs are retained, including small-scene startup variance.

The ordinary 128-layer translucent workload, comparing committed HEAD 7e53dfd
with this renderer in one process, measures 5.40335 versus
5.41798 ms (+0.27%). Three trials
use 500 warm-up and 1000 timed frames per variant; every image pair is exact.

Raw samples, protocols, source/library hashes and preliminary runs are in
[the benchmark report](benchmarks/shader-pipeline-metal-2026-10-11.json). Temporary
measurement source and the offline-library test hook have been removed.

## Indexed-input and output-growth completion measurements

A separate completion run compares the frozen pre-change renderer with the
indexed-input/output-growth implementation. Actual NativeRenderer drawing,
capture, filtering, composition, submission and wait are timed; compilation and
readback are excluded. Three serial, alternating-order trials use 500 warm-up and
500 timed frames per variant, with Metal validation off on Apple M5.

| Workload | Before p50 | After p50 | Difference |
| --- | ---: | ---: | ---: |
| 320×200 backdrop blur in 1280×800 | 0.44952 ms | 0.44854 ms | -0.22% |
| 1280×800 backdrop blur | 0.91029 ms | 0.90677 ms | -0.39% |
| 3840×2160 backdrop blur | 4.96221 ms | 4.92429 ms | -0.76% |
| 128 translucent rectangles | 3.99473 ms | 3.98804 ms | -0.17% |

The new graph workload uses SourceAlpha → offset → horizontal/vertical blur →
two-input source-over composition. Its custom composite measures 4.33813 ms
against 4.42023 ms for the built-in composite (-1.86%); the rest of the graph is
identical. All 15 before/after or custom/built-in image pairs match exactly. These
measurements show no regression in the measured workloads, rather than a general
performance guarantee for arbitrary user algorithms.

Raw samples and protocols are in the
[completion benchmark report](benchmarks/shader-pipeline-inputs-metal-2026-10-11.json).

## Built-in migration measurements

Apple M5; three serial trials, 150 warm-up and 300 timed frames per workload
per variant (900 samples per cell). Variant order alternates by trial. Both
frozen binaries use the actual NativeRenderer, including scene drawing, filters,
submission and GPU wait; compilation and readback are excluded.

| Workload | Before | Pipeline | Change |
| --- | ---: | ---: | ---: |
| 128 translucent rectangles | 0.74500 ms | 0.74823 ms | +0.43% |
| Small backdrop blur | 0.47173 ms | 0.47279 ms | +0.23% |
| 1280×800 backdrop blur | 0.97954 ms | 0.97877 ms | -0.08% |
| 3840×2160 backdrop blur | 5.05002 ms | 5.05535 ms | +0.11% |
| Liquid glass | 0.99925 ms | 1.00050 ms | +0.13% |
| Progressive blur | 1.86448 ms | 1.87194 ms | +0.40% |
| Drop shadows | 3.56896 ms | 3.57265 ms | +0.10% |
| Eight color adjustments | 1.34933 ms | 1.34944 ms | +0.01% |

All 24 image pairs match pixel-for-pixel. Changes are within the +0.43% variation
of the ordinary drawing control; these runs show no material performance
regression. This measures the renderer workloads above, not every possible
filter/scene combination. The final full SVG run also matches all 1,730 baseline
outputs, and all seven rendered examples match. Temporary measurement source
was removed. Raw samples, frozen binary hashes and validation results are in
[the migration report](benchmarks/builtin-filter-pipelines-metal-2026-10-11.json).

## Public interface completion measurements

The final comparison links the frozen 14:30 renderer and the completed renderer
into one process, with separate Metal contexts on the same Apple M5. It measures
the cumulative built-in pipeline migration and public-interface completion,
including removal of `Filter::Program`. It is not a comparison limited to the
last private-constructor removal. The same scene descriptions are used for both.

Each frame alternates which renderer runs first, with submission and GPU wait
included. Three trials reverse the initial order, using 150 warm-up and 300 timed
frames per workload/variant/trial (900 samples per cell). Compilation and
readback are excluded, Metal validation is disabled, and no build/test work runs
alongside measurements. Initial separate-process measurements showed large
system shifts even in ordinary drawing; their samples are retained in the report
but are not used for the final steady-execution conclusion.

| Workload | Before p50 | Completed p50 | Change |
| --- | ---: | ---: | ---: |
| 128 translucent rectangles | 0.60575 ms | 0.60662 ms | +0.14% |
| 320×200 backdrop blur | 1.12210 ms | 1.12650 ms | +0.39% |
| 1280×800 backdrop blur | 4.22929 ms | 4.14877 ms | -1.90% |
| 3840×2160 backdrop blur | 30.67471 ms | 30.68865 ms | +0.05% |
| Liquid glass | 1.76250 ms | 1.76506 ms | +0.15% |
| Progressive blur | 2.31000 ms | 2.30308 ms | -0.30% |
| Drop shadow | 3.93962 ms | 3.93502 ms | -0.12% |
| Eight color adjustments | 1.60908 ms | 1.60737 ms | -0.11% |
| User-source glass | 1.20952 ms | 1.20996 ms | +0.04% |
| User-source brush | 0.90554 ms | 0.92133 ms | +1.74% |

Filter workloads remain close to their baseline; the large-blur improvement varies
by trial and is not a general speedup claim. The brush has a measured 0.01579 ms
increase, so the shared interface is not claimed to have zero overhead. This
benchmark's default Gaussian convolution differs from earlier downsampled scenes;
absolute blur times should not be compared across those workload definitions.
All 30 paired images and all 30 separate-process image pairs match exactly.
Temporary measurement sources have been removed. Full raw samples, p95 values,
source/library/binary hashes, workload definitions and validation results are in
[the public-interface report](benchmarks/unified-filter-interface-metal-2026-10-11.json).
