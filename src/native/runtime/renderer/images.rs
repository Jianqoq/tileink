use super::super::{Result, compute::ComputeBatch, program::scene::SceneImages};
use crate::shared::image_resource::GpuImageResourceUpload;

/// The placement upload and its GPU images cannot be supplied independently to
/// frame execution. This fixes the association invariant at the safe boundary.
pub(crate) struct Images<'a> {
    upload: &'a GpuImageResourceUpload,
    textures: SceneImages,
}

impl<'a> Images<'a> {
    pub(crate) fn record(
        batch: &mut ComputeBatch,
        upload: &'a GpuImageResourceUpload,
    ) -> Result<Self> {
        Ok(Self {
            upload,
            textures: SceneImages::record(batch, upload)?,
        })
    }
    pub(super) fn upload(&self) -> &GpuImageResourceUpload {
        self.upload
    }
    pub(crate) fn textures(&self) -> &SceneImages {
        &self.textures
    }
    pub(super) fn validate(&self, batch: &ComputeBatch) -> Result<()> {
        for id in [
            self.textures.atlas,
            self.textures.table,
            self.textures.sampler,
        ] {
            batch.size(id)?;
        }
        Ok(())
    }
}

mod cache;
pub(crate) use cache::ImageCache;
