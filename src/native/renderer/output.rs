//! Target routing and content identity, separate from frame submission.
use super::NativeRenderer;
use crate::native::runtime::{
    compute::{ComputeBatch, ResourceId, TextureCopy},
    texture::State,
};
use crate::native::{NativeError, NativeRenderTarget, NativeTexture, target::History};
use crate::render::retained::HistoryOwner;
use std::rc::{Rc, Weak};

pub(super) struct HistoryRecord {
    texture: Weak<State>,
    content_version: u64,
    origin: [u32; 2],
}
pub(super) struct OutputRoute<'a> {
    pub render_target: NativeTexture,
    pub owns_target: bool,
    pub history_owner: HistoryOwner,
    history_texture: NativeTexture,
    history_origin: [u32; 2],
    copy: Option<NativeRenderTarget<'a>>,
    external: bool,
}
impl<'a> OutputRoute<'a> {
    pub fn prepare(
        renderer: &NativeRenderer,
        size: (u32, u32),
        output: Option<NativeRenderTarget<'a>>,
    ) -> Result<Self, NativeError> {
        if let Some(output) = output {
            if output.texture.array {
                return Err(NativeError::Recording(
                    "native output must be a two-dimensional texture".into(),
                ));
            }
            let extent = output.texture.size();
            if matches!(output.history, History::Tracked) && extent != size {
                return Err(NativeError::Recording(
                    "native output size differs from canvas".into(),
                ));
            }
            if output.origin[0]
                .checked_add(size.0)
                .is_none_or(|end| end > extent.0)
                || output.origin[1]
                    .checked_add(size.1)
                    .is_none_or(|end| end > extent.1)
            {
                return Err(NativeError::Recording(
                    "native output rectangle exceeds target".into(),
                ));
            }
            if !renderer
                .context
                .adapter
                .same_device(&output.texture.context.adapter)
            {
                return Err(NativeError::Recording(
                    "native output belongs to another logical device".into(),
                ));
            }
        }
        let copy = output.filter(|target| {
            matches!(target.history, History::Transient)
                || target.origin != [0; 2]
                || target.texture.size() != size
        });
        let owns_target = output.is_none() || copy.is_some();
        let render_target = if owns_target {
            match &renderer.target {
                Some(target) if target.size() == size => target.clone(),
                _ => renderer.context.create_texture(size.0, size.1)?,
            }
        } else {
            output.unwrap().texture.clone()
        };
        let history = output.filter(|target| !matches!(target.history, History::Transient));
        Ok(Self {
            history_texture: history
                .map_or_else(|| render_target.clone(), |target| target.texture.clone()),
            history_origin: history.map_or([0; 2], |target| target.origin),
            history_owner: match history.map(|target| target.history) {
                Some(History::Persistent(id)) => HistoryOwner::External(id),
                _ => HistoryOwner::Internal,
            },
            render_target,
            owns_target,
            copy,
            external: output.is_some(),
        })
    }
    pub fn matches(&self, previous: &HistoryRecord) -> bool {
        previous.texture.as_ptr() == Rc::as_ptr(&self.history_texture.state)
            && previous.content_version == self.history_texture.state.content_version.get()
            && previous.origin == self.history_origin
    }
    /// Capture only after the queue accepts every write in the frame.
    pub fn capture_history(&self) -> HistoryRecord {
        HistoryRecord {
            texture: Rc::downgrade(&self.history_texture.state),
            content_version: self.history_texture.state.content_version.get(),
            origin: self.history_origin,
        }
    }
    pub fn encode_copy(
        &self,
        batch: &mut ComputeBatch,
        source: ResourceId,
    ) -> Result<(), NativeError> {
        if let Some(target) = self.copy {
            let destination = batch
                .import_texture(target.texture)
                .map_err(NativeError::Recording)?;
            let size = self.render_target.size();
            batch
                .copy_texture(TextureCopy {
                    source,
                    destination,
                    source_origin: [0; 3],
                    destination_origin: [target.origin[0], target.origin[1], 0],
                    extent: [size.0, size.1, 1],
                })
                .map_err(NativeError::Recording)?;
        }
        Ok(())
    }
    pub fn record_stats(&self, stats: &mut crate::IncrementalRenderStats) {
        stats.history_copied_to_output = self.copy.is_some();
        stats.output_mode = if self.external && self.copy.is_none() {
            crate::IncrementalOutputMode::ExternalHistory
        } else {
            crate::IncrementalOutputMode::InternalHistory
        };
    }
}
