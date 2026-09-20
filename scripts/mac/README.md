# macOS scripts

All entrypoints keep the terminal concise: successful runs show stage progress and the final log
path. Complete command output is stored in a unique `${TMPDIR:-/tmp}/tileink-*` file. Failures
print that path and the last 80 log lines.

`run_tests.sh` always runs release tests with exactly one test thread. SVG category wrappers delegate
to `run_svg_tests.sh`, and `run_retained_benchmarks.sh` exposes all Criterion retained-scene suites.

## Native Metal

`run_metal_probes.sh` verifies the independent MSL toolchain and same-device probes.
`run_native_metal_tests.sh` runs focused native runtime/render checks by default.
Use `--svg`, `--examples` or `--retained` for the complete corresponding same-device
corpus, and `--present` for the host presentation GPU test and real window smoke.
Reference and Metal builds are exclusive; all GPU processes run sequentially with
Metal validation, release optimization and one test thread. Evidence lives under
`target/metal-validation`; these native parity runners never update checked-in PNGs.
See [Metal setup](../../docs/native/metal.md) and [Mac M5](../../docs/native/m5-metal.md).
