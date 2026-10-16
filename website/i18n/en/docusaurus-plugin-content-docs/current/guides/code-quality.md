---
title: Code quality checks
sidebar_position: 6
---

# Code quality checks

`rust-toolchain.toml` pins Rust, rustfmt, and Clippy. Before submitting a change,
run `cargo fmt --all --check` and select your platform's backend:

```sh
# Windows DX12
cargo clippy --release --all-targets -- -D warnings
# Windows Vulkan
cargo clippy --release --all-targets --no-default-features --features vulkan -- -D warnings
# macOS Metal
cargo clippy --release --all-targets --no-default-features --features metal -- -D warnings
```

Backend features are mutually exclusive, so `--all-features` is invalid. Linux
currently checks Vulkan shader compilation and shared code without a native
runtime adapter. That profile uses `-A dead_code -A unused_imports -D warnings`
after `--` to allow execution paths that have no adapter consuming them.

`Cargo.toml` enables `all`, `pedantic`, `nursery`, `cargo`, and selected
`restriction` rules. `all` includes style, complexity, performance, and
correctness checks. Additional checks cover API error contracts, unsafe safety
comments, redundant return wrappers, and `unwrap`, `expect`, and `panic!`.
CI applies the policy to tests, examples, and build scripts too.

Tests may abort on failed setup or assertions. Production invariants need a
local `#[allow(..., reason = "...")]` explanation; caller-input and I/O errors
should follow the existing error contract. Replacing `unwrap` with `expect`
alone does not satisfy the policy.

Numeric quantization, exact floating-point comparisons, and GPU ABI exceptions
are documented in configuration or at their use sites. Since `restriction`
contains conflicting rules, CI also audits every group with `--force-warn` and
uploads the diagnostics for review. Exempted rules remain visible in that audit.

See the repository's [Clippy development guide](https://github.com/Jianqoq/tileink/blob/main/docs/development/clippy.md)
for the full policy, exception reasons, and audit command. Semantic or ownership
changes also need single-threaded release tests. Rendering changes require SVG,
example pixel, and real application workload checks; lint suggestions alone do
not justify changing an algorithm.
