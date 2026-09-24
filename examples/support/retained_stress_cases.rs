//! The actual stress workload cases, independent of renderer setup.

#[derive(Clone, Copy, Debug)]
pub enum StressScenario {
    DeepHierarchyRevision,
    DeepHierarchyJournalGap,
    ManyBackdropsRevision,
    ManyRootLayersAddRemove,
    LargeChunkRevision,
    DeltaRotation,
}

impl StressScenario {
    pub const ALL: [Self; 6] = [
        Self::DeepHierarchyRevision,
        Self::DeepHierarchyJournalGap,
        Self::ManyBackdropsRevision,
        Self::ManyRootLayersAddRemove,
        Self::LargeChunkRevision,
        Self::DeltaRotation,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::DeepHierarchyRevision => "deep-hierarchy-revision",
            Self::DeepHierarchyJournalGap => "deep-hierarchy-journal-gap",
            Self::ManyBackdropsRevision => "many-backdrops-revision",
            Self::ManyRootLayersAddRemove => "many-root-layers-add-remove",
            Self::LargeChunkRevision => "large-chunk-revision",
            Self::DeltaRotation => "delta-rotation",
        }
    }

    pub const fn counts(self) -> &'static [usize] {
        match self {
            Self::DeepHierarchyRevision | Self::DeepHierarchyJournalGap => &[8, 32, 128, 256, 512],
            Self::ManyBackdropsRevision => &[1, 4, 16, 64, 256],
            Self::ManyRootLayersAddRemove => &[8, 32, 128, 512, 2_048],
            Self::LargeChunkRevision => &[100, 1_000, 5_000, 20_000],
            Self::DeltaRotation => &[256, 1_000, 5_000, 20_000],
        }
    }
}
