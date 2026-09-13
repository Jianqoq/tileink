# M1 shared-renderer closeout

M1 implements the optional default wgpu feature, CPU-only scene/materializer
builds, backend-independent preparation/execution/filter scheduling, and explicit
unavailable native-constructor contracts. It does not implement native DX12,
Vulkan or Metal rendering. M2 will maintain HLSL for DX12/Vulkan and independent
MSL source for macOS; actual Mac validation remains future work.

On 2026-09-13 the user instructed: **“不用比较性能了”** (stop performance comparisons). For this M0/M1 closeout, no further Criterion comparisons, resize timing or telemetry are required. Performance is not an acceptance gate for this delivery, and `performance_accepted` remains false. Historical regressions, rejected experiments and incomplete timing runs remain preserved. This does not waive functionality, exact pixels, feature/dependency checks, release tests or code review.

## Selected implementation and correctness

The selected source is `target/backend-parity/m1-new-batch-walk-1/current/source`.
Its frozen ancestry includes filter logical-texel sampling, glass/refraction and
retained history/damage corrections, shared resource lifetime and error handling,
and redundant retained classification/index work removal. These are production
semantics and algorithms with focused regressions; temporary timing probes and
rejected dense-dirty/projection proposals are not integrated.

- Default release library: 938 tests; CPU-only release library: 620 tests.
- Fresh feature checks: 21 combinations across Windows and Linux/macOS x86_64,
  bench-internals all-target build, three native-only dependency audits and
  aggregate native contracts. Cross compilation is not native hardware execution.
- Single-native-feature constructors explicitly return unavailable status.
- Latest parity: 15,448 outputs in six fresh runtime/precompiled
  SVG/example/retained processes, zero differing pixels, including alpha.
- Earlier broad GPU regressions, reflected shader inventory, package and website
  checks remain recorded against their source ancestry. Approved ordinary PNG
  outputs are integrated from the recorded human-review set.

Evidence is in `target/backend-parity/m1-new-batch-walk-1/{summary.json,
features-1/summary.json,native-feature-contracts-1/summary.json,
strict-pixels-1/summary.json}` and
`target/backend-parity/m1-glass-final-1/png-review-approval-1.json`.
The M0 historical source and its known defects remain documented separately.
The four native/wgpu API equality requirement is unchanged; today's four wgpu
API/texture routes are not four native/wgpu implementations.

## Final integration

The working tree was assembled after a scoped backup and conflict review. All
5,540 production-source/resource members were checked against the selected
snapshot; the 64 changed approved PNGs use the human-reviewed image bundle.
No rejected dense-dirty/projection proposal or temporary partition probe is
included. The original modular example helpers are retained: reviewed moved
definitions are identical to the selected snapshot, while CPU and GPU harnesses
share configuration and workload definitions instead of duplicating them.

Integrated checks completed on 2026-09-13, with tests single-threaded:

- `cargo test --release -- --test-threads=1`: 938 library tests plus integration
  suites passed. Explicitly ignored hardware tests are not counted as GPU proof.
- `cargo test --release --no-default-features -- --test-threads=1`: 620 library
  tests plus integration suites passed.
- `cargo test --release --no-default-features --features native -- --test-threads=1`:
  623 library tests plus integration suites passed, including unavailable-native
  constructor contracts.
- Focused `benchmark_matrix` semantic tests passed after restoring the modular
  helpers. No Criterion benchmark was executed.
- `cargo fmt --all` and formatting verification passed.
- `cargo clippy --release --all-targets --features bench-internals,native -- -D warnings`
  passed, including the permanent correctness example.
- `cargo clippy --release --no-default-features --all-targets --features native`
  passed with unused internal-item warnings. An additional `-D warnings` attempt
  on this CPU-only configuration failed on those warnings: shared renderer
  internals have no concrete GPU consumer in that build. CPU-only warning-free
  status is not claimed, and no broad lint suppression was added.

The full earlier GPU/pixel evidence remains bound to the identical production
implementation; default-skipped tests are not presented as fresh GPU execution.
Standards and specification reviews found no blocking integration issue. Local
editor settings are excluded from the delivery commit.

`examples/scoped_benchmark_pixels.rs` is deliberately retained as a permanent
non-timing correctness tool, superseding its removal from an earlier staging
inventory. It checks scoped filter/backdrop cases over successive states against
ForceFull and an independent immediate oracle, including complete RGBA bytes and
analytic color assertions. Its addition fixes the manifest/file membership
omission without changing renderer algorithms.

Performance remains **waived for this M0/M1 delivery, not accepted**. M0 baseline
acquisition and M1 shared-renderer implementation are complete; native GPU
renderers and the HLSL/independent-MSL toolchains remain later milestones.
