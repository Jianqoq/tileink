use super::super::{
    Result,
    compute::{ComputeBatch, ResourceId},
    program::scene::SceneImages,
};
use crate::{Canvas, shared::image_resource::GpuImageResourceUpload};
use std::rc::Rc;

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
    pub(crate) fn record_with_vectors(
        batch: &mut ComputeBatch,
        upload: &'a GpuImageResourceUpload,
        render: impl FnMut(&mut ComputeBatch, &Rc<Canvas>) -> Result<ResourceId>,
    ) -> Result<Self> {
        Ok(Self {
            upload,
            textures: SceneImages::record_with_vectors(batch, upload, render)?,
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
