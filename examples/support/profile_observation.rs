use std::time::Duration;

/// A profile scope can be absent without making its surrounding operation free.
/// Keep presence separate from elapsed time so Criterion never receives invented
/// timings for an unexecuted stage, and incomplete profiles cannot certify it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageObservation {
    pub profiled_frames: u64,
    pub entry_count: u64,
    pub timed_entries: u64,
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageState {
    Unobserved,
    NotExecuted,
    MissingTiming,
    ZeroDuration,
    Measured,
}

impl StageObservation {
    pub fn record_frame(&mut self, entries: impl IntoIterator<Item = Option<Duration>>) {
        self.profiled_frames += 1;
        for duration in entries {
            self.entry_count += 1;
            if let Some(duration) = duration {
                self.timed_entries += 1;
                self.elapsed += duration;
            }
        }
    }

    pub fn state(self) -> StageState {
        if self.profiled_frames == 0 {
            StageState::Unobserved
        } else if self.entry_count == 0 {
            StageState::NotExecuted
        } else if self.timed_entries != self.entry_count {
            StageState::MissingTiming
        } else if self.elapsed.is_zero() {
            StageState::ZeroDuration
        } else {
            StageState::Measured
        }
    }

    /// `None` is an observed absence, never a numeric performance sample.
    pub fn duration(self) -> Result<Option<Duration>, StageState> {
        match self.state() {
            StageState::NotExecuted => Ok(None),
            StageState::Measured => Ok(Some(self.elapsed)),
            state => Err(state),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn no_profiled_frames_cannot_certify_an_absent_stage() {
        use super::*;
        let observation = StageObservation::default();
        assert_eq!(observation.duration(), Err(StageState::Unobserved));
    }

    #[test]
    fn an_absent_stage_keeps_its_profiled_frame_count_without_a_fake_time() {
        use super::*;
        let mut observation = StageObservation::default();
        observation.record_frame([]);
        observation.record_frame([]);
        assert_eq!(observation.profiled_frames, 2);
        assert_eq!(observation.entry_count, 0);
        assert_eq!(observation.timed_entries, 0);
        assert_eq!(observation.state(), StageState::NotExecuted);
        assert_eq!(observation.duration(), Ok(None));
    }

    #[test]
    fn an_executed_zero_time_scope_is_not_an_absent_scope() {
        use super::*;
        let mut observation = StageObservation::default();
        observation.record_frame([Some(Duration::ZERO)]);
        assert_eq!(observation.entry_count, 1);
        assert_eq!(observation.timed_entries, 1);
        assert_eq!(observation.duration(), Err(StageState::ZeroDuration));
    }

    #[test]
    fn missing_timing_is_rejected_even_when_another_entry_has_a_duration() {
        use super::*;
        let mut observation = StageObservation::default();
        observation.record_frame([Some(Duration::from_nanos(17)), None]);
        assert_eq!(observation.entry_count, 2);
        assert_eq!(observation.timed_entries, 1);
        assert_eq!(observation.elapsed, Duration::from_nanos(17));
        assert_eq!(observation.duration(), Err(StageState::MissingTiming));
    }

    #[test]
    fn every_entry_and_frame_is_retained_in_the_observation() {
        use super::*;
        let mut observation = StageObservation::default();
        observation.record_frame([
            Some(Duration::from_nanos(17)),
            Some(Duration::from_nanos(23)),
        ]);
        observation.record_frame([]);
        observation.record_frame([Some(Duration::from_nanos(11))]);
        assert_eq!(observation.profiled_frames, 3);
        assert_eq!(observation.entry_count, 3);
        assert_eq!(observation.timed_entries, 3);
        assert_eq!(observation.state(), StageState::Measured);
        assert_eq!(observation.duration(), Ok(Some(Duration::from_nanos(51))));
    }
    #[test]
    fn a_stage_appearing_late_is_retained_after_an_empty_prefix() {
        use super::*;
        let mut observation = StageObservation::default();
        for _ in 0..20 {
            observation.record_frame([]);
        }
        assert_eq!(observation.duration(), Ok(None));
        observation.record_frame([Some(Duration::from_nanos(7))]);
        assert_eq!(observation.duration(), Ok(Some(Duration::from_nanos(7))));
        assert_eq!(observation.profiled_frames, 21);
        assert_eq!(observation.entry_count, 1);
    }
}
