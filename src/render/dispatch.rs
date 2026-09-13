/// Linear compute workloads use two dimensions once one device dimension is exhausted. Shaders
/// that use this helper must linearize `workgroup_id` with the dispatched X width in the same order.
pub(crate) fn dispatch_2d(workgroups: u32, maximum_dimension: u32) -> (u32, u32) {
    assert!(workgroups > 0 && maximum_dimension > 0);
    let x = workgroups.min(maximum_dimension);
    let y = workgroups.div_ceil(x);
    assert!(
        y <= maximum_dimension,
        "compute dispatch exceeds the device's 2D workgroup capacity"
    );
    // Padded tail groups also compute a u32 linear workgroup ID. More than
    // 2^32 dispatched groups would wrap that ID before the shader's tail guard.
    // A 2^16-wide grid covers every u32 workload without that wrap. This case
    // can only occur when both device dimensions already allow that width.
    if u64::from(x) * u64::from(y) > (1_u64 << 32) {
        let safe_width = 1 << 16;
        (safe_width, workgroups.div_ceil(safe_width))
    } else {
        (x, y)
    }
}

#[cfg(test)]
mod tests;
