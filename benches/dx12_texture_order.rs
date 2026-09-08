#[cfg(windows)]
#[path = "../tests/support/dx12_texture_order.rs"]
mod support;

#[cfg(windows)]
fn texture_order(c: &mut criterion::Criterion) {
    let writes = support::TextureWrites::new();
    let mut group = c.benchmark_group("dx12_write_only_texture_order");
    group.sample_size(40);
    for pairs in [1, 8] {
        // Compile and initialize before timing; the regression test checks pixels.
        let encoder = writes.encode(pairs);
        writes.queue.submit([encoder.finish()]);
        writes
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(pairs),
            &pairs,
            |b, &pairs| {
                b.iter(|| {
                    let encoder = writes.encode(pairs);
                    writes.queue.submit([encoder.finish()]);
                    writes
                        .device
                        .poll(wgpu::PollType::wait_indefinitely())
                        .unwrap();
                });
            },
        );
    }
    // Keep the same support implementation used by the pixel regression linked
    // into the benchmark, without including readback in the timed section.
    writes.assert_cleared(1, 0);
    group.finish();
}

#[cfg(windows)]
criterion::criterion_group!(benches, texture_order);
#[cfg(windows)]
criterion::criterion_main!(benches);

#[cfg(not(windows))]
fn main() {}
