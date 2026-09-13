//! Criterion access to the production image uploader; disabled in normal builds.

use super::{Renderer, WgpuRangeScatterPipeline, WgpuSceneBuffers};
use crate::shared::image_resource::{GpuImageResourceUpload, ImageResourceStore};
use crate::{Image, ImageKey};
use std::rc::Rc;

/// Prepacked inputs for an atlas growth with unchanged independent raster images.
#[doc(hidden)]
pub struct ImageResourceUploadBenchmark {
    device: ::wgpu::Device,
    queue: ::wgpu::Queue,
    pipeline: Rc<WgpuRangeScatterPipeline>,
    before: GpuImageResourceUpload,
    after: GpuImageResourceUpload,
}

/// One prepared old allocation, consumed by a timed upload and completion.
#[doc(hidden)]
pub struct PreparedImageResourceUpload<'a> {
    device: &'a ::wgpu::Device,
    queue: &'a ::wgpu::Queue,
    after: &'a GpuImageResourceUpload,
    buffers: WgpuSceneBuffers,
}

impl ImageResourceUploadBenchmark {
    pub fn new(renderer: &Renderer, standalone_height: u32, standalone_count: u32) -> Self {
        assert!(renderer.device.limits().max_texture_dimension_2d >= 2048);
        assert!((1..=2048).contains(&standalone_height));
        assert!(standalone_count <= 16);
        let mut images = ImageResourceStore::default();
        let empty = ImageResourceStore::default();
        for key in 1..=4 {
            images.insert(
                ImageKey::new(key),
                Image::from_rgba8(
                    1022,
                    1022,
                    [key as u8 * 20, 40, 60, 255].repeat(1022 * 1022),
                ),
            );
        }
        for index in 0..standalone_count {
            images.insert(
                ImageKey::new(u64::from(index) + 100),
                Image::from_rgba8(
                    2048,
                    standalone_height,
                    [160, 40, 60, 255].repeat(2048 * standalone_height as usize),
                ),
            );
        }
        // Four padded 1022-square images fill one real 2048 atlas page.
        // Width 2048 cannot fit its padding, so the remaining images use textures.
        let before = images.upload_merged(&empty, 2048, 2, standalone_count, None);
        images.insert(
            ImageKey::new(5),
            Image::from_rgba8(1022, 1022, [100, 40, 60, 255].repeat(1022 * 1022)),
        );
        let after = images.upload_merged(&empty, 2048, 2, standalone_count, Some(&before));
        assert_eq!(before.atlas_page_count(), 1);
        assert_eq!(after.atlas_page_count(), 2);
        assert!(!after.atlas_pages()[0].dirty && after.atlas_pages()[1].dirty);
        assert_eq!(after.textures().len(), standalone_count as usize);
        assert!(after.textures().iter().all(|texture| !texture.dirty));
        Self {
            device: renderer.device.clone(),
            queue: renderer.queue.clone(),
            pipeline: Rc::clone(&renderer.range_scatter_pipeline),
            before,
            after,
        }
    }

    /// Allocation and the first upload/finish are setup, excluded by iter_batched_ref.
    pub fn prepare(&self) -> PreparedImageResourceUpload<'_> {
        let mut buffers = WgpuSceneBuffers::new(&self.device, Rc::clone(&self.pipeline));
        buffers.upload_image_resources(&self.device, &self.queue, &self.before, false);
        self.queue.submit([]);
        self.device
            .poll(::wgpu::PollType::wait_indefinitely())
            .expect("finish initial image upload");
        PreparedImageResourceUpload {
            device: &self.device,
            queue: &self.queue,
            after: &self.after,
            buffers,
        }
    }
}

impl PreparedImageResourceUpload<'_> {
    /// Measures the production upload, submit and completion. CPU packing and
    /// initial resources are already prepared; there is no profiling/readback.
    pub fn upload(&mut self, force_all: bool) {
        self.buffers
            .upload_image_resources(self.device, self.queue, self.after, force_all);
        self.queue.submit([]);
        self.device
            .poll(::wgpu::PollType::wait_indefinitely())
            .expect("finish atlas growth upload");
    }
}
