# Native mesh gradient brush

`Brush::Mesh(MeshGradient)` adds a row-major colored vertex grid to the existing
paint pipeline. It is a native sampled brush, not a CPU bitmap or a set of
blurred radial gradients. The compiled payload is immutable and shared by
clones; transforms, DPI conversion, append, and offscreen localization only
change its inverse coordinate transform.

## Geometry and color contract

- At least two columns and two rows; position and color arrays both have exactly
  `columns * rows` entries. Positions are in local drawing coordinates.
- Each cell is a straight-sided convex quad. Geometry uses the bilinear map
  `P(u,v) = a + b*u + c*v + d*u*v`, for `u,v` in `[0,1]`.
- All cells must have the same nonzero Jacobian sign. A simple outer boundary
  then gives an injective mesh. Validation accepts either winding direction and
  rejects folded, self-intersecting, nonfinite, and numerically degenerate input.
- Patch coordinates are normalized to each patch's bounding box before GPU
  inversion. The normalized corner Jacobian magnitude must exceed 1e-4, and
  parameter-bound tolerances account for the inverse Jacobian's amplification of
  f32 coordinate rounding. A fixed UV epsilon otherwise creates holes in thin
  valid quads; a GPU regression test covers this. Subnormal extents are rejected
  because GPU arithmetic may flush
  them to zero; a focused regression test demonstrates this construction error.
- Vertex colors are premultiplied sRGB RGBA8, matching existing brush storage.
  Bilinear mode mixes all four vertex colors in floating point and quantizes
  once. Default Smooth mode replaces each parameter with `t*t*(3-2*t)` before
  mixing. This preserves transparency and avoids color overshoot.
- Shared edges have identical colors. Smooth parameter interpolation does not
  promise continuous spatial derivatives on distorted quads. This is not a
  general bicubic/Coons implementation with Bézier handles.
- Samples outside the mesh boundary are transparent. Mesh boundary sampling does
  not add a separate antialiasing contour; paint a path/SDF or clip region to
  define antialiased visible edges. There is no SVG mesh parser in this feature.

The choice of bilinear geometry makes inversion analytic and removes a fixed
Newton iteration budget that can fail to converge on highly skewed patches.
The shader uses a cancellation-resistant quadratic root solve, validates the
parameter domain, and handles affine/parallelogram cells as a linear equation.
Test references use known forward parameter points as an independent oracle, rather
than reproducing the shader's inverse algorithm.

## Storage and lookup

The existing 9-word header + 12-word parameter layout is unchanged; mesh kind is
8. Header words 4/5 hold the bin grid size, word 6 the payload-relative patch
record base, and word 7 selects Smooth (1) or Bilinear (0). Parameters 0–5 hold
the inverse affine transform; 6–9 hold the mesh bounds.

The payload contains `bin_count + 1` relative offsets, a list of candidate patch
indices per bin, and 16 words per patch: bounding box (4), normalized `a,b,c,d`
(8), and top-left/top-right/bottom-left/bottom-right colors (4). The bin grid is
capped at 32×32, without limiting the vertex grid dimensions. Candidate bounds
include shared bin edges and floating-point rounding margin. The shader only
inverts candidates in the sample's spatial bin, checks all payload ranges, and
returns transparent for malformed records.

Metal and HLSL use the same layout and algorithm. Metal now has the same
`brush_words` validation entry as HLSL, including its backend-private buffer
length metadata. The Metal fine config was moved to a shared include so the
validation entry cannot duplicate its ABI.

## Usage

Construct `MeshGradient::new(columns, rows, positions, colors)?`, optionally call
`with_interpolation(MeshInterpolation::Bilinear)`, and pass it directly to
`Canvas::push_rect` or convert it with `.into()` for APIs taking `Brush`.
The same brush works for paths, SDFs, filter Flood/DropShadow/Image brush inputs,
and retained scene changes through the existing paint upload channel.

Run `cargo run --release --no-default-features --features metal --example
mesh_gradient` to save `target/mesh_gradient.png`.

## Validation

Focused host and real-GPU release tests cover construction errors, winding,
nonparallelograms, known parameter samples, shared bin/patch edges, transparent
vertices/exterior, malformed payloads, encoding round trips, DPI conversion,
append, rotation, offscreen and filter coordinates, and retained replacement,
translation and removal. All test execution is single-threaded.

Hardware validation in this workspace uses Apple M5 / Metal. HLSL source and
serialized constants are updated, but this host has no DXC or DX12/Vulkan device;
those compilation/hardware results are not claimed.

## Measurements, 2026-10-11

Apple M5 / Metal, release builds, validation off during timing. Three alternating
A/B trials compare frozen commit `7e53dfd0` with this implementation, using the
same driver and dependencies. Each case warms 16 frames and samples 80 frames
per trial (240 per variant). Wall time includes the render call and explicit
wait, not scene construction/readback; it is not pure GPU time or presentation FPS.
Existing workloads use the repository's candlestick, gradient, and tiger SVG
example scenes, plus 128 full-screen alpha-119 layers at 1280×800. All compared
outputs match RGBA exactly in each trial.

| Existing workload | P50 before → after | Change |
| --- | ---: | ---: |
| Candlestick example, 520×320 | 0.261 → 0.258 ms | -1.2% |
| Gradient example, 720×520 | 0.332 → 0.335 ms | +0.8% |
| Tiger SVG, width 1280 | 1.406 → 1.405 ms | -0.1% |
| 128 transparent layers, 1280×800 | 5.234 → 5.293 ms | +1.1% |

These small differences do not establish a general speedup or zero overhead.
The purpose is to check that adding the mesh shader branch preserves existing
workload cost within the observed variation.

| Mesh vertex grid | Output | P50 | P95 |
| --- | --- | ---: | ---: |
| 3×3 | 1280×800 | 0.810 ms | 0.902 ms |
| 9×9 | 1280×800 | 0.838 ms | 0.899 ms |
| 33×33 | 1280×800 | 0.927 ms | 1.308 ms |
| 3×3 | 3840×2160 | 4.517 ms | 5.321 ms |
| 9×9 | 3840×2160 | 4.697 ms | 5.503 ms |

Mesh cells have deterministic interior displacement of up to 10% of grid spacing,
with fixed boundary vertices and deterministic opaque colors. All use Smooth.
Mesh construction is outside the render timing. Full distributions and raw
samples are in [the measurement report](benchmarks/mesh-gradient-metal-2026-10-11.json).
Temporary measurement driver source is removed after validation.

## Completion checks

- Eight host semantic/edge tests and six real-GPU tests pass. The host subset
  also passes without any native backend feature enabled.
- Full single-threaded release library run: 858 passed, the same six pre-existing
  glyph/clip/SDF oracle failures confirmed before this feature. Integration and
  example tests pass except the same pre-existing scaled-turbulence 1600 px
  reference mismatch and two compiler tests requiring the absent pinned DXC.
- All 1730 SVGs (1712 fixtures plus 18 root/extra cases) were rendered and match
  frozen pre-change native RGBA output exactly. Both progressive blur qualities
  match too. Native window smoke completes eight frames and a real resize.
- `cargo fmt --all -- --check` and release Metal clippy for all targets complete;
  existing unrelated warnings remain, with no new warnings in mesh files.
- Chinese/English API and brush guides are updated. Website build is unavailable
  because npm and website dependencies are absent on this host.
- New preview: `target/mesh_gradient.png`. Raw evidence, logs, measurement
  captures and frozen binaries remain in `target/mesh-validation/`.

For measurement reproduction, interior mesh positions add
`sin(x*y) * width/(n-1) * 0.1` to x and
`cos(x+y) * height/(n-1) * 0.1` to y; boundary vertices stay fixed. Row-major color
index i gives RGB `(37*i, 71*i, 97*i)` modulo 256, alpha 255. The transparent
control uses RGB `(37*i mod 256, 80, 150)`, alpha 119 over a black background.
