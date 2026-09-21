# Clip count, nesting and area

The matrix separates independent clip stacks, nesting depth, and per-stack bounding-box
area. All 16 workloads have lower completed-frame mean times on native DX12 and
Vulkan than on the corresponding wgpu route on this RTX 4090. This is not a claim
about every GPU, geometry, window presentation, or multi-frame throughput.

## Workloads and measurement

- 1280 x 800. Each retained node contains one colored rectangle under path clips.
- Count sweep: 1, 8, 32, 128, 384 independent stacks; depth 1; 128 x 80 bounds (1%).
- Depth sweep: 1, 2, 4, 8, 16, 32; 8 stacks; the same 1% bounds. The depth-1 reference
  is `clip-count-8`. Rounded corners repeat three radii; this measures stack cost,
  not masks that shrink progressively with nesting. Interior tiles can elide masks.
- Area sweep: 8 stacks, depth 1; 40 x 24 (0.09375%), 128 x 80 (1%), 400 x 256 (10%),
  896 x 576 (50.4%), 1280 x 800 (100%). These are individual clip bounding boxes,
  not covered-pixel percentages or the union of overlapping clips. Full-size masks
  extend one pixel past the viewport on odd phases.
- Two interactions: 32 stacks x depth 8 x 1%; 8 stacks x depth 8 x 10%.
- Every node alternates between two positions one pixel apart. One Criterion
  iteration completes 16 frames, including transaction, CPU submission and GPU wait.
  This preserves effective work instead of benchmarking unchanged retained history.
- Release, explicit adapter `0f42010000000000`, RTX 4090 driver 610.62, Ryzen 9 9950X3D.
  No concurrent GPU tests or builds during timing. Shader/pipeline setup and image
  readback are outside timed samples; cached shaders remain enabled.
- Broad matrix: one run per route, 20 Criterion samples, 250 ms warm-up, 1 s target
  measurement (Criterion extends slow cases). Separately: 64 warm-up frames and
  128 frame latencies. Five representative cases have three alternating before/after
  rounds, with identical sample counts. Do not compare their PMax directly with the
  previous report's 400-frame samples; observed maxima are not latency guarantees.

## Four-route matrix

Per-frame Criterion means in milliseconds; this table is the single broad sweep,
not the median of the separate confirmation rounds. All 1,024 saved phase images
match exactly across routes, including the first frame after switching workloads.

| Case | wgpu DX12 | Native DX12 | wgpu Vulkan | Native Vulkan |
|---|---:|---:|---:|---:|
| clip-count-1 | 0.358 | 0.200 | 0.230 | 0.158 |
| clip-count-8 | 1.083 | 0.439 | 0.659 | 0.330 |
| clip-count-32 | 3.446 | 1.108 | 2.013 | 0.808 |
| clip-count-128 | 7.820 | 3.497 | 7.820 | 2.571 |
| clip-count-384 | 26.479 | 10.058 | 26.402 | 7.016 |
| clip-depth-2 | 1.164 | 0.461 | 0.749 | 0.359 |
| clip-depth-4 | 1.367 | 0.506 | 0.835 | 0.422 |
| clip-depth-8 | 1.373 | 0.614 | 1.006 | 0.534 |
| clip-depth-16 | 1.728 | 0.881 | 1.415 | 0.770 |
| clip-depth-32 | 2.479 | 1.279 | 2.288 | 1.287 |
| clip-area-0.094pct | 0.897 | 0.404 | 0.618 | 0.308 |
| clip-area-10pct | 0.962 | 0.549 | 0.712 | 0.470 |
| clip-area-50.4pct | 1.260 | 0.845 | 0.986 | 0.700 |
| clip-area-100pct | 1.742 | 1.428 | 1.531 | 1.291 |
| clip-mixed-32x8 | 3.943 | 1.855 | 3.463 | 1.639 |
| clip-mixed-8x8-large | 1.530 | 0.750 | 1.189 | 0.718 |

Independent stack count dominates this matrix. The 384-stack stress workload still
exceeds 6.94 ms, so it does not meet a 144 completed-frames/s budget. This is distinct
from the application's resize/presentation scenario.

## Further optimization

Fixed-slot clip emission now uses one lane per active tile when there are at least
`COARSE_WORKGROUP_SIZE` active tiles (256, sourced from HLSLI). Smaller selections
keep the existing workgroup-per-tile emitter. The scalar emitter avoids per-tile
workgroup prefix scans and classifies tiles for fine's existing specialized paths.
Clip geometry, AA, painter order and slot capacity do not change. Eligibility still
excludes text and mixed non-clip schedules; Metal behavior is unchanged.

For the 8-stack, depth-1, 50.4% case, median of three means/p95 values and worst of
three observed maxima are:

| API | Mean before -> after (ms) | Reduction | p95 before -> after | PMax before -> after |
|---|---:|---:|---:|---:|
| DX12 | 0.949 -> 0.856 | 9.8% | 1.195 -> 1.230 | 1.821 -> 1.895 |
| VULKAN | 0.770 -> 0.691 | 10.3% | 0.911 -> 0.825 | 1.303 -> 1.306 |

Both APIs report Criterion improvement for this case in all three confirmation
rounds. DX12 tail latency did not improve along with its mean. Small clips and
deep small-bounds clips retain their old scheduling; no new general deep-clip
speedup is claimed.

Two rejected experiments informed this selection: forcing full-viewport clips into
fixed slots lost useful tile specialization and regressed; using scalar emission
for every small clip regressed the 384-stack case by about 13-16%. Neither policy
is retained. One full-viewport Vulkan confirmation comparison against the first
round reported +2.3%, while the three-round median was unchanged. A further three
rounds against fresh adjacent baselines reported no change or change within the
noise threshold for all adaptive comparisons. Their reference means were
1.220/1.174/1.175 ms, same-binary repeats 1.175/1.170/1.185 ms, and adaptive means
1.216/1.202/1.189 ms. These controls do not establish a fullscreen speedup or
prove zero overhead; the original outlier remains in the evidence.

## Correctness fixes exposed by the matrix

`PersistentPathPlans` previously failed to mark newly regrown *vacant* scan-range
slots dirty. The CPU's new zero entries matched their defaults, while a retained
GPU allocation could still contain old nonzero ranges. Switching from a large
shallow scene to a smaller deep scene exposed corrupted first-frame wgpu pixels.
The entire regrown suffix is now uploaded. This fixes the update contract rather
than masking it with an extra render or discarding the first frame.

Mixed scalar/parallel clip batches must replace both particles and tile kind.
Parallel emission explicitly resets the previous scalar batch's EMPTY/COLOR kind;
rejected clips terminate reused streams. Regressions exercise scalar-to-parallel
transitions, depth 1-32, and dispatch sizes immediately below/at/above a full group.
The stale-kind and vacant-range tests were observed failing before their fixes.

## Verification and reproduction

Release tests and strict all-target Clippy passed separately for wgpu, DX12 and
Vulkan; formatting passed. Native GPU regressions passed on both APIs, including
validation, and the first-frame wgpu regression passed on both APIs. The existing
11 workloads also passed native pixel comparison (176 phase images per API).

Full SVG, example and retained corpora passed on all four Windows routes: 1,931
outputs per route, 7,724 total, with zero image differences. The new matrix has
1,024 exact phase images; the confirmation and fullscreen controls also compare
every captured phase against the fixed wgpu reference. See the linked JSON for
all receipts and timings. This run does not validate other GPUs or Metal.

```powershell
python scripts/native/benchmark.py --clip-matrix --gpu 0f42010000000000 `
  --dxcompiler target/toolchains/dxc-v1.8.2502/bin/x64/dxcompiler.dll `
  --output target/clip-matrix-new-run --runs 3
```

The script builds each exclusive backend separately. Native builds use the project's
DXC discovery/cache configuration; `TILEINK_NATIVE_DXC_PATH` can pin the executable.
Omit `--clip-matrix` to retain the existing 11-workload suite.

[Measurements and verification](benchmarks/clip-matrix-2026-09-21.json).
Local detailed evidence: `target/clip-matrix/adaptive/`, `adaptive-confirm/`,
`fullscreen-control/`, `checks/`, `verification/`, and `corpus-*`.
