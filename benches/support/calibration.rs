use std::time::Duration;

/// Prime the case's driver cache before `Bencher::iter_custom` starts calibration.
/// A cold pipeline compile otherwise turns the warmup estimate into minutes per
/// iteration even when the returned frame duration is milliseconds. Each invocation
/// still creates its own scene, renderer resources and history inside `routine`.
pub fn warm_once<R>(ready: &mut bool, mut routine: R) -> R
where
    R: FnMut(u64) -> Duration,
{
    if !*ready {
        std::hint::black_box(routine(1));
        *ready = true;
    }
    routine
}

#[cfg(test)]
mod tests {
    #[test]
    fn prewarming_precedes_sampling_and_does_not_change_requested_iterations() {
        use super::*;
        let mut ready = false;
        let mut calls = Vec::new();
        for iterations in [1, 2, 9] {
            let mut routine = warm_once(&mut ready, |count| {
                calls.push(count);
                Duration::from_nanos(count)
            });
            assert_eq!(routine(iterations), Duration::from_nanos(iterations));
        }
        assert_eq!(calls, [1, 1, 2, 9]);
    }

    #[test]
    fn each_case_has_its_own_warmup() {
        use super::*;
        let mut ready = [false; 2];
        let mut calls = Vec::new();
        for case in [0, 1, 0] {
            let mut routine = warm_once(&mut ready[case], |count| {
                calls.push((case, count));
                Duration::ZERO
            });
            routine(7);
        }
        assert_eq!(calls, [(0, 1), (0, 7), (1, 1), (1, 7), (0, 7)]);
    }
}
