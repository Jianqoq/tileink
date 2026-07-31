# PowerShell scripts

All entrypoints keep the terminal concise: successful runs show only stage progress and the final
log path. Complete stdout/stderr from Cargo and renderer executables is stored in a unique
`%TEMP%\tileink-*.log` file. On failure, the script prints the log path and the last 80 lines before
returning a non-zero exit code.

`run_tests.ps1` always runs release tests with exactly one test thread; callers do not pass a thread
count. SVG category wrappers delegate to `run_svg_tests.ps1` and inherit the same logging policy.
