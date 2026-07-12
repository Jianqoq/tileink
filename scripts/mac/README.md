# macOS scripts

All entrypoints keep the terminal concise: successful runs show stage progress and the final log
path. Complete command output is stored in a unique `${TMPDIR:-/tmp}/tileink-*` file. Failures
print that path and the last 80 log lines.

`run_tests.sh` always runs release tests with exactly one test thread. SVG category wrappers delegate
to `run_svg_tests.sh`, and `run_retained_benchmarks.sh` exposes all Criterion retained-scene suites.
