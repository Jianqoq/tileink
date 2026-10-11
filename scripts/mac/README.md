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
