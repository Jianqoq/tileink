# Windows release runners

Run `run_tests.ps1` for serial release tests, `run_examples.ps1` to build all native examples, and `run_svg_tests.ps1` for the complete SVG fixture corpus. The SVG runner accepts `-Type` to select one fixture group and `-ContinueOnError` to collect all failures. Output is logged through `quiet_runner.ps1`.

The test runner fixes both the test harness and Rayon to one thread, including when
the parent environment requests parallel execution. Put Cargo arguments before
`--` and harness arguments after it; the runner inserts the separator only when
none was supplied. For example:

```powershell
.\scripts\ps1\run_tests.ps1 -CargoArgs @('--no-default-features', '--features', 'vulkan', '--', '--include-ignored')
```

Native commands use the first executable on PATH. Executable regression tests run
the shipped scripts under Windows PowerShell, checking command selection,
argument quoting, working directory, serial execution and failure diagnostics.
These checks replace source-string assertions that could not detect runner bugs.

The SVG runner writes a `.native.png` beside each fixture. Keep these images for visual review and comparison with the reference PNGs.

The default Cargo feature is DX12. To build Vulkan directly, pass `--no-default-features --features vulkan` to Cargo.
