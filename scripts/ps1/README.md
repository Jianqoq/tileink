# PowerShell scripts

All entrypoints keep the terminal concise: successful runs show only stage progress and the final
log path. Complete stdout/stderr from Cargo and renderer executables is stored in a unique
`%TEMP%\tileink-*.log` file. On failure, the script prints the log path and the last 80 lines before
returning a non-zero exit code.

Child commands use the current PowerShell filesystem location, including `Push-Location` changes.
This fixes inherited native working directories selecting another Cargo package or zero matching
tests when an entrypoint is invoked from outside the repository.

`run_tests.ps1` always runs release tests with exactly one test thread; callers do not pass a thread
count. SVG category wrappers delegate to `run_svg_tests.ps1` and inherit the same logging policy.

## Check regenerated PNG pixels

From the repository root:

```powershell
.\scripts\ps1\compare_png_pixels.ps1
.\scripts\ps1\compare_png_pixels.ps1 -BaseRef HEAD~1
```

If the terminal is already in `scripts`, use `.\ps1\compare_png_pixels.ps1`.
The default baseline is the same file in Git `HEAD`, resolved to a commit once at startup.
Every PNG in that commit or tracked in the working tree is checked; new, non-ignored PNGs are
included and fail if they have no baseline. Deleted/unreadable files also fail. The script only
reads images and Git objects; it never regenerates, restores, stages or updates a PNG baseline.

This check exists because PNG compression/filter or metadata changes can produce a Git binary
diff with identical pixels. It requires exactly equal dimensions and decoded RGBA samples,
including alpha and RGB under full transparency. Palette, grayscale and tRNS inputs are expanded;
16-bit precision is preserved, with 8-bit samples scaled by 257 for comparison. There is no
tolerance or color-profile conversion. Compression and metadata are excluded from pixel equality;
this does not assert that color-management metadata is unchanged. Corrupt/truncated files and
animated PNGs fail explicitly rather than silently comparing an incomplete image.

The terminal prints a summary and the full log path. Each log entry identifies the file and
whether its bytes match, only its pixels match, or the check failed. Pixel differences include
the changed-pixel count and first `(x, y)` coordinate (zero-based), with old/new RGBA values on a
0–65535 scale. Exit codes: `0` = all pixels match, `1` = differences or image errors, `2` = setup/Git
failure. A failed Cargo build also returns non-zero.

The Rust helper reuses the existing `png` dependency and needs no Python or imaging installation.
It can also run on other platforms:

```sh
cargo run --release --example compare_png_pixels -- HEAD
cargo test --release --example compare_png_pixels -- --test-threads=1
```

Its semantic and temporary-Git-repository tests also run with the regular `cargo test --release` suite.
