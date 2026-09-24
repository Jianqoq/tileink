//! Immediate scene preparation; vector children and the root share one GPU batch.

use super::Images;
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
                    .record(
                        &mut batch,
                        &canvas,
                        &resources,
                        None,
                        limits,
                        Default::default()
                    )
                    .is_err()
            );
            assert!(batch.resources().is_empty());
            assert!(recording.signature.is_none());
        }
        // Invalid attempts leave the same recorder usable for subsequent frames.
        let mut batch = ComputeBatch::new();
        let output = recording
            .record(
                &mut batch,
                &canvas,
                &resources,
                None,
                valid,
                Default::default(),
            )
            .unwrap();
        assert!(batch.outputs().is_empty());
        batch.readback(output).unwrap();
        assert_eq!(batch.outputs().len(), 1);
    }
}

#[derive(Default)]
pub(crate) struct Recording {
    filter_scenes: super::filter_scenes::FilterSceneCache,
    pub(crate) clear_color: u32,
    pub(crate) retained: crate::render::retained::RetainedRenderState<super::Surface>,
    scene: SceneCache,
    vectors: crate::render::vector_images::VectorImageCache<Recording>,
    upload: GpuImageResourceUpload,
    gpu_images: Option<super::images::ImageCache>,
    signature: Option<ImageResourceUploadSignature>,
    text: Option<PreparedTextData>,
    text_environment: Option<(crate::TextRasterOptions, u64)>,
}

impl Recording {
    pub(crate) fn release_retained_plan(&mut self) {
        self.scene.release_retained_plan();
    }

    pub(crate) fn install_retained_plan(&mut self, canvas: &Canvas) {
        self.scene.install_retained_plan(canvas);
    }

    pub(crate) fn set_text_environment(&mut self, context: Option<&TextContext>) {
        let environment =
            context.map(|context| (context.raster_options(), context.cache_generation()));
        if self.text_environment != environment {
            // Text options and font-cache invalidation do not edit RetainedScene.
            // Discard both root history and offscreen glyph pixels before damage planning.
            self.retained.invalidate();
            self.text_environment = environment;
        }
    }

    pub(crate) fn record(
        &mut self,
        batch: &mut ComputeBatch,
        canvas: &Canvas,
        resources: &ImageResourceStore,
        text: Option<(&mut TextFontSystem, &mut TextContext)>,
        limits: Limits,
        options: super::FrameOptions,
    ) -> Result<ResourceId> {
        if limits.image_dimension == 0 || limits.atlas_pages == 0 || limits.dispatch_dimension == 0
        {
            return Err("native frame limits must be nonzero".into());
        }
        // No damaged tiles means the persistent output is already complete. Avoid
        // rebuilding descriptors and uploading image resources for an empty frame.
        if self
            .retained
            .active_tiles()
            .is_some_and(|tiles| tiles.list().is_empty())
        {
            return options
                .target
                .ok_or_else(|| "empty native damage requires history".into());
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
        // Different image keys can reference the same immutable child. Its cached
        // buffers may only be uploaded once per batch; reuse the rendered output.
        let mut rendered = std::collections::HashMap::new();
        let images = Images::record_cached(
            batch,
            &self.upload,
            &mut self.gpu_images,
            signature,
            |batch, child| {
                let identity = std::rc::Rc::as_ptr(child);
                if let Some(&image) = rendered.get(&identity) {
                    return Ok(image);
                }
                let image = self
                    .vectors
                    .get_or_insert(child, Self::default)
                    .value
                    .record(
                        batch,
                        child,
                        &ImageResourceStore::default(),
                        None,
                        limits,
                        super::FrameOptions {
                            chunked: options.chunked,
                            ..Default::default()
                        },
                    )?;
                rendered.insert(identity, image);
                Ok(image)
            },
        )?;
        if let Some((fonts, context)) = text {
            crate::render::prepare::prepare_text(&mut self.text, canvas, fonts, context);
        } else {
            self.text = None;
        }
        super::frame::record(
            &mut self.scene,
            batch,
            canvas,
            super::FrameResources {
                filter_scenes: self.filter_scenes.frame(),
                images: &images,
                text: self.text.as_ref(),
                retained: &mut self.retained,
            },
            crate::native::runtime::renderer::FrameOptions {
                clear_color: self.clear_color,
                ..options
            },
            limits.dispatch_dimension,
        )
    }
}

#[cfg(test)]
mod gpu_tests;
