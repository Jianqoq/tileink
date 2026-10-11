# macOS scripts

All entrypoints keep the terminal concise: successful runs show stage progress and the final log
path. Complete command output is stored in a unique `${TMPDIR:-/tmp}/tileink-*` file. Failures
print that path and the last 80 log lines.

`run_tests.sh` always runs Metal release tests with one harness thread and one Rayon worker. Cargo arguments go
before `--`; test-harness arguments go after it. SVG category wrappers delegate to
`run_svg_tests.sh`.

## Native Metal

The former WGSL parity/probe runners are removed because their reference runtime and Cargo
targets no longer exist. Use the current native suite from the repository root:

```sh
RAYON_NUM_THREADS=1 TILEINK_NATIVE_GPU=<registry-id> MTL_DEBUG_LAYER=1 \
  scripts/mac/run_tests.sh --lib native::runtime -- --include-ignored
```

For focused geometry checks, replace `native::runtime` with `native::runtime::sdf_gpu`.
These tests use analytic CPU references, geometric equivalences and physical coordinate
invariance; they require no archived WGSL output. The Metal toolchain checks remain available
through `run_tests.sh --test metal_toolchain -- --include-ignored`.

`run_examples.sh` builds the native examples, and `run_svg_tests.sh` renders the complete SVG
corpus or a selected category. The SVG runner writes reference/native PNGs beside fixtures.
Install Xcode command-line tools before running native Metal checks, pin the physical GPU,
and run GPU processes sequentially.

## DXC conformance on macOS

The ignored HLSL conformance tests compile real DXIL and SPIR-V programs and
require `TILEINK_NATIVE_DXC_PATH`. Metal rendering itself requires only Xcode.
Build the pinned Microsoft DXC v1.8.2502 tag (`b4711839eb9a87da7c3436d9b212e0492359fbbd`)
with Python 3, CMake 3.31 and Ninja:

```sh
git clone --depth 1 --branch v1.8.2502 https://github.com/microsoft/DirectXShaderCompiler.git target/toolchains/dxc-src-v1.8.2502
git -C target/toolchains/dxc-src-v1.8.2502 submodule update --init --depth 1 external/SPIRV-Headers external/SPIRV-Tools external/DirectX-Headers
cmake -S target/toolchains/dxc-src-v1.8.2502 -B target/toolchains/dxc-v1.8.2502 \
  -C target/toolchains/dxc-src-v1.8.2502/cmake/caches/PredefinedParams.cmake -G Ninja \
  -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_FLAGS=-Wno-invalid-specialization \
  -DHLSL_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_TESTS=OFF -DSPIRV_BUILD_TESTS=OFF -DLLVM_ENABLE_ZLIB=OFF
cmake --build target/toolchains/dxc-v1.8.2502 --target dxc dxcompiler dxildll --parallel 6
export TILEINK_NATIVE_DXC_PATH="$PWD/target/toolchains/dxc-v1.8.2502/bin/dxc"
TILEINK_TEST_GPU="$TILEINK_NATIVE_GPU" scripts/mac/run_tests.sh -- --include-ignored
```

The Clang diagnostic option permits this fixed DXC version's old standard-library
trait declarations with recent Apple SDKs. Compiler discovery hashes the macOS
`libdxcompiler.dylib`, including packages with a sibling `lib` directory. These
identities also include `libdxil.dylib` bytes or its absence, since DXC can select
an external validator. The tests verify both target containers and the DXIL
resource ABI; executing DX12 or Vulkan shaders still requires the corresponding
supported host and GPU.
