use ::cubecl::{client::ComputeClient, prelude::Runtime};
#[cfg(feature = "profile")]
use std::{
    cell::RefCell,
    fmt,
    rc::Rc,
    time::{Duration, Instant},
};

#[cfg(feature = "profile")]
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderProfileMemorySpace {
    Cpu,
    Gpu,
}

#[cfg(feature = "profile")]
impl RenderProfileMemorySpace {
    fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
        }
    }
}

#[cfg(feature = "profile")]
impl fmt::Display for RenderProfileMemorySpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(feature = "profile")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderProfileMemoryEntry {
    pub space: RenderProfileMemorySpace,
    pub name: &'static str,
    /// Logical bytes addressed by kernels or uploads for this renderer-owned group.
    pub used_bytes: usize,
    /// Requested or retained allocation capacity for this renderer-owned group.
    pub allocated_bytes: usize,
}

#[cfg(feature = "profile")]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderProfile {
    entries: Vec<RenderProfileEntry>,
    memory_entries: Vec<RenderProfileMemoryEntry>,
    wall_time: Duration,
}

#[cfg(feature = "profile")]
impl RenderProfile {
    pub fn entries(&self) -> &[RenderProfileEntry] {
        &self.entries
    }

    /// Renderer-owned memory snapshot captured by `end_profile`.
    ///
    /// This reports logical bytes and retained buffer capacity requested by the
    /// renderer. It does not include driver heap overhead, allocator metadata,
    /// or memory owned by the caller's input scene.
    pub fn memory_entries(&self) -> &[RenderProfileMemoryEntry] {
        &self.memory_entries
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

    /// Total logical bytes addressed by renderer-owned memory.
    pub fn memory_used_bytes(&self) -> usize {
        self.memory_entries
            .iter()
            .map(|entry| entry.used_bytes)
            .sum()
    }

    pub fn memory_used_bytes_in(&self, space: RenderProfileMemorySpace) -> usize {
        self.memory_entries
            .iter()
            .filter(|entry| entry.space == space)
            .map(|entry| entry.used_bytes)
            .sum()
    }

    /// Total retained capacity for renderer-owned memory.
    pub fn memory_allocated_bytes(&self) -> usize {
        self.memory_entries
            .iter()
            .map(|entry| entry.allocated_bytes)
            .sum()
    }

    pub fn memory_allocated_bytes_in(&self, space: RenderProfileMemorySpace) -> usize {
        self.memory_entries
            .iter()
            .filter(|entry| entry.space == space)
            .map(|entry| entry.allocated_bytes)
            .sum()
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderProfileReport {
    profile: RenderProfile,
    iterations: usize,
}

#[cfg(feature = "profile")]
impl RenderProfileReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one completed profile to the report.
    ///
    /// Timing entries are accumulated and printed as per-iteration averages.
    /// Memory is a retained-capacity snapshot, so the report keeps the latest
    /// snapshot instead of averaging capacities across frames.
    pub fn push(&mut self, profile: &RenderProfile) {
        self.iterations += 1;
        self.profile.entries.extend_from_slice(profile.entries());
        self.profile.wall_time += profile.wall_time();
        self.profile.memory_entries = profile.memory_entries().to_vec();
    }

    pub fn iterations(&self) -> usize {
        self.iterations
    }

    pub fn profile(&self) -> &RenderProfile {
        &self.profile
    }
}

#[cfg(feature = "profile")]
impl fmt::Display for RenderProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format_profile_tables(self, 1))
    }
}

#[cfg(feature = "profile")]
impl fmt::Display for RenderProfileReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            format_profile_tables(&self.profile, self.iterations.max(1))
        )
    }
}

#[cfg(feature = "profile")]
fn format_profile_tables(profile: &RenderProfile, iterations: usize) -> String {
    let mut output = format_timing_table(profile, iterations);
    if !profile.memory_entries.is_empty() {
        output.push_str("\n\n");
        output.push_str(&format_memory_table(profile));
    }
    output
}

#[cfg(feature = "profile")]
fn format_timing_table(profile: &RenderProfile, iterations: usize) -> String {
    let mut table = profile_table();
    table.set_header(vec![
        Cell::new("event"),
        right_cell("kernel us"),
        right_cell("wall us"),
        right_cell("kernel %"),
        right_cell("event %"),
        right_cell("wall %"),
    ]);
    for summary in profile.summary() {
        table.add_row(vec![
            Cell::new(summary.name),
            right_cell(
                summary
                    .kernel_duration
                    .map(|duration| format!("{:.3}", avg_micros(duration, iterations)))
                    .unwrap_or_else(|| "-".to_string()),
            ),
            right_cell(format!("{:.3}", avg_micros(summary.duration, iterations))),
            right_cell(format!("{:.2}%", summary.percent_of_kernel)),
            right_cell(format!("{:.2}%", summary.percent_of_attributed)),
            right_cell(format!("{:.2}%", summary.percent_of_wall)),
        ]);
    }

    let unattributed = profile.unattributed_time();
    if unattributed > Duration::ZERO {
        table.add_row(vec![
            Cell::new("unattributed"),
            right_cell(""),
            right_cell(format!("{:.3}", avg_micros(unattributed, iterations))),
            right_cell(""),
            right_cell(""),
            right_cell(format!("{:.2}%", percent(unattributed, profile.wall_time))),
        ]);
    }
    table.add_row(vec![
        Cell::new("total"),
        right_cell(format!(
            "{:.3}",
            avg_micros(profile.kernel_time(), iterations)
        )),
        right_cell(format!("{:.3}", avg_micros(profile.wall_time, iterations))),
        right_cell(""),
        right_cell(""),
        right_cell("100.00%"),
    ]);
    table.to_string()
}

#[cfg(feature = "profile")]
fn format_memory_table(profile: &RenderProfile) -> String {
    let mut table = profile_table();
    table.set_header(vec![
        Cell::new("space"),
        Cell::new("memory"),
        right_cell("used"),
        right_cell("allocated"),
    ]);
    for entry in &profile.memory_entries {
        table.add_row(vec![
            Cell::new(entry.space.label()),
            Cell::new(entry.name),
            right_cell(format_bytes(entry.used_bytes)),
            right_cell(format_bytes(entry.allocated_bytes)),
        ]);
    }
    for space in [RenderProfileMemorySpace::Gpu, RenderProfileMemorySpace::Cpu] {
        if profile
            .memory_entries
            .iter()
            .any(|entry| entry.space == space)
        {
            table.add_row(vec![
                Cell::new(space.label()),
                Cell::new("subtotal"),
                right_cell(format_bytes(profile.memory_used_bytes_in(space))),
                right_cell(format_bytes(profile.memory_allocated_bytes_in(space))),
            ]);
        }
    }
    table.add_row(vec![
        Cell::new(""),
        Cell::new("total"),
        right_cell(format_bytes(profile.memory_used_bytes())),
        right_cell(format_bytes(profile.memory_allocated_bytes())),
    ]);
    table.to_string()
}

#[cfg(feature = "profile")]
fn profile_table() -> Table {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL_CONDENSED);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table
}

#[cfg(feature = "profile")]
fn right_cell(value: impl ToString) -> Cell {
    Cell::new(value).set_alignment(CellAlignment::Right)
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
                memory_entries: Vec::new(),
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

    pub(crate) fn set_memory_entries(&mut self, memory_entries: Vec<RenderProfileMemoryEntry>) {
        self.profile.memory_entries = memory_entries;
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
fn avg_micros(duration: Duration, iterations: usize) -> f64 {
    micros(duration) / iterations as f64
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

#[cfg(feature = "profile")]
fn format_bytes(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let value = bytes as f64;
    if value >= GIB {
        format!("{:.2} GiB", value / GIB)
    } else if value >= MIB {
        format!("{:.2} MiB", value / MIB)
    } else if value >= KIB {
        format!("{:.2} KiB", value / KIB)
    } else {
        format!("{bytes} B")
    }
}
