# Windows release runners

Run `run_tests.ps1` for serial release tests, `run_examples.ps1` to build all native examples, and `run_svg_tests.ps1` for the complete SVG fixture corpus. The SVG runner accepts `-Type` to select one fixture group and `-ContinueOnError` to collect all failures. Output is logged through `quiet_runner.ps1`.

The SVG runner writes a `.native.png` beside each fixture. Keep these images for visual review and comparison with the reference PNGs.

The default Cargo feature is DX12. To build Vulkan directly, pass `--no-default-features --features vulkan` to Cargo.
