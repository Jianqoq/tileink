# Clippy quality checks

`rust-toolchain.toml` pins Rust/Clippy so developers and CI use the same rules.
`Cargo.toml` enables `all`, `pedantic`, `nursery`, and `cargo`, plus selected
`restriction` checks. CI rejects warnings for each supported platform/backend
profile, including tests, examples, and build scripts.

```sh
# Windows DX12
cargo clippy --release --all-targets -- -D warnings
# macOS Metal
cargo clippy --release --all-targets --no-default-features --features metal -- -D warnings
# Windows Vulkan
cargo clippy --release --all-targets --no-default-features --features vulkan -- -D warnings
# Linux Vulkan shader/shared-code profile (native adapter is not implemented)
cargo clippy --release --all-targets --no-default-features --features vulkan -- -A dead_code -A unused_imports -D warnings
```

Backend features are mutually exclusive. `--all-features` is therefore invalid;
the CI matrix checks them independently. The backend-free library profile is
also checked. It and Linux's shader/shared-code profile allow Rust's unused
imports/dead execution paths because no native adapter consumes them. Scoped
conditional exceptions keep unavailable-backend stubs consistent with the
runtime API; Windows and macOS check the actual implementations. Tests run in release mode with `RAYON_NUM_THREADS=1` and
`--test-threads=1`.

## What is checked

The default groups cover correctness, suspicious code, performance, unnecessary
complexity, and style. Additional groups check API error/panic contracts, unsafe
documentation, ownership and copies, numeric operations, constant functions,
Cargo metadata, and duplicate dependencies. Selected restriction rules also
check `unwrap`/`expect`/explicit `panic`, debug placeholders, pointer casts,
accidentally ignored results, and missing reasons on lint exceptions.

Tests may abort when setup or an assertion fails. Test-only exceptions do not
apply to ordinary library/build-script compilation. Production invariant
assertions need a local, documented exception; I/O and caller-input errors
should use the existing error contract. Replacing `unwrap` with `expect` alone
does not pass the policy.

## Reviewed exceptions

Exceptions are recorded next to the relevant configuration or code. They are
part of the permanent policy, not temporary warnings hidden to make CI pass.

| Rule or configuration | Reason |
| --- | --- |
| Lossy numeric casts | GPU coordinates, packed formats, and color quantization deliberately narrow values. Allocation sizes, resource addressing, and public validation still require checked arithmetic/conversions. |
| `float_cmp` | Exact identity/state comparisons and tile-edge ownership are intentional. |
| `suboptimal_flops` | Fusing/reordering operations can change SVG reference rounding and pixels. |
| Algebraic names, wildcard module preludes, repeated module names, explicit crate visibility | Renderer naming/module conventions; inspected in the complete audit. |
| Function line count and helper declaration order | Algorithms and conformance tables are reviewed by structure; helpers may sit beside their use. |
| `option_if_let_else` | Explicit branches are clearer when updating several caches or mutable buffers. |
| `tuple_array_conversions` | Explicit coordinate components preserve const evaluation and show GPU ABI component order. |
| `assert_is_empty` | Empty checks should not require collection elements to implement `PartialEq`/`Debug`. |
| 512-byte pass-by-value limit | Fixed filter uniform ABI snapshots are passed into command recording. Larger values are still checked. |
| Allowed duplicate font/XML crates | `usvg` and `cosmic-text` require overlapping upstream versions. The named allowlist catches new duplicate crates. |
| Local arithmetic, immediate cleanup, and native ABI exceptions | Mathematical formulas, unconditional resource release on failure, and backend-defined layouts need specific comments at the use site. |

`restriction` includes mutually exclusive requirements and bans on ordinary
renderer operations such as floating-point arithmetic. As recommended by
[Clippy](https://doc.rust-lang.org/clippy/lints.html#restriction), the entire
group is audited rather than used as a blanket CI gate. Nursery rules are
experimental; review new diagnostics before changing rendering semantics.

## Complete audit

CI also runs every group with `--force-warn`, including rules exempted locally,
and uploads JSON diagnostics and build logs as `clippy-audit-<os>-<backend>`
artifacts for 14 days. Warnings in this audit are informational; compilation
errors still fail the audit. The test-related configuration still applies.

To reproduce a complete audit locally, select the appropriate backend:

```sh
mkdir -p target/clippy-audit
cargo clippy --release --all-targets --no-default-features --features metal --message-format=json -- \
  --force-warn clippy::all --force-warn clippy::pedantic --force-warn clippy::nursery \
  --force-warn clippy::restriction --force-warn clippy::cargo \
  > target/clippy-audit/diagnostics.jsonl 2> target/clippy-audit/build.log
```

Do not apply every suggested fix blindly. In particular, changing eager
`result.and(release_scratch(...))` cleanup into a lazy closure would skip resource
release when recording fails. Numeric and ownership changes require focused
semantic tests, the relevant release suites, and rendering/performance checks.

## Initial rollout validation (2026-10-12)

Rust/Clippy 1.99.0 passed the selected policy with zero warnings for native
macOS Metal, cross-compiled Windows GNU DX12/Vulkan, cross-compiled Linux Vulkan
shader/shared code, and the backend-free library. Cross-compilation checks
Windows/Linux source and shaders; native Windows GPU execution was not available
on this host. The complete five-group audit also compiled successfully.

Temporary probes confirmed that production `unwrap`, `expect`, `panic!`,
redundant `Result`/`Option` returns, and a style violation fail the strict check,
while documented test exceptions pass. Probe sources were removed afterward.

On the pinned Apple M5 (`0000000100000575`), 1,224 release tests passed across
45 test targets, including ignored GPU/compiler tests, with API validation and
single-threaded execution. The 1,712 SVG corpus images, 18 additional SVG
resource scenes, and seven example outputs matched the previous images byte for
byte. Native presentation completed eight frames and a real window resize.
Formatting, doctests, and both website locales also passed.

Real 960×640 example workloads were compared against `abaa7e7` using 15
alternating before/after pairs. Each run warmed up for 32 frames, then measured
400 `render().wait()` frames without CPU readback or API validation:

| Workload | Before median | After median | Median paired change |
| --- | ---: | ---: | ---: |
| Mesh gradient | 0.593 ms | 0.591 ms | −0.09% |
| Custom glass | 0.671 ms | 0.671 ms | +0.03% |

These wall-clock measurements include intermittent slow runs in both binaries;
no samples were discarded. They show no persistent median regression and do not
establish a speedup. Temporary measurement sources were removed. Local audit,
pixel-comparison, and raw timing reports remain under `target/clippy-audit/`.
