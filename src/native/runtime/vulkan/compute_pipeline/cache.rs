//! Vulkan owns opaque cache compatibility; driver failures propagate unchanged.
use super::{Result, pipeline_cache};
use ash::vk;
use std::{cell::RefCell, io};

pub(super) fn create(
    device: &ash::Device,
    properties: &vk::PhysicalDeviceProperties,
    key: &str,
    stage: vk::PipelineShaderStageCreateInfo<'_>,
    layout: vk::PipelineLayout,
) -> Result<(vk::Pipeline, bool)> {
    let identity = serde_json::to_vec(
        &serde_json::json!({"api":"vulkan","version":properties.api_version,"vendor":properties.vendor_id,"device":properties.device_id,"driver":properties.driver_version,"uuid":properties.pipeline_cache_uuid}),
    )?;
    let owned = RefCell::new(None);
    let build = |data: &[u8]| Owned::new(device, stage, layout, data).map_err(io::Error::other);
    let hit = pipeline_cache::load_or_create_for_layout(
        &identity,
        key,
        b"native-compute-buffer-table-v1",
        |data| {
            // Vulkan ignores incompatible initial data. An API error is not a
            // cache miss, and a hit does not need to export the cache again.
            *owned.borrow_mut() = Some(build(data)?);
            Ok(true)
        },
        || {
            let created = build(&[])?;
            let bytes = unsafe { device.get_pipeline_cache_data(created.cache) }
                .map_err(io::Error::other)?;
            *owned.borrow_mut() = Some(created);
            Ok(bytes)
        },
    )?;
    Ok((owned.into_inner().unwrap().take_pipeline(), hit))
}

/// Keep both native objects owned until cache publication succeeds. This also
/// retires the pipeline if exporting or writing its driver cache fails.
struct Owned<'a> {
    device: &'a ash::Device,
    cache: vk::PipelineCache,
    pipeline: vk::Pipeline,
}
impl<'a> Owned<'a> {
    fn new(
        device: &'a ash::Device,
        stage: vk::PipelineShaderStageCreateInfo<'_>,
        layout: vk::PipelineLayout,
        data: &[u8],
    ) -> std::result::Result<Self, vk::Result> {
        let mut owned = Self {
            device,
            cache: unsafe {
                device.create_pipeline_cache(
                    &vk::PipelineCacheCreateInfo::default().initial_data(data),
                    None,
                )?
            },
            pipeline: vk::Pipeline::null(),
        };
        unsafe {
            match device.create_compute_pipelines(
                owned.cache,
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(stage)
                    .layout(layout)],
                None,
            ) {
                Ok(pipelines) => owned.pipeline = pipelines[0],
                Err((partial, error)) => {
                    for pipeline in partial {
                        device.destroy_pipeline(pipeline, None);
                    }
                    return Err(error);
                }
            }
        }
        Ok(owned)
    }
    fn take_pipeline(mut self) -> vk::Pipeline {
        std::mem::replace(&mut self.pipeline, vk::Pipeline::null())
    }
}
impl Drop for Owned<'_> {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_cache(self.cache, None);
        }
    }
}
