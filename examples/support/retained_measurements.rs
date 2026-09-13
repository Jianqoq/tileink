//! CPU data shared by the benchmark runner and its inventory tests.

use std::time::Duration;

#[derive(Clone, Copy)]
pub struct BenchConfig {
    pub warmup: usize,
    pub frames: usize,
    /// Profiled wall preserves historical diagnostic timings. Production wall includes
    /// scene mutation, rendering and GPU completion, with all profile readbacks disabled.
    pub profile: bool,
}

impl BenchConfig {
    #[allow(dead_code)]
    pub fn alternating(warmup: usize, iterations: u64, profile: bool) -> Self {
        Self {
            warmup,
            // Production samples contain both alternating phases even when Criterion
            // chooses one iteration; version-dependent iteration counts cannot bias the mix.
            frames: usize::try_from(iterations)
                .expect("benchmark iteration count exceeds usize")
                .checked_mul(if profile { 1 } else { 2 })
                .expect("benchmark frame count overflow"),
            profile,
        }
    }

    /// Complete both mutation phases even for a single diagnostic iteration. This fixes
    /// version-dependent phase weighting while preserving legacy stress sample units.
    #[allow(dead_code)]
    pub fn paired_cycles(warmup: usize, iterations: u64, profile: bool) -> Self {
        Self {
            profile,
            ..Self::alternating(warmup, iterations, false)
        }
    }
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            warmup: 3,
            frames: 20,
            profile: true,
        }
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct Measurements {
    pub wall: Vec<Duration>,
    pub transaction: Duration,
    pub cpu: Duration,
    pub collect: Duration,
    pub materialize: Duration,
    pub materialize_analysis: Duration,
    pub materialize_chunks: Duration,
    pub materialize_plan_sync: Duration,
    pub materialize_frame: Duration,
    pub root_fragment_stages: [profile_observation::StageObservation; 4],
    pub damage: Duration,
    pub prepare: Duration,
    pub scan: Duration,
    pub raster: Duration,
    pub coarse_gpu: Duration,
    pub plan_select: Duration,
    pub plan_execute: Duration,
    pub draw_batch: Duration,
    pub group_cache: Duration,
    pub group_children: Duration,
    pub group_scratch: Duration,
    pub group_render: Duration,
    pub group_mask: Duration,
    pub group_composite: Duration,
    pub dirty_tiles: u64,
    pub changed_tiles: u64,
    pub draw_batches: u64,
    pub root_draw_batches: u64,
    pub total_tiles: u32,
    pub chunks_rebuilt: u64,
    pub plan_fragments_rebuilt: u64,
    pub full_scene_syncs: u64,
    pub cpu_copied_bytes: u64,
    pub gpu_uploaded_bytes: u64,
    pub tile_pages_rewritten: u64,
    pub tile_page_compactions: u64,
    pub arena_live_bytes: u64,
    pub arena_capacity_bytes: u64,
    pub arena_fragmentation: f64,
    pub arena_compactions: u64,
}

#[cfg(test)]
mod measurement_tests {
    #[test]
    fn production_samples_include_complete_alternating_cycles() {
        use super::BenchConfig;
        for iterations in [1, 2, 3, 17] {
            let config = BenchConfig::alternating(3, iterations, false);
            let phases = (config.warmup..config.warmup + config.frames).fold(
                [0usize; 2],
                |mut phases, frame| {
                    phases[frame % 2] += 1;
                    phases
                },
            );
            assert_eq!(phases, [iterations as usize; 2]);
            let diagnostic = BenchConfig::alternating(3, iterations, true);
            assert_eq!(diagnostic.frames, iterations as usize);
        }
    }
}

// Shared by independent example/benchmark binaries; each consumes a different
// subset of the profile observation API.
#[allow(dead_code)]
#[path = "profile_observation.rs"]
pub mod profile_observation;

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum MutationPhase {
    Insert,
    Remove,
}

/// These scopes instrument append_root_plan_fragment. Removal has separate
/// plan/spatial/metadata work; it must not be described as taking zero time.
#[derive(Clone, Copy)]
#[repr(usize)]
pub enum RootFragmentStage {
    Compile,
    Spatial,
    Plan,
    Metadata,
}

impl RootFragmentStage {
    pub const ALL: [Self; 4] = [Self::Compile, Self::Spatial, Self::Plan, Self::Metadata];

    fn names(self) -> (&'static str, &'static str) {
        match self {
            Self::Compile => ("root-fragment-compile", "retained.root_fragment.compile"),
            Self::Spatial => ("root-fragment-spatial", "retained.root_fragment.spatial"),
            Self::Plan => ("root-fragment-plan", "retained.root_fragment.plan"),
            Self::Metadata => ("root-fragment-metadata", "retained.root_fragment.metadata"),
        }
    }

    // Other diagnostic binaries aggregate scopes without exposing per-stage IDs.
    #[allow(dead_code)]
    pub fn name(self) -> &'static str {
        self.names().0
    }
    pub fn scope(self) -> &'static str {
        self.names().1
    }
}

#[cfg(test)]
mod paired_cycle_tests {
    #[test]
    fn diagnostic_and_production_samples_include_complete_cycles() {
        use super::BenchConfig;
        for profile in [false, true] {
            for iterations in [1, 2, 3, 17] {
                let config = BenchConfig::paired_cycles(3, iterations, profile);
                let phases = (config.warmup..config.warmup + config.frames).fold(
                    [0usize; 2],
                    |mut phases, frame| {
                        phases[frame % 2] += 1;
                        phases
                    },
                );
                assert_eq!(phases, [iterations as usize; 2]);
                assert_eq!(config.profile, profile);
            }
        }
    }
}
