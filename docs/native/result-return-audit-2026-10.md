# Result return audit — 2026-10-12

## Scope and method

The initial scan parsed all 692 maintained Rust files, including the build
script, examples and tests. It found 1,047 `Result` function signatures, of which
1,018 have bodies. Three Vulkan FFI callbacks returning the unrelated
`vk::Result` status enum were excluded from those counts.

`ra-lsp` was unavailable. A temporary `syn` AST scan located definitions,
aliases, trait contracts, return paths, closures and macro call sites. All 196
functions without a direct `?` or `Err` were reviewed for forwarded errors,
fallible iterator collection and callback/macro error propagation. The 29 trait
declarations and 26 additional closure candidates were checked against their
contracts.
Release Clippy checks covered all targets for Metal on macOS and cross-compiled
DX12/Vulkan on Windows, including exported functions. Linux-specific branches
were also inspected in source. Nested returns such as the Metal completion
probe's `Option<Result<()>>` retain their pending/success/error states. Temporary
scanner code is removed after use.

## Changes

- `metal::frame::readback::Output::read` returns `Vec<u8>`. Shared memory is
  already allocated and the GPU has completed before these bytes are copied.
- `metal::frame::Frame::readback` returns `Vec<Vec<u8>>`. Collecting those
  infallible copies needs no error wrapper either.
- The private `PatternBrush::new_resource` constructor returns `Self`, removing
  an always-present `Option` and three unreachable SVG rejection branches.
  Resource registration still validates images; the rectangle and natural-size
  helpers retain their real `None` cases.
- A backend-free lint check exposed an unused native-only constant
  accessor. It is now compiled with the native shader generator that uses it;
  host constant generation remains unconditional. CI also checks the
  backend-free library's wrapper lint. That profile intentionally has no
  renderer consuming its internal execution code; only its dead-code/import
  diagnostics are excluded from this additional check. Supported native
  profiles retain the complete `-D warnings` checks.

These are permanent type/call-chain corrections, not temporary adapters. No
public renderer API changed. `Metal::readback_batch` still reports submission,
completion, device and ticket errors before materializing the result. DX12
readback still returns `Result` because mapping its readback resource can fail.

## Required contracts retained

The macOS example's `Window::inner_size` and `Window::request_inner_size` share
the application-facing contract with Win32. `GetClientRect`,
`AdjustWindowRectEx`, checked dimension conversion and `SetWindowPos` can fail.
Removing the common `Result` would discard those errors or split every caller
by platform. The two macOS methods carry narrowly scoped lint expectations
explaining this requirement.

`fmt::Write`, `HasWindowHandle`, renderer adapters and shader-cache callbacks
retain their required trait/callback signatures. Success-only test callbacks
implement fallible contracts; their production counterparts or failure-injection
cases actually fail. Functions that forward a parser/allocation/validation
error, collect fallible iterators, use checked arithmetic or convert missing
resources to errors remain fallible.

## Regression prevention and validation

`clippy::unnecessary_wraps` is enabled in Cargo, and `clippy.toml` includes
exported APIs in the check. The check failed on the old implementations before
the cleanup. The new GPU regression test also failed to compile against both
old readback signatures. It checks the infallible signatures, repeated reads,
mixed buffer/texture output order, aligned and padded rows, and array layers.
Existing tests cover empty outputs and actual completion failures.

Validation used Rust 1.99.0, release mode, one test/Rayon thread and the physical
Apple M5 (`0000000100000575`) with `MTL_DEBUG_LAYER=1`:

- Focused mixed-readback GPU regression passed.
- All 1,218 tests across 45 test binaries passed, with no failures or ignored
  tests. The documentation-test command also completed (no Rust doctests).
- Strict `-D warnings` Clippy passed for macOS Metal and Windows GNU
  cross-compiled DX12/Vulkan, including all test/example targets.
- The backend-free library's additional `unnecessary_wraps` check passed.
- The library builds with no native backend, all Metal examples build, and
  `cargo fmt --all --check` and `git diff --check` pass.
- All 1,730 SVGs were freshly rendered; their PNGs are byte-identical to the
  pre-change output and the SVG sources are unchanged. All seven image-example
  PNGs are also fresh and byte-identical.
- Native Metal presentation completed eight frames and resized from 640×360 to
  480×270.

Windows checks here are cross-compilation/lint checks; GPU execution validation
was performed on Metal. This cleanup makes no runtime speedup claim.
