//! Submission failure must not fabricate a waitable fence value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retirement {
    Idle,
    Unfenced,
    Signaled(u64),
    Failed,
}

impl Retirement {
    pub fn wait_value(self) -> Result<Option<u64>, &'static str> {
        match self {
            Self::Idle => Ok(None),
            Self::Signaled(value) => Ok(Some(value)),
            Self::Unfenced => Err("GPU work was submitted without a confirmed fence signal"),
            Self::Failed => Err("GPU completion could not be confirmed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Retirement;
    #[test]
    fn failed_signal_cannot_turn_into_a_wait_for_an_unqueued_fence() {
        let state = Retirement::Unfenced;
        assert!(state.wait_value().is_err());
        assert_ne!(state, Retirement::Idle);
        assert_eq!(Retirement::Signaled(9).wait_value(), Ok(Some(9)));
    }
    #[test]
    fn a_failed_wait_stays_unretired_instead_of_releasing_live_resources() {
        assert!(Retirement::Failed.wait_value().is_err());
        assert_eq!(Retirement::Idle.wait_value(), Ok(None));
    }
}
