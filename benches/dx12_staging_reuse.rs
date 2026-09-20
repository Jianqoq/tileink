use criterion::{criterion_group, criterion_main};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
#[path = "support/dx12_staging.rs"]
mod support;
criterion_group!(benches, support::benchmark);
criterion_main!(benches);
