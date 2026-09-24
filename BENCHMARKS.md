# Benchmarks

Criterion benchmarks measure frame preparation, native submission, and retained scene updates. Run release benchmarks with the feature for the device under test:

```sh
cargo bench --no-default-features --features dx12
cargo bench --no-default-features --features vulkan
cargo bench --no-default-features --features metal
```

The target machine must support the selected GPU API. Criterion stores measurements under `target/criterion`. Compare runs on the same physical adapter and keep the scene, output size, validation setting, and toolchain fixed.

Performance changes require a scenario-specific Criterion benchmark and a result showing no regression beyond noise or an improvement.
