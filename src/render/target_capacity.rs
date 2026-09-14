//! Capacity policy shared by owned render targets and scratch allocations.
//!
//! The adapter allocates only when a replacement size is returned and commits
//! that capacity only after allocation succeeds. Logical render bounds stay separate.

pub(crate) fn resized_capacity(
    current: (u32, u32),
    required: (u32, u32),
    limit: impl FnOnce() -> u32,
) -> Option<(u32, u32)> {
    let fits = required.0 <= current.0 && required.1 <= current.1;
    if fits {
        let excessively_wide = required.0.saturating_mul(2) < current.0;
        let excessively_tall = required.1.saturating_mul(2) < current.1;
        return (excessively_wide || excessively_tall).then_some(required);
    }
    let limit = limit();
    Some((
        if required.0 > current.0 {
            grown_dimension(current.0, required.0, limit)
        } else {
            current.0
        },
        if required.1 > current.1 {
            grown_dimension(current.1, required.1, limit)
        } else {
            current.1
        },
    ))
}

fn grown_dimension(current: u32, required: u32, limit: u32) -> u32 {
    debug_assert!(current > 0);
    debug_assert!(required > current);
    required.max(current.saturating_add(current / 2).min(limit))
}

#[cfg(test)]
mod tests;
