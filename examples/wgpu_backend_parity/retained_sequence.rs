//! One immutable input set and a deterministic transaction sequence shared by every route.

// The frame catalog is portable; construction is used by GPU runs and CPU tests.
#[cfg(any(windows, test))]
#[path = "retained_sequence/state.rs"]
mod state;
#[cfg(any(windows, test))]
pub use state::Sequence;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Initial,
    Static,
    Geometry,
    Image,
    Translate,
    Reorder,
    Reparent,
    Mask,
    Background,
    Text,
    Remove,
    Reinsert,
    Invalidate,
    Grow,
    Shrink,
    Tile15,
    Tile16,
    Tile17,
    Dpi,
    AfterResize,
    ReplaceTarget,
    FreshHistory,
    ExternalClear,
    SwapImage,
    ReturnImage,
    JournalGap,
    Resume,
    Empty,
    EmptyStatic,
}

pub const FRAMES: &[Frame] = &[
    Frame::Initial,
    Frame::Static,
    Frame::Geometry,
    Frame::Image,
    Frame::Translate,
    Frame::Reorder,
    Frame::Reparent,
    Frame::Mask,
    Frame::Background,
    Frame::Text,
    Frame::Remove,
    Frame::Reinsert,
    Frame::Invalidate,
    Frame::Grow,
    Frame::Shrink,
    Frame::Tile15,
    Frame::Tile16,
    Frame::Tile17,
    Frame::Dpi,
    Frame::AfterResize,
    Frame::ReplaceTarget,
    Frame::FreshHistory,
    Frame::ExternalClear,
    Frame::SwapImage,
    Frame::ReturnImage,
    Frame::JournalGap,
    Frame::Resume,
    Frame::Empty,
    Frame::EmptyStatic,
];

impl Frame {
    pub fn name(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Static => "static",
            Self::Geometry => "replace-geometry",
            Self::Image => "replace-image",
            Self::Translate => "fractional-transform",
            Self::Reorder => "reorder",
            Self::Reparent => "reparent-into-clip",
            Self::Mask => "mask-edit",
            Self::Background => "backdrop-background-edit",
            Self::Text => "replace-clipped-text",
            Self::Remove => "remove",
            Self::Reinsert => "reinsert",
            Self::Invalidate => "invalidate-rect",
            Self::Grow => "resize-grow",
            Self::Shrink => "resize-shrink",
            Self::Tile15 => "resize-tile-15",
            Self::Tile16 => "resize-tile-16",
            Self::Tile17 => "resize-tile-17",
            Self::Dpi => "resize-dpi",
            Self::AfterResize => "local-edit-after-resize",
            Self::ReplaceTarget => "same-size-new-target",
            Self::FreshHistory => "same-target-new-history",
            Self::ExternalClear => "external-clear-new-history",
            Self::SwapImage => "swap-image",
            Self::ReturnImage => "return-to-older-image",
            Self::JournalGap => "journal-overflow",
            Self::Resume => "incremental-after-journal-overflow",
            Self::Empty => "remove-all-content",
            Self::EmptyStatic => "empty-static",
        }
    }
}

pub fn names() -> Vec<String> {
    FRAMES
        .iter()
        .enumerate()
        .map(|(index, frame)| format!("{index:02}-{}", frame.name()))
        .collect()
}
