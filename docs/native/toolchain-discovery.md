# Native DXC discovery

Native `dx12` and `vulkan` builds compile HLSL at build time. They no longer require
an environment variable when the maintained DXC release is already cached. This
fixes ordinary consumer/Gallery builds that could not use an installed toolchain.
Built executables continue to use embedded shaders and never require DXC at runtime.

Selection order:

1. `TILEINK_NATIVE_DXC_PATH`, then `TILEINK_DXC_PATH`: an explicit absolute path.
   Invalid overrides fail; they are never silently replaced by another compiler.
2. Cargo's effective target output directory, under `toolchains/dxc-v1.8.2502`.
   Explicit target-triple builds search the triple directory, then the shared target directory.
3. `<tileink package>/target/toolchains/dxc-v1.8.2502`.
4. `<user cache>/tileink/toolchains/dxc-v1.8.2502`.

The user cache is `%LOCALAPPDATA%` on Windows, or `$XDG_CACHE_HOME` / `$HOME/.cache`
on Linux. The build output root is derived from Cargo's absolute `OUT_DIR`, so
relative `CARGO_TARGET_DIR` and Cargo configuration work from consumer workspaces
without being incorrectly resolved against the Tileink dependency. Inside each release directory,
Windows uses `bin/x64/dxc.exe` (`bin/arm64/dxc.exe` on an ARM64 build host); Linux
uses `bin/dxc`. Install the compiler's companion libraries alongside it according
to the release layout. The version and compiler/library contents remain part of
the shader cache key, including when selected from a default cache.

No compiler is downloaded automatically and PATH/Windows SDK are not searched:
the SDK compiler used by some wgpu installations lacks SPIR-V support. Missing
tools report the searched executable paths and override variables. Ordinary wgpu
builds do not use this native discovery path. Routine shader-cache hits are build
output (visible with `cargo -vv`), not compiler warnings.

`tests/native_toolchain_discovery.rs` covers cache precedence, paths containing
spaces, authoritative overrides, absent compilers, consumer roots and target triples.
The Clippy cleanup removes obsolete cross-backend match arms and one-element
test loops after the features became mutually exclusive; no shader or rendering
algorithm changes are required.
