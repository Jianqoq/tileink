---
title: Installation
---

# Installation

Add `tileink = { path = "../tileink" }` and `peniko = "0.6.1"` to Cargo.toml. The default feature selects DX12 on Windows. For Vulkan on Windows/Linux or Metal on macOS, disable default features and enable exactly one backend: `cargo test --release --no-default-features --features vulkan -- --test-threads=1` or `--features metal`. The documentation site requires Node.js 20 or newer; run `npm install && npm run build` in `website/`.

Metal rendering on macOS requires the Xcode toolchain. Full tests with `--include-ignored` also require a pinned GPU and an absolute `TILEINK_NATIVE_DXC_PATH`, so HLSL tests actually compile DXIL and SPIR-V. DXC uses `libdxcompiler.dylib` on macOS; see the repository's `scripts/mac/README.md` for the pinned build instructions. These compilation checks do not replace GPU execution on Windows/Linux.
