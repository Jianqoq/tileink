# Expanded Windows backend comparisons (2026-09-22)

Completed 181 retained configurations, 62 immediate configurations and 44 burst configurations on each of wgpu DX12, native DX12, wgpu Vulkan and native Vulkan. Every captured phase matched raw RGBA exactly. Five retained and two immediate outliers were additionally repeated three times per route in fresh processes, reversing route order in the second repetition.

[Follow-up: raster invalidation root fix and repeated results](manual-invalidation-performance.md) resolves the ordinary invalidation bottleneck; cropped filters remain slower. The measurements below are the pre-fix baseline.

**Performance parity has not been achieved.** Earlier 11-scene wins did not cover these broader workloads. Pixel correctness, successful execution and performance acceptance are separate results.

| Matrix | Configurations per route | DX12 nominally slower | Vulkan nominally slower |
|---|---:|---:|---:|
| retained | 181 | 75 | 73 |
| immediate | 62 | 53 | 50 |
| burst | 44 | 5 | 23 |

These are single-sweep mean comparisons, including small differences that can be noise; they are not counts of statistically established regressions. The table gives every configuration equal weight, not every rendered pixel or application frame.

## Isolated repeated measurements

Median of three Criterion mean frame times, in milliseconds. No outliers were discarded.

| Configuration | wgpu DX12 | Native DX12 | wgpu Vulkan | Native Vulkan |
|---|---:|---:|---:|---:|
| scale-affine-clip-update-100000 | 0.4306 | 2.3398 | 0.2847 | 2.5856 |
| scale-backdrop-manual-invalidation-100000 | 0.4907 | 21.2715 | 0.3594 | 23.3026 |
| scale-cropped-filter-manual-invalidation-100000 | 0.8046 | 29.7101 | 0.5437 | 24.5740 |
| scale-manual-invalidation-100000 | 0.2790 | 21.3442 | 0.1723 | 22.2667 |
| scale-one-move-100000 | 0.3369 | 1.5988 | 0.1724 | 1.5655 |
| dirty-immediate-60.0pct | 0.5875 | 0.4373 | 0.3853 | 0.3773 |
| raster-gradient-300 | 0.3560 | 0.2452 | 0.2707 | 0.2011 |

The two immediate outliers became faster than wgpu when isolated, while the retained outliers remained substantially slower. The full-sweep immediate slowdowns therefore need investigation of run-order or execution-context effects; these data do not establish their root cause. Both execution contexts remain in the artifact. Retained manual invalidation is a reproducible outstanding bottleneck.

## Fixes discovered by the expanded coverage

- Retained opacity updates reused an old execution plan on native (.75 to .25 kept old pixels). Structural reuse now preserves metadata without retaining stale parameter values. CPU and GPU regressions cover the change and 0/1 boundaries.
- Large DX12 layer scenes exhausted a single shader-visible descriptor heap. Whole pass tables are paged into bounded heap pairs; every page survives submission completion. Actual pass indices select pages in both directions, accounting for initialization skips and prepended scatter uploads. Boundary/order tests and a two-frame 5,000-layer GPU regression cover this.
- A 1600-wide turbulence image had one native Vulkan green byte off by one. Explicit mad in linear-to-sRGB conversion preserves the fused final rounding in the tested strict SPIR-V path. Whole-image canonical tests at widths 300 and 1600 passed on all four routes. No tolerance or coordinate-specific correction was added.

Four-route SVG/examples/retained regression after the shared plan fix compared 7,724 outputs with no differences. Native DX12 was revalidated after paging; both native backends were revalidated after the turbulence change (1,931 outputs per route). Three feature-specific release suites, all-target strict Clippy with bench-internals, formatting and focused GPU tests passed.

## Burst throughput

In the single broad pass, native DX12's mean per-frame time at burst 4 versus burst 1 fell about 32% for clips and 43% for the large scene. Native Vulkan's corresponding reductions were about 31% and 13%. These compare batch sizes within a backend; they do not imply the same change in per-frame latency or window FPS.

## Measurement contracts and limits

- Release Criterion: ten Flat samples, 200 ms warmup, 1 s requested measurement. Slow cases necessarily exceed this duration; 100,000-node fragmentation required minutes. Initialization, target creation and capture/readback are outside timing. Ordinary samples wait for GPU completion.
- Retained cases preserve both alternating mutations per timed cycle. Delta rotation repeats 255 warm and 255 measured frames. Most latency series stop at 64 frames or at a complete pair after 1 s (minimum 4); P95 is omitted below 20 samples. Maxima from unequal sample counts have unequal statistical weight.
- Immediate uses preallocated targets, including the four-size glass sequence. It does not measure swapchain resize or texture allocation. Each immediate canvas is compared.
- Burst configurations submit 1/2/3/4 frames before draining the batch; every timed cycle contains 48 frames. Mean is per completed frame, but P95/max are whole-burst latency. Pixel checks cover batch boundaries of the first 48 frames, not every timed frame. This is not a sliding frames-in-flight or window-presentation benchmark.
- The burst `image_replace` revision continues increasing during Criterion calibration. Its captured first 48-frame trajectory agrees across routes, but all timed inputs are not guaranteed to be identical frame by frame across routes.
- The broad sweeps are single repetitions. Only the seven selected outliers have three independent repetitions. Per-run means, confidence intervals, latency summaries, sample counts and ratios are retained in the result artifact. Raw latency series remain in the local evidence directories.
- RTX 4090 and driver 610.62 only. A reboot changed the Windows LUID from 0f42010000000000 to fe3e010000000000. Retained was measured before reboot; immediate/burst and isolated repeats afterward. Other GPUs, Metal, Linux and application/window interaction remain outside this dataset.
- Retained finished after the plan/paging fixes, before the turbulence-only shader correction. It contains no turbulence workload. Its completed wgpu DX12 baseline predates the DX12-only paging change; that unchanged backend binary, source differences and failed parent-run provenance remain explicit in its receipt. Other routes were sampled after paging. These datasets are not described as a single frozen final build.
- The original 39-target/970-case inventory remains historical execution coverage, including shared CPU and backend-specific microbenchmarks. The expanded matrices compare common rendering workloads; they do not turn every backend-specific internal microbenchmark into a like-for-like cross-API comparison.

## Reproduce

```powershell
$env:TILEINK_NATIVE_DXC_PATH=(Resolve-Path target/toolchains/dxc-v1.8.2502/bin/x64/dxc.exe).Path
python scripts/native/retained_benchmark.py --suite retained --gpu <current-LUID> --dxcompiler target/toolchains/dxc-v1.8.2502/bin/x64/dxcompiler.dll --output target/new-retained-run
# Repeat with --suite immediate and --suite pipelined; output directories must be new.
# --case accepts comma-separated prefixes; --runs 3 alternates route order.
```

[All measurements and evidence hashes](benchmarks/expanded-windows-2026-09-22.json).
