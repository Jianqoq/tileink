//! Content identity shared by all external-target renderers.

/// Stable identity for a caller-owned texture whose pixels persist between retained frames.
///
/// Reuse an ID only while passing the same texture with unmodified contents. Allocate a new ID
/// after recreating, resizing, clearing, or otherwise mutating that texture outside tileink.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExternalTextureHistoryId(u64);

impl ExternalTextureHistoryId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

// Logical plan targets are independent of the GPU API and resource handles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderTargetId {
    Main,
    Scratch(usize),
}
