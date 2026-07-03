use std::{
    cell::RefCell,
    fmt,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WgpuRenderProfileEntry {
    pub name: &'static str,
    pub cpu_duration: Option<Duration>,
    pub gpu_duration: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WgpuRenderProfileEventSummary {
    pub name: &'static str,
    pub cpu_duration: Option<Duration>,
    pub gpu_duration: Option<Duration>,
    pub percent_of_cpu: f64,
    pub percent_of_gpu: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WgpuRenderProfile {
    entries: Vec<WgpuRenderProfileEntry>,
    cpu_total: Duration,
}

impl WgpuRenderProfile {
    pub fn entries(&self) -> &[WgpuRenderProfileEntry] {
        &self.entries
    }

    pub fn cpu_time(&self) -> Duration {
        self.cpu_total
    }

    pub fn gpu_time(&self) -> Duration {
        self.entries
            .iter()
            .filter_map(|entry| entry.gpu_duration)
            .sum()
    }

    pub fn summary(&self) -> Vec<WgpuRenderProfileEventSummary> {
        let cpu_total = self.cpu_time();
        let gpu_total = self.gpu_time();
        let mut summaries = Vec::<WgpuRenderProfileEventSummary>::new();

        for entry in &self.entries {
            if let Some(summary) = summaries
                .iter_mut()
                .find(|summary| summary.name == entry.name)
            {
                summary.cpu_duration =
                    merge_optional_duration(summary.cpu_duration, entry.cpu_duration);
                summary.gpu_duration =
                    merge_optional_duration(summary.gpu_duration, entry.gpu_duration);
            } else {
                summaries.push(WgpuRenderProfileEventSummary {
                    name: entry.name,
                    cpu_duration: entry.cpu_duration,
                    gpu_duration: entry.gpu_duration,
                    percent_of_cpu: 0.0,
                    percent_of_gpu: 0.0,
                });
            }
        }

        for summary in &mut summaries {
            summary.percent_of_cpu = summary
                .cpu_duration
                .map(|duration| percent(duration, cpu_total))
                .unwrap_or(0.0);
            summary.percent_of_gpu = summary
                .gpu_duration
                .map(|duration| percent(duration, gpu_total))
                .unwrap_or(0.0);
        }

        summaries
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WgpuRenderProfileReport {
    profile: WgpuRenderProfile,
    iterations: usize,
}

impl WgpuRenderProfileReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, profile: &WgpuRenderProfile) {
        self.iterations += 1;
        self.profile.entries.extend_from_slice(profile.entries());
        self.profile.cpu_total += profile.cpu_time();
    }

    pub fn iterations(&self) -> usize {
        self.iterations
    }

    pub fn profile(&self) -> &WgpuRenderProfile {
        &self.profile
    }
}

impl fmt::Display for WgpuRenderProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format_profile_table(self, 1))
    }
}

impl fmt::Display for WgpuRenderProfileReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            format_profile_table(&self.profile, self.iterations.max(1))
        )
    }
}

#[derive(Debug)]
pub(crate) struct WgpuRenderProfiler {
    state: Rc<RefCell<ProfileState>>,
    profile: WgpuRenderProfile,
}

impl Default for WgpuRenderProfiler {
    fn default() -> Self {
        Self {
            state: Rc::new(RefCell::new(ProfileState::default())),
            profile: WgpuRenderProfile::default(),
        }
    }
}

impl WgpuRenderProfiler {
    pub(crate) fn start(&mut self, device: &::wgpu::Device) {
        let _ = device.poll(::wgpu::PollType::wait_indefinitely());
        {
            let mut state = self.state.borrow_mut();
            state.entries.clear();
            state.pending_gpu.clear();
            state.started = Some(Instant::now());
            state.active = true;
        }
        self.profile = WgpuRenderProfile::default();
        ACTIVE_PROFILER.with(|active| {
            *active.borrow_mut() = Some(Rc::clone(&self.state));
        });
    }

    pub(crate) fn end(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
    ) -> &WgpuRenderProfile {
        let (mut entries, pending_gpu, cpu_total) = {
            let mut state = self.state.borrow_mut();
            state.active = false;
            let cpu_total = state
                .started
                .take()
                .map(|started| started.elapsed())
                .unwrap_or_default();
            (
                std::mem::take(&mut state.entries),
                std::mem::take(&mut state.pending_gpu),
                cpu_total,
            )
        };

        entries.extend(resolve_gpu_timers(device, queue, pending_gpu));
        self.profile = WgpuRenderProfile { entries, cpu_total };

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

        &self.profile
    }

    pub(crate) fn profile(&self) -> &WgpuRenderProfile {
        &self.profile
    }
}

#[derive(Debug, Default)]
struct ProfileState {
    entries: Vec<WgpuRenderProfileEntry>,
    pending_gpu: Vec<WgpuGpuProfileScope>,
    started: Option<Instant>,
    active: bool,
}

thread_local! {
    static ACTIVE_PROFILER: RefCell<Option<Rc<RefCell<ProfileState>>>> = const { RefCell::new(None) };
}

pub(crate) struct WgpuCpuProfileScope {
    state: Rc<RefCell<ProfileState>>,
    name: &'static str,
    started: Instant,
}

impl Drop for WgpuCpuProfileScope {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        if state.active {
            state.entries.push(WgpuRenderProfileEntry {
                name: self.name,
                cpu_duration: Some(self.started.elapsed()),
                gpu_duration: None,
            });
        }
    }
}

pub(crate) fn start_cpu_scope(name: &'static str) -> Option<WgpuCpuProfileScope> {
    let state = ACTIVE_PROFILER.with(|active| active.borrow().clone())?;
    if !state.borrow().active {
        return None;
    }
    Some(WgpuCpuProfileScope {
        state,
        name,
        started: Instant::now(),
    })
}

pub(crate) fn profile_cpu<T>(name: &'static str, work: impl FnOnce() -> T) -> T {
    let _scope = start_cpu_scope(name);
    work()
}

#[derive(Debug)]
pub(crate) struct WgpuGpuProfileScope {
    name: &'static str,
    query_set: ::wgpu::QuerySet,
    resolve_buffer: ::wgpu::Buffer,
    readback_buffer: ::wgpu::Buffer,
}

impl WgpuGpuProfileScope {
    pub(crate) fn timestamp_writes(&self) -> ::wgpu::ComputePassTimestampWrites<'_> {
        ::wgpu::ComputePassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    fn resolve(&self, encoder: &mut ::wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.query_set, 0..2, &self.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve_buffer,
            0,
            &self.readback_buffer,
            0,
            2 * ::wgpu::QUERY_SIZE as ::wgpu::BufferAddress,
        );
    }
}

pub(crate) fn start_gpu_scope(
    device: &::wgpu::Device,
    name: &'static str,
) -> Option<WgpuGpuProfileScope> {
    let state = ACTIVE_PROFILER.with(|active| active.borrow().clone())?;
    if !state.borrow().active {
        return None;
    }
    if !device
        .features()
        .contains(::wgpu::Features::TIMESTAMP_QUERY)
    {
        state.borrow_mut().entries.push(WgpuRenderProfileEntry {
            name,
            cpu_duration: None,
            gpu_duration: None,
        });
        return None;
    }

    let query_set = device.create_query_set(&::wgpu::QuerySetDescriptor {
        label: Some("tileink wgpu profile timestamps"),
        ty: ::wgpu::QueryType::Timestamp,
        count: 2,
    });
    let buffer_size = 2 * ::wgpu::QUERY_SIZE as ::wgpu::BufferAddress;
    let resolve_buffer = device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some("tileink wgpu profile timestamp resolve"),
        size: buffer_size,
        usage: ::wgpu::BufferUsages::QUERY_RESOLVE | ::wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback_buffer = device.create_buffer(&::wgpu::BufferDescriptor {
        label: Some("tileink wgpu profile timestamp readback"),
        size: buffer_size,
        usage: ::wgpu::BufferUsages::COPY_DST | ::wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    Some(WgpuGpuProfileScope {
        name,
        query_set,
        resolve_buffer,
        readback_buffer,
    })
}

pub(crate) fn finish_gpu_scope(
    encoder: &mut ::wgpu::CommandEncoder,
    timer: Option<WgpuGpuProfileScope>,
) {
    let Some(timer) = timer else {
        return;
    };
    timer.resolve(encoder);
    ACTIVE_PROFILER.with(|active| {
        if let Some(state) = active.borrow().as_ref()
            && state.borrow().active
        {
            state.borrow_mut().pending_gpu.push(timer);
        }
    });
}

fn resolve_gpu_timers(
    device: &::wgpu::Device,
    queue: &::wgpu::Queue,
    timers: Vec<WgpuGpuProfileScope>,
) -> Vec<WgpuRenderProfileEntry> {
    if timers.is_empty() {
        return Vec::new();
    }
    let _ = device.poll(::wgpu::PollType::wait_indefinitely());
    timers
        .into_iter()
        .map(|timer| {
            let (tx, rx) = mpsc::channel();
            timer
                .readback_buffer
                .slice(..)
                .map_async(::wgpu::MapMode::Read, move |result| {
                    tx.send(result).unwrap()
                });
            device
                .poll(::wgpu::PollType::wait_indefinitely())
                .expect("poll wgpu device for profile timestamp readback");
            rx.recv()
                .expect("receive profile timestamp map result")
                .expect("map profile timestamp readback buffer");

            let mapped = timer.readback_buffer.slice(..).get_mapped_range();
            let values: &[u64] = bytemuck::cast_slice(&mapped);
            let ticks = values[1].saturating_sub(values[0]);
            let nanos = ticks as f64 * queue.get_timestamp_period() as f64;
            drop(mapped);
            timer.readback_buffer.unmap();

            WgpuRenderProfileEntry {
                name: timer.name,
                cpu_duration: None,
                gpu_duration: Some(Duration::from_nanos(nanos.round() as u64)),
            }
        })
        .collect()
}

fn format_profile_table(profile: &WgpuRenderProfile, iterations: usize) -> String {
    let summaries = profile.summary();
    let mut rows = Vec::<[String; 5]>::with_capacity(summaries.len() + 2);
    rows.push([
        "event".to_string(),
        "cpu us".to_string(),
        "cpu %".to_string(),
        "gpu us".to_string(),
        "gpu %".to_string(),
    ]);
    for summary in summaries {
        rows.push([
            summary.name.to_string(),
            optional_duration(summary.cpu_duration, iterations),
            format!("{:.2}%", summary.percent_of_cpu),
            optional_duration(summary.gpu_duration, iterations),
            format!("{:.2}%", summary.percent_of_gpu),
        ]);
    }
    rows.push([
        "total".to_string(),
        format!("{:.3}", avg_micros(profile.cpu_time(), iterations)),
        "100.00%".to_string(),
        format!("{:.3}", avg_micros(profile.gpu_time(), iterations)),
        "100.00%".to_string(),
    ]);
    format_rows(&rows)
}

fn format_rows(rows: &[[String; 5]]) -> String {
    let mut widths = [0usize; 5];
    for row in rows {
        for (ix, cell) in row.iter().enumerate() {
            widths[ix] = widths[ix].max(cell.len());
        }
    }

    let mut out = String::new();
    for (row_ix, row) in rows.iter().enumerate() {
        for (cell_ix, cell) in row.iter().enumerate() {
            if cell_ix == 0 {
                out.push_str(&format!("{cell:<width$}", width = widths[cell_ix]));
            } else {
                out.push_str("  ");
                out.push_str(&format!("{cell:>width$}", width = widths[cell_ix]));
            }
        }
        if row_ix == 0 {
            out.push('\n');
            for (ix, width) in widths.iter().enumerate() {
                if ix > 0 {
                    out.push_str("  ");
                }
                out.push_str(&"-".repeat(*width));
            }
        }
        if row_ix + 1 < rows.len() {
            out.push('\n');
        }
    }
    out
}

fn optional_duration(duration: Option<Duration>, iterations: usize) -> String {
    duration
        .map(|duration| format!("{:.3}", avg_micros(duration, iterations)))
        .unwrap_or_else(|| "-".to_string())
}

fn merge_optional_duration(left: Option<Duration>, right: Option<Duration>) -> Option<Duration> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(duration), None) | (None, Some(duration)) => Some(duration),
        (None, None) => None,
    }
}

fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

fn avg_micros(duration: Duration, iterations: usize) -> f64 {
    micros(duration) / iterations as f64
}

fn percent(duration: Duration, total: Duration) -> f64 {
    if total.is_zero() {
        0.0
    } else {
        duration.as_secs_f64() * 100.0 / total.as_secs_f64()
    }
}
