# Native DXC discovery

Native DX12 and Vulkan builds compile HLSL during the Cargo build. The executable embeds shaders and does not need DXC at runtime.

Compiler selection order:

1. `TILEINK_NATIVE_DXC_PATH`, then `TILEINK_DXC_PATH`, each requiring an absolute path. An invalid explicit override fails instead of silently selecting another compiler.
2. Cargo's effective target output directory under `toolchains/dxc-v1.8.2502`, checking a target-triple directory before the shared target directory.
3. The Tileink package's `target/toolchains/dxc-v1.8.2502`.
4. The user's `tileink/toolchains/dxc-v1.8.2502` cache under `%LOCALAPPDATA%` on Windows, or `$XDG_CACHE_HOME` / `$HOME/.cache` on Linux.

Windows uses `bin/x64/dxc.exe` or `bin/arm64/dxc.exe` for an ARM64 build host; Linux uses `bin/dxc`. Companion compiler libraries must be installed with the release. The compiler version and binaries participate in shader cache identity. Discovery does not download tools. Missing tools report the searched paths and override variables.

Run `cargo test --release --test native_toolchain_discovery -- --test-threads=1` to verify precedence, spaces in paths, overrides, consumer roots, and target triples.
