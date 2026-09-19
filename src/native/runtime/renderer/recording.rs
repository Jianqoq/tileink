//! Immediate scene preparation; vector children and the root share one GPU batch.

use super::{Execution, Images};
use crate::{
    Canvas, TextContext, TextFontSystem,
    native::runtime::{
        Result,
        compute::{ComputeBatch, ResourceId},
        program::scene::SceneCache,
    },
    shared::image_resource::{
        GpuImageResourceUpload, ImageResourceStore, ImageResourceUploadSignature,
    },
    text::PreparedTextData,
};

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub image_dimension: u32,
    pub atlas_pages: u32,
    pub texture_table_len: u32,
    pub dispatch_dimension: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_frame_limits_fail_before_resources_or_cache_are_modified() {
        let canvas = Canvas::new(3, 2, 1.0);
        let resources = ImageResourceStore::default();
        let valid = Limits {
            image_dimension: 32,
            atlas_pages: 1,
            texture_table_len: 0,
            dispatch_dimension: 65535,
        };
        let mut recording = Recording::default();
        for limits in [
            Limits {
                image_dimension: 0,
                ..valid
            },
            Limits {
                atlas_pages: 0,
                ..valid
            },
            Limits {
                dispatch_dimension: 0,
                ..valid
            },
        ] {
            let mut batch = ComputeBatch::new();
            assert!(
                recording
                    .record(&mut batch, &canvas, &resources, None, limits, false)
                    .is_err()
            );
            assert!(batch.resources().is_empty());
            assert!(recording.signature.is_none());
        }
        // Invalid attempts leave the same recorder usable for subsequent frames.
        let mut batch = ComputeBatch::new();
        let output = recording
            .record(&mut batch, &canvas, &resources, None, valid, false)
            .unwrap();
        assert!(batch.outputs().is_empty());
        batch.readback(output).unwrap();
        assert_eq!(batch.outputs().len(), 1);
    }
}

#[derive(Default)]
pub(crate) struct Recording {
    pub(crate) clear_color: u32,
    scene: SceneCache,
    vectors: crate::render::vector_images::VectorImageCache<Recording>,
    upload: GpuImageResourceUpload,
    signature: Option<ImageResourceUploadSignature>,
    text: Option<PreparedTextData>,
}

impl Recording {
    pub(crate) fn record(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        resources: &ImageResourceStore,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        limits: Limits,
        chunked: bool,
    ) -> Result<ResourceId> {
        if limits.image_dimension == 0 || limits.atlas_pages == 0 || limits.dispatch_dimension == 0
        {
            return Err("native frame limits must be nonzero".into());
        }
        let signature = resources.upload_signature(
            canvas.scene_image_resources(),
            limits.image_dimension,
            limits.atlas_pages,
            limits.texture_table_len,
        );
        if self.signature != Some(signature) {
            self.upload = resources.upload_merged(
                canvas.scene_image_resources(),
                limits.image_dimension,
                limits.atlas_pages,
                limits.texture_table_len,
                Some(&self.upload),
            );
            self.signature = Some(signature);
        }
        self.vectors
            .retain_sources(self.upload.vectors().iter().map(|vector| &vector.canvas));
        // Children own their scene namespace, just as in the wgpu renderer. Passing
        // the parent's global store would recursively render unrelated vector images.
        let images = Images::record_with_vectors(batch, &self.upload, |batch, child| {
            self.vectors
                .get_or_insert(child, Self::default)
                .value
                .record(
                    batch,
                    child,
                    &ImageResourceStore::default(),
                    None,
                    limits,
                    chunked,
                )
        })?;
        if let Some((fonts, context)) = text {
            crate::render::prepare::prepare_text(&mut self.text, canvas, fonts, context);
        } else {
            self.text = None;
        }
        Execution::record(
            &mut self.scene,
            batch,
            canvas,
            &images,
            self.text.as_ref(),
            crate::native::runtime::renderer::FrameOptions {
                chunked,
                clear_color: self.clear_color,
            },
            limits.dispatch_dimension,
        )
    }
}
