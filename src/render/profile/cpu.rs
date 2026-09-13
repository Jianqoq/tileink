//! CPU scopes belong to the shared executor and never require a GPU adapter.
use super::RenderProfileEntry;
use crate::shared::cpu_time::CpuInstant;
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Debug, Default)]
pub(crate) struct CpuProfile {
    pub entries: Vec<RenderProfileEntry>,
    pub total: Duration,
}

#[derive(Debug, Default)]
struct ProfileState {
    entries: Vec<RenderProfileEntry>,
    started: Option<CpuInstant>,
    generation: u64,
}

#[derive(Clone)]
struct ActiveSession {
    state: Rc<RefCell<ProfileState>>,
    generation: u64,
}

thread_local! {
    static ACTIVE: RefCell<Vec<ActiveSession>> = const { RefCell::new(Vec::new()) };
}

/// Profiling stays allocation-free until requested. A stack restores the outer
/// session after nested execution; generations reject scopes from an older frame.
#[derive(Debug, Default)]
pub(crate) struct CpuProfiler {
    state: Option<Rc<RefCell<ProfileState>>>,
}

impl CpuProfiler {
    pub(crate) fn start(&mut self) {
        self.deactivate();
        let state = self.state.get_or_insert_with(Default::default);
        let generation = {
            let mut state = state.borrow_mut();
            state.entries.clear();
            state.generation = state
                .generation
                .checked_add(1)
                .expect("CPU profile generation exhausted");
            state.started = Some(CpuInstant::now());
            state.generation
        };
        ACTIVE.with(|active| {
            active.borrow_mut().push(ActiveSession {
                state: Rc::clone(state),
                generation,
            })
        });
    }

    pub(crate) fn finish(&mut self) -> CpuProfile {
        let Some(state) = &self.state else {
            return CpuProfile::default();
        };
        let report = {
            let mut state = state.borrow_mut();
            CpuProfile {
                entries: std::mem::take(&mut state.entries),
                total: state
                    .started
                    .take()
                    .map(|start| start.elapsed())
                    .unwrap_or_default(),
            }
        };
        self.remove_active();
        report
    }

    fn deactivate(&self) {
        if let Some(state) = &self.state {
            state.borrow_mut().started = None;
            self.remove_active();
        }
    }

    fn remove_active(&self) {
        if let Some(state) = &self.state {
            // Remove only this owner, including when a parent ends before its child.
            // A TLS-owned profiler can outlive ACTIVE during thread teardown.
            // At that point its entries are already gone; destruction needs no TLS access.
            let _ = ACTIVE.try_with(|active| {
                active
                    .borrow_mut()
                    .retain(|entry| !Rc::ptr_eq(&entry.state, state))
            });
        }
    }
}

impl Drop for CpuProfiler {
    fn drop(&mut self) {
        self.deactivate();
    }
}

pub(crate) struct CpuProfileScope {
    session: ActiveSession,
    name: &'static str,
    started: CpuInstant,
}

impl Drop for CpuProfileScope {
    fn drop(&mut self) {
        let mut state = self.session.state.borrow_mut();
        if state.started.is_some() && state.generation == self.session.generation {
            state.entries.push(RenderProfileEntry {
                name: self.name,
                cpu_duration: Some(self.started.elapsed()),
                gpu_duration: None,
            });
        }
    }
}

fn active_session() -> Option<ActiveSession> {
    let session = ACTIVE.with(|active| active.borrow().last().cloned())?;
    {
        let state = session.state.borrow();
        if state.started.is_none() || state.generation != session.generation {
            return None;
        }
    }
    Some(session)
}

pub(crate) fn start_cpu_scope(name: &'static str) -> Option<CpuProfileScope> {
    Some(CpuProfileScope {
        session: active_session()?,
        name,
        started: CpuInstant::now(),
    })
}

pub(crate) fn record_unavailable_gpu_scope(name: &'static str) {
    if let Some(session) = active_session() {
        // Keep the attempted GPU event in its original position among CPU events.
        session.state.borrow_mut().entries.push(RenderProfileEntry {
            name,
            cpu_duration: None,
            gpu_duration: None,
        });
    }
}

pub(crate) fn profile_cpu<T>(name: &'static str, work: impl FnOnce() -> T) -> T {
    let _scope = start_cpu_scope(name);
    work()
}

#[cfg(test)]
mod tests;
