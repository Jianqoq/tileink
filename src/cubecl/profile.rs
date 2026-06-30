use ::cubecl::{client::ComputeClient, prelude::Runtime};
#[cfg(feature = "profile")]
use std::{
    cell::RefCell,
    fmt,
    rc::Rc,
    time::{Duration, Instant},
};

#[cfg(feature = "profile")]
use cubecl_common::profile::TimingMethod;

#[cfg(feature = "profile")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderProfileEntry {
    pub name: &'static str,
    /// CPU-observed wall time for the profiled event, including profiling overhead.
    pub duration: Duration,
    /// GPU timestamp duration for kernel launches. Non-kernel CPU events, and
    /// runtimes without device timestamps, leave this empty.
    pub kernel_duration: Option<Duration>,
}

#[cfg(feature = "profile")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderProfileEventSummary {
    pub name: &'static str,
    /// Aggregated CPU-observed wall time.
    pub duration: Duration,
    /// Aggregated GPU timestamp duration when available.
    pub kernel_duration: Option<Duration>,
    pub percent_of_attributed: f64,
    pub percent_of_kernel: f64,
    pub percent_of_wall: f64,
}

#[cfg(feature = "profile")]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderProfile {
    entries: Vec<RenderProfileEntry>,
    wall_time: Duration,
}

#[cfg(feature = "profile")]
impl RenderProfile {
    pub fn entries(&self) -> &[RenderProfileEntry] {
        &self.entries
    }

    pub fn wall_time(&self) -> Duration {
        self.wall_time
    }

    pub fn attributed_time(&self) -> Duration {
        self.entries
            .iter()
            .map(|entry| entry.duration)
            .sum::<Duration>()
    }

    pub fn unattributed_time(&self) -> Duration {
        self.wall_time
            .checked_sub(self.attributed_time())
            .unwrap_or(Duration::ZERO)
    }

    pub fn kernel_time(&self) -> Duration {
        self.entries
            .iter()
            .filter_map(|entry| entry.kernel_duration)
            .sum::<Duration>()
    }

    pub fn summary(&self) -> Vec<RenderProfileEventSummary> {
        let attributed = self.attributed_time();
        let kernel = self.kernel_time();
        let mut summaries = Vec::<RenderProfileEventSummary>::new();
        for entry in &self.entries {
            if let Some(summary) = summaries
                .iter_mut()
                .find(|summary| summary.name == entry.name)
            {
                summary.duration += entry.duration;
                summary.kernel_duration =
                    merge_optional_duration(summary.kernel_duration, entry.kernel_duration);
            } else {
                summaries.push(RenderProfileEventSummary {
                    name: entry.name,
                    duration: entry.duration,
                    kernel_duration: entry.kernel_duration,
                    percent_of_attributed: 0.0,
                    percent_of_kernel: 0.0,
                    percent_of_wall: 0.0,
                });
            }
        }
        for summary in &mut summaries {
            summary.percent_of_attributed = percent(summary.duration, attributed);
            summary.percent_of_kernel = summary
                .kernel_duration
                .map(|duration| percent(duration, kernel))
                .unwrap_or(0.0);
            summary.percent_of_wall = percent(summary.duration, self.wall_time);
        }
        summaries
    }
}

#[cfg(feature = "profile")]
impl fmt::Display for RenderProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{:<36} {:>12} {:>12} {:>10} {:>10} {:>9}",
            "event", "kernel us", "wall us", "kernel %", "event %", "wall %"
        )?;
        for summary in self.summary() {
            let kernel_micros = summary
                .kernel_duration
                .map(|duration| format!("{:.3}", micros(duration)))
                .unwrap_or_else(|| "-".to_string());
            writeln!(
                f,
                "{:<36} {:>12} {:>12.3} {:>9.2}% {:>9.2}% {:>8.2}%",
                summary.name,
                kernel_micros,
                micros(summary.duration),
                summary.percent_of_kernel,
                summary.percent_of_attributed,
                summary.percent_of_wall
            )?;
        }
        let unattributed = self.unattributed_time();
        if unattributed > Duration::ZERO {
            writeln!(
                f,
                "{:<36} {:>12} {:>12.3} {:>10} {:>9} {:>8.2}%",
                "unattributed",
                "",
                micros(unattributed),
                "",
                "",
                percent(unattributed, self.wall_time)
            )?;
        }
        writeln!(
            f,
            "{:<36} {:>12.3} {:>12.3} {:>10} {:>9} {:>8.2}%",
            "total",
            micros(self.kernel_time()),
            micros(self.wall_time),
            "",
            "",
            100.0
        )
    }
}

#[cfg(feature = "profile")]
#[derive(Debug, Default)]
struct ProfileState {
    entries: Vec<RenderProfileEntry>,
    wall_time: Duration,
    active: bool,
    render_start: Option<Instant>,
}

#[cfg(feature = "profile")]
thread_local! {
    static ACTIVE_PROFILER: RefCell<Option<Rc<RefCell<ProfileState>>>> = const { RefCell::new(None) };
}

#[cfg(feature = "profile")]
#[derive(Debug)]
pub(crate) struct RenderProfiler {
    state: Rc<RefCell<ProfileState>>,
    profile: RenderProfile,
}

#[cfg(feature = "profile")]
impl Default for RenderProfiler {
    fn default() -> Self {
        Self {
            state: Rc::new(RefCell::new(ProfileState::default())),
            profile: RenderProfile::default(),
        }
    }
}

#[cfg(feature = "profile")]
impl RenderProfiler {
    pub(crate) fn start(&mut self) {
        {
            let mut state = self.state.borrow_mut();
            state.entries.clear();
            state.wall_time = Duration::ZERO;
            state.active = true;
            state.render_start = Some(Instant::now());
        }
        self.profile = RenderProfile::default();
        ACTIVE_PROFILER.with(|active| {
            *active.borrow_mut() = Some(Rc::clone(&self.state));
        });
    }

    pub(crate) fn end(&mut self) {
        self.profile = {
            let mut state = self.state.borrow_mut();
            if let Some(start) = state.render_start.take() {
                state.wall_time = start.elapsed();
            }
            state.active = false;
            RenderProfile {
                entries: state.entries.clone(),
                wall_time: state.wall_time,
            }
        };
        let state = Rc::clone(&self.state);
        ACTIVE_PROFILER.with(|active| {
            let mut active = active.borrow_mut();
            if active
                .as_ref()
                .is_some_and(|active| Rc::ptr_eq(active, &state))
            {
                *active = None;
            }
        });
    }

    pub(crate) fn profile(&self) -> &RenderProfile {
        &self.profile
    }
}

#[cfg(feature = "profile")]
pub(crate) struct RenderProfileTimer {
    profiler: Rc<RefCell<ProfileState>>,
    name: &'static str,
    start: Instant,
}

#[cfg(feature = "profile")]
pub(crate) fn start_profile_scope(name: &'static str) -> Option<RenderProfileTimer> {
    let profiler = ACTIVE_PROFILER.with(|active| active.borrow().clone())?;
    let active = profiler.borrow().active;
    if !active {
        return None;
    }
    Some(RenderProfileTimer {
        profiler,
        name,
        start: Instant::now(),
    })
}

#[cfg(feature = "profile")]
pub(crate) fn finish_profile_scope<R: Runtime>(
    client: &ComputeClient<R>,
    timer: Option<RenderProfileTimer>,
) {
    let Some(timer) = timer else {
        return;
    };
    sync_client(client);
    timer
        .profiler
        .borrow_mut()
        .entries
        .push(RenderProfileEntry {
            name: timer.name,
            duration: timer.start.elapsed(),
            kernel_duration: None,
        });
}

pub(crate) fn profile_launch<R: Runtime>(
    client: &ComputeClient<R>,
    name: &'static str,
    launch: impl FnOnce() + Send,
) {
    profile_scope(client, name, launch);
}

#[cfg(feature = "profile")]
pub(crate) fn profile_scope<R: Runtime>(
    client: &ComputeClient<R>,
    name: &'static str,
    work: impl FnOnce() + Send,
) {
    let profiler = ACTIVE_PROFILER.with(|active| active.borrow().clone());
    let Some(profiler) = profiler else {
        work();
        return;
    };
    if !profiler.borrow().active {
        work();
        return;
    }

    let wall_start = Instant::now();
    let (_, profile) = client
        .profile(work, name)
        .expect("CubeCL profile launch failed");
    let kernel_duration = if profile.timing_method() == TimingMethod::Device {
        Some(cubecl_common::future::block_on(profile.resolve()).duration())
    } else {
        None
    };
    profiler.borrow_mut().entries.push(RenderProfileEntry {
        name,
        duration: wall_start.elapsed(),
        kernel_duration,
    });
}

#[cfg(not(feature = "profile"))]
pub(crate) fn profile_scope<R: Runtime>(
    _client: &ComputeClient<R>,
    _name: &'static str,
    work: impl FnOnce() + Send,
) {
    work();
}

#[cfg(feature = "profile")]
pub(crate) fn sync_client<R: Runtime>(client: &ComputeClient<R>) {
    cubecl_common::future::block_on(client.sync()).expect("CubeCL profile sync failed");
}

#[cfg(feature = "profile")]
fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

#[cfg(feature = "profile")]
fn merge_optional_duration(left: Option<Duration>, right: Option<Duration>) -> Option<Duration> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(duration), None) | (None, Some(duration)) => Some(duration),
        (None, None) => None,
    }
}

#[cfg(feature = "profile")]
fn percent(duration: Duration, total: Duration) -> f64 {
    if total.is_zero() {
        0.0
    } else {
        duration.as_secs_f64() * 100.0 / total.as_secs_f64()
    }
}
