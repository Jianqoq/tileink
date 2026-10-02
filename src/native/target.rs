use super::NativeTexture;
use crate::ExternalTextureHistoryId;

/// A same-device output image and the promise made about its previous contents.
/// The canvas extent starts at `origin`; pixels outside that rectangle are preserved.
#[derive(Clone, Copy, Debug)]
pub struct NativeRenderTarget<'a> {
    pub(super) texture: &'a NativeTexture,
    pub(super) origin: [u32; 2],
    pub(super) history: History,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum History {
    Tracked,
    Transient,
    Persistent(ExternalTextureHistoryId),
}
impl<'a> NativeRenderTarget<'a> {
    /// No previous output contents are promised. Tileink keeps its own history.
    pub fn transient(texture: &'a NativeTexture) -> Self {
        Self {
            texture,
            origin: [0; 2],
            history: History::Transient,
        }
    }
    /// Change the identity after any external modification or image replacement.
    /// Tileink writes through other renderers are additionally detected automatically.
    pub fn persistent(texture: &'a NativeTexture, history: ExternalTextureHistoryId) -> Self {
        Self {
            texture,
            origin: [0; 2],
            history: History::Persistent(history),
        }
    }
    pub fn with_origin(mut self, x: u32, y: u32) -> Self {
        self.origin = [x, y];
        self
    }
}
impl<'a> From<&'a NativeTexture> for NativeRenderTarget<'a> {
    fn from(texture: &'a NativeTexture) -> Self {
        Self {
            texture,
            origin: [0; 2],
            history: History::Tracked,
        }
    }
}
