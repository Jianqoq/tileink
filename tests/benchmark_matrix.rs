// Compile the exact Criterion inventory without a GPU dependency. The runner and
// these tests share their configuration, measurement DTOs, case IDs and scales.
#[allow(dead_code)]
#[path = "../examples/support/retained_measurements.rs"]
mod retained_bench;
#[allow(dead_code)]
#[path = "../examples/support/retained_stress_cases.rs"]
mod retained_stress;
#[allow(dead_code)]
#[path = "../benches/support/retained_stress_matrix.rs"]
mod stress_matrix;
